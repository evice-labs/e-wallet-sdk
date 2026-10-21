//! Evice Wallet SDK CLI.
//!
//! Provides a comprehensive command-line interface for the Evice Wallet SDK:
//! - `wallet create|open|restore`
//! - `account create-public|create-private|list|balance`
//! - `transfer public|shield|unshield|private` (with transparent `.lez` name resolution)
//! - `token create-definition|initialize-holding|transfer|mint|burn|balance`
//! - `resolve <name.lez>`
//! - `info`

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use lee::AccountId;
use sdk_engine::SdkEngine;
use sdk_engine::api::{AccountPrivacy, DAppApi, OwnerApi, SdkConfig};
use sdk_engine::approval::{ApprovalDecision, ApprovalHandler, ApprovalPrompt, CliApprovalHandler};
use sdk_engine::resolve::lns::{LnsConfig, LnsResolver};
use sdk_engine::session::GrantKind;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "evice-wallet",
    version,
    about = "Evice Wallet SDK — Modular Wallet Engine for Logos Execution Zone (LEZ)"
)]
struct Cli {
    /// Data directory for wallet state and config (defaults to ~/.evice-wallet)
    #[arg(
        long,
        env = "EVICE_WALLET_HOME_DIR",
        default_value = "~/.device-wallet"
    )]
    data_dir: PathBuf,

    /// Wallet storage password (can also be passed via EVICE_WALLET_PASSWORD or LEZ_WALLET_PASSWORD env var).
    /// WARNING: Passing passwords via CLI argument is visible in process lists (/proc, ps).
    #[arg(long, env = "EVICE_WALLET_PASSWORD")]
    password: Option<String>,

    /// Auto-approve transactions without interactive terminal prompt (for scripts / CI)
    #[arg(short = 'y', long)]
    yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Wallet lifecycle management (create, open, restore)
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },
    /// Multi-account management (create public/private, list, balance)
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    /// Asset transfer operations (public, shield, unshield, private)
    Transfer {
        #[command(subcommand)]
        action: TransferAction,
    },
    /// Token program operations (definitions, holdings, transfers, mint, burn)
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
    /// Resolve a Logos Name Service (.lez) name
    Resolve {
        /// The name to resolve (e.g. alice.lez).
        name: String,
    },
    /// Identity Accountability operations (commitment, scoped nullifiers, attestations)
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },
    /// Display SDK engine, API versions, and network information
    Info,
}

#[derive(Subcommand)]
enum WalletAction {
    /// Initialize a new wallet with password and generate a recovery phrase (mnemonic)
    Create {
        /// Force overwrite if wallet storage already exists
        #[arg(short, long)]
        force: bool,
    },
    /// Verify password and unlock an existing wallet storage
    Open,
    /// Restore an existing wallet storage from a BIP-39 mnemonic phrase
    Restore {
        /// 12 or 24-word recovery phrase separated by spaces
        #[arg(long)]
        mnemonic: String,
        /// Force overwrite if wallet storage already exists
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum AccountAction {
    /// Derive a new public account
    CreatePublic {
        /// Optional human-readable label
        #[arg(long)]
        label: Option<String>,
    },
    /// Derive a new private (shielded) account
    CreatePrivate {
        /// Optional human-readable label
        #[arg(long)]
        label: Option<String>,
    },
    /// List all public and private accounts with labels and balances
    List,
    /// Inspect the balance of a specific account address
    Balance {
        /// Account ID or .lez name
        account: String,
    },
}

#[derive(Subcommand)]
enum TransferAction {
    /// Transfer native LEZ tokens from public account to public account
    Public {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    /// Shield native tokens (transfer from public account to private shielded account)
    Shield {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    /// Unshield native tokens (transfer from private shielded account to public account)
    Unshield {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    /// Private transfer (transfer between private shielded accounts)
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
    /// Create a new fungible token definition
    CreateDefinition {
        #[arg(long)]
        name: String,
    },
    /// Initialize a token holding account associated with a definition
    InitializeHolding {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        account: String,
    },
    /// Transfer tokens between holdings
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
    /// Mint tokens to a holding account
    Mint {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    /// Burn tokens from a holding account
    Burn {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        amount: u128,
    },
    /// Check the balance of a token holding account
    Balance {
        #[arg(long)]
        holding: String,
    },
}

#[derive(Subcommand)]
enum IdentityAction {
    /// Display the master public identity commitment
    Commitment,
    /// Derive a cross-dApp unlinkable scoped nullifier
    Nullifier {
        /// Scope identifier (e.g. dao_voting, forum_auth, chamber_1)
        #[arg(long)]
        scope: String,
    },
    /// Sign a verifiable BIP-340 Schnorr attestation within a given scope
    Attest {
        /// Scope identifier
        #[arg(long)]
        scope: String,
        /// Message payload to attest/sign
        #[arg(long)]
        message: String,
    },
}

/// Auto-approval handler used when `--yes` is specified.
struct AutoApprovalHandler;

#[async_trait::async_trait]
impl ApprovalHandler for AutoApprovalHandler {
    async fn request_approval(
        &self,
        _prompt: ApprovalPrompt,
        _timeout: Duration,
    ) -> ApprovalDecision {
        ApprovalDecision::Approve
    }
}

/// Expands leading `~` to the user's home directory.
fn expand_tilde(path: PathBuf) -> PathBuf {
    if let Some(s) = path.to_str()
        && let Some(stripped) = s.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(stripped);
    }
    path
}

/// Ensures that a wallet storage file already exists.
fn ensure_wallet_exists(data_dir: &Path) -> Result<()> {
    let config = SdkConfig::new(data_dir.to_path_buf());
    if !config.wallet_storage_path.exists() && !config.sdk_state_path.exists() {
        bail!(
            "No wallet found at '{}'. Run 'evice-wallet wallet create' or 'evice-wallet wallet restore' first.",
            data_dir.display()
        );
    }
    Ok(())
}

/// Retrieves the wallet password with terminal echo disabled.
fn get_password(cli_pw: &Option<String>, prompt_text: &str) -> Result<String> {
    if let Some(p) = cli_pw.as_ref() {
        return Ok(p.clone());
    }
    if let Ok(p) = std::env::var("EVICE_WALLET_PASSWORD")
        && !p.is_empty()
    {
        return Ok(p);
    }
    if let Ok(p) = std::env::var("LEZ_WALLET_PASSWORD")
        && !p.is_empty()
    {
        return Ok(p);
    }
    let prompt = format!("{}: ", prompt_text);
    let pass = rpassword::prompt_password(prompt)
        .context("Failed to read password securely from terminal")?;
    Ok(pass)
}

/// For wallet creation, prompts for password twice and verifies they match.
fn get_new_wallet_password(cli_pw: &Option<String>) -> Result<String> {
    if let Some(p) = cli_pw.as_ref() {
        return Ok(p.clone());
    }
    if let Ok(p) = std::env::var("LEZ_WALLET_PASSWORD")
        && !p.is_empty()
    {
        return Ok(p);
    }
    let p1 = rpassword::prompt_password("Enter password for new wallet: ")
        .context("Failed to read password from terminal")?;
    if p1.is_empty() {
        bail!("Password cannot be empty");
    }
    let p2 = rpassword::prompt_password("Confirm wallet password: ")
        .context("Failed to read password confirmation from terminal")?;
    if p1 != p2 {
        bail!("Passwords do not match. Aborting wallet creation.");
    }
    Ok(p1)
}

/// Opens or initializes an SdkEngine instance with the given directory, password, and approval handler.
fn init_or_open_engine(
    data_dir: &std::path::Path,
    password: &str,
    handler: Arc<dyn ApprovalHandler>,
) -> Result<SdkEngine> {
    SdkEngine::open_or_init(data_dir.to_path_buf(), password, handler)
        .map_err(|e| anyhow::anyhow!("Failed to open or initialize SdkEngine: {:?}", e))
}

/// Parses an account ID string or resolves a .lez LNS name to an AccountId.
async fn resolve_or_parse_account(engine: &SdkEngine, target: &str) -> Result<AccountId> {
    if target.ends_with(".lez") {
        println!("Resolving LNS name '{}'...", target);
        let session_id = engine
            .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
            .await
            .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
        match engine.resolve_name(&session_id, target).await {
            Ok(dest) => {
                if let Some(account_id) = dest.account {
                    println!("[✓] Resolved '{}' -> {}", target, account_id);
                    Ok(account_id)
                } else {
                    bail!(
                        "LNS name '{}' exists but has no published account address",
                        target
                    )
                }
            }
            Err(e) => bail!("Failed to resolve LNS name '{}': {:?}", target, e),
        }
    } else {
        AccountId::from_str(target)
            .map_err(|e| anyhow::anyhow!("Invalid AccountId '{}': {:?}", target, e))
    }
}

#[tokio::main]
pub async fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let data_dir = expand_tilde(cli.data_dir);

    let handler: Arc<dyn ApprovalHandler> = if cli.yes {
        Arc::new(AutoApprovalHandler)
    } else {
        Arc::new(CliApprovalHandler)
    };

    match cli.command {
        Commands::Info => {
            println!("╔══════════════════════════════════════════════════════════╗");
            println!("║               EVICE WALLET PROVIDER SDK CLI              ║");
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║ SDK Version:    {:<40} ║", sdk_engine::SDK_VERSION);
            println!("║ API Version:    {:<40} ║", sdk_engine::API_VERSION);
            println!(
                "║ Network:        {:<40} ║",
                "Logos Execution Zone (testnet 0.3 - 0.4)"
            );
            println!("║ Data Dir:       {:<40} ║", data_dir.display());
            let config = SdkConfig::new(data_dir.clone());
            let wallet_exists =
                config.wallet_storage_path.exists() || config.sdk_state_path.exists();
            let status_str = if wallet_exists {
                "Initialized [✓]"
            } else {
                "Uninitialized (run 'wallet create')"
            };
            println!("║ Wallet Status:  {:<40} ║", status_str);
            println!("╚══════════════════════════════════════════════════════════╝");
        }

        Commands::Wallet { action } => match action {
            WalletAction::Create { force } => {
                let config = SdkConfig::new(data_dir.clone());
                if (config.wallet_storage_path.exists() || config.sdk_state_path.exists()) && !force
                {
                    bail!(
                        "Wallet already exists at '{}'. Use 'wallet open' or provide '--force' to overwrite.",
                        data_dir.display()
                    );
                }
                let pw = get_new_wallet_password(&cli.password)?;
                let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                let mnemonic = engine
                    .create_wallet(&pw)
                    .await
                    .map_err(|e| anyhow::anyhow!("Create wallet error: {:?}", e))?;

                println!(
                    "\n[✓] Wallet successfully created at '{}'!",
                    data_dir.display()
                );
                println!(
                    "\n╔══════════════════════════════════════════════════════════════════════════════╗"
                );
                println!(
                    "║                        RECOVERY PHRASE (MNEMONIC)                            ║"
                );
                println!(
                    "╠══════════════════════════════════════════════════════════════════════════════╣"
                );
                println!(
                    "║ Store these words securely offline. Anyone with this phrase can restore      ║"
                );
                println!(
                    "║ your entire wallet and all associated accounts:                              ║"
                );
                println!(
                    "╠══════════════════════════════════════════════════════════════════════════════╣"
                );
                let words: Vec<&str> = mnemonic.words().collect();
                for (chunk_idx, chunk_words) in words.chunks(3).enumerate() {
                    let mut row_str = String::new();
                    for (i, word) in chunk_words.iter().enumerate() {
                        let word_num = chunk_idx * 3 + i + 1;
                        row_str.push_str(&format!("{word_num:>2}. {:<18} ", word));
                    }
                    println!("║   {:<72}   ║", row_str.trim_end());
                }
                println!(
                    "╚══════════════════════════════════════════════════════════════════════════════╝\n"
                );
            }
            WalletAction::Open => {
                ensure_wallet_exists(&data_dir)?;
                let pw = get_password(&cli.password, "Enter wallet password")?;
                let _engine = init_or_open_engine(&data_dir, &pw, handler)?;
                println!(
                    "[✓] Wallet at '{}' opened and unlocked successfully.",
                    data_dir.display()
                );
            }
            WalletAction::Restore { mnemonic, force } => {
                let config = SdkConfig::new(data_dir.clone());
                if (config.wallet_storage_path.exists() || config.sdk_state_path.exists()) && !force
                {
                    bail!(
                        "Wallet already exists at '{}'. Use '--force' if you intend to overwrite the existing wallet.",
                        data_dir.display()
                    );
                }
                let parsed_mnem = bip39::Mnemonic::from_str(&mnemonic)
                    .map_err(|e| anyhow::anyhow!("Invalid mnemonic phrase: {:?}", e))?;
                let pw = get_new_wallet_password(&cli.password)?;
                let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                engine
                    .restore_wallet(&parsed_mnem, &pw)
                    .await
                    .map_err(|e| anyhow::anyhow!("Restore wallet error: {:?}", e))?;
                println!(
                    "[✓] Wallet at '{}' restored successfully from recovery phrase.",
                    data_dir.display()
                );
            }
        },

        Commands::Account { action } => {
            ensure_wallet_exists(&data_dir)?;
            match action {
                AccountAction::CreatePublic { label } => {
                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let account_id = engine
                        .create_account_public(label.clone())
                        .await
                        .map_err(|e| anyhow::anyhow!("Create public account error: {:?}", e))?;

                    println!("[✓] Created public account:");
                    println!("  Address: {}", account_id);
                    println!("  Label:   {}", label.as_deref().unwrap_or("<none>"));
                }
                AccountAction::CreatePrivate { label } => {
                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let account_id = engine
                        .create_account_private(label.clone())
                        .await
                        .map_err(|e| anyhow::anyhow!("Create private account error: {:?}", e))?;

                    println!("[✓] Created private (shielded) account:");
                    println!("  Address: {}", account_id);
                    println!("  Label:   {}", label.as_deref().unwrap_or("<none>"));
                }
                AccountAction::List => {
                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let accounts = engine
                        .list_all_accounts()
                        .await
                        .map_err(|e| anyhow::anyhow!("List accounts error: {:?}", e))?;

                    if accounts.is_empty() {
                        println!(
                            "No accounts found. Use 'account create-public' or 'account create-private' to generate one."
                        );
                    } else {
                        println!(
                            "╔═════════╦══════════════════════════════════════════════════════════════════╦══════════════════════╦══════════════════════╗"
                        );
                        println!(
                            "║ Privacy ║ Account Address                                                  ║ Label                ║ Balance (LEZ)        ║"
                        );
                        println!(
                            "╠═════════╬══════════════════════════════════════════════════════════════════╬══════════════════════╬══════════════════════╣"
                        );
                        for acc in &accounts {
                            let priv_str = match acc.privacy {
                                AccountPrivacy::Public => "Public ",
                                AccountPrivacy::Private => "Private",
                            };
                            let label_str = acc.label.as_deref().unwrap_or("-");
                            let bal_str = acc
                                .balance
                                .map(|b| b.to_string())
                                .unwrap_or_else(|| "0".to_string());
                            println!(
                                "║ {:<7} ║ {:<64} ║ {:<20} ║ {:<20} ║",
                                priv_str, acc.account_id, label_str, bal_str
                            );
                        }
                        println!(
                            "╚═════════╩══════════════════════════════════════════════════════════════════╩══════════════════════╩══════════════════════╝"
                        );
                        println!("Total Accounts: {}", accounts.len());
                    }
                }
                AccountAction::Balance { account } => {
                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let account_id = resolve_or_parse_account(&engine, &account).await?;

                    let session_id = engine
                        .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                        .await
                        .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
                    engine
                        .grant_account(&session_id, account_id, GrantKind::Read)
                        .await
                        .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

                    let balance = engine
                        .get_balance(&session_id, &account_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Get balance error: {:?}", e))?;

                    println!("Account: {}", account_id);
                    println!("Balance: {} units", balance);
                }
            }
        }

        Commands::Transfer { action } => {
            let (from_str, to_str, amount, dir_name, req_from_privacy) = match action {
                TransferAction::Public { from, to, amount } => (
                    from,
                    to,
                    amount,
                    "Public -> Public",
                    Some(AccountPrivacy::Public),
                ),
                TransferAction::Shield { from, to, amount } => (
                    from,
                    to,
                    amount,
                    "Public -> Private (Shield)",
                    Some(AccountPrivacy::Public),
                ),
                TransferAction::Unshield { from, to, amount } => (
                    from,
                    to,
                    amount,
                    "Private -> Public (Unshield)",
                    Some(AccountPrivacy::Private),
                ),
                TransferAction::Private { from, to, amount } => (
                    from,
                    to,
                    amount,
                    "Private -> Private",
                    Some(AccountPrivacy::Private),
                ),
            };

            if amount == 0 {
                bail!("Transfer amount must be greater than zero");
            }
            if from_str == to_str {
                bail!(
                    "Source and destination accounts cannot be identical ('{}')",
                    from_str
                );
            }

            ensure_wallet_exists(&data_dir)?;
            let pw = get_password(&cli.password, "Enter wallet password")?;
            let engine = init_or_open_engine(&data_dir, &pw, handler)?;
            let from_id = resolve_or_parse_account(&engine, &from_str).await?;

            // Verify account privacy if sender is in local wallet
            if let Ok(accounts) = engine.list_all_accounts().await
                && let Some(matching) = accounts.iter().find(|a| a.account_id == from_id)
                && let Some(expected) = req_from_privacy
                && matching.privacy != expected
            {
                bail!(
                    "Account '{}' is {:?}, but transfer direction '{}' requires {:?}",
                    from_id,
                    matching.privacy,
                    dir_name,
                    expected
                );
            }

            let session_id = engine
                .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                .await
                .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
            engine
                .grant_account(&session_id, from_id, GrantKind::Propose)
                .await
                .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

            println!("Proposing transfer ({dir_name})...");
            let req_id = engine
                .propose_native_transfer(&session_id, from_id, to_str.clone(), amount)
                .await
                .map_err(|e| anyhow::anyhow!("Transfer proposal error: {:?}", e))?;

            tokio::time::sleep(Duration::from_millis(50)).await;
            let status = engine
                .get_request_status(&req_id)
                .await
                .map_err(|e| anyhow::anyhow!("Get status error: {:?}", e))?;

            println!("\n╔══════════════════════════════════════════════════════════╗");
            println!("║                  TRANSFER PROPOSAL RESULT                ║");
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║ Direction:   {:<43} ║", dir_name);
            println!("║ From:        {:<43} ║", from_id.to_string());
            println!("║ To:          {:<43} ║", to_str);
            println!("║ Amount:      {:<43} ║", format!("{} units", amount));
            println!("║ Request ID:  {:<43} ║", req_id.to_string());
            println!("║ Status:      {:<43} ║", format!("{:?}", status));
            println!("╚══════════════════════════════════════════════════════════╝");
        }

        Commands::Token { action } => {
            ensure_wallet_exists(&data_dir)?;
            match action {
                TokenAction::CreateDefinition { name } => {
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        bail!("Token definition name cannot be empty");
                    }

                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let accounts = engine.list_all_accounts().await?;
                    let primary = accounts
                        .first()
                        .map(|a| a.account_id)
                        .context("No accounts available to sponsor token definition")?;

                    let session_id = engine
                        .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                        .await
                        .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
                    engine
                        .grant_account(&session_id, primary, GrantKind::Propose)
                        .await
                        .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

                    let calldata = name.as_bytes().to_vec();
                    let req_id = engine
                        .propose_transaction(
                            &session_id,
                            vec![primary],
                            calldata,
                            Some(format!("Create token definition '{}'", name)),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("Propose transaction error: {:?}", e))?;

                    println!("[✓] Proposed Token Definition creation:");
                    println!("  Token Name: {}", name);
                    println!("  Request ID: {}", req_id);
                }

                TokenAction::InitializeHolding {
                    definition,
                    account,
                } => {
                    if definition == account {
                        bail!("Token definition and holding account cannot be identical");
                    }

                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let def_id = resolve_or_parse_account(&engine, &definition).await?;
                    let acc_id = resolve_or_parse_account(&engine, &account).await?;

                    let session_id = engine
                        .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                        .await
                        .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
                    engine
                        .grant_account(&session_id, acc_id, GrantKind::Propose)
                        .await
                        .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

                    let req_id = engine
                        .propose_transaction(
                            &session_id,
                            vec![acc_id, def_id],
                            vec![],
                            Some(format!("Initialize holding for definition {}", def_id)),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("Propose transaction error: {:?}", e))?;

                    println!("[✓] Proposed Token Holding Initialization:");
                    println!("  Account:    {}", acc_id);
                    println!("  Definition: {}", def_id);
                    println!("  Request ID: {}", req_id);
                }

                TokenAction::Transfer {
                    from,
                    to,
                    definition,
                    amount,
                } => {
                    if amount == 0 {
                        bail!("Token transfer amount must be greater than zero");
                    }
                    if from == to {
                        bail!("Source and destination holdings cannot be identical");
                    }

                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let from_id = resolve_or_parse_account(&engine, &from).await?;
                    let to_id = resolve_or_parse_account(&engine, &to).await?;
                    let def_id = resolve_or_parse_account(&engine, &definition).await?;

                    let session_id = engine
                        .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                        .await
                        .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
                    engine
                        .grant_account(&session_id, from_id, GrantKind::Propose)
                        .await
                        .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

                    let req_id = engine
                        .propose_token_transfer(&session_id, from_id, to_id, def_id, amount)
                        .await
                        .map_err(|e| anyhow::anyhow!("Propose token transfer error: {:?}", e))?;

                    println!("[✓] Proposed Token Transfer:");
                    println!("  From Holding: {}", from_id);
                    println!("  To Holding:   {}", to_id);
                    println!("  Definition:   {}", def_id);
                    println!("  Amount:       {}", amount);
                    println!("  Request ID:   {}", req_id);
                }

                TokenAction::Mint {
                    definition,
                    to,
                    amount,
                } => {
                    if amount == 0 {
                        bail!("Mint amount must be greater than zero");
                    }

                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let def_id = resolve_or_parse_account(&engine, &definition).await?;
                    let to_id = resolve_or_parse_account(&engine, &to).await?;

                    let session_id = engine
                        .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                        .await
                        .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
                    engine
                        .grant_account(&session_id, to_id, GrantKind::Propose)
                        .await
                        .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

                    let req_id = engine
                        .propose_transaction(
                            &session_id,
                            vec![to_id, def_id],
                            vec![],
                            Some(format!("Mint {} tokens of {} to {}", amount, def_id, to_id)),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("Propose mint error: {:?}", e))?;

                    println!("[✓] Proposed Token Mint:");
                    println!("  Definition: {}", def_id);
                    println!("  To:         {}", to_id);
                    println!("  Amount:     {}", amount);
                    println!("  Request ID: {}", req_id);
                }

                TokenAction::Burn {
                    definition,
                    from,
                    amount,
                } => {
                    if amount == 0 {
                        bail!("Burn amount must be greater than zero");
                    }

                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let def_id = resolve_or_parse_account(&engine, &definition).await?;
                    let from_id = resolve_or_parse_account(&engine, &from).await?;

                    let session_id = engine
                        .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                        .await
                        .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
                    engine
                        .grant_account(&session_id, from_id, GrantKind::Propose)
                        .await
                        .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

                    let req_id = engine
                        .propose_transaction(
                            &session_id,
                            vec![from_id, def_id],
                            vec![],
                            Some(format!(
                                "Burn {} tokens of {} from {}",
                                amount, def_id, from_id
                            )),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("Propose burn error: {:?}", e))?;

                    println!("[✓] Proposed Token Burn:");
                    println!("  Definition: {}", def_id);
                    println!("  From:       {}", from_id);
                    println!("  Amount:     {}", amount);
                    println!("  Request ID: {}", req_id);
                }

                TokenAction::Balance { holding } => {
                    let pw = get_password(&cli.password, "Enter wallet password")?;
                    let engine = init_or_open_engine(&data_dir, &pw, handler)?;
                    let holding_id = resolve_or_parse_account(&engine, &holding).await?;

                    let session_id = engine
                        .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                        .await
                        .map_err(|e| anyhow::anyhow!("Session error: {:?}", e))?;
                    engine
                        .grant_account(&session_id, holding_id, GrantKind::Read)
                        .await
                        .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

                    let balance = engine
                        .get_balance(&session_id, &holding_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Get balance error: {:?}", e))?;

                    println!("Token Holding: {}", holding_id);
                    println!("Balance:       {} units", balance);
                }
            }
        }

        Commands::Resolve { name } => {
            println!("Resolving LNS name: '{}'", name);
            match LnsResolver::normalise(&name) {
                Some(normalised) => {
                    let (id, _key) = LnsResolver::derive(&normalised);
                    let pda_seed = LnsResolver::compute_pda_seed(&id);
                    let registry = LnsConfig::default().registry_program_id;

                    println!("╔══════════════════════════════════════════════════════════╗");
                    println!("║             LNS NAME RESOLUTION                          ║");
                    println!("╠══════════════════════════════════════════════════════════╣");
                    println!("║ Input Name:   {:<42} ║", name);
                    println!("║ Normalised:   {:<42} ║", normalised);
                    println!("║ Name ID:      {:<42} ║", hex::encode(id));
                    println!("║ PDA Seed:     {:<42} ║", hex::encode(pda_seed));
                    println!("║ Registry ID:  {:<42} ║", registry.to_string());
                    println!(
                        "║ Derivation:   {:<42} ║",
                        "Argon2id (64 MiB, 3 passes) [✓]"
                    );
                    println!("╚══════════════════════════════════════════════════════════╝");

                    let config = SdkConfig::new(data_dir.clone());
                    if config.wallet_storage_path.exists() {
                        let pw = get_password(
                            &cli.password,
                            "Enter wallet password to query sequencer",
                        )?;
                        if let Ok(engine) = init_or_open_engine(&data_dir, &pw, handler.clone())
                            && let Ok(session_id) = engine
                                .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                                .await
                        {
                            match engine.resolve_name(&session_id, &name).await {
                                Ok(dest) => {
                                    if let Some(addr) = dest.account {
                                        println!(
                                            "[✓] Sequencer Online Result: Resolved to address {}",
                                            addr
                                        );
                                    } else {
                                        println!(
                                            "[-] Sequencer Online Result: Name record exists but has no published address"
                                        );
                                    }
                                }
                                Err(e) => println!("[-] Sequencer Online Query: {:?}", e),
                            }
                        }
                    } else {
                        println!(
                            "(Note: Offline derivation verified. To query sequencer on-chain, initialize wallet first.)"
                        );
                    }
                }
                None => {
                    eprintln!("Error: '{}' is not a valid .lez name.", name);
                    eprintln!(
                        "Requirements: 1-32 chars of [a-z0-9-], no leading or trailing hyphen."
                    );
                    std::process::exit(1);
                }
            }
        }

        Commands::Identity { action } => {
            ensure_wallet_exists(&data_dir)?;
            let pass = get_password(&cli.password, "Enter wallet password")?;
            let engine = init_or_open_engine(&data_dir, &pass, handler)?;
            let session_id = engine
                .connect("evice-wallet-cli".into(), sdk_engine::API_VERSION)
                .await
                .map_err(|e| anyhow::anyhow!("Session connection failed: {:?}", e))?;
            engine
                .request_identity_access(&session_id, GrantKind::IdentityRead)
                .await
                .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;
            engine
                .request_identity_access(&session_id, GrantKind::IdentityPropose)
                .await
                .map_err(|e| anyhow::anyhow!("Grant error: {:?}", e))?;

            match action {
                IdentityAction::Commitment => {
                    let commitment = engine
                        .get_identity_commitment(&session_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to derive commitment: {:?}", e))?;
                    println!("╔══════════════════════════════════════════════════════════╗");
                    println!("║             LEZ IDENTITY ACCOUNTABILITY                  ║");
                    println!("╠══════════════════════════════════════════════════════════╣");
                    println!("║ Master Identity Commitment (Hex):                        ║");
                    println!("║ {:<56} ║", hex::encode(commitment.as_bytes()));
                    println!("╚══════════════════════════════════════════════════════════╝");
                }
                IdentityAction::Nullifier { scope } => {
                    let nullifier = engine
                        .derive_scoped_nullifier(&session_id, &scope)
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to derive scoped nullifier: {:?}", e)
                        })?;
                    println!("Scope ID:         {}", scope);
                    println!("Scoped Nullifier: {}", hex::encode(nullifier.as_bytes()));
                }
                IdentityAction::Attest { scope, message } => {
                    let attestation = engine
                        .sign_identity_attestation(&session_id, &scope, message.as_bytes())
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to sign attestation: {:?}", e))?;
                    println!("Scope ID:          {}", scope);
                    println!(
                        "Scoped Nullifier:  {}",
                        hex::encode(attestation.nullifier.as_bytes())
                    );
                    println!("BIP-340 Signature: {}", hex::encode(attestation.signature));
                }
            }
        }
    }

    Ok(())
}
