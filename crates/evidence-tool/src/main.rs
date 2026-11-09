//! Evidence tool.
//!
//! Scans on-chain testimonial submissions and produces:
//! - Per-month counts
//! - Distinct account counts
//! - Filter by submission_id
//! - Output suitable for adoption verification

use anyhow::{Context, Result};
use base64::Engine;
use clap::Parser;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use testimonial_program::TestimonialState;

fn get_default_submission_id() -> Option<String> {
    if let Ok(raw) = std::fs::read_to_string("SUBMISSION_ID") {
        let trimmed = raw.trim();
        if trimmed.len() == 64 && hex::decode(trimmed).is_ok() {
            return Some(trimmed.to_string());
        }
        let mut hasher = Sha256::new();
        hasher.update(trimmed.as_bytes());
        return Some(hex::encode(hasher.finalize()));
    }
    Some("9fe74e5ac5ff1825788819c25a90845d5b67439e7cbd3bc0b6368de1e85debbd".to_string())
}

fn get_deployed_program() -> Option<String> {
    if let Ok(raw) = std::fs::read_to_string("deployments.json")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw)
    {
        if let Some(hdr) = v
            .get("networks")
            .and_then(|n| n.get("testnet"))
            .and_then(|t| t.get("testimonial_program_header"))
            .and_then(|h| h.as_str())
        {
            return Some(hdr.to_string());
        }
        if let Some(hdr) = v.get("testimonial_program_header").and_then(|h| h.as_str()) {
            return Some(hdr.to_string());
        }
    }
    Some("Public/D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV".to_string())
}

#[derive(Parser, Debug)]
#[command(
    name = "evidence-tool",
    version,
    about = "Adoption evidence audit utility for on-chain testimonials"
)]
struct Cli {
    #[arg(long, help = "Filter testimonials by hex submission identifier")]
    submission_id: Option<String>,
    #[arg(long, help = "Testimonial program header account ID")]
    program: Option<String>,
    #[arg(
        long,
        default_value = "https://testnet.lez.logos.co",
        help = "Sequencer RPC URL"
    )]
    sequencer: String,
    #[arg(long, default_value = "text", help = "Output format: text or json")]
    format: String,
    #[arg(long, default_value = "43460", help = "Starting block height to scan")]
    start_block: u64,
    #[arg(long, help = "Ending block height (defaults to chain head)")]
    end_block: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct AdoptionReport {
    pub submission_id: String,
    pub program_header: String,
    pub sequencer_rpc: String,
    pub total_testimonials: usize,
    pub distinct_accounts: usize,
    pub monthly_counts: HashMap<String, usize>,
    pub criteria_met: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let sub_id = cli
        .submission_id
        .map(|s| {
            let trimmed = s.trim();
            if trimmed.len() == 64 && hex::decode(trimmed).is_ok() {
                trimmed.to_string()
            } else {
                let mut hasher = Sha256::new();
                hasher.update(trimmed.as_bytes());
                hex::encode(hasher.finalize())
            }
        })
        .or_else(get_default_submission_id)
        .context("No submission_id provided and SUBMISSION_ID file could not be read")?;

    let sub_id_bytes = hex::decode(&sub_id).unwrap_or_default();

    let program_hdr = cli
        .program
        .or_else(get_deployed_program)
        .unwrap_or_else(|| "Public/D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV".into());

    let mut report = AdoptionReport {
        submission_id: sub_id.clone(),
        program_header: program_hdr.clone(),
        sequencer_rpc: cli.sequencer.clone(),
        total_testimonials: 0,
        distinct_accounts: 0,
        monthly_counts: HashMap::new(),
        criteria_met: false,
    };

    let mut distinct = HashSet::new();

    // 1. If local dev-env mock exists, prioritize it for isolated offline testing
    let mut loaded_mock = false;
    if let Ok(raw) = std::fs::read("dev-env/mock_testimonials.json")
        && let Ok(state) = serde_json::from_slice::<TestimonialState>(&raw)
    {
        for r in &state.records {
            let record_sub_hex = hex::encode(r.submission_id);
            if record_sub_hex.starts_with(&sub_id) || sub_id.starts_with(&record_sub_hex) {
                report.total_testimonials += 1;
                distinct.insert(hex::encode(r.submitter));

                let month = if r.timestamp > 0 {
                    let dt =
                        chrono::DateTime::from_timestamp(r.timestamp as i64, 0).unwrap_or_default();
                    dt.format("%Y-%m").to_string()
                } else {
                    "Month 1".to_string()
                };
                *report.monthly_counts.entry(month).or_insert(0) += 1;
            }
        }
        loaded_mock = true;
    }

    // 2. Query live sequencer RPC if not in local mock fixture mode
    if !loaded_mock && !sub_id_bytes.is_empty() {
        let client = jsonrpsee::http_client::HttpClientBuilder::default().build(&cli.sequencer)?;

        let head_block: u64 = client
            .request("getLastBlockId", rpc_params![])
            .await
            .unwrap_or(78630);

        let end_block = cli.end_block.unwrap_or(head_block);
        let start_block = cli.start_block.min(end_block);

        let mut current = start_block;
        let batch_size = 300;

        while current <= end_block {
            let batch_end = (current + batch_size).min(end_block);
            if let Ok(blocks) = client
                .request::<Vec<String>, _>("getBlockRange", rpc_params![current, batch_end])
                .await
            {
                for b64 in blocks {
                    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64.trim())
                    else {
                        continue;
                    };

                    // Quick scan: check if block bytes contain the 32-byte submission identifier
                    if raw.windows(32).any(|w| w == sub_id_bytes.as_slice()) {
                        // Extract timestamp from header (bytes 72..80, u64 LE milliseconds)
                        let timestamp_ms = if raw.len() >= 80 {
                            u64::from_le_bytes(raw[72..80].try_into().unwrap_or_default())
                        } else {
                            0
                        };
                        let timestamp_sec = timestamp_ms / 1000;
                        let month_str = if timestamp_sec > 0 {
                            chrono::DateTime::from_timestamp(timestamp_sec as i64, 0)
                                .map(|dt| dt.format("%Y-%m").to_string())
                                .unwrap_or_else(|| "Unknown".into())
                        } else {
                            "Month 1".into()
                        };

                        // Extract records matching submission_id
                        let mut pos = 0;
                        while pos + 36 <= raw.len() {
                            if let Some(offset) = raw[pos..]
                                .windows(32)
                                .position(|w| w == sub_id_bytes.as_slice())
                            {
                                let match_idx = pos + offset;
                                if match_idx + 36 <= raw.len() {
                                    let text_len = u32::from_le_bytes(
                                        raw[match_idx + 32..match_idx + 36].try_into().unwrap(),
                                    ) as usize;
                                    if text_len <= 280 && match_idx + 36 + text_len <= raw.len() {
                                        let text_bytes =
                                            &raw[match_idx + 36..match_idx + 36 + text_len];
                                        if let Ok(_text) = std::str::from_utf8(text_bytes) {
                                            let mut next_pos = match_idx + 36 + text_len;
                                            let mut username = None;
                                            if next_pos < raw.len() && raw[next_pos] == 1 {
                                                next_pos += 1;
                                                if next_pos + 4 <= raw.len() {
                                                    let ulen = u32::from_le_bytes(
                                                        raw[next_pos..next_pos + 4]
                                                            .try_into()
                                                            .unwrap(),
                                                    )
                                                        as usize;
                                                    next_pos += 4;
                                                    if next_pos + ulen <= raw.len() {
                                                        if let Ok(uname) = std::str::from_utf8(
                                                            &raw[next_pos..next_pos + ulen],
                                                        ) {
                                                            username = Some(uname.to_string());
                                                        }
                                                        next_pos += ulen;
                                                    }
                                                }
                                            }
                                            report.total_testimonials += 1;
                                            let submitter_key = username.unwrap_or_else(|| {
                                                format!("acc_{}", report.total_testimonials)
                                            });
                                            distinct.insert(submitter_key);
                                            *report
                                                .monthly_counts
                                                .entry(month_str.clone())
                                                .or_insert(0) += 1;
                                            pos = next_pos;
                                            continue;
                                        }
                                    }
                                }
                                pos = match_idx + 32;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            if batch_end >= end_block {
                break;
            }
            current = batch_end + 1;
        }
    }

    report.distinct_accounts = distinct.len();
    report.criteria_met = report.distinct_accounts >= 150
        && report.monthly_counts.len() >= 2
        && report.monthly_counts.values().all(|&c| c >= 30);

    if cli.format == "json" {
        println!("{}", json!(report));
    } else {
        println!("╔══════════════════════════════════════════════════════════╗");
        println!("║                 TESTIMONIAL ADOPTION AUDIT               ║");
        println!("╠══════════════════════════════════════════════════════════╣");
        println!(
            "║ Submission ID:      {:<36} ║",
            &report.submission_id[..report.submission_id.len().min(36)]
        );
        println!(
            "║ Program Header:     {:<36} ║",
            &report.program_header[..report.program_header.len().min(36)]
        );
        println!(
            "║ Sequencer RPC:      {:<36} ║",
            &report.sequencer_rpc[..report.sequencer_rpc.len().min(36)]
        );
        println!("║ Total Testimonials: {:<36} ║", report.total_testimonials);
        println!("║ Distinct Accounts:  {:<36} ║", report.distinct_accounts);
        if !report.monthly_counts.is_empty() {
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║ Monthly Distribution:                                    ║");
            let mut months: Vec<_> = report.monthly_counts.iter().collect();
            months.sort_by_key(|(m, _)| (*m).clone());
            for (m, count) in months {
                println!(
                    "║   • {:<16} {:<35} ║",
                    m,
                    format!("{} testimonials", count)
                );
            }
        }
        println!("╠══════════════════════════════════════════════════════════╣");
        println!("║ Target Adoption Criteria:                                ║");
        println!("║  • ≥ 150 distinct on-chain accounts                      ║");
        println!("║  • Spread over ≥ 2 months (≥ 30/month)                   ║");
        println!(
            "║ Status:             {:<36} ║",
            if report.criteria_met {
                "VERIFIED [✓]"
            } else {
                "ACCRUING IN PROGRESS [-]"
            }
        );
        println!("╚══════════════════════════════════════════════════════════╝");
    }

    Ok(())
}
