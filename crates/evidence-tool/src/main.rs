//! Evidence tool.
//!
//! Scans on-chain testimonial submissions and produces:
//! - Per-month counts
//! - Distinct account counts
//! - Filter by submission_id
//! - Output suitable for the submission

use clap::Parser;

#[derive(Parser)]
#[command(name = "evidence-tool", version, about = "Adoption evidence tool")]
struct Cli {
    #[arg(long)]
    submission_id: Option<String>,
    #[arg(long, default_value = "https://testnet.lez.logos.co")]
    sequencer: String,
    #[arg(long, default_value = "text")]
    format: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("Evidence Tool v{}", env!("CARGO_PKG_VERSION"));
    println!("Sequencer: {}", cli.sequencer);

    if let Some(ref sid) = cli.submission_id {
        println!("Filtering by submission_id: {}", sid);
    }

    // TODO: Implement chain scanning for testimonial submissions
    println!("⚠ Chain scanning not yet implemented");
    Ok(())
}
