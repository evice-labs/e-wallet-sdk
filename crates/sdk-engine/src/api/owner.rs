use super::*;

impl OwnerApi for SdkEngine {
    async fn create_wallet(&self, password: &str) -> SdkResult<Mnemonic> {
        let config_path = self.config.wallet_config_path.clone();
        let storage_path = self.config.wallet_storage_path.clone();

        let gas_limit_override = self.store.lock().await.state().gas_limit_override;
        let overrides = WalletConfigOverrides {
            gas_limit: gas_limit_override,
            ..Default::default()
        };
        let client = build_wallet_client(&config_path, &overrides)?;

        let (new_wallet, mnemonic) = WalletCore::new_init_storage_with_client(
            config_path,
            storage_path,
            Some(overrides),
            password,
            client,
        )
        .map_err(SdkError::Internal)?;

        let mut wallet = new_wallet;
        let _ = wallet.start_at_chain_tip().await;
        wallet.store_persistent_data().map_err(SdkError::Internal)?;
        *self.wallet.lock().await = Some(wallet);

        let mut store = self.store.lock().await;
        let seed = mnemonic.to_seed(password);
        let identity = crate::identity::derive_identity_secret(&seed);
        *self.identity_secret.lock().await = Some(identity);

        store.state_mut().locked = false;
        store.save().map_err(SdkError::Internal)?;

        Ok(mnemonic)
    }

    async fn restore_wallet(&self, mnemonic: &Mnemonic, password: &str) -> SdkResult<()> {
        let mut guard = self.wallet.lock().await;
        let mut wallet = match guard.take() {
            Some(w) => w,
            None => {
                let gas_limit_override = self.store.lock().await.state().gas_limit_override;
                let overrides = WalletConfigOverrides {
                    gas_limit: gas_limit_override,
                    ..Default::default()
                };
                let (w, _) = WalletCore::new_init_storage_with_client(
                    self.config.wallet_config_path.clone(),
                    self.config.wallet_storage_path.clone(),
                    Some(overrides),
                    password,
                    self.client.clone(),
                )
                .map_err(SdkError::Internal)?;
                w
            }
        };
        wallet.restore_storage(mnemonic);
        let _ = wallet.start_at_chain_tip().await;
        if !password.is_empty() {
            wallet.set_password(password).map_err(SdkError::Internal)?;
        }
        wallet.store_persistent_data().map_err(SdkError::Internal)?;
        *guard = Some(wallet);

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
        *self.wallet.lock().await = None;
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

            let gas_limit_override = self.store.lock().await.state().gas_limit_override;
            let overrides = WalletConfigOverrides {
                gas_limit: gas_limit_override,
                ..Default::default()
            };
            let client = build_wallet_client(&self.config.wallet_config_path, &overrides)?;

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
                *wallet = Some(loaded_wallet);
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
        let mut guard = self.wallet.lock().await;
        let wallet = guard.as_mut().ok_or(SdkError::WalletLocked)?;
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
            let mut guard = self.wallet.lock().await;
            let wallet = guard.as_mut().ok_or(SdkError::WalletLocked)?;
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

        self.grant_internal_ui_sessions(account_id).await;

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
            let mut guard = self.wallet.lock().await;
            let wallet = guard.as_mut().ok_or(SdkError::WalletLocked)?;
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

        self.grant_internal_ui_sessions(account_id).await;

        Ok(account_id)
    }

    async fn list_all_accounts(&self) -> SdkResult<Vec<AccountView>> {
        {
            let store = self.store.lock().await;
            if store.state().locked {
                return Err(SdkError::WalletLocked);
            }
        }
        let mut guard = self.wallet.lock().await;
        let wallet = guard.as_mut().ok_or(SdkError::WalletLocked)?;
        if wallet
            .storage()
            .key_chain()
            .private_account_ids()
            .next()
            .is_some()
        {
            ensure_private_sync_tip(wallet).await;
        }
        let store = self.store.lock().await;
        let custom_labels = &store.state().account_labels;

        // Use the key chain to enumerate all accounts
        let key_chain = wallet.storage().key_chain();
        let public_accounts = key_chain.public_account_ids();

        let mut views = Vec::new();
        for (account_id, _) in public_accounts {
            let balance = wallet.get_account_balance(account_id).await.ok();
            let label_text =
                resolve_account_label(wallet, custom_labels, account_id, &AccountPrivacy::Public);
            views.push(AccountView {
                account_id,
                privacy: AccountPrivacy::Public,
                label: label_text,
                balance,
            });
        }

        let private_accounts = key_chain.private_account_ids();
        for (account_id, _) in private_accounts {
            let label_text =
                resolve_account_label(wallet, custom_labels, account_id, &AccountPrivacy::Private);
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
        let entries = {
            let store = self.store.lock().await;
            let mut parsed = Vec::new();
            for entry in &store.state().token_holdings {
                let holding_id = match parse_account_id(&entry.holding_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let definition_id = match parse_account_id(&entry.definition_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                parsed.push((
                    holding_id,
                    definition_id,
                    entry.symbol.clone(),
                    entry.name.clone(),
                    entry.is_shielded,
                ));
            }
            parsed
        };

        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let (client, private_balances) = {
            let guard = self.wallet.lock().await;
            let wallet = guard.as_ref().ok_or(SdkError::WalletLocked)?;
            let mut priv_map = std::collections::HashMap::new();
            for (holding_id, definition_id, _, _, is_shielded) in &entries {
                if *is_shielded {
                    let bal = wallet
                        .private_account_state(*holding_id)
                        .and_then(|acc| acc.data.actor_states.get(definition_id))
                        .map(|bytes| decode_token_actor_balance(bytes))
                        .unwrap_or(0);
                    priv_map.insert((*holding_id, *definition_id), bal);
                }
            }
            (wallet.client().clone(), priv_map)
        };

        let mut public_account_cache: std::collections::HashMap<AccountId, Option<lee::Account>> =
            std::collections::HashMap::new();
        let mut views = Vec::with_capacity(entries.len());

        for (holding_id, definition_id, symbol, name, is_shielded) in entries {
            let balance = if is_shielded {
                Some(
                    private_balances
                        .get(&(holding_id, definition_id))
                        .copied()
                        .unwrap_or(0),
                )
            } else {
                if let std::collections::hash_map::Entry::Vacant(e) =
                    public_account_cache.entry(holding_id)
                {
                    let fetched =
                        <TestnetSequencerClient as wallet::WalletSequencerClient>::get_account(
                            &client, holding_id,
                        )
                        .await
                        .ok()
                        .flatten();
                    e.insert(fetched);
                }
                match public_account_cache.get(&holding_id) {
                    Some(Some(acc)) => Some(
                        acc.data
                            .actor_states
                            .get(&definition_id)
                            .map(|bytes| decode_token_actor_balance(bytes))
                            .unwrap_or(0),
                    ),
                    Some(None) => Some(0),
                    None => None,
                }
            };

            views.push(TokenHoldingView {
                holding_id,
                definition_id,
                symbol,
                name,
                is_shielded,
                balance,
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
            &self.submitted_tx_nonces,
            recipient_id,
            amount_str,
        )
        .await
    }

    async fn get_transaction_status(&self, tx_hash_str: &str) -> SdkResult<TransactionStatusView> {
        let clean_hex = tx_hash_str.trim().trim_start_matches("0x").to_lowercase();
        let bytes = hex::decode(&clean_hex)
            .map_err(|e| SdkError::InvalidArgument(format!("Invalid tx_hash hex: {}", e)))?;
        if bytes.len() != 32 {
            return Err(SdkError::InvalidArgument(
                "tx_hash must be 32 bytes (64 hex characters)".into(),
            ));
        }
        let hash = common::HashType(bytes.try_into().unwrap());
        let client = self.client.clone();

        let included_block = match client.get_transaction(hash).await {
            Ok(Some((_tx, block_id))) => Some(block_id),
            Ok(None) => None,
            Err(e) => {
                return Err(SdkError::Internal(anyhow::anyhow!(
                    "Sequencer get_transaction error: {:?}",
                    e
                )));
            }
        };

        if let Some(block_id) = included_block {
            self.submitted_tx_nonces.remove(&clean_hex);
            return Ok(TransactionStatusView {
                tx_hash: tx_hash_str.to_string(),
                status: "confirmed".to_string(),
                block_id: Some(block_id),
            });
        }

        // Factual mempool drop detection: if the tx is not in any block and the sender's
        // on-chain nonce has already moved past `tx_nonce`, the tx was superseded/dropped.
        if let Some(entry) = self.submitted_tx_nonces.get(&clean_hex) {
            let (sender_id, tx_nonce) = *entry.value();
            drop(entry);
            if let Ok(nonces) = client.get_accounts_nonces(vec![sender_id]).await
                && let Some(on_chain_nonce) = nonces.into_iter().next()
                && on_chain_nonce.0 > tx_nonce.0
            {
                self.submitted_tx_nonces.remove(&clean_hex);
                return Ok(TransactionStatusView {
                    tx_hash: tx_hash_str.to_string(),
                    status: "dropped".to_string(),
                    block_id: None,
                });
            }
        }

        Ok(TransactionStatusView {
            tx_hash: tx_hash_str.to_string(),
            status: "pending".to_string(),
            block_id: None,
        })
    }
}
