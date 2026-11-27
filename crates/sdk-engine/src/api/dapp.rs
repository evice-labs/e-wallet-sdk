use super::*;

impl DAppApi for SdkEngine {
    async fn sdk_info(&self) -> SdkResult<SdkInfo> {
        let store = self.store.lock().await;
        Ok(SdkInfo {
            sdk_version: crate::SDK_VERSION.to_string(),
            api_version: crate::API_VERSION,
            network: "testnet-0.4".to_string(),
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
                let guard = self.wallet.lock().await;
                let wallet = guard.as_ref().ok_or(SdkError::WalletLocked)?;
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
        let guard = self.wallet.lock().await;
        let wallet = guard.as_ref().ok_or(SdkError::WalletLocked)?;

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
        let guard = self.wallet.lock().await;
        let wallet = guard.as_ref().ok_or(SdkError::WalletLocked)?;
        if is_private_account_local(wallet.storage().key_chain(), *account) {
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
            let guard = self.wallet.lock().await;
            let wallet = guard.as_ref().ok_or(SdkError::WalletLocked)?;
            let kc = wallet.storage().key_chain();
            let from_priv = match from_privacy {
                Some(AccountPrivacy::Private) => true,
                Some(AccountPrivacy::Public) => false,
                None => is_private_account_local(kc, from),
            };
            let to_priv = if is_to_explicit_private {
                true
            } else if is_to_explicit_public {
                false
            } else {
                is_private_account_local(kc, to_account)
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
        let submitted_tx_nonces = self.submitted_tx_nonces.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let mut guard = wallet.lock().await;
                    let Some(w) = guard.as_mut() else {
                        requests
                            .set_status(
                                &rid,
                                RequestStatus::Failed {
                                    reason: "Wallet is locked".into(),
                                },
                            )
                            .await;
                        return;
                    };

                    ensure_private_sync_tip(w).await;
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

                    let pre_nonce = if !is_from_private {
                        w.get_accounts_nonces(&[from])
                            .await
                            .ok()
                            .and_then(|v| v.into_iter().next())
                    } else {
                        None
                    };

                    let transfer_res = {
                        let facade =
                            wallet::program_facades::native_token_transfer::NativeTokenTransfer(w);
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
                            if let Some(tx_nonce) = pre_nonce {
                                submitted_tx_nonces.insert(hex::encode(tx_hash), (from, tx_nonce));
                            }
                            if let Err(e) = finish_and_sync_transaction(w, tx_hash).await {
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
                                    "{} (Hint: When shielding to a fresh 0-balance private account, the transfer amount must exceed the private transaction fee reserve)",
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
            let guard = self.wallet.lock().await;
            let wallet = guard.as_ref().ok_or(SdkError::WalletLocked)?;
            let kc = wallet.storage().key_chain();
            let is_from_private = is_private_account_local(kc, from_holding);
            let is_to_private = is_private_account_local(kc, to_holding);
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
        let submitted_tx_nonces = self.submitted_tx_nonces.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let guard = wallet.lock().await;
                    let Some(w) = guard.as_ref() else {
                        requests
                            .set_status(
                                &rid,
                                RequestStatus::Failed {
                                    reason: "Wallet is locked".into(),
                                },
                            )
                            .await;
                        return;
                    };
                    let kc = w.storage().key_chain();
                    let is_from_private = is_private_account_local(kc, from_holding);
                    let is_to_private = is_private_account_local(kc, to_holding);

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
                            .without_authorization()
                    } else {
                        wallet::AccountIdentity::PublicNoSign(to_holding)
                            .select_program_actor_state(definition)
                    };

                    let pre_nonce = if !is_from_private {
                        w.get_accounts_nonces(&[from_holding])
                            .await
                            .ok()
                            .and_then(|v| v.into_iter().next())
                    } else {
                        None
                    };
                    let accounts = vec![sender_actor, recipient_actor];
                    let message = amount.to_le_bytes().to_vec();
                    match w.send_pub_tx(accounts, 0, message).await {
                        Ok(tx_hash) => {
                            if let Some(tx_nonce) = pre_nonce {
                                submitted_tx_nonces
                                    .insert(hex::encode(tx_hash), (from_holding, tx_nonce));
                            }
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
        let submitted_tx_nonces = self.submitted_tx_nonces.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;
        let mentions_to_send = mentions.clone();
        let calldata_to_send = calldata.clone();

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let guard = wallet.lock().await;
                    let Some(w) = guard.as_ref() else {
                        requests
                            .set_status(
                                &rid,
                                RequestStatus::Failed {
                                    reason: "Wallet is locked".into(),
                                },
                            )
                            .await;
                        return;
                    };
                    if let Some(primary) = mentions_to_send.first() {
                        let program = *primary;
                        let kc = w.storage().key_chain();
                        let mut accounts = Vec::with_capacity(mentions_to_send.len());
                        for (idx, &acc) in mentions_to_send.iter().enumerate() {
                            let is_private = is_private_account_local(kc, acc);
                            let is_owned_public = kc.public_account_ids().any(|(id, _)| id == acc);

                            let identity = if is_private {
                                wallet::AccountIdentity::PrivateOwned(acc)
                            } else if idx == 0 || is_owned_public {
                                wallet::AccountIdentity::Public(acc)
                            } else {
                                wallet::AccountIdentity::PublicNoSign(acc)
                            };
                            accounts.push(identity.select_program_actor_state(program));
                        }
                        let is_primary_private = is_private_account_local(kc, *primary);
                        let pre_nonce = if !is_primary_private {
                            w.get_accounts_nonces(&[*primary])
                                .await
                                .ok()
                                .and_then(|v| v.into_iter().next())
                        } else {
                            None
                        };
                        match w.send_pub_tx(accounts, 0, calldata_to_send).await {
                            Ok(tx_hash) => {
                                if let Some(tx_nonce) = pre_nonce {
                                    submitted_tx_nonces
                                        .insert(hex::encode(tx_hash), (*primary, tx_nonce));
                                }
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
        let submitted_tx_nonces = self.submitted_tx_nonces.clone();
        let rid = request_id.clone();
        let timeout = self.config.approval_timeout;

        tokio::spawn(async move {
            let decision = handler.request_approval(prompt, timeout).await;
            match decision {
                ApprovalDecision::Approve => {
                    requests.set_status(&rid, RequestStatus::Approved).await;
                    let guard = wallet.lock().await;
                    let Some(w) = guard.as_ref() else {
                        requests
                            .set_status(
                                &rid,
                                RequestStatus::Failed {
                                    reason: "Wallet is locked".into(),
                                },
                            )
                            .await;
                        return;
                    };

                    let kc = w.storage().key_chain();
                    let mut accounts = Vec::with_capacity(mentions.len());
                    for &acc in &mentions {
                        let is_private = is_private_account_local(kc, acc);
                        let is_owned_public = kc.public_account_ids().any(|(id, _)| id == acc);

                        let identity = if is_private {
                            wallet::AccountIdentity::PrivateOwned(acc)
                        } else if acc == payer_id || is_owned_public {
                            wallet::AccountIdentity::Public(acc)
                        } else {
                            wallet::AccountIdentity::PublicNoSign(acc)
                        };
                        accounts.push(identity.select_program_actor_state(program_id));
                    }

                    let is_payer_private = is_private_account_local(kc, payer_id);
                    let pre_nonce = if !is_payer_private {
                        w.get_accounts_nonces(&[payer_id])
                            .await
                            .ok()
                            .and_then(|v| v.into_iter().next())
                    } else {
                        None
                    };
                    match w
                        .send_pub_tx_paid_by(accounts, 0, calldata, Some(payer_id))
                        .await
                    {
                        Ok(tx_hash) => {
                            if let Some(tx_nonce) = pre_nonce {
                                submitted_tx_nonces
                                    .insert(hex::encode(tx_hash), (payer_id, tx_nonce));
                            }
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
