//! SDK API surface.
//!
//! [`SdkEngine`] is the central orchestrator. It owns `WalletCore`,
//! session store, request store, tx queue, decoder registry, verifier, and
//! resolver. Two API surfaces:
//!
//! - **[`DAppApi`]** callable by third-party dApps via sessions.
//! - **[`OwnerApi`]** callable only by the wallet UI (privileged).

use bip39::Mnemonic;
use lee::AccountId;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use wallet::account::{AccountIdWithPrivacy, Label};
use wallet::config::{WalletConfig, WalletConfigOverrides};
use wallet::{WalletCore, WalletSequencerClient};

use crate::approval::{ApprovalDecision, ApprovalHandler, ApprovalPrompt};
use crate::client::TestnetSequencerClient;
use crate::decode::DecoderRegistry;
use crate::effects::EffectSimulator;
use crate::error::{SdkError, SdkResult};
use crate::queue::TxQueue;
use crate::request::{PendingRequest, RequestId, RequestKind, RequestStatus, RequestStore};
use crate::resolve::NameResolver;
use crate::resolve::lns::{LnsConfig, LnsResolver};
use crate::session::{GrantKind, SessionId, SessionStore};
use crate::store::PersistentStore;
use crate::verifier::{BuiltinVerifier, ProgramVerifier};

const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(24 * 3600);
const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Canonical default execution gas limit aligned with LEZ v0.4.0 (`fee_core::market::MAX_GAS_EXEC`).
/// Public transactions are dry-run measured by `WalletCore` and refunded for excess gas.
pub const DEFAULT_WALLET_GAS_LIMIT: u64 = wallet::DEFAULT_GAS_LIMIT;

#[derive(Debug, Clone)]
pub struct SdkConfig {
    pub wallet_config_path: PathBuf,
    pub wallet_storage_path: PathBuf,
    pub sdk_state_path: PathBuf,
    pub session_ttl: Duration,
    pub approval_timeout: Duration,
    pub lns_config: LnsConfig,
    pub program_verification_enabled: bool,
}

impl SdkConfig {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            wallet_config_path: data_dir.join("wallet_config.json"),
            wallet_storage_path: data_dir.join("wallet_storage.json"),
            sdk_state_path: data_dir.join("sdk_state.json"),
            session_ttl: DEFAULT_SESSION_TTL,
            approval_timeout: DEFAULT_APPROVAL_TIMEOUT,
            lns_config: LnsConfig::default(),
            program_verification_enabled: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SdkInfo {
    pub sdk_version: String,
    pub api_version: u32,
    pub network: String,
    pub wallet_initialized: bool,
    pub wallet_locked: bool,
}

#[derive(Debug, Clone)]
pub struct AccountView {
    pub account_id: AccountId,
    pub privacy: AccountPrivacy,
    pub label: Option<String>,
    pub balance: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccountPrivacy {
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "private")]
    Private,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedAccountInfo {
    pub account_id: AccountId,
    pub privacy: AccountPrivacy,
    pub canonical_address: String,
    pub label: Option<String>,
    pub is_local: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionStatusView {
    pub tx_hash: String,
    pub status: String,
    pub block_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TokenHoldingView {
    pub holding_id: AccountId,
    pub definition_id: AccountId,
    pub symbol: String,
    pub name: String,
    pub is_shielded: bool,
    pub balance: Option<u128>,
}

/// The central SDK engine.
pub struct SdkEngine {
    pub(super) wallet: Arc<Mutex<Option<WalletCore<TestnetSequencerClient>>>>,
    pub(super) client: TestnetSequencerClient,
    pub(super) sessions: SessionStore,
    pub(super) requests: RequestStore,
    pub(super) tx_queue: TxQueue,
    pub(super) decoders: DecoderRegistry,
    pub(super) verifier: Box<dyn ProgramVerifier>,
    pub(super) resolver: Box<dyn NameResolver>,
    pub(super) store: Arc<Mutex<PersistentStore>>,
    pub(super) approval_handler: Arc<dyn ApprovalHandler>,
    pub(super) identity_secret: Arc<Mutex<Option<crate::identity::IdentitySecret>>>,
    pub(super) pow_manager: Arc<crate::pow::PowBridgeManager>,
    pub(super) submitted_tx_nonces:
        Arc<dashmap::DashMap<String, (AccountId, lee_core::account::Nonce)>>,
    pub(super) config: SdkConfig,
}

/// dApp-facing API .
pub trait DAppApi {
    fn sdk_info(&self) -> impl std::future::Future<Output = SdkResult<SdkInfo>> + Send;

    fn connect(
        &self,
        caller_identity: String,
        api_version: u32,
    ) -> impl std::future::Future<Output = SdkResult<SessionId>> + Send;

    fn disconnect(
        &self,
        session_id: &SessionId,
    ) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn request_accounts(
        &self,
        session_id: &SessionId,
    ) -> impl std::future::Future<Output = SdkResult<Vec<AccountView>>> + Send;

    fn list_accounts(
        &self,
        session_id: &SessionId,
    ) -> impl std::future::Future<Output = SdkResult<Vec<AccountView>>> + Send;

    fn get_balance(
        &self,
        session_id: &SessionId,
        account: &AccountId,
    ) -> impl std::future::Future<Output = SdkResult<u128>> + Send;

    fn propose_native_transfer(
        &self,
        session_id: &SessionId,
        from: AccountId,
        to: String,
        amount: u128,
        from_privacy: Option<AccountPrivacy>,
    ) -> impl std::future::Future<Output = SdkResult<RequestId>> + Send;

    fn propose_token_transfer(
        &self,
        session_id: &SessionId,
        from_holding: AccountId,
        to_holding: AccountId,
        definition: AccountId,
        amount: u128,
    ) -> impl std::future::Future<Output = SdkResult<RequestId>> + Send;

    fn propose_transaction(
        &self,
        session_id: &SessionId,
        mentions: Vec<AccountId>,
        calldata: Vec<u8>,
        description: Option<String>,
    ) -> impl std::future::Future<Output = SdkResult<RequestId>> + Send;

    fn propose_contract_call(
        &self,
        session_id: &SessionId,
        program_id: AccountId,
        payer_id: AccountId,
        mentions: Vec<AccountId>,
        calldata: Vec<u8>,
        description: Option<String>,
    ) -> impl std::future::Future<Output = SdkResult<RequestId>> + Send;

    fn list_token_holdings(
        &self,
        session_id: &SessionId,
    ) -> impl std::future::Future<Output = SdkResult<Vec<TokenHoldingView>>> + Send;

    fn get_request_status(
        &self,
        request_id: &RequestId,
    ) -> impl std::future::Future<Output = SdkResult<RequestStatus>> + Send;

    fn cancel_request(
        &self,
        request_id: &RequestId,
    ) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn resolve_name(
        &self,
        session_id: &SessionId,
        name: &str,
    ) -> impl std::future::Future<Output = SdkResult<crate::resolve::ResolvedDestination>> + Send;

    /// Resolve a `.lez` name directly from on-chain state, bypassing the cache.
    fn resolve_name_fresh(
        &self,
        session_id: &SessionId,
        name: &str,
    ) -> impl std::future::Future<Output = SdkResult<crate::resolve::ResolvedDestination>> + Send;

    /// Request identity permission grants (`IdentityRead` or `IdentityPropose`) for this session.
    fn request_identity_access(
        &self,
        session_id: &SessionId,
        kind: GrantKind,
    ) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    /// Retrieve the user's public identity commitment derived from the root seed.
    ///
    /// Requires `IdentityRead` grant.
    fn get_identity_commitment(
        &self,
        session_id: &SessionId,
    ) -> impl std::future::Future<Output = SdkResult<crate::identity::IdentityCommitment>> + Send;

    /// Derive a scope-specific pseudonymous nullifier for unlinkable actions across dApps.
    ///
    /// Requires `IdentityRead` grant.
    fn derive_scoped_nullifier(
        &self,
        session_id: &SessionId,
        scope_id: &str,
    ) -> impl std::future::Future<Output = SdkResult<crate::identity::ScopedNullifier>> + Send;

    /// Sign an arbitrary message (e.g. voting ballot or membership claim) using the scoped credential.
    ///
    /// Requires `IdentityPropose` grant.
    fn sign_identity_attestation(
        &self,
        session_id: &SessionId,
        scope_id: &str,
        message: &[u8],
    ) -> impl std::future::Future<Output = SdkResult<crate::identity::AttestationPayload>> + Send;

    /// Prepare an on-chain registration payload with SSS threshold shares encrypted to designated accountability trustees.
    ///
    /// Requires `IdentityPropose` grant.
    fn prepare_identity_registration(
        &self,
        session_id: &SessionId,
        scope_id: &str,
        node_pubkeys: &[[u8; 32]],
        threshold_k: u8,
    ) -> impl std::future::Future<Output = SdkResult<crate::identity::RegistrationPayload>> + Send;
}

/// Owner API  — privileged.
pub trait OwnerApi {
    fn create_wallet(
        &self,
        password: &str,
    ) -> impl std::future::Future<Output = SdkResult<Mnemonic>> + Send;

    fn restore_wallet(
        &self,
        mnemonic: &Mnemonic,
        password: &str,
    ) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn lock(&self) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn unlock(&self, password: &str) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn change_password(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn create_account_public(
        &self,
        label: Option<String>,
    ) -> impl std::future::Future<Output = SdkResult<AccountId>> + Send;

    fn create_account_private(
        &self,
        label: Option<String>,
    ) -> impl std::future::Future<Output = SdkResult<AccountId>> + Send;

    fn list_all_accounts(
        &self,
    ) -> impl std::future::Future<Output = SdkResult<Vec<AccountView>>> + Send;

    fn list_sessions(
        &self,
    ) -> impl std::future::Future<Output = SdkResult<Vec<crate::session::Session>>> + Send;

    fn revoke_session(
        &self,
        session_id: &SessionId,
    ) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn register_token_holding(
        &self,
        holding_id: AccountId,
        definition_id: AccountId,
        symbol: String,
        name: String,
        is_shielded: bool,
    ) -> impl std::future::Future<Output = SdkResult<()>> + Send;

    fn list_all_token_holdings(
        &self,
    ) -> impl std::future::Future<Output = SdkResult<Vec<TokenHoldingView>>> + Send;

    fn pow_start_mining(
        &self,
        threads: u32,
    ) -> impl std::future::Future<Output = SdkResult<crate::pow::PowStatusView>> + Send;

    fn pow_stop_mining(
        &self,
    ) -> impl std::future::Future<Output = SdkResult<crate::pow::PowStatusView>> + Send;

    fn pow_status(
        &self,
    ) -> impl std::future::Future<Output = SdkResult<crate::pow::PowStatusView>> + Send;

    fn channel_deposit(
        &self,
        recipient_account: &str,
        amount_str: &str,
    ) -> impl std::future::Future<Output = SdkResult<crate::pow::ChannelDepositResult>> + Send;

    fn get_transaction_status(
        &self,
        tx_hash: &str,
    ) -> impl std::future::Future<Output = SdkResult<TransactionStatusView>> + Send;
}

mod dapp;
mod engine;
mod owner;

pub(super) async fn finish_and_sync_transaction(
    w: &mut WalletCore<TestnetSequencerClient>,
    tx_hash: common::HashType,
) -> anyhow::Result<()> {
    let (tx, block_id) = w.poller().poll_tx(tx_hash).await?;
    if let common::transaction::LeeTransaction::PrivacyPreserving(pp_tx) = tx {
        let block = w
            .get_block(block_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Block {} including {} not found", block_id, tx_hash))?;
        let reverted = block
            .body
            .transactions
            .iter()
            .position(|included| included.hash() == tx_hash)
            .is_some_and(|position| block.body.is_reverted(position));
        let key_chain = w.storage_mut().key_chain_mut();
        let mut index = key_chain.build_latest_nullifier_index();
        key_chain.follow_transaction(&pp_tx.message, reverted, false, &mut index);
    }
    w.store_persistent_data()?;
    Ok(())
}

/// Decodes a token program actor state byte slice into a raw `u128` token balance.
/// Supports canonical 16-byte little-endian `u128` (`wrapped_token` / `native_token` layout),
/// 48-byte `TokenHolding` (`definition_id: [u8; 32]` + `balance: u128`), and 8-byte `u64`.
pub(super) fn decode_token_actor_balance(data: &[u8]) -> u128 {
    if data.is_empty() {
        return 0;
    }
    if data.len() == 16 {
        return u128::from_le_bytes(data.try_into().unwrap());
    }
    if data.len() == 48 {
        return u128::from_le_bytes(data[32..48].try_into().unwrap());
    }
    if data.len() == 8 {
        return u64::from_le_bytes(data.try_into().unwrap()) as u128;
    }
    0
}

pub(crate) fn is_private_account_local(
    kc: &wallet::storage::key_chain::UserKeyChain,
    account_id: AccountId,
) -> bool {
    kc.private_account(account_id).is_some()
        || kc.managed_private_account(account_id).is_some()
        || kc.shared_private_account(account_id).is_some()
}

pub(super) fn build_wallet_client(
    config_path: &std::path::Path,
    overrides: &WalletConfigOverrides,
) -> SdkResult<TestnetSequencerClient> {
    let mut wallet_config =
        WalletConfig::from_path_or_initialize_default(config_path).map_err(SdkError::Internal)?;
    wallet_config.apply_overrides(overrides.clone());
    let inner_client =
        WalletCore::client_from_config(&wallet_config).map_err(SdkError::Internal)?;
    Ok(TestnetSequencerClient::new(inner_client))
}

pub(super) async fn ensure_private_sync_tip(wallet: &mut WalletCore<TestnetSequencerClient>) {
    if wallet.storage().last_synced_block() == 0 {
        let any_initialized = wallet
            .storage()
            .key_chain()
            .private_accounts()
            .any(|found| *found.account != lee::Account::default());
        if !any_initialized && wallet.start_at_chain_tip().await.is_ok() {
            let _ = wallet.store_persistent_data();
        }
    }
}

pub(super) fn resolve_account_label(
    wallet: &WalletCore<TestnetSequencerClient>,
    custom_labels: &HashMap<String, String>,
    account_id: AccountId,
    privacy: &AccountPrivacy,
) -> Option<String> {
    custom_labels
        .get(&account_id.to_string())
        .cloned()
        .or_else(|| {
            let target_privacy = match privacy {
                AccountPrivacy::Public => AccountIdWithPrivacy::Public(account_id),
                AccountPrivacy::Private => AccountIdWithPrivacy::Private(account_id),
            };
            wallet
                .storage()
                .labels_for_account(target_privacy)
                .next()
                .map(|l| l.as_ref().to_string())
        })
}

pub(super) fn parse_account_id(input: &str) -> SdkResult<AccountId> {
    let trimmed = input.trim();
    let clean = trimmed
        .strip_prefix("Public/")
        .or_else(|| trimmed.strip_prefix("public/"))
        .or_else(|| trimmed.strip_prefix("Private/"))
        .or_else(|| trimmed.strip_prefix("private/"))
        .unwrap_or(trimmed);
    clean
        .parse::<AccountId>()
        .map_err(|e| SdkError::InvalidArgument(format!("invalid account id '{}': {}", input, e)))
}
