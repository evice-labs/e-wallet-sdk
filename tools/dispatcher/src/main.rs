use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lee::AccountId;
use program_loader_core::MAX_SEGMENT_DATA_LEN;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::str::FromStr;
use testimonial_program::{TestimonialInstruction, derive_testimonial_pda_seed};
use wallet::{
    WalletCore, account::AccountIdWithPrivacy, cli::CliAccountMention,
    program_facades::program_loader::ProgramLoader,
};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct NetworkDeployment {
    pub sequencer_rpc: String,
    pub testimonial_program_header: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeploymentsConfig {
    pub network: String,
    pub sequencer_rpc: String,
    pub testimonial_program_header: Option<String>,
    #[serde(default)]
    pub networks: HashMap<String, NetworkDeployment>,
}

impl Default for DeploymentsConfig {
    fn default() -> Self {
        let mut networks = HashMap::new();
        networks.insert(
            "standalone".into(),
            NetworkDeployment {
                sequencer_rpc: "http://127.0.0.1:3040".into(),
                testimonial_program_header: None,
            },
        );
        networks.insert(
            "testnet".into(),
            NetworkDeployment {
                sequencer_rpc: "https://testnet.lez.logos.co".into(),
                testimonial_program_header: None,
            },
        );
        Self {
            network: "standalone".into(),
            sequencer_rpc: "http://127.0.0.1:3040".into(),
            testimonial_program_header: None,
            networks,
        }
    }
}

pub fn get_deployments() -> DeploymentsConfig {
    const EMBEDDED: &str = include_str!("../../../deployments.json");
    if let Ok(content) = std::fs::read_to_string("deployments.json")
        && let Ok(cfg) = serde_json::from_str(&content)
    {
        return cfg;
    }
    serde_json::from_str(EMBEDDED).unwrap_or_default()
}

pub fn save_deployments(cfg: &DeploymentsConfig) -> Result<()> {
    let serialized = serde_json::to_string_pretty(cfg)?;
    std::fs::write("deployments.json", serialized)?;
    Ok(())
}

fn extract_account_id(acc: AccountIdWithPrivacy) -> AccountId {
    match acc {
        AccountIdWithPrivacy::Public(id) | AccountIdWithPrivacy::Private(id) => id,
    }
}

pub fn alice_account_id() -> AccountId {
    let mut bytes = [0u8; 32];
    hex::decode_to_slice(
        "6aa4c365699b89d963ef85a59868b9612b6001f90750b4970662a62776332e81",
        &mut bytes,
    )
    .expect("valid hex");
    AccountId::new(bytes)
}

fn get_default_submission_id() -> Result<[u8; 32]> {
    let raw =
        std::fs::read_to_string("SUBMISSION_ID").context("Could not read SUBMISSION_ID file")?;
    let trimmed = raw.trim();
    let mut hasher = Sha256::new();
    hasher.update(trimmed.as_bytes());
    Ok(hasher.finalize().into())
}

pub fn get_wallet_core(network: &str) -> Result<WalletCore> {
    if network == "standalone" {
        let dev_dir = std::path::PathBuf::from("dev-env/.dev_wallet");
        std::fs::create_dir_all(&dev_dir)?;
        let config_path = dev_dir.join("wallet_config.json");
        let storage_path = dev_dir.join("storage.json");

        if !config_path.exists() {
            let cfg = serde_json::json!({
                "sequencer": {
                    "sequencer_addr": "http://127.0.0.1:3040"
                },
                "seq_poll_timeout": "30s",
                "seq_tx_poll_max_blocks": 15,
                "seq_poll_max_retries": 10,
                "seq_block_poll_max_amount": 100
            });
            std::fs::write(&config_path, serde_json::to_string_pretty(&cfg)?)?;
        }

        let mut wallet_core = if storage_path.exists() {
            WalletCore::new_update_chain(config_path, storage_path, None, "")?
        } else {
            let (mut wallet, _mnemonic) =
                WalletCore::new_init_storage(config_path, storage_path, None, "")?;
            const ALICE_KEY_HEX: &str =
                "7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb";
            let alice_key = lee::PrivateKey::from_str(ALICE_KEY_HEX)
                .map_err(|e| anyhow::anyhow!("Failed to parse Alice private key: {e}"))?;
            wallet
                .storage_mut()
                .key_chain_mut()
                .add_imported_public_account(alice_key);
            wallet.store_persistent_data()?;
            wallet
        };

        const ALICE_KEY_HEX: &str =
            "7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb";
        let alice_id = alice_account_id();
        if wallet_core
            .storage()
            .key_chain()
            .pub_account_signing_key(alice_id)
            .is_none()
        {
            let alice_key = lee::PrivateKey::from_str(ALICE_KEY_HEX)
                .map_err(|e| anyhow::anyhow!("Failed to parse Alice private key: {e}"))?;
            wallet_core
                .storage_mut()
                .key_chain_mut()
                .add_imported_public_account(alice_key);
            let _ = wallet_core.store_persistent_data();
        }
        Ok(wallet_core)
    } else {
        WalletCore::from_env().context("Failed to initialize WalletCore from environment")
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "lez-dispatcher",
    version,
    about = "On-Chain Dispatcher and Program Deployment Tool for Evice Wallet SDK"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Deploy a compiled program ELF/binary to LEZ via ProgramLoader
    Deploy {
        #[arg(long, help = "Path to compiled program ELF or binary")]
        bin: String,
        #[arg(
            long,
            help = "Payer account address (defaults to first public account in wallet)"
        )]
        payer: Option<String>,
        #[arg(
            long,
            default_value = "standalone",
            help = "Network target: standalone or testnet"
        )]
        network: String,
    },
    /// Submit an on-chain testimonial attributed to SUBMISSION_ID
    SubmitTestimonial {
        #[arg(long, help = "Testimonial text message (max 280 bytes)")]
        text: String,
        #[arg(long, help = "Optional user handle or moniker (max 64 bytes)")]
        username: Option<String>,
        #[arg(
            long,
            help = "Hex-encoded 32-byte submission identifier (defaults to SUBMISSION_ID file)"
        )]
        submission_id: Option<String>,
        #[arg(long, help = "Target program header (defaults to deployments.json)")]
        program: Option<String>,
        #[arg(long, help = "Payer/submitter account")]
        payer: Option<String>,
        #[arg(
            long,
            default_value = "standalone",
            help = "Network target: standalone or testnet"
        )]
        network: String,
        #[arg(long, default_value = "false")]
        json: bool,
    },
    /// Inspect deployments.json and on-chain environment status
    Status {
        #[arg(long, default_value = "false")]
        json: bool,
    },
    /// Submit a raw instruction call to any on-chain program
    Raw {
        #[arg(long)]
        program: String,
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        accounts: Vec<String>,
        #[arg(long)]
        data_hex: String,
        #[arg(long)]
        payer: Option<String>,
        #[arg(
            long,
            default_value = "standalone",
            help = "Network target: standalone or testnet"
        )]
        network: String,
        #[arg(long, default_value = "false")]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut deployments = get_deployments();

    match cli.command {
        Commands::Status { json } => {
            if json {
                println!("{}", json!(deployments));
            } else {
                println!("╔══════════════════════════════════════════════════════════╗");
                println!("║                 LEZ PROGRAM DEPLOYMENTS                  ║");
                println!("╠══════════════════════════════════════════════════════════╣");
                println!("║ Active Network:   {:<38} ║", deployments.network);
                println!("║ Sequencer RPC:    {:<38} ║", deployments.sequencer_rpc);
                println!(
                    "║ Program Header:   {:<38} ║",
                    deployments
                        .testimonial_program_header
                        .as_deref()
                        .unwrap_or("Not Deployed (null)")
                );
                println!("╚══════════════════════════════════════════════════════════╝");
            }
            Ok(())
        }

        Commands::Deploy {
            bin,
            payer,
            network,
        } => {
            let mut wallet_core = get_wallet_core(&network)?;

            let bytecode = std::fs::read(&bin)
                .with_context(|| format!("Failed to read program binary at '{}'", bin))?;

            // Automatically encapsulate raw guest ELF into ProgramBinary if needed
            let program_bytes = if bytecode.starts_with(b"\x7fELF") {
                risc0_binfmt::ProgramBinary::new(&bytecode, risc0_zkos_v1compat::V1COMPAT_ELF)
                    .encode()
            } else {
                bytecode
            };

            let payer_id = if let Some(p) = payer {
                let mention = CliAccountMention::from_str(&p)
                    .map_err(|e| anyhow::anyhow!("Invalid payer mention: {e}"))?;
                extract_account_id(mention.resolve(wallet_core.storage())?)
            } else {
                let alice_id = alice_account_id();
                if wallet_core
                    .storage()
                    .key_chain()
                    .pub_account_signing_key(alice_id)
                    .is_some()
                {
                    alice_id
                } else {
                    let (first, _) = wallet_core
                        .storage()
                        .key_chain()
                        .public_account_ids()
                        .next()
                        .context(
                            "No public accounts available in wallet keystore to pay deployment fee",
                        )?;
                    first
                }
            };

            let binary = risc0_binfmt::ProgramBinary::decode(&program_bytes)
                .context("Failed to decode program binary as RISC0 ProgramBinary")?;
            let segment_count = binary.user_elf.len().div_ceil(MAX_SEGMENT_DATA_LEN);

            println!(
                "Deploying program from '{}' (total size: {} bytes, user_elf: {} bytes, segments: {})...",
                bin,
                program_bytes.len(),
                binary.user_elf.len(),
                segment_count
            );
            println!("Payer Account: {}", payer_id);

            let header = wallet_core.create_new_account_public(None).0;
            let segments: Vec<AccountId> =
                std::iter::repeat_with(|| wallet_core.create_new_account_public(None).0)
                    .take(segment_count)
                    .collect();
            wallet_core.store_persistent_data()?;

            let program_id = ProgramLoader(&wallet_core)
                .deploy(header, &segments, program_bytes, true, Some(payer_id))
                .await
                .context("Program deployment failed")?;

            let header_str = format!("Public/{}", program_id);
            println!("[✓] Program Deployed Successfully!");
            println!("    Program Header Account ID: {}", header_str);

            if let Some(entry) = deployments.networks.get_mut(&network) {
                entry.testimonial_program_header = Some(header_str.clone());
            }
            if deployments.network == network {
                deployments.testimonial_program_header = Some(header_str);
            }
            save_deployments(&deployments)?;
            println!("[✓] Updated deployments.json for network '{}'", network);
            Ok(())
        }

        Commands::SubmitTestimonial {
            text,
            username,
            submission_id,
            program,
            payer,
            network,
            json,
        } => {
            let wallet_core = get_wallet_core(&network)?;

            let sub_id_bytes = if let Some(hex_str) = submission_id {
                let clean = hex_str.trim_start_matches("0x").trim();
                let bytes = hex::decode(clean).context("Invalid hex for submission_id")?;
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            } else {
                get_default_submission_id()?
            };

            let prog_str = program
                .or_else(|| {
                    deployments
                        .networks
                        .get(&network)
                        .and_then(|n| n.testimonial_program_header.clone())
                })
                .or_else(|| deployments.testimonial_program_header.clone())
                .context("No program header specified and deployments.json has null testimonial_program_header")?;

            let program_mention = CliAccountMention::from_str(&prog_str)
                .map_err(|e| anyhow::anyhow!("Invalid program header: {e}"))?;
            let program_id = extract_account_id(program_mention.resolve(wallet_core.storage())?);

            let payer_id = if let Some(p) = payer {
                let mention = CliAccountMention::from_str(&p)
                    .map_err(|e| anyhow::anyhow!("Invalid payer mention: {e}"))?;
                extract_account_id(mention.resolve(wallet_core.storage())?)
            } else {
                let alice_id = alice_account_id();
                if wallet_core
                    .storage()
                    .key_chain()
                    .pub_account_signing_key(alice_id)
                    .is_some()
                {
                    alice_id
                } else {
                    let (first, _) = wallet_core
                        .storage()
                        .key_chain()
                        .public_account_ids()
                        .next()
                        .context("No public accounts available in wallet keystore")?;
                    first
                }
            };

            let pda_seed_bytes = derive_testimonial_pda_seed(payer_id.value(), &sub_id_bytes);
            let pda_id = AccountId::for_public_pda(
                &program_id,
                &lee_core::program::PdaSeed::new(pda_seed_bytes),
            );

            let instruction = TestimonialInstruction::SubmitTestimonial {
                submission_id: sub_id_bytes,
                text: text.clone(),
                username: username.clone(),
            };
            let instruction_data = instruction
                .to_bytes()
                .map_err(|e| anyhow::anyhow!("Failed to serialize instruction: {e}"))?;

            let mentions = vec![
                wallet::AccountIdentity::PublicNoSign(pda_id)
                    .select_program_actor_state(program_id),
                wallet::AccountIdentity::Public(payer_id).select_program_actor_state(program_id),
            ];

            if !json {
                println!(
                    "Submitting testimonial to sequencer: {}",
                    deployments.sequencer_rpc
                );
                println!("  Target Program:  {}", program_id);
                println!("  Submitter/Payer: {}", payer_id);
                println!("  Derived PDA:     {}", pda_id);
                println!("  Text:            \"{}\"", text);
                if let Some(ref u) = username {
                    println!("  Username:        {}", u);
                }
            }

            let tx_hash = wallet_core
                .send_pub_tx_paid_by(mentions, 0, instruction_data, Some(payer_id))
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to submit transaction to sequencer: {:?}", e)
                })?;

            let poller = wallet_core.poller();
            let (_, block_id) = poller
                .poll_tx(tx_hash)
                .await
                .context("Transaction polling failed")?;

            if json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "tx_hash": tx_hash.to_string(),
                        "block_id": block_id.to_string(),
                        "pda_account": format!("Public/{}", pda_id),
                        "program_id": format!("Public/{}", program_id),
                        "submitter": format!("Public/{}", payer_id),
                    })
                );
            } else {
                println!("[✓] Testimonial Confirmed on LEZ!");
                println!("    Transaction Hash: {}", tx_hash);
                println!("    Block ID:         {}", block_id);
                println!("    PDA Account:      Public/{}", pda_id);
            }
            Ok(())
        }

        Commands::Raw {
            program,
            accounts,
            data_hex,
            payer,
            network,
            json,
        } => {
            let wallet_core = get_wallet_core(&network)?;

            let program_mention =
                CliAccountMention::from_str(&program).map_err(|e| anyhow::anyhow!("{e}"))?;
            let program_id = extract_account_id(program_mention.resolve(wallet_core.storage())?);

            let clean_hex = data_hex.trim_start_matches("0x").trim();
            let instruction_data = hex::decode(clean_hex).context("Failed to decode data_hex")?;

            let mut mentions = Vec::new();
            for raw in &accounts {
                let trimmed = raw.trim();
                let (clean, force_sign) = if let Some(stripped) = trimmed.strip_suffix(":sign") {
                    (stripped, Some(true))
                } else if let Some(stripped) = trimmed.strip_suffix(":nosign") {
                    (stripped, Some(false))
                } else {
                    (trimmed, None)
                };
                let mention =
                    CliAccountMention::from_str(clean).map_err(|e| anyhow::anyhow!("{e}"))?;
                let resolved = mention.resolve(wallet_core.storage())?;
                let id = extract_account_id(resolved);
                let should_sign = force_sign
                    .unwrap_or_else(|| wallet_core.get_account_public_signing_key(id).is_some());
                let identity = if should_sign {
                    wallet::AccountIdentity::Public(id)
                } else {
                    wallet::AccountIdentity::PublicNoSign(id)
                };
                mentions.push(identity.select_program_actor_state(program_id));
            }

            let payer_id = if let Some(p) = payer {
                let m = CliAccountMention::from_str(&p).map_err(|e| anyhow::anyhow!("{e}"))?;
                Some(extract_account_id(m.resolve(wallet_core.storage())?))
            } else {
                None
            };

            let tx_hash = wallet_core
                .send_pub_tx_paid_by(mentions, 0, instruction_data, payer_id)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to submit raw transaction: {:?}", e))?;

            let poller = wallet_core.poller();
            let (_, block_id) = poller
                .poll_tx(tx_hash)
                .await
                .context("Transaction confirmation error")?;

            if json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "tx_hash": tx_hash.to_string(),
                        "block_id": block_id.to_string()
                    })
                );
            } else {
                println!(
                    "[✓] Confirmed on-chain! tx_hash={} block_id={}",
                    tx_hash, block_id
                );
            }
            Ok(())
        }
    }
}
