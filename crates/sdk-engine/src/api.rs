//! SDK API surface.
//!
//! [`SdkEngine`] is the central orchestrator. It owns `WalletCore`,
//! session store, request store, tx queue, decoder registry, verifier, and
//! resolver. Two API surfaces:
//!
//! - **[`DAppApi`]** — callable by third-party dApps via sessions.
//! - **[`OwnerApi`]** — callable only by the wallet UI (privileged).

use bip39::Mnemonic;
use lee::AccountId;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use wallet::account::{AccountIdWithPrivacy, Label};
use wallet::config::WalletConfig;
use wallet::{WalletCore, WalletSequencerClient};

use crate::approval::{ApprovalDecision, ApprovalHandler, ApprovalPrompt};
use crate::client::{SequencerProtocolVersion, TestnetSequencerClient};
use crate::decode::DecoderRegistry;
use crate::effects::EffectSimulator;
use crate::error::{SdkError, SdkResult};
use crate::queue::TxQueue;
use crate::request::{PendingRequest, RequestId, RequestKind, RequestStatus, RequestStore};
use crate::resolve::NameResolver;
use crate::resolve::lns::{LnsConfig, LnsResolver};
use crate::session::{GrantKind, SessionId, SessionStore};
use crate::store::PersistentStore;
use crate::v03_compat;
use crate::verifier::{BuiltinVerifier, ProgramVerifier};

const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(24 * 3600);
const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Canonical default gas limit for standard transactions in EVICE WALLET SDK.
/// Aligns with standard LEZ execution limits for native and token transfers.
pub const DEFAULT_WALLET_GAS_LIMIT: u64 = 34_000;

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
}

/// The central SDK engine.
pub struct SdkEngine {
    wallet: Arc<Mutex<WalletCore<TestnetSequencerClient>>>,
    sessions: SessionStore,
    requests: RequestStore,
    tx_queue: TxQueue,
    decoders: DecoderRegistry,
    verifier: Box<dyn ProgramVerifier>,
    resolver: Box<dyn NameResolver>,
    store: Arc<Mutex<PersistentStore>>,
    approval_handler: Arc<dyn ApprovalHandler>,
    identity_secret: Arc<Mutex<Option<crate::identity::IdentitySecret>>>,
    pow_manager: Arc<crate::pow::PowBridgeManager>,
    config: SdkConfig,
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

impl SdkEngine {
    pub fn new(
        wallet: WalletCore<TestnetSequencerClient>,
        approval_handler: Arc<dyn ApprovalHandler>,
        config: SdkConfig,
    ) -> SdkResult<Self> {
        let store =
            PersistentStore::open(config.sdk_state_path.clone()).map_err(SdkError::Internal)?;

        let lns_enabled = store.state().lns_enabled;
        let verification_enabled = store.state().program_verification_enabled;
        let initial_blocks_mined = store.state().pow_blocks_mined;
        let initial_unbridged_balance = store.state().unbridged_l1_balance;

        let mut lns_config = config.lns_config.clone();
        lns_config.enabled = lns_enabled;

        Ok(Self {
            wallet: Arc::new(Mutex::new(wallet)),
            sessions: SessionStore::new(config.session_ttl),
            requests: RequestStore::new(),
            tx_queue: TxQueue::new(),
            decoders: DecoderRegistry::with_builtins(),
            verifier: Box::new(BuiltinVerifier::new(verification_enabled)),
            resolver: Box::new(LnsResolver::new(lns_config)),
            store: Arc::new(Mutex::new(store)),
            approval_handler,
            identity_secret: Arc::new(Mutex::new(None)),
            pow_manager: Arc::new(crate::pow::PowBridgeManager::new(
                initial_blocks_mined,
                initial_unbridged_balance,
            )),
            config,
        })
    }

    async fn validate_session(&self, session_id: &SessionId) -> SdkResult<crate::session::Session> {
        self.sessions
            .get(session_id)
            .await
            .ok_or_else(|| SdkError::SessionNotFound(session_id.clone()))
    }

    async fn require_grant(
        &self,
        session_id: &SessionId,
        account: &AccountId,
        kind: GrantKind,
    ) -> SdkResult<crate::session::Session> {
        let session = self.validate_session(session_id).await?;
        if !session.has_grant(account, kind) {
            return Err(SdkError::InsufficientGrant);
        }
        Ok(session)
    }

    async fn require_identity_grant(
        &self,
        session_id: &SessionId,
        kind: GrantKind,
    ) -> SdkResult<crate::session::Session> {
        let session = self.validate_session(session_id).await?;
        if !session.has_identity_grant(kind) {
            return Err(SdkError::InsufficientGrant);
        }
        Ok(session)
    }

    pub async fn set_identity_secret(&self, secret: Option<crate::identity::IdentitySecret>) {
        *self.identity_secret.lock().await = secret;
    }

    fn derive_identity_from_wallet(
        wallet: &WalletCore<TestnetSequencerClient>,
    ) -> Option<crate::identity::IdentitySecret> {
        let key_chain = wallet.storage().key_chain();
        let first_pub_id = key_chain.public_account_ids().next().map(|(id, _)| id)?;
        let signing_key = wallet.get_account_public_signing_key(first_pub_id)?;
        let key_bytes = serde_json::to_vec(signing_key).ok()?;
        Some(crate::identity::derive_identity_secret(&key_bytes))
    }

    async fn resolve_destination(&self, input: &str) -> SdkResult<AccountId> {
        match self.resolver.resolve(input).await {
            Ok(Some(dest)) => {
                if let Some(account) = dest.account {
                    Ok(account)
                } else {
                    Err(SdkError::InvalidArgument(format!(
                        "Name '{}' resolved but has no public account; private .lez transfers not yet supported",
                        dest.name
                    )))
                }
            }
            Ok(None) => parse_account_id(input),
            Err(crate::resolve::ResolveError::Disabled) if !input.ends_with(".lez") => {
                parse_account_id(input)
            }
            Err(e) => Err(SdkError::NameResolutionError(e.to_string())),
        }
    }

    /// Check whether a persistent wallet storage file already exists in the given directory.
    pub fn wallet_exists(data_dir: &std::path::Path) -> bool {
        let config = SdkConfig::new(data_dir.to_path_buf());
        config.wallet_storage_path.exists()
    }

    pub fn open_or_init(
        data_dir: PathBuf,
        password: &str,
        approval_handler: Arc<dyn ApprovalHandler>,
    ) -> SdkResult<Self> {
        let config = SdkConfig::new(data_dir.clone());
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir).map_err(|e| SdkError::Internal(e.into()))?;
        }

        let mut store =
            PersistentStore::open(config.sdk_state_path.clone()).map_err(SdkError::Internal)?;
        let effective_gas_limit = store
            .state()
            .gas_limit_override
            .unwrap_or(DEFAULT_WALLET_GAS_LIMIT);
        let overrides = wallet::config::WalletConfigOverrides {
            gas_limit: Some(effective_gas_limit),
            ..Default::default()
        };

        let mut wallet_config =
            WalletConfig::from_path_or_initialize_default(&config.wallet_config_path)
                .map_err(SdkError::Internal)?;
        wallet_config.apply_overrides(overrides.clone());

        let inner_client =
            WalletCore::client_from_config(&wallet_config).map_err(SdkError::Internal)?;
        let client = TestnetSequencerClient::new(inner_client);

        let (wallet, initial_identity) = if config.wallet_storage_path.exists() {
            match WalletCore::new_update_chain_with_client(
                config.wallet_config_path.clone(),
                config.wallet_storage_path.clone(),
                Some(overrides.clone()),
                password,
                client.clone(),
            ) {
                Ok(w) => {
                    let id = if !store.state().locked {
                        Self::derive_identity_from_wallet(&w)
                    } else {
                        None
                    };
                    (w, id)
                }
                Err(e) if password.is_empty() => {
                    log::info!(
                        "Encrypted wallet storage detected at startup; starting in locked state ({})",
                        e
                    );
                    store.state_mut().locked = true;
                    let _ = store.save();
                    let (placeholder, _mnemonic) = WalletCore::new_init_storage_with_client(
                        config.wallet_config_path.clone(),
                        config.wallet_storage_path.clone(),
                        Some(overrides),
                        "",
                        client,
                    )
                    .map_err(SdkError::Internal)?;
                    (placeholder, None)
                }
                Err(e) => return Err(SdkError::Internal(e)),
            }
        } else if password.is_empty() {
            return Err(SdkError::WalletNotInitialized);
        } else {
            let (wallet, _mnemonic) = WalletCore::new_init_storage_with_client(
                config.wallet_config_path.clone(),
                config.wallet_storage_path.clone(),
                Some(overrides),
                password,
                client,
            )
            .map_err(SdkError::Internal)?;
            let id = Self::derive_identity_from_wallet(&wallet);
            (wallet, id)
        };

        let engine = Self::new(wallet, approval_handler, config)?;
        if let (Some(secret), Ok(mut guard)) = (initial_identity, engine.identity_secret.try_lock())
        {
            *guard = Some(secret);
        }
        Ok(engine)
    }

    pub async fn get_gas_limit(&self) -> u64 {
        let store = self.store.lock().await;
        if let Some(limit) = store.state().gas_limit_override {
            limit
        } else {
            let wallet = self.wallet.lock().await;
            wallet.config().gas_limit
        }
    }

    pub async fn set_gas_limit(&self, limit: u64) -> SdkResult<()> {
        {
            let mut store = self.store.lock().await;
            store.state_mut().gas_limit_override = Some(limit);
            store.save().map_err(SdkError::Internal)?;
        }
        {
            let mut wallet = self.wallet.lock().await;
            let mut cfg = wallet.config().clone();
            cfg.gas_limit = limit;
            wallet.set_config(cfg);
            if let Err(e) = wallet.store_config_changes().await {
                log::warn!("Failed to persist updated wallet config to disk: {:?}", e);
            }
        }
        Ok(())
    }

    pub async fn grant_account(
        &self,
        session_id: &SessionId,
        account: AccountId,
        kind: GrantKind,
    ) -> SdkResult<()> {
        self.sessions
            .update(session_id, |s| {
                s.add_grant(account, kind);
            })
            .await
            .ok_or_else(|| SdkError::SessionNotFound(session_id.clone()))?;
        Ok(())
    }

    pub async fn set_account_label(
        &self,
        account: AccountId,
        label: String,
        is_private: bool,
    ) -> SdkResult<()> {
        let mut wallet = self.wallet.lock().await;
        let privacy_account = if is_private {
            AccountIdWithPrivacy::Private(account)
        } else {
            AccountIdWithPrivacy::Public(account)
        };
        let _ = wallet
            .storage_mut()
            .add_label(Label::new(label.clone()), privacy_account);
        let _ = wallet.store_persistent_data();

        let mut store = self.store.lock().await;
        store
            .state_mut()
            .account_labels
            .insert(account.to_string(), label);
        store.save().map_err(SdkError::Internal)?;
        Ok(())
    }

    pub async fn resolve_account(&self, input: &str) -> SdkResult<ResolvedAccountInfo> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(SdkError::InvalidArgument("Empty account address".into()));
        }

        // 1. Check LNS domain name
        if trimmed.ends_with(".lez") {
            let dest = self
                .resolver
                .resolve(trimmed)
                .await
                .map_err(|e| SdkError::NameResolutionError(e.to_string()))?
                .ok_or_else(|| {
                    SdkError::InvalidArgument(format!("Name '{}' not found", trimmed))
                })?;
            let account_id = dest.account.ok_or_else(|| {
                SdkError::InvalidArgument(format!("Name '{}' has no account record", trimmed))
            })?;
            return Ok(ResolvedAccountInfo {
                account_id,
                privacy: AccountPrivacy::Public,
                canonical_address: format!("Public/{}", account_id),
                label: Some(trimmed.to_string()),
                is_local: false,
            });
        }

        // 2. Check explicit prefix
        let (raw_str, explicit_privacy) = if let Some(stripped) = trimmed
            .strip_prefix("Public/")
            .or_else(|| trimmed.strip_prefix("public/"))
        {
            (stripped, Some(AccountPrivacy::Public))
        } else if let Some(stripped) = trimmed
            .strip_prefix("Private/")
            .or_else(|| trimmed.strip_prefix("private/"))
        {
            (stripped, Some(AccountPrivacy::Private))
        } else {
            (trimmed, None)
        };

        let account_id = raw_str.parse::<AccountId>().map_err(|e| {
            SdkError::InvalidArgument(format!("Invalid account id '{}': {}", trimmed, e))
        })?;

        let wallet = self.wallet.lock().await;
        let store = self.store.lock().await;
        let custom_labels = &store.state().account_labels;

        let is_private_local = wallet
            .storage()
            .key_chain()
            .private_account(account_id)
            .is_some()
            || wallet
                .storage()
                .key_chain()
                .shared_private_account(account_id)
                .is_some();
        let is_public_local = wallet
            .storage()
            .key_chain()
            .pub_account_signing_key(account_id)
            .is_some();

        let privacy = if let Some(p) = explicit_privacy {
            p
        } else if is_private_local {
            AccountPrivacy::Private
        } else if is_public_local {
            AccountPrivacy::Public
        } else {
            // Check if account has public representation on-chain
            if let Ok(Some(_)) = wallet.get_account_public(account_id).await {
                AccountPrivacy::Public
            } else {
                AccountPrivacy::Public
            }
        };

        let is_local = is_private_local || is_public_local;
        let prefix = match privacy {
            AccountPrivacy::Public => "Public",
            AccountPrivacy::Private => "Private",
        };
        let canonical_address = format!("{}/{}", prefix, account_id);

        let label = custom_labels
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
            });

        Ok(ResolvedAccountInfo {
            account_id,
            privacy,
            canonical_address,
            label,
            is_local,
        })
    }

    pub fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    pub fn requests(&self) -> &RequestStore {
        &self.requests
    }

    pub fn approval_handler(&self) -> &Arc<dyn ApprovalHandler> {
        &self.approval_handler
    }

    pub fn wallet(&self) -> &Arc<Mutex<WalletCore<TestnetSequencerClient>>> {
        &self.wallet
    }

    pub fn config(&self) -> &SdkConfig {
        &self.config
    }

    /// Clear the entire LNS name resolution cache.
    pub fn clear_lns_cache(&self) {
        self.resolver.clear_cache();
    }

    /// Evict a specific LNS name from the resolution cache immediately.
    pub fn evict_lns_name(&self, name: &str) {
        self.resolver.evict(name);
    }
}

impl DAppApi for SdkEngine {
    async fn sdk_info(&self) -> SdkResult<SdkInfo> {
        let store = self.store.lock().await;
        Ok(SdkInfo {
            sdk_version: crate::SDK_VERSION.to_string(),
            api_version: crate::API_VERSION,
            network: "testnet-0.3-0.4".to_string(),
            wallet_initialized: true,
            wallet_locked: store.state().locked,
        })
    }

    async fn connect(&self, caller_identity: String, api_version: u32) -> SdkResult<SessionId> {
        {
            let store = self.store.lock().await;
            if store.state().locked {
                return Err(SdkError::WalletLocked);
            }
        }
        let is_internal_ui = caller_identity == "internal_wallet_ui";
        let session = self.sessions.create(caller_identity, api_version).await;
        if is_internal_ui {
            let owned_ids: Vec<AccountId> = {
                let wallet = self.wallet.lock().await;
                let kc = wallet.storage().key_chain();
                kc.public_account_ids()
                    .map(|(id, _)| id)
                    .chain(kc.private_account_ids().map(|(id, _)| id))
                    .collect()
            };
            self.sessions
                .update(&session.id, |s| {
                    s.add_identity_grant(GrantKind::IdentityRead);
                    s.add_identity_grant(GrantKind::IdentityPropose);
                    for acc in &owned_ids {
                        s.add_grant(*acc, GrantKind::Read);
                        s.add_grant(*acc, GrantKind::Propose);
                    }
                })
                .await;
        }
        log::info!("Session created: {}", session.id);
        Ok(session.id)
    }

    async fn disconnect(&self, session_id: &SessionId) -> SdkResult<()> {
        if self.sessions.revoke(session_id).await {
            Ok(())
        } else {
            Err(SdkError::SessionNotFound(session_id.clone()))
        }
    }

    async fn request_accounts(&self, session_id: &SessionId) -> SdkResult<Vec<AccountView>> {
        let session = self.validate_session(session_id).await?;
        let all_accounts = self.list_all_accounts().await?;

        let selected = self
            .approval_handler
            .request_account_selection(
                session.caller_identity.clone(),
                all_accounts,
                self.config.approval_timeout,
            )
            .await;

        for acc in &selected {
            self.sessions
                .update(session_id, |s| {
                    s.add_grant(*acc, GrantKind::Read);
                    s.add_grant(*acc, GrantKind::Propose);
                })
                .await;
        }

        self.list_accounts(session_id).await
    }

    async fn list_accounts(&self, session_id: &SessionId) -> SdkResult<Vec<AccountView>> {
        let session = self.validate_session(session_id).await?;
        let granted = session.granted_accounts();
        let wallet = self.wallet.lock().await;

        let mut views = Vec::new();
        for account_id in granted {
            let balance = wallet.get_account_balance(account_id).await.ok();
            views.push(AccountView {
                account_id,
                privacy: AccountPrivacy::Public,
                label: None,
                balance,
            });
        }
        Ok(views)
    }

    async fn get_balance(&self, session_id: &SessionId, account: &AccountId) -> SdkResult<u128> {
        self.require_grant(session_id, account, GrantKind::Read)
            .await?;
        let wallet = self.wallet.lock().await;
        if wallet
            .storage()
            .key_chain()
            .private_account(*account)
            .is_some()
            || wallet
                .storage()
                .key_chain()
                .shared_private_account(*account)
                .is_some()
        {
            let shielded = wallet
                .private_account_state(*account)
                .and_then(|acc| acc.data.native_balance().ok())
                .unwrap_or(0);
            Ok(shielded)
        } else {
            wallet
                .get_account_balance(*account)
                .await
                .map_err(SdkError::Internal)
        }
    }

    async fn propose_native_transfer(
        &self,
        session_id: &SessionId,
        from: AccountId,
        to: String,
        amount: u128,
        from_privacy: Option<AccountPrivacy>,
    ) -> SdkResult<RequestId> {
        self.require_grant(session_id, &from, GrantKind::Propose)
            .await?;

        if amount == 0 {
            return Err(SdkError::InvalidArgument(
                "Transfer amount must be greater than zero".into(),
            ));
        }

        let to_trimmed = to.trim();
        let is_to_explicit_private = to_trimmed.to_lowercase().starts_with("private/");
        let is_to_explicit_public = to_trimmed.to_lowercase().starts_with("public/");

        let to_account = self.resolve_destination(&to).await?;

        let (is_from_private, is_to_private) = {
            let wallet = self.wallet.lock().await;
            let from_priv = match from_privacy {
                Some(AccountPrivacy::Private) => true,
                Some(AccountPrivacy::Public) => false,
                None => {
                    wallet
                        .storage()
                        .key_chain()
                        .managed_private_account(from)
                        .is_some()
                        || wallet.storage().key_chain().private_account(from).is_some()
                }
            };
            let to_priv = if is_to_explicit_private {
                true
            } else if is_to_explicit_public {
                false
            } else {
                wallet
                    .storage()
                    .key_chain()
                    .managed_private_account(to_account)
                    .is_some()
                    || wallet
                        .storage()
                        .key_chain()
                        .private_account(to_account)
                        .is_some()
            };
            (from_priv, to_priv)
        };

        if from == to_account && is_from_private == is_to_private {
            return Err(SdkError::InvalidArgument(format!(
                "Cannot transfer to the same account: {}",
                from
            )));
        }

        let request = PendingRequest::new(
            session_id.clone(),
            RequestKind::NativeTransfer {
                from,
                to: to_account,
                amount,
            },
        );
        let request_id = request.id.clone();

        let current_gas_limit = self.get_gas_limit().await;
        let summary = ApprovalPrompt::auto_summary(&request);
        let effects = EffectSimulator::native_transfer(from, to_account, amount, current_gas_limit);

        let verification = self
            .verifier
            .verify(&lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID)
            .await;

        let prompt = ApprovalPrompt {
            request: request.clone(),
            summary,
            dapp_identity: self
                .sessions
                .get(session_id)
                .await
                .map(|s| s.caller_identity.clone())
                .unwrap_or_else(|| "unknown".into()),
            effects: Some(format!("{:?}", effects.source)),
            verification_status: Some(verification.to_string()),
            warnings: effects.warnings,
        };

        self.requests.insert(request).await;
        self.tx_queue
            .enqueue(crate::queue::QueueEntry {
                request_id: request_id.clone(),
                priority: 0,
            })
            .await;

        let handler = self.approval_handler.clone();
        let requests = self.requests.clone();
        let wallet = self.wallet.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let mut w = wallet.lock().await;

                    let client = w.client().clone();
                    let protocol_version = client.detect_protocol_version().await;

                    if protocol_version == SequencerProtocolVersion::V03ShardModel
                        && !is_from_private
                        && !is_to_private
                    {
                        let signing_key = match w.get_account_public_signing_key(from) {
                            Some(k) => k.clone(),
                            None => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!(
                                                "No public signing key found for account {}",
                                                from
                                            ),
                                        },
                                    )
                                    .await;
                                return;
                            }
                        };

                        let nonce = match w.get_accounts_nonces(&[from]).await {
                            Ok(nonces) => nonces
                                .into_iter()
                                .next()
                                .unwrap_or(lee_core::account::Nonce(0)),
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!("Failed to fetch account nonce: {}", e),
                                        },
                                    )
                                    .await;
                                return;
                            }
                        };

                        let quote = client.get_fee_state().await.unwrap_or(
                            sequencer_service_rpc::FeeStateQuote {
                                height: 0,
                                base_fee_exec: 8,
                                base_fee_stor: 8,
                                next_base_fee_exec_floor: 8,
                                next_base_fee_exec_ceiling: 9,
                                next_base_fee_stor_floor: 8,
                                next_base_fee_stor_ceiling: 9,
                                max_gas_exec: 10_000_000,
                                max_gas_stor: 1_000_000,
                            },
                        );

                        let gas_limit = w.config().gas_limit;

                        match v03_compat::build_and_sign_v03_native_transfer(
                            from,
                            to_account,
                            amount,
                            nonce,
                            &signing_key,
                            gas_limit,
                            quote.base_fee_exec,
                        ) {
                            Ok((payload, tx_hash)) => {
                                match client.send_raw_transaction(payload).await {
                                    Ok(returned_hash) => {
                                        let final_hash =
                                            if returned_hash == common::HashType::default() {
                                                tx_hash
                                            } else {
                                                returned_hash
                                            };
                                        requests
                                            .set_status(
                                                &rid,
                                                RequestStatus::Submitted {
                                                    tx_hash: final_hash,
                                                },
                                            )
                                            .await;
                                    }
                                    Err(e) => {
                                        requests
                                            .set_status(
                                                &rid,
                                                RequestStatus::Failed {
                                                    reason: e.to_string(),
                                                },
                                            )
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!(
                                                "Failed to construct v0.3 transfer: {}",
                                                e
                                            ),
                                        },
                                    )
                                    .await;
                            }
                        }
                    } else {
                        if w.storage().last_synced_block() == 0 {
                            let any_initialized = w
                                .storage()
                                .key_chain()
                                .private_accounts()
                                .any(|found| *found.account != lee::Account::default());
                            if !any_initialized && w.start_at_chain_tip().await.is_ok() {
                                let _ = w.store_persistent_data();
                            }
                        }
                        if let Ok(tip) = w.get_last_block_id().await {
                            let last_synced = w.storage().last_synced_block();
                            if tip > last_synced && tip.saturating_sub(last_synced) <= 64 {
                                let _ = w.sync_to_latest_block().await;
                            }
                        }

                        let sender = if is_from_private {
                            wallet::AccountIdentity::PrivateOwned(from)
                        } else {
                            wallet::AccountIdentity::Public(from)
                        };

                        let recipient = if is_to_private {
                            wallet::AccountIdentity::PrivateOwned(to_account)
                        } else {
                            wallet::AccountIdentity::PublicNoSign(to_account)
                        };

                        let transfer_res = {
                            let facade =
                                wallet::program_facades::native_token_transfer::NativeTokenTransfer(
                                    &w,
                                );
                            facade
                                .transfer(
                                    sender,
                                    recipient,
                                    amount,
                                    wallet::program_facades::CreditDelivery::Automatic,
                                )
                                .await
                        };
                        match transfer_res {
                            Ok((tx_hash, _secrets)) => {
                                if let Err(e) = finish_and_sync_transaction(&mut w, tx_hash).await {
                                    log::warn!(
                                        "finish_and_sync_transaction warning for {}: {:?}",
                                        tx_hash,
                                        e
                                    );
                                    let _ = w.sync_to_latest_block().await;
                                }
                                requests
                                    .set_status(&rid, RequestStatus::Submitted { tx_hash })
                                    .await;
                            }
                            Err(e) => {
                                let raw_err = format!("{:?}", e);
                                let reason = if raw_err.contains("Fee reserve of")
                                    && raw_err.contains("holds less than the transferred amount")
                                {
                                    format!(
                                        "{} (Hint: When shielding to a fresh 0-balance private account, the transfer amount must exceed the private fee reserve of ~5,070,616 raw units)",
                                        raw_err
                                    )
                                } else {
                                    raw_err
                                };
                                requests
                                    .set_status(&rid, RequestStatus::Failed { reason })
                                    .await;
                            }
                        }
                    }
                }
                ApprovalDecision::Reject => {
                    requests.set_status(&rid, RequestStatus::Rejected).await;
                }
            }
        });

        Ok(request_id)
    }

    async fn propose_token_transfer(
        &self,
        session_id: &SessionId,
        from_holding: AccountId,
        to_holding: AccountId,
        definition: AccountId,
        amount: u128,
    ) -> SdkResult<RequestId> {
        self.require_grant(session_id, &from_holding, GrantKind::Propose)
            .await?;

        if amount == 0 {
            return Err(SdkError::InvalidArgument(
                "Transfer amount must be greater than zero".into(),
            ));
        }

        if from_holding == to_holding {
            let wallet = self.wallet.lock().await;
            let is_from_private = wallet
                .storage()
                .key_chain()
                .managed_private_account(from_holding)
                .is_some()
                || wallet
                    .storage()
                    .key_chain()
                    .private_account(from_holding)
                    .is_some();
            let is_to_private = wallet
                .storage()
                .key_chain()
                .managed_private_account(to_holding)
                .is_some()
                || wallet
                    .storage()
                    .key_chain()
                    .private_account(to_holding)
                    .is_some();
            if is_from_private == is_to_private {
                return Err(SdkError::InvalidArgument(format!(
                    "Cannot transfer to the same holding account: {}",
                    from_holding
                )));
            }
        }

        let request = PendingRequest::new(
            session_id.clone(),
            RequestKind::TokenTransfer {
                from_holding,
                to_holding,
                definition,
                amount,
            },
        );
        let request_id = request.id.clone();

        let current_gas_limit = self.get_gas_limit().await;
        let summary = ApprovalPrompt::auto_summary(&request);
        let effects = EffectSimulator::token_transfer(
            from_holding,
            to_holding,
            definition,
            amount,
            current_gas_limit,
        );

        let verification = self.verifier.verify(&definition).await;

        let prompt = ApprovalPrompt {
            request: request.clone(),
            summary,
            dapp_identity: self
                .sessions
                .get(session_id)
                .await
                .map(|s| s.caller_identity.clone())
                .unwrap_or_else(|| "unknown".into()),
            effects: Some(format!("{:?}", effects.source)),
            verification_status: Some(verification.to_string()),
            warnings: effects.warnings,
        };

        self.requests.insert(request).await;
        self.tx_queue
            .enqueue(crate::queue::QueueEntry {
                request_id: request_id.clone(),
                priority: 0,
            })
            .await;

        let handler = self.approval_handler.clone();
        let requests = self.requests.clone();
        let wallet = self.wallet.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let w = wallet.lock().await;
                    let is_from_private = w
                        .storage()
                        .key_chain()
                        .managed_private_account(from_holding)
                        .is_some()
                        || w.storage()
                            .key_chain()
                            .private_account(from_holding)
                            .is_some();
                    let is_to_private = w
                        .storage()
                        .key_chain()
                        .managed_private_account(to_holding)
                        .is_some()
                        || w.storage()
                            .key_chain()
                            .private_account(to_holding)
                            .is_some();

                    let sender_actor = if is_from_private {
                        wallet::AccountIdentity::PrivateOwned(from_holding)
                            .select_program_actor_state(definition)
                    } else {
                        wallet::AccountIdentity::Public(from_holding)
                            .select_program_actor_state(definition)
                    };

                    let recipient_actor = if is_to_private {
                        wallet::AccountIdentity::PrivateOwned(to_holding)
                            .select_program_actor_state(definition)
                    } else {
                        wallet::AccountIdentity::PublicNoSign(to_holding)
                            .select_program_actor_state(definition)
                    };

                    let client = w.client().clone();
                    let protocol_version = client.detect_protocol_version().await;

                    if protocol_version == SequencerProtocolVersion::V03ShardModel
                        && !is_from_private
                        && !is_to_private
                    {
                        let signing_key = match w.get_account_public_signing_key(from_holding) {
                            Some(k) => k.clone(),
                            None => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!(
                                                "No public signing key found for account {}",
                                                from_holding
                                            ),
                                        },
                                    )
                                    .await;
                                return;
                            }
                        };

                        let nonce = match w.get_accounts_nonces(&[from_holding]).await {
                            Ok(nonces) => nonces
                                .into_iter()
                                .next()
                                .unwrap_or(lee_core::account::Nonce(0)),
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!("Failed to fetch account nonce: {}", e),
                                        },
                                    )
                                    .await;
                                return;
                            }
                        };

                        let quote = client.get_fee_state().await.unwrap_or(
                            sequencer_service_rpc::FeeStateQuote {
                                height: 0,
                                base_fee_exec: 8,
                                base_fee_stor: 8,
                                next_base_fee_exec_floor: 8,
                                next_base_fee_exec_ceiling: 9,
                                next_base_fee_stor_floor: 8,
                                next_base_fee_stor_ceiling: 9,
                                max_gas_exec: 10_000_000,
                                max_gas_stor: 1_000_000,
                            },
                        );

                        let gas_limit = w.config().gas_limit;
                        let mentions = vec![to_holding];
                        let calldata = amount.to_le_bytes().to_vec();

                        match v03_compat::build_and_sign_v03_contract_call(
                            definition,
                            from_holding,
                            nonce,
                            &signing_key,
                            &mentions,
                            calldata,
                            gas_limit,
                            quote.base_fee_exec,
                        ) {
                            Ok((payload, tx_hash)) => {
                                match client.send_raw_transaction(payload).await {
                                    Ok(returned_hash) => {
                                        let final_hash =
                                            if returned_hash == common::HashType::default() {
                                                tx_hash
                                            } else {
                                                returned_hash
                                            };
                                        requests
                                            .set_status(
                                                &rid,
                                                RequestStatus::Submitted {
                                                    tx_hash: final_hash,
                                                },
                                            )
                                            .await;
                                    }
                                    Err(e) => {
                                        requests
                                            .set_status(
                                                &rid,
                                                RequestStatus::Failed {
                                                    reason: e.to_string(),
                                                },
                                            )
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!(
                                                "Failed to construct v0.3 token transfer: {}",
                                                e
                                            ),
                                        },
                                    )
                                    .await;
                            }
                        }
                    } else {
                        let accounts = vec![sender_actor, recipient_actor];
                        let message = amount.to_le_bytes().to_vec();
                        match w.send_pub_tx(accounts, 0, message).await {
                            Ok(tx_hash) => {
                                requests
                                    .set_status(&rid, RequestStatus::Submitted { tx_hash })
                                    .await;
                            }
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!("{:?}", e),
                                        },
                                    )
                                    .await;
                            }
                        }
                    }
                }
                ApprovalDecision::Reject => {
                    requests.set_status(&rid, RequestStatus::Rejected).await;
                }
            }
        });

        Ok(request_id)
    }

    async fn propose_transaction(
        &self,
        session_id: &SessionId,
        mentions: Vec<AccountId>,
        calldata: Vec<u8>,
        description: Option<String>,
    ) -> SdkResult<RequestId> {
        if let Some(primary) = mentions.first() {
            self.require_grant(session_id, primary, GrantKind::Propose)
                .await?;
        } else {
            return Err(SdkError::InvalidArgument(
                "Transaction must mention at least one account".into(),
            ));
        }

        let request = PendingRequest::new(
            session_id.clone(),
            RequestKind::GenericCall {
                mentions: mentions.clone(),
                calldata: calldata.clone(),
                description,
            },
        );
        let request_id = request.id.clone();

        let decoded = mentions
            .first()
            .map(|primary| self.decoders.decode(primary, &calldata));

        let summary = if let Some(ref d) = decoded {
            if d.operation != "Unknown" {
                format!("{}: {}", d.program, d.operation)
            } else {
                ApprovalPrompt::auto_summary(&request)
            }
        } else {
            ApprovalPrompt::auto_summary(&request)
        };

        let effects_source = if let Some(ref d) = decoded {
            if d.operation != "Unknown" {
                format!("Decoded({})", d.operation)
            } else {
                format!(
                    "{:?}",
                    EffectSimulator::unavailable(wallet::DEFAULT_GAS_LIMIT).source
                )
            }
        } else {
            format!(
                "{:?}",
                EffectSimulator::unavailable(wallet::DEFAULT_GAS_LIMIT).source
            )
        };

        let verification = if let Some(primary) = mentions.first() {
            Some(self.verifier.verify(primary).await.to_string())
        } else {
            None
        };

        let prompt = ApprovalPrompt {
            request: request.clone(),
            summary,
            dapp_identity: self
                .sessions
                .get(session_id)
                .await
                .map(|s| s.caller_identity.clone())
                .unwrap_or_else(|| "unknown".into()),
            effects: Some(effects_source),
            verification_status: verification,
            warnings: vec![],
        };

        self.requests.insert(request).await;
        self.tx_queue
            .enqueue(crate::queue::QueueEntry {
                request_id: request_id.clone(),
                priority: 0,
            })
            .await;

        let handler = self.approval_handler.clone();
        let requests = self.requests.clone();
        let wallet = self.wallet.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;
        let mentions_to_send = mentions.clone();
        let calldata_to_send = calldata.clone();

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let w = wallet.lock().await;
                    if let Some(primary) = mentions_to_send.first() {
                        let program = *primary;
                        let mut accounts = Vec::with_capacity(mentions_to_send.len());
                        for (idx, &acc) in mentions_to_send.iter().enumerate() {
                            let is_private = w
                                .storage()
                                .key_chain()
                                .managed_private_account(acc)
                                .is_some()
                                || w.storage().key_chain().private_account(acc).is_some();
                            let is_owned_public = w
                                .storage()
                                .key_chain()
                                .public_account_ids()
                                .any(|(id, _)| id == acc);

                            let identity = if is_private {
                                wallet::AccountIdentity::PrivateOwned(acc)
                            } else if idx == 0 || is_owned_public {
                                wallet::AccountIdentity::Public(acc)
                            } else {
                                wallet::AccountIdentity::PublicNoSign(acc)
                            };
                            accounts.push(identity.select_program_actor_state(program));
                        }
                        let client = w.client().clone();
                        let protocol_version = client.detect_protocol_version().await;
                        let any_private = mentions_to_send.iter().any(|&acc| {
                            w.storage()
                                .key_chain()
                                .managed_private_account(acc)
                                .is_some()
                                || w.storage().key_chain().private_account(acc).is_some()
                        });

                        if protocol_version == SequencerProtocolVersion::V03ShardModel
                            && !any_private
                        {
                            let payer_id = *primary;
                            let signing_key = match w.get_account_public_signing_key(payer_id) {
                                Some(k) => k.clone(),
                                None => {
                                    requests
                                        .set_status(
                                            &rid,
                                            RequestStatus::Failed {
                                                reason: format!("No public signing key found for primary account {}", payer_id),
                                            },
                                        )
                                        .await;
                                    return;
                                }
                            };

                            let nonce = match w.get_accounts_nonces(&[payer_id]).await {
                                Ok(nonces) => nonces
                                    .into_iter()
                                    .next()
                                    .unwrap_or(lee_core::account::Nonce(0)),
                                Err(e) => {
                                    requests
                                        .set_status(
                                            &rid,
                                            RequestStatus::Failed {
                                                reason: format!(
                                                    "Failed to fetch account nonce: {}",
                                                    e
                                                ),
                                            },
                                        )
                                        .await;
                                    return;
                                }
                            };

                            let quote = client.get_fee_state().await.unwrap_or(
                                sequencer_service_rpc::FeeStateQuote {
                                    height: 0,
                                    base_fee_exec: 8,
                                    base_fee_stor: 8,
                                    next_base_fee_exec_floor: 8,
                                    next_base_fee_exec_ceiling: 9,
                                    next_base_fee_stor_floor: 8,
                                    next_base_fee_stor_ceiling: 9,
                                    max_gas_exec: 10_000_000,
                                    max_gas_stor: 1_000_000,
                                },
                            );

                            let gas_limit = w.config().gas_limit;
                            let other_mentions: Vec<AccountId> =
                                mentions_to_send.iter().copied().skip(1).collect();

                            match v03_compat::build_and_sign_v03_contract_call(
                                program,
                                payer_id,
                                nonce,
                                &signing_key,
                                &other_mentions,
                                calldata_to_send,
                                gas_limit,
                                quote.base_fee_exec,
                            ) {
                                Ok((payload, tx_hash)) => {
                                    match client.send_raw_transaction(payload).await {
                                        Ok(returned_hash) => {
                                            let final_hash =
                                                if returned_hash == common::HashType::default() {
                                                    tx_hash
                                                } else {
                                                    returned_hash
                                                };
                                            requests
                                                .set_status(
                                                    &rid,
                                                    RequestStatus::Submitted {
                                                        tx_hash: final_hash,
                                                    },
                                                )
                                                .await;
                                        }
                                        Err(e) => {
                                            requests
                                                .set_status(
                                                    &rid,
                                                    RequestStatus::Failed {
                                                        reason: e.to_string(),
                                                    },
                                                )
                                                .await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    requests
                                        .set_status(
                                            &rid,
                                            RequestStatus::Failed {
                                                reason: format!(
                                                    "Failed to construct v0.3 transaction: {}",
                                                    e
                                                ),
                                            },
                                        )
                                        .await;
                                }
                            }
                        } else {
                            match w.send_pub_tx(accounts, 0, calldata_to_send).await {
                                Ok(tx_hash) => {
                                    requests
                                        .set_status(&rid, RequestStatus::Submitted { tx_hash })
                                        .await;
                                }
                                Err(e) => {
                                    requests
                                        .set_status(
                                            &rid,
                                            RequestStatus::Failed {
                                                reason: format!("{:?}", e),
                                            },
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
                ApprovalDecision::Reject => {
                    requests.set_status(&rid, RequestStatus::Rejected).await;
                }
            }
        });

        Ok(request_id)
    }

    async fn propose_contract_call(
        &self,
        session_id: &SessionId,
        program_id: AccountId,
        payer_id: AccountId,
        mentions: Vec<AccountId>,
        calldata: Vec<u8>,
        description: Option<String>,
    ) -> SdkResult<RequestId> {
        self.require_grant(session_id, &payer_id, GrantKind::Propose)
            .await?;

        let request = PendingRequest::new(
            session_id.clone(),
            RequestKind::ContractCall {
                program_id,
                payer_id,
                mentions: mentions.clone(),
                calldata: calldata.clone(),
                description: description.clone(),
            },
        );
        let request_id = request.id.clone();

        let decoded = self.decoders.decode(&program_id, &calldata);
        let summary = if decoded.operation != "Unknown" {
            format!("{}: {}", decoded.program, decoded.operation)
        } else if let Some(ref desc) = description {
            desc.clone()
        } else {
            ApprovalPrompt::auto_summary(&request)
        };

        let effects_source = if decoded.operation != "Unknown" {
            format!("Decoded({})", decoded.operation)
        } else {
            format!(
                "{:?}",
                EffectSimulator::unavailable(wallet::DEFAULT_GAS_LIMIT).source
            )
        };

        let verification = self.verifier.verify(&program_id).await;

        let prompt = ApprovalPrompt {
            request: request.clone(),
            summary,
            dapp_identity: self
                .sessions
                .get(session_id)
                .await
                .map(|s| s.caller_identity.clone())
                .unwrap_or_else(|| "unknown".into()),
            effects: Some(effects_source),
            verification_status: Some(verification.to_string()),
            warnings: vec![],
        };

        self.requests.insert(request).await;
        self.tx_queue
            .enqueue(crate::queue::QueueEntry {
                request_id: request_id.clone(),
                priority: 0,
            })
            .await;

        let handler = self.approval_handler.clone();
        let requests = self.requests.clone();
        let wallet = self.wallet.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let w = wallet.lock().await;

                    let mut accounts = Vec::with_capacity(mentions.len());
                    for &acc in &mentions {
                        let is_private = w
                            .storage()
                            .key_chain()
                            .managed_private_account(acc)
                            .is_some()
                            || w.storage().key_chain().private_account(acc).is_some();
                        let is_owned_public = w
                            .storage()
                            .key_chain()
                            .public_account_ids()
                            .any(|(id, _)| id == acc);

                        let identity = if is_private {
                            wallet::AccountIdentity::PrivateOwned(acc)
                        } else if acc == payer_id || is_owned_public {
                            wallet::AccountIdentity::Public(acc)
                        } else {
                            wallet::AccountIdentity::PublicNoSign(acc)
                        };
                        accounts.push(identity.select_program_actor_state(program_id));
                    }

                    let client = w.client().clone();
                    let protocol_version = client.detect_protocol_version().await;
                    let any_private = mentions.iter().any(|&acc| {
                        w.storage()
                            .key_chain()
                            .managed_private_account(acc)
                            .is_some()
                            || w.storage().key_chain().private_account(acc).is_some()
                    }) || w
                        .storage()
                        .key_chain()
                        .managed_private_account(payer_id)
                        .is_some()
                        || w.storage().key_chain().private_account(payer_id).is_some();

                    if protocol_version == SequencerProtocolVersion::V03ShardModel && !any_private {
                        let signing_key = match w.get_account_public_signing_key(payer_id) {
                            Some(k) => k.clone(),
                            None => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!(
                                                "No public signing key found for payer {}",
                                                payer_id
                                            ),
                                        },
                                    )
                                    .await;
                                return;
                            }
                        };

                        let nonce = match w.get_accounts_nonces(&[payer_id]).await {
                            Ok(nonces) => nonces
                                .into_iter()
                                .next()
                                .unwrap_or(lee_core::account::Nonce(0)),
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!("Failed to fetch account nonce: {}", e),
                                        },
                                    )
                                    .await;
                                return;
                            }
                        };

                        let quote = client.get_fee_state().await.unwrap_or(
                            sequencer_service_rpc::FeeStateQuote {
                                height: 0,
                                base_fee_exec: 8,
                                base_fee_stor: 8,
                                next_base_fee_exec_floor: 8,
                                next_base_fee_exec_ceiling: 9,
                                next_base_fee_stor_floor: 8,
                                next_base_fee_stor_ceiling: 9,
                                max_gas_exec: 10_000_000,
                                max_gas_stor: 1_000_000,
                            },
                        );

                        let gas_limit = w.config().gas_limit;

                        match v03_compat::build_and_sign_v03_contract_call(
                            program_id,
                            payer_id,
                            nonce,
                            &signing_key,
                            &mentions,
                            calldata,
                            gas_limit,
                            quote.base_fee_exec,
                        ) {
                            Ok((payload, tx_hash)) => {
                                match client.send_raw_transaction(payload).await {
                                    Ok(returned_hash) => {
                                        let final_hash =
                                            if returned_hash == common::HashType::default() {
                                                tx_hash
                                            } else {
                                                returned_hash
                                            };
                                        requests
                                            .set_status(
                                                &rid,
                                                RequestStatus::Submitted {
                                                    tx_hash: final_hash,
                                                },
                                            )
                                            .await;
                                    }
                                    Err(e) => {
                                        requests
                                            .set_status(
                                                &rid,
                                                RequestStatus::Failed {
                                                    reason: e.to_string(),
                                                },
                                            )
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!(
                                                "Failed to construct v0.3 contract call: {}",
                                                e
                                            ),
                                        },
                                    )
                                    .await;
                            }
                        }
                    } else {
                        match w
                            .send_pub_tx_paid_by(accounts, 0, calldata, Some(payer_id))
                            .await
                        {
                            Ok(tx_hash) => {
                                requests
                                    .set_status(&rid, RequestStatus::Submitted { tx_hash })
                                    .await;
                            }
                            Err(e) => {
                                requests
                                    .set_status(
                                        &rid,
                                        RequestStatus::Failed {
                                            reason: format!("{:?}", e),
                                        },
                                    )
                                    .await;
                            }
                        }
                    }
                }
                ApprovalDecision::Reject => {
                    requests.set_status(&rid, RequestStatus::Rejected).await;
                }
            }
        });

        Ok(request_id)
    }

    async fn list_token_holdings(
        &self,
        session_id: &SessionId,
    ) -> SdkResult<Vec<TokenHoldingView>> {
        let _session = self.validate_session(session_id).await?;
        self.list_all_token_holdings().await
    }

    async fn get_request_status(&self, request_id: &RequestId) -> SdkResult<RequestStatus> {
        self.requests
            .get_status(request_id)
            .await
            .ok_or_else(|| SdkError::RequestNotFound(request_id.clone()))
    }

    async fn cancel_request(&self, request_id: &RequestId) -> SdkResult<()> {
        let status = self.get_request_status(request_id).await?;
        if status == RequestStatus::PendingApproval {
            self.requests
                .set_status(request_id, RequestStatus::Cancelled)
                .await;
            Ok(())
        } else {
            Err(SdkError::RequestAlreadyFinalized)
        }
    }

    async fn resolve_name(
        &self,
        session_id: &SessionId,
        name: &str,
    ) -> SdkResult<crate::resolve::ResolvedDestination> {
        let _session = self.validate_session(session_id).await?;
        match self.resolver.resolve(name).await {
            Ok(Some(dest)) => Ok(dest),
            Ok(None) => Err(SdkError::InvalidArgument(format!(
                "'{}' is not a .lez name",
                name
            ))),
            Err(e) => Err(SdkError::NameResolutionError(e.to_string())),
        }
    }

    async fn resolve_name_fresh(
        &self,
        session_id: &SessionId,
        name: &str,
    ) -> SdkResult<crate::resolve::ResolvedDestination> {
        let _session = self.validate_session(session_id).await?;
        match self.resolver.resolve_fresh(name).await {
            Ok(Some(dest)) => Ok(dest),
            Ok(None) => Err(SdkError::InvalidArgument(format!(
                "'{}' is not a .lez name",
                name
            ))),
            Err(e) => Err(SdkError::NameResolutionError(e.to_string())),
        }
    }

    async fn request_identity_access(
        &self,
        session_id: &SessionId,
        kind: GrantKind,
    ) -> SdkResult<()> {
        let _ = self.validate_session(session_id).await?;
        if kind != GrantKind::IdentityRead && kind != GrantKind::IdentityPropose {
            return Err(SdkError::InvalidArgument(
                "Must specify IdentityRead or IdentityPropose".into(),
            ));
        }
        self.sessions
            .update(session_id, |s| {
                s.add_identity_grant(kind);
            })
            .await
            .ok_or_else(|| SdkError::SessionNotFound(session_id.clone()))?;
        Ok(())
    }

    async fn get_identity_commitment(
        &self,
        session_id: &SessionId,
    ) -> SdkResult<crate::identity::IdentityCommitment> {
        self.require_identity_grant(session_id, GrantKind::IdentityRead)
            .await?;
        let guard = self.identity_secret.lock().await;
        let secret = guard.as_ref().ok_or(SdkError::WalletLocked)?;
        Ok(crate::identity::derive_identity_commitment(secret))
    }

    async fn derive_scoped_nullifier(
        &self,
        session_id: &SessionId,
        scope_id: &str,
    ) -> SdkResult<crate::identity::ScopedNullifier> {
        self.require_identity_grant(session_id, GrantKind::IdentityRead)
            .await?;
        let guard = self.identity_secret.lock().await;
        let secret = guard.as_ref().ok_or(SdkError::WalletLocked)?;
        Ok(crate::identity::derive_scoped_nullifier(secret, scope_id))
    }

    async fn sign_identity_attestation(
        &self,
        session_id: &SessionId,
        scope_id: &str,
        message: &[u8],
    ) -> SdkResult<crate::identity::AttestationPayload> {
        self.require_identity_grant(session_id, GrantKind::IdentityPropose)
            .await?;
        let guard = self.identity_secret.lock().await;
        let secret = guard.as_ref().ok_or(SdkError::WalletLocked)?;
        crate::identity::sign_identity_attestation(secret, scope_id, message)
    }

    async fn prepare_identity_registration(
        &self,
        session_id: &SessionId,
        scope_id: &str,
        node_pubkeys: &[[u8; 32]],
        threshold_k: u8,
    ) -> SdkResult<crate::identity::RegistrationPayload> {
        self.require_identity_grant(session_id, GrantKind::IdentityPropose)
            .await?;
        let guard = self.identity_secret.lock().await;
        let secret = guard.as_ref().ok_or(SdkError::WalletLocked)?;
        crate::identity::prepare_identity_registration(secret, scope_id, node_pubkeys, threshold_k)
    }
}

impl OwnerApi for SdkEngine {
    async fn create_wallet(&self, password: &str) -> SdkResult<Mnemonic> {
        let config_path = self.config.wallet_config_path.clone();
        let storage_path = self.config.wallet_storage_path.clone();

        let effective_gas_limit = self.get_gas_limit().await;
        let overrides = wallet::config::WalletConfigOverrides {
            gas_limit: Some(effective_gas_limit),
            ..Default::default()
        };

        let mut wallet_config = WalletConfig::from_path_or_initialize_default(&config_path)
            .map_err(SdkError::Internal)?;
        wallet_config.apply_overrides(overrides.clone());
        let inner_client =
            WalletCore::client_from_config(&wallet_config).map_err(SdkError::Internal)?;
        let client = TestnetSequencerClient::new(inner_client);

        let (new_wallet, mnemonic) = WalletCore::new_init_storage_with_client(
            config_path,
            storage_path,
            Some(overrides),
            password,
            client,
        )
        .map_err(SdkError::Internal)?;

        let mut wallet = self.wallet.lock().await;
        *wallet = new_wallet;
        let _ = wallet.start_at_chain_tip().await;
        wallet.store_persistent_data().map_err(SdkError::Internal)?;

        let mut store = self.store.lock().await;
        let seed = mnemonic.to_seed(password);
        let identity = crate::identity::derive_identity_secret(&seed);
        *self.identity_secret.lock().await = Some(identity);

        store.state_mut().locked = false;
        store.save().map_err(SdkError::Internal)?;

        Ok(mnemonic)
    }

    async fn restore_wallet(&self, mnemonic: &Mnemonic, password: &str) -> SdkResult<()> {
        let mut wallet = self.wallet.lock().await;
        wallet.restore_storage(mnemonic);
        let _ = wallet.start_at_chain_tip().await;
        if !password.is_empty() {
            wallet.set_password(password).map_err(SdkError::Internal)?;
        }
        wallet.store_persistent_data().map_err(SdkError::Internal)?;

        let seed = mnemonic.to_seed(password);
        let identity = crate::identity::derive_identity_secret(&seed);
        *self.identity_secret.lock().await = Some(identity);

        let mut store = self.store.lock().await;
        store.state_mut().locked = false;
        store.save().map_err(SdkError::Internal)?;
        Ok(())
    }

    async fn lock(&self) -> SdkResult<()> {
        let sessions = self.sessions.list().await;
        for s in &sessions {
            self.sessions.revoke(&s.id).await;
        }
        let _ = self.pow_manager.stop_mining(&self.store);
        *self.identity_secret.lock().await = None;
        let mut store = self.store.lock().await;
        store.state_mut().locked = true;
        store.save().map_err(SdkError::Internal)?;
        log::info!("Wallet locked, {} sessions revoked", sessions.len());
        Ok(())
    }

    async fn unlock(&self, password: &str) -> SdkResult<()> {
        let storage_path = &self.config.wallet_storage_path;
        if storage_path.exists() {
            if wallet::storage::Storage::from_path(storage_path, password).is_err() {
                return Err(SdkError::InvalidPassword);
            }

            let effective_gas_limit = self.get_gas_limit().await;
            let overrides = wallet::config::WalletConfigOverrides {
                gas_limit: Some(effective_gas_limit),
                ..Default::default()
            };
            let mut wallet_config =
                WalletConfig::from_path_or_initialize_default(&self.config.wallet_config_path)
                    .map_err(SdkError::Internal)?;
            wallet_config.apply_overrides(overrides.clone());
            let inner_client =
                WalletCore::client_from_config(&wallet_config).map_err(SdkError::Internal)?;
            let client = TestnetSequencerClient::new(inner_client);

            let loaded_wallet = WalletCore::new_update_chain_with_client(
                self.config.wallet_config_path.clone(),
                storage_path.clone(),
                Some(overrides),
                password,
                client,
            )
            .map_err(|_| SdkError::InvalidPassword)?;

            let derived_id = Self::derive_identity_from_wallet(&loaded_wallet);
            {
                let mut wallet = self.wallet.lock().await;
                *wallet = loaded_wallet;
            }
            {
                let mut id_guard = self.identity_secret.lock().await;
                if id_guard.is_none() {
                    *id_guard = derived_id;
                }
            }
        }
        let mut store = self.store.lock().await;
        store.state_mut().locked = false;
        store.save().map_err(SdkError::Internal)?;
        Ok(())
    }

    async fn change_password(&self, old_password: &str, new_password: &str) -> SdkResult<()> {
        let storage_path = &self.config.wallet_storage_path;
        if storage_path.exists()
            && wallet::storage::Storage::from_path(storage_path, old_password).is_err()
        {
            return Err(SdkError::InvalidPassword);
        }
        let mut wallet = self.wallet.lock().await;
        wallet
            .set_password(new_password)
            .map_err(SdkError::Internal)?;
        let mut store = self.store.lock().await;
        store.state_mut().locked = false;
        store.save().map_err(SdkError::Internal)?;
        log::info!("Wallet encryption password changed successfully");
        Ok(())
    }

    async fn create_account_public(&self, label: Option<String>) -> SdkResult<AccountId> {
        {
            let store = self.store.lock().await;
            if store.state().locked {
                return Err(SdkError::WalletLocked);
            }
        }
        let account_id = {
            let mut wallet = self.wallet.lock().await;
            let (account_id, _chain_index) = wallet.create_new_account_public(None);

            if let Some(label_str) = label {
                let _ = wallet.storage_mut().add_label(
                    Label::new(label_str.clone()),
                    AccountIdWithPrivacy::Public(account_id),
                );

                let mut store = self.store.lock().await;
                store
                    .state_mut()
                    .account_labels
                    .insert(account_id.to_string(), label_str);
                let _ = store.save();
            }
            wallet.store_persistent_data().map_err(SdkError::Internal)?;
            account_id
        };

        for s in self.sessions.list().await {
            if s.caller_identity == "internal_wallet_ui" {
                self.sessions
                    .update(&s.id, |sess| {
                        sess.add_grant(account_id, GrantKind::Read);
                        sess.add_grant(account_id, GrantKind::Propose);
                    })
                    .await;
            }
        }

        Ok(account_id)
    }

    async fn create_account_private(&self, label: Option<String>) -> SdkResult<AccountId> {
        {
            let store = self.store.lock().await;
            if store.state().locked {
                return Err(SdkError::WalletLocked);
            }
        }
        let account_id = {
            let mut wallet = self.wallet.lock().await;
            let (account_id, _chain_index) = wallet.create_new_account_private(None);

            if let Some(label_str) = label {
                let _ = wallet.storage_mut().add_label(
                    Label::new(label_str.clone()),
                    AccountIdWithPrivacy::Private(account_id),
                );

                let mut store = self.store.lock().await;
                store
                    .state_mut()
                    .account_labels
                    .insert(account_id.to_string(), label_str);
                let _ = store.save();
            }
            wallet.store_persistent_data().map_err(SdkError::Internal)?;
            account_id
        };

        for s in self.sessions.list().await {
            if s.caller_identity == "internal_wallet_ui" {
                self.sessions
                    .update(&s.id, |sess| {
                        sess.add_grant(account_id, GrantKind::Read);
                        sess.add_grant(account_id, GrantKind::Propose);
                    })
                    .await;
            }
        }

        Ok(account_id)
    }

    async fn list_all_accounts(&self) -> SdkResult<Vec<AccountView>> {
        let mut wallet = self.wallet.lock().await;
        if wallet
            .storage()
            .key_chain()
            .private_account_ids()
            .next()
            .is_some()
        {
            let last_synced = wallet.storage().last_synced_block();
            if last_synced == 0 {
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
        let store = self.store.lock().await;
        let custom_labels = &store.state().account_labels;

        // Use the key chain to enumerate all accounts
        let key_chain = wallet.storage().key_chain();
        let public_accounts = key_chain.public_account_ids();

        let mut views = Vec::new();
        for (account_id, _) in public_accounts {
            let balance = wallet.get_account_balance(account_id).await.ok();
            let label_text = custom_labels
                .get(&account_id.to_string())
                .cloned()
                .or_else(|| {
                    wallet
                        .storage()
                        .labels_for_account(AccountIdWithPrivacy::Public(account_id))
                        .next()
                        .map(|l| l.as_ref().to_string())
                });
            views.push(AccountView {
                account_id,
                privacy: AccountPrivacy::Public,
                label: label_text,
                balance,
            });
        }

        let private_accounts = key_chain.private_account_ids();
        for (account_id, _) in private_accounts {
            let label_text = custom_labels
                .get(&account_id.to_string())
                .cloned()
                .or_else(|| {
                    wallet
                        .storage()
                        .labels_for_account(AccountIdWithPrivacy::Private(account_id))
                        .next()
                        .map(|l| l.as_ref().to_string())
                });
            let shielded_bal = wallet
                .private_account_state(account_id)
                .and_then(|acc| acc.data.native_balance().ok())
                .unwrap_or(0);
            views.push(AccountView {
                account_id,
                privacy: AccountPrivacy::Private,
                label: label_text,
                balance: Some(shielded_bal),
            });
        }

        Ok(views)
    }

    async fn list_sessions(&self) -> SdkResult<Vec<crate::session::Session>> {
        Ok(self.sessions.list().await)
    }

    async fn revoke_session(&self, session_id: &SessionId) -> SdkResult<()> {
        if self.sessions.revoke(session_id).await {
            Ok(())
        } else {
            Err(SdkError::SessionNotFound(session_id.clone()))
        }
    }

    async fn register_token_holding(
        &self,
        holding_id: AccountId,
        definition_id: AccountId,
        symbol: String,
        name: String,
        is_shielded: bool,
    ) -> SdkResult<()> {
        let mut store = self.store.lock().await;
        let entry = crate::store::TokenHoldingEntry {
            holding_id: holding_id.to_string(),
            definition_id: definition_id.to_string(),
            symbol,
            name,
            is_shielded,
        };
        if let Some(existing) = store
            .state_mut()
            .token_holdings
            .iter_mut()
            .find(|h| h.holding_id == entry.holding_id)
        {
            *existing = entry;
        } else {
            store.state_mut().token_holdings.push(entry);
        }
        store.save().map_err(SdkError::Internal)?;
        Ok(())
    }

    async fn list_all_token_holdings(&self) -> SdkResult<Vec<TokenHoldingView>> {
        let store = self.store.lock().await;
        let mut views = Vec::new();
        for entry in &store.state().token_holdings {
            let holding_id = match parse_account_id(&entry.holding_id) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let definition_id = match parse_account_id(&entry.definition_id) {
                Ok(id) => id,
                Err(_) => continue,
            };
            views.push(TokenHoldingView {
                holding_id,
                definition_id,
                symbol: entry.symbol.clone(),
                name: entry.name.clone(),
                is_shielded: entry.is_shielded,
            });
        }
        Ok(views)
    }

    async fn pow_start_mining(&self, threads: u32) -> SdkResult<crate::pow::PowStatusView> {
        {
            let store = self.store.lock().await;
            if store.state().locked {
                return Err(SdkError::WalletLocked);
            }
        }
        Ok(self.pow_manager.start_mining(threads, self.store.clone()))
    }

    async fn pow_stop_mining(&self) -> SdkResult<crate::pow::PowStatusView> {
        Ok(self.pow_manager.stop_mining(&self.store))
    }

    async fn pow_status(&self) -> SdkResult<crate::pow::PowStatusView> {
        Ok(self.pow_manager.status(&self.store).await)
    }

    async fn channel_deposit(
        &self,
        recipient_account: &str,
        amount_str: &str,
    ) -> SdkResult<crate::pow::ChannelDepositResult> {
        let recipient_id = parse_account_id(recipient_account)?;
        crate::pow::execute_channel_deposit(
            &self.pow_manager,
            &self.wallet,
            &self.store,
            recipient_id,
            amount_str,
        )
        .await
    }

    async fn get_transaction_status(&self, tx_hash_str: &str) -> SdkResult<TransactionStatusView> {
        let clean_hex = tx_hash_str.trim().trim_start_matches("0x");
        let bytes = hex::decode(clean_hex)
            .map_err(|e| SdkError::InvalidArgument(format!("Invalid tx_hash hex: {}", e)))?;
        if bytes.len() != 32 {
            return Err(SdkError::InvalidArgument(
                "tx_hash must be 32 bytes (64 hex characters)".into(),
            ));
        }
        let hash = common::HashType(bytes.try_into().unwrap());
        let client = {
            let w = self.wallet.lock().await;
            w.client().clone()
        };
        let res = client.get_transaction(hash).await.map_err(|e| {
            SdkError::Internal(anyhow::anyhow!("Sequencer get_transaction error: {:?}", e))
        })?;

        match res {
            Some((_tx, block_id)) => Ok(TransactionStatusView {
                tx_hash: tx_hash_str.to_string(),
                status: "confirmed".to_string(),
                block_id: Some(block_id),
            }),
            None => Ok(TransactionStatusView {
                tx_hash: tx_hash_str.to_string(),
                status: "pending".to_string(),
                block_id: None,
            }),
        }
    }
}

async fn finish_and_sync_transaction(
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

fn parse_account_id(input: &str) -> SdkResult<AccountId> {
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
