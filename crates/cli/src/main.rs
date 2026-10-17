//! LEZ Wallet SDK CLI.
//!
//! Commands:
//! - wallet create|open|restore
//! - account create-public|create-private|list|balance
//! - transfer public|shield|unshield|private (accepts .lez names)
//! - token create-definition|initialize-holding|transfer|mint|burn|balance
//! - resolve <name.lez>
//! - info

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lez-wallet", version, about = "LEZ Wallet SDK CLI")]
struct Cli {
    #[arg(long, env = "LEZ_WALLET_HOME_DIR", default_value = "~/.lez-wallet")]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    Transfer {
        #[command(subcommand)]
        action: TransferAction,
    },
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
    Resolve {
        /// The name to resolve (e.g. alice.lez).
        name: String,
    },
    Info,
}

#[derive(Subcommand)]
enum WalletAction {
    Create,
    Open,
    Restore {
        #[arg(long)]
        mnemonic: String,
    },
}

#[derive(Subcommand)]
enum AccountAction {
    CreatePublic {
        #[arg(long)]
        label: Option<String>,
    },
    CreatePrivate {
        #[arg(long)]
        label: Option<String>,
    },
    List,
    Balance {
        account: String,
    },
}

#[derive(Subcommand)]
enum TransferAction {
    Public {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    Shield {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    Unshield {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    Private {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
}

#[derive(Subcommand)]
enum TokenAction {
    CreateDefinition {
        #[arg(long)]
        name: String,
    },
    InitializeHolding {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        account: String,
    },
    Transfer {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        definition: String,
        #[arg(long)]
        amount: u128,
    },
    Mint {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    Burn {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        amount: u128,
    },
    Balance {
        #[arg(long)]
        holding: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    println!("LEZ Wallet SDK CLI v{}", sdk_engine::SDK_VERSION);
    println!("Data dir: {}", cli.data_dir.display());

    match cli.command {
        Commands::Info => {
            println!("SDK version: {}", sdk_engine::SDK_VERSION);
            println!("API version: {}", sdk_engine::API_VERSION);
        }
        Commands::Resolve { name } => {
            println!("Resolving: {}", name);
            match sdk_engine::resolve::lns::LnsResolver::normalise(&name) {
                Some(normalised) => {
                    println!("Normalised: {}", normalised);
                    let (id, _key) = sdk_engine::resolve::lns::LnsResolver::derive(&normalised);
                    println!("Name ID: {}", hex::encode(id));
                    let pda_seed = sdk_engine::resolve::lns::LnsResolver::compute_pda_seed(&id);
                    println!("PDA seed: {}", hex::encode(pda_seed));
                    // TODO: Full resolution via sequencer
                    println!("⚠ Full resolution (sequencer lookup) not yet implemented");
                }
                None => {
                    eprintln!("Error: '{}' is not a valid .lez name", name);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            // TODO: Implement full command handlers with SdkEngine
            println!("⚠ Command not yet implemented. Run with --help for usage.");
        }
    }

    Ok(())
}
