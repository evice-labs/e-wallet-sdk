use super::*;

impl SdkEngine {
    pub fn new(
        wallet: Option<WalletCore<TestnetSequencerClient>>,
        client: TestnetSequencerClient,
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
            client,
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
            submitted_tx_nonces: Arc::new(dashmap::DashMap::new()),
            config,
        })
    }

    pub(super) async fn validate_session(
        &self,
        session_id: &SessionId,
    ) -> SdkResult<crate::session::Session> {
        self.sessions
            .get(session_id)
            .await
            .ok_or_else(|| SdkError::SessionNotFound(session_id.clone()))
    }

    pub(super) async fn require_grant(
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

    pub(super) async fn require_identity_grant(
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

    pub(super) fn derive_identity_from_wallet(
        wallet: &WalletCore<TestnetSequencerClient>,
    ) -> Option<crate::identity::IdentitySecret> {
        let key_chain = wallet.storage().key_chain();
        let first_pub_id = key_chain.public_account_ids().next().map(|(id, _)| id)?;
        let secret = key_chain.pub_account_secret(first_pub_id)?;
        let key_bytes = serde_json::to_vec(secret).ok()?;
        Some(crate::identity::derive_identity_secret(&key_bytes))
    }

    pub(super) async fn resolve_destination(&self, input: &str) -> SdkResult<AccountId> {
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

        let store =
            PersistentStore::open(config.sdk_state_path.clone()).map_err(SdkError::Internal)?;
        let overrides = WalletConfigOverrides {
            gas_limit: store.state().gas_limit_override,
            ..Default::default()
        };
        let client = build_wallet_client(&config.wallet_config_path, &overrides)?;

        let mut store = store;

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
                    (Some(w), id)
                }
                Err(e) if password.is_empty() => {
                    log::info!(
                        "Encrypted wallet storage detected at startup; starting in locked state ({})",
                        e
                    );
                    store.state_mut().locked = true;
                    let _ = store.save();
                    (None, None)
                }
                Err(e) => return Err(SdkError::Internal(e)),
            }
        } else {
            let (wallet, _mnemonic) = WalletCore::new_init_storage_with_client(
                config.wallet_config_path.clone(),
                config.wallet_storage_path.clone(),
                Some(overrides),
                password,
                client.clone(),
            )
            .map_err(SdkError::Internal)?;
            let id = Self::derive_identity_from_wallet(&wallet);
            (Some(wallet), id)
        };

        let engine = Self::new(wallet, client, approval_handler, config)?;
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
            let guard = self.wallet.lock().await;
            guard
                .as_ref()
                .map(|w| w.config().gas_limit)
                .unwrap_or(DEFAULT_WALLET_GAS_LIMIT)
        }
    }

    pub async fn set_gas_limit(&self, limit: u64) -> SdkResult<()> {
        {
            let mut store = self.store.lock().await;
            store.state_mut().gas_limit_override = Some(limit);
            store.save().map_err(SdkError::Internal)?;
        }
        {
            let mut guard = self.wallet.lock().await;
            if let Some(wallet) = guard.as_mut() {
                let mut cfg = wallet.config().clone();
                cfg.gas_limit = limit;
                wallet.set_config(cfg);
                if let Err(e) = wallet.store_config_changes().await {
                    log::warn!("Failed to persist updated wallet config to disk: {:?}", e);
                }
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
        let mut guard = self.wallet.lock().await;
        let wallet = guard.as_mut().ok_or(SdkError::WalletLocked)?;
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

        let guard = self.wallet.lock().await;
        let wallet = guard.as_ref().ok_or(SdkError::WalletLocked)?;
        let store = self.store.lock().await;
        let custom_labels = &store.state().account_labels;

        let is_private_local = is_private_account_local(wallet.storage().key_chain(), account_id);
        let is_public_local = wallet
            .storage()
            .key_chain()
            .pub_account_secret(account_id)
            .is_some();

        let privacy = if let Some(p) = explicit_privacy {
            p
        } else if is_private_local {
            AccountPrivacy::Private
        } else {
            AccountPrivacy::Public
        };

        let is_local = is_private_local || is_public_local;
        let prefix = match privacy {
            AccountPrivacy::Public => "Public",
            AccountPrivacy::Private => "Private",
        };
        let canonical_address = format!("{}/{}", prefix, account_id);

        let label = resolve_account_label(wallet, custom_labels, account_id, &privacy);

        Ok(ResolvedAccountInfo {
            account_id,
            privacy,
            canonical_address,
            label,
            is_local,
        })
    }

    pub(super) async fn grant_internal_ui_sessions(&self, account_id: AccountId) {
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

    pub fn wallet(&self) -> &Arc<Mutex<Option<WalletCore<TestnetSequencerClient>>>> {
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
