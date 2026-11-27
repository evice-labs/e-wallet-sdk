use e_wallet_sdk::*;
use integration_tests::harness::{MockApprovalHandler, TestEnv};
use sdk_engine::SdkEngine;
use sdk_engine::api::{AccountPrivacy, DAppApi, OwnerApi};
use sdk_engine::approval::ApprovalDecision;
use sdk_engine::session::GrantKind;
use std::ffi::{CStr, CString};
use std::sync::Arc;

#[tokio::test]
async fn test_wallet_lifecycle_and_account_derivation() {
    let env = TestEnv::new();
    let handler = Arc::new(MockApprovalHandler::new(ApprovalDecision::Approve));

    // Initialize engine with clean temporary storage
    let engine = SdkEngine::open_or_init(env.data_dir.clone(), "testpass_master_123", handler)
        .expect("failed to initialize wallet engine");

    let _mnemonic = engine
        .create_wallet("testpass_master_123")
        .await
        .expect("failed to create wallet");

    // Check SDK info
    let info = engine.sdk_info().await.expect("failed to get sdk info");
    assert_eq!(info.api_version, sdk_engine::API_VERSION);
    assert!(info.wallet_initialized);
    assert!(!info.wallet_locked);

    // Derive public account
    let pub_acc = engine
        .create_account_public(None)
        .await
        .expect("failed to derive public account");

    // Derive private shielded account
    let priv_acc = engine
        .create_account_private(Some("E2E Shielded Pool".to_string()))
        .await
        .expect("failed to derive private account");

    // Verify both accounts are listed
    let accounts = engine
        .list_all_accounts()
        .await
        .expect("failed to list accounts");
    assert!(
        accounts
            .iter()
            .any(|a| a.account_id == pub_acc && a.privacy == AccountPrivacy::Public)
    );
    assert!(
        accounts
            .iter()
            .any(|a| a.account_id == priv_acc && a.privacy == AccountPrivacy::Private)
    );

    // Update label
    engine
        .set_account_label(pub_acc, "Renamed Treasury".to_string(), false)
        .await
        .expect("failed to rename account");
    let updated = engine
        .list_all_accounts()
        .await
        .expect("failed to list accounts");
    let found = updated.iter().find(|a| a.account_id == pub_acc).unwrap();
    assert_eq!(found.label.as_deref(), Some("Renamed Treasury"));

    // Lock the wallet
    engine.lock().await.expect("failed to lock wallet");
    let locked_info = engine.sdk_info().await.expect("failed to get sdk info");
    assert!(locked_info.wallet_locked);

    // Operations requiring private keys should fail when locked
    let err = engine.create_account_public(None).await;
    assert!(err.is_err());

    // Security check: Unlock wallet with WRONG password must fail with InvalidPassword
    let bad_unlock = engine.unlock("wrong_password_xyz").await;
    assert!(
        matches!(
            bad_unlock,
            Err(sdk_engine::error::SdkError::InvalidPassword)
        ),
        "unlock with wrong password must return InvalidPassword error, got: {:?}",
        bad_unlock
    );
    let still_locked_info = engine.sdk_info().await.expect("failed to get sdk info");
    assert!(still_locked_info.wallet_locked);

    // Unlock wallet with correct password
    engine
        .unlock("testpass_master_123")
        .await
        .expect("failed to unlock wallet");
    let unlocked_info = engine.sdk_info().await.expect("failed to get sdk info");
    assert!(!unlocked_info.wallet_locked);

    // Verify accounts persist after unlock
    let post_unlock_accs = engine
        .list_all_accounts()
        .await
        .expect("failed to list accounts post-unlock");
    assert!(post_unlock_accs.len() >= 2);
    assert!(post_unlock_accs.iter().any(|a| a.account_id == pub_acc));
    assert!(post_unlock_accs.iter().any(|a| a.account_id == priv_acc));
}

async fn spawn_mock_balance_rpc() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock rpc listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = Vec::with_capacity(2048);
                let mut chunk = [0u8; 1024];
                let header_end;
                loop {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = pos + 4;
                        break;
                    }
                }
                let headers = String::from_utf8_lossy(&buf[..header_end]);
                let content_len = headers
                    .lines()
                    .find_map(|line| {
                        let (k, v) = line.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                while buf.len() < header_end + content_len {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let req_body = &buf[header_end..(header_end + content_len).min(buf.len())];
                let parsed = serde_json::from_slice::<serde_json::Value>(req_body).ok();
                let id = parsed
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .cloned()
                    .unwrap_or(serde_json::json!(1));
                let method = parsed
                    .as_ref()
                    .and_then(|v| v.get("method"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                let resp_json = match method {
                    "getAccountBalance" | "getLastBlockId" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": 0
                    }),
                    _ => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32601, "message": "Method not mocked"}
                    }),
                }
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_json}",
                    resp_json.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn test_dapp_session_and_granular_grants() {
    let env = TestEnv::new();
    let mock_url = spawn_mock_balance_rpc().await;
    let wallet_cfg = serde_json::json!({
        "sequencer": {
            "sequencer_addr": mock_url,
        },
        "seq_poll_timeout": "5s",
        "seq_tx_poll_max_blocks": 5,
        "seq_poll_max_retries": 2,
        "seq_block_poll_max_amount": 10
    });
    std::fs::write(
        env.data_dir.join("wallet_config.json"),
        serde_json::to_string_pretty(&wallet_cfg).unwrap(),
    )
    .expect("write mock wallet_config.json");

    let handler = Arc::new(MockApprovalHandler::new(ApprovalDecision::Approve));
    let engine = SdkEngine::open_or_init(env.data_dir.clone(), "testpass_master_123", handler)
        .expect("failed to initialize wallet engine");

    let alice = engine
        .create_account_public(Some("Alice".to_string()))
        .await
        .expect("failed to derive alice");
    let bob = engine
        .create_account_public(Some("Bob".to_string()))
        .await
        .expect("failed to derive bob");

    // Connect dApp
    let session_id = engine
        .connect("dex.lez".to_string(), 1)
        .await
        .expect("failed to connect dApp");

    // Without grants, listing authorized accounts should be empty
    let initial_accounts = engine
        .list_accounts(&session_id)
        .await
        .expect("failed to list dApp accounts");
    assert!(initial_accounts.is_empty());

    // Balance query without Read grant must fail
    let bal_err = engine.get_balance(&session_id, &alice).await;
    assert!(bal_err.is_err());

    // Transfer proposal without Propose grant must fail
    let prop_err = engine
        .propose_native_transfer(&session_id, alice, bob.to_string(), 500, None)
        .await;
    assert!(prop_err.is_err());

    // Issue Read grant for Alice
    engine
        .grant_account(&session_id, alice, GrantKind::Read)
        .await
        .expect("failed to issue read grant");

    // Now balance query succeeds
    let balance = engine
        .get_balance(&session_id, &alice)
        .await
        .expect("failed to get balance with read grant");
    assert_eq!(balance, 0);

    // But transfer proposal still fails (needs Propose grant)
    let prop_err2 = engine
        .propose_native_transfer(&session_id, alice, bob.to_string(), 500, None)
        .await;
    assert!(prop_err2.is_err());

    // Issue Propose grant for Alice
    engine
        .grant_account(&session_id, alice, GrantKind::Propose)
        .await
        .expect("failed to issue propose grant");

    // Propose transfer now succeeds, producing RequestId
    let request_id = engine
        .propose_native_transfer(&session_id, alice, bob.to_string(), 500, None)
        .await
        .expect("failed to propose transfer with propose grant");

    // Check request status
    let status = engine
        .get_request_status(&request_id)
        .await
        .expect("failed to query request status");
    // Under mock approve handler, it will transition
    assert!(matches!(
        status,
        sdk_engine::RequestStatus::PendingApproval
            | sdk_engine::RequestStatus::Approved
            | sdk_engine::RequestStatus::Failed { .. }
    ));

    // Revoke session
    engine
        .revoke_session(&session_id)
        .await
        .expect("failed to revoke session");

    // Operations after revocation must fail
    let post_revoke_err = engine.get_balance(&session_id, &alice).await;
    assert!(post_revoke_err.is_err());
}

#[tokio::test]
async fn test_identity_accountability_scoped_nullifiers() {
    let env = TestEnv::new();
    let handler = Arc::new(MockApprovalHandler::new(ApprovalDecision::Approve));
    let engine = SdkEngine::open_or_init(env.data_dir.clone(), "testpass_master_123", handler)
        .expect("failed to initialize wallet engine");

    let _mnemonic = engine
        .create_wallet("testpass_master_123")
        .await
        .expect("failed to create wallet");

    let session_id = engine
        .connect("forum.lez".to_string(), 1)
        .await
        .expect("failed to connect");

    // Request IdentityRead grant
    engine
        .request_identity_access(&session_id, GrantKind::IdentityRead)
        .await
        .expect("failed to request identity access");

    // Retrieve master commitment
    let commitment = engine
        .get_identity_commitment(&session_id)
        .await
        .expect("failed to get identity commitment");
    assert_eq!(commitment.0.len(), 32);

    // Derive scoped nullifier for Governance Alpha
    let nullifier_alpha = engine
        .derive_scoped_nullifier(&session_id, "governance_alpha")
        .await
        .expect("failed to derive alpha nullifier");

    // Derive scoped nullifier for Governance Beta
    let nullifier_beta = engine
        .derive_scoped_nullifier(&session_id, "governance_beta")
        .await
        .expect("failed to derive beta nullifier");

    // Cross-dApp Unlinkability Guarantee: different scopes must produce different nullifiers
    assert_ne!(nullifier_alpha, nullifier_beta);

    // Determinism Guarantee: same scope must produce identical nullifier
    let nullifier_alpha_repeat = engine
        .derive_scoped_nullifier(&session_id, "governance_alpha")
        .await
        .expect("failed to re-derive alpha nullifier");
    assert_eq!(nullifier_alpha, nullifier_alpha_repeat);

    // Request IdentityPropose grant for attestation signing and SSS
    engine
        .request_identity_access(&session_id, GrantKind::IdentityPropose)
        .await
        .expect("failed to request propose grant");

    // Sign BIP-340 Schnorr attestation
    let attestation = engine
        .sign_identity_attestation(&session_id, "governance_alpha", b"vote_ballot_proposal_42")
        .await
        .expect("failed to sign identity attestation");
    assert_eq!(attestation.signature.len(), 64);

    // Two-Tier Shamir Secret Sharing registration payload
    let sk1 = sdk_engine::identity::generate_ephemeral_scalar();
    let pk1 = sdk_engine::identity::derive_xonly_pubkey(&sk1);
    let sk2 = sdk_engine::identity::generate_ephemeral_scalar();
    let pk2 = sdk_engine::identity::derive_xonly_pubkey(&sk2);
    let sk3 = sdk_engine::identity::generate_ephemeral_scalar();
    let pk3 = sdk_engine::identity::derive_xonly_pubkey(&sk3);
    let trustee_pubkeys = vec![pk1, pk2, pk3];
    let reg_payload = engine
        .prepare_identity_registration(&session_id, "community_council", &trustee_pubkeys, 2)
        .await
        .expect("failed to prepare SSS registration");
    assert_eq!(reg_payload.threshold_k, 2);
    assert_eq!(reg_payload.encrypted_shares.len(), 3);
}

unsafe extern "C" fn mock_e2e_approval_cb(
    _prompt_json: *const std::os::raw::c_char,
    _user_data: *mut std::ffi::c_void,
) -> i32 {
    -1 // deferred modal
}

#[test]
fn test_ffi_c_abi_end_to_end_lifecycle() {
    let env = TestEnv::new();
    let path_str = CString::new(env.data_dir.to_str().unwrap()).unwrap();

    let engine = unsafe { e_wallet_engine_new(path_str.as_ptr()) };
    assert!(!engine.is_null());

    // Register callback
    let cb_res = unsafe {
        e_wallet_set_approval_callback(engine, mock_e2e_approval_cb, std::ptr::null_mut())
    };
    unsafe { e_wallet_free_string(cb_res) };

    // Create wallet via FFI
    let pass = CString::new("testpass_ffi_123").unwrap();
    let res_ptr = unsafe { e_wallet_create_wallet(engine, pass.as_ptr()) };
    let res_str = unsafe { CStr::from_ptr(res_ptr) }.to_str().unwrap();
    let res_json: serde_json::Value = serde_json::from_str(res_str).unwrap();
    assert_eq!(res_json["ok"], true);
    unsafe { e_wallet_free_string(res_ptr) };

    // Create sender and recipient
    let l_alice = CString::new("Alice FFI").unwrap();
    let alice_ptr = unsafe { e_wallet_create_account_public(engine, l_alice.as_ptr()) };
    let alice_str = unsafe { CStr::from_ptr(alice_ptr) }.to_str().unwrap();
    let alice_json: serde_json::Value = serde_json::from_str(alice_str).unwrap();
    let alice_acc = alice_json["account_id"].as_str().unwrap().to_string();
    unsafe { e_wallet_free_string(alice_ptr) };

    let l_bob = CString::new("Bob FFI").unwrap();
    let bob_ptr = unsafe { e_wallet_create_account_public(engine, l_bob.as_ptr()) };
    let bob_str = unsafe { CStr::from_ptr(bob_ptr) }.to_str().unwrap();
    let bob_json: serde_json::Value = serde_json::from_str(bob_str).unwrap();
    let bob_acc = bob_json["account_id"].as_str().unwrap().to_string();
    unsafe { e_wallet_free_string(bob_ptr) };

    // Connect dApp
    let caller = CString::new("e2e_dapp").unwrap();
    let conn_ptr = unsafe { e_wallet_connect(engine, caller.as_ptr(), 1) };
    let conn_str = unsafe { CStr::from_ptr(conn_ptr) }.to_str().unwrap();
    let conn_json: serde_json::Value = serde_json::from_str(conn_str).unwrap();
    let sid = conn_json["session_id"].as_str().unwrap().to_string();
    unsafe { e_wallet_free_string(conn_ptr) };

    // Grant propose
    let sid_cstr = CString::new(sid).unwrap();
    let alice_cstr = CString::new(alice_acc).unwrap();
    let grant_cstr = CString::new("propose").unwrap();
    let g_ptr = unsafe {
        e_wallet_grant_account(
            engine,
            sid_cstr.as_ptr(),
            alice_cstr.as_ptr(),
            grant_cstr.as_ptr(),
        )
    };
    unsafe { e_wallet_free_string(g_ptr) };

    // Propose transfer
    let bob_cstr = CString::new(bob_acc).unwrap();
    let amt_cstr = CString::new("1000").unwrap();
    let prop_ptr = unsafe {
        e_wallet_propose_native_transfer(
            engine,
            sid_cstr.as_ptr(),
            alice_cstr.as_ptr(),
            bob_cstr.as_ptr(),
            amt_cstr.as_ptr(),
        )
    };
    let prop_str = unsafe { CStr::from_ptr(prop_ptr) }.to_str().unwrap();
    let prop_json: serde_json::Value = serde_json::from_str(prop_str).unwrap();
    assert_eq!(prop_json["ok"], true);
    let req_id = prop_json["request_id"].as_str().unwrap().to_string();
    unsafe { e_wallet_free_string(prop_ptr) };

    std::thread::sleep(std::time::Duration::from_millis(50));

    // Submit user decision: 0 (Reject)
    let req_cstr = CString::new(req_id.clone()).unwrap();
    let dec_ptr = unsafe { e_wallet_submit_approval_decision(engine, req_cstr.as_ptr(), 0) };
    let dec_str = unsafe { CStr::from_ptr(dec_ptr) }.to_str().unwrap();
    let dec_json: serde_json::Value = serde_json::from_str(dec_str).unwrap();
    assert_eq!(dec_json["ok"], true);
    unsafe { e_wallet_free_string(dec_ptr) };

    std::thread::sleep(std::time::Duration::from_millis(50));

    // Verify rejection status
    let status_ptr = unsafe { e_wallet_get_request_status(engine, req_cstr.as_ptr()) };
    let status_str = unsafe { CStr::from_ptr(status_ptr) }.to_str().unwrap();
    let status_json: serde_json::Value = serde_json::from_str(status_str).unwrap();
    assert_eq!(status_json["ok"], true);
    assert_eq!(status_json["status"], "rejected");
    unsafe { e_wallet_free_string(status_ptr) };

    // Free engine
    unsafe { e_wallet_engine_free(engine) };
}

#[tokio::test]
async fn test_security_flaws_and_access_control() {
    let env = TestEnv::new();
    let handler = Arc::new(MockApprovalHandler::new(ApprovalDecision::Approve));
    let engine = SdkEngine::open_or_init(
        env.data_dir.clone(),
        "correct_master_pass_123",
        handler.clone(),
    )
    .expect("failed to initialize wallet engine");

    let _mnemonic = engine
        .create_wallet("correct_master_pass_123")
        .await
        .expect("failed to create wallet");

    let alice = engine
        .create_account_public(Some("Alice Safe".to_string()))
        .await
        .expect("failed to derive alice");
    let bob = engine
        .create_account_public(Some("Bob Safe".to_string()))
        .await
        .expect("failed to derive bob");

    // 1. Password Verification Security
    engine.lock().await.expect("failed to lock");

    // Test multiple wrong password permutations
    for bad_pwd in &[
        "",
        "wrong",
        "correct_master_pass",
        "correct_master_pass_1234",
    ] {
        let res = engine.unlock(bad_pwd).await;
        assert!(
            matches!(res, Err(sdk_engine::error::SdkError::InvalidPassword)),
            "unlock with '{}' must return InvalidPassword, got {:?}",
            bad_pwd,
            res
        );
        let info = engine.sdk_info().await.expect("info");
        assert!(
            info.wallet_locked,
            "wallet must remain locked after failed attempt"
        );
    }

    // Unlock with correct password
    engine
        .unlock("correct_master_pass_123")
        .await
        .expect("unlock with correct password must succeed");

    // 2. Granular Grant Boundary & Privilege Escalation Protection
    let session_id = engine
        .connect("untrusted_dapp.lez".to_string(), 1)
        .await
        .expect("connect dApp");

    // Attempt calling identity API without grant -> must fail
    let id_err = engine.get_identity_commitment(&session_id).await;
    assert!(matches!(
        id_err,
        Err(sdk_engine::error::SdkError::InsufficientGrant)
    ));

    let null_err = engine.derive_scoped_nullifier(&session_id, "scope_a").await;
    assert!(matches!(
        null_err,
        Err(sdk_engine::error::SdkError::InsufficientGrant)
    ));

    // Attempt signing attestation with only IdentityRead -> must fail (requires IdentityPropose)
    engine
        .request_identity_access(&session_id, GrantKind::IdentityRead)
        .await
        .expect("grant IdentityRead");
    let sign_err = engine
        .sign_identity_attestation(&session_id, "scope_a", b"malicious_message")
        .await;
    assert!(matches!(
        sign_err,
        Err(sdk_engine::error::SdkError::InsufficientGrant)
    ));

    // 3. Program Verification & Transaction Inspection
    engine
        .grant_account(&session_id, alice, GrantKind::Propose)
        .await
        .expect("grant propose");

    let req_id = engine
        .propose_native_transfer(&session_id, alice, bob.to_string(), 250, None)
        .await
        .expect("propose native transfer");

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Verify approval prompt carries verification status
    let prompts = handler.recorded_prompts().await;
    assert!(!prompts.is_empty());
    let last_prompt = prompts.last().unwrap();
    assert!(
        last_prompt
            .verification_status
            .as_deref()
            .unwrap_or_default()
            .contains("Verified"),
        "Native token transfer must be recognized as Verified, got: {:?}",
        last_prompt.verification_status
    );

    // Verify request ID matches
    assert_eq!(last_prompt.request.id, req_id);

    // 4. Session Revocation Security
    engine
        .revoke_session(&session_id)
        .await
        .expect("revoke session");

    // Post-revocation access is completely blocked
    let post_rev_bal = engine.get_balance(&session_id, &alice).await;
    assert!(matches!(
        post_rev_bal,
        Err(sdk_engine::error::SdkError::SessionNotFound(_))
    ));
}

#[tokio::test]
async fn test_lns_name_resolution_and_ffi_safety() {
    let env = TestEnv::new();
    let handler = Arc::new(MockApprovalHandler::new(ApprovalDecision::Approve));
    let engine = SdkEngine::open_or_init(env.data_dir.clone(), "testpass_lns_123", handler)
        .expect("failed to initialize wallet engine");

    let alice = engine
        .create_account_public(Some("Alice".to_string()))
        .await
        .expect("create alice");

    let session_id = engine
        .connect("test_resolver.lez".to_string(), 1)
        .await
        .expect("connect");

    // 1. Invalid Name Format Rejections (SPEC §2)
    let invalid_names = [
        "-leading-hyphen.lez",
        "trailing-hyphen-.lez",
        ".lez",
        "has space.lez",
        "UPPER!.lez",
    ];
    for invalid in &invalid_names {
        let res = engine.resolve_name(&session_id, invalid).await;
        assert!(res.is_err(), "Invalid name '{}' must be rejected", invalid);
    }

    // Bare name without .lez passed to resolve_name directly must return InvalidArgument
    let bare_res = engine.resolve_name(&session_id, "bob").await;
    assert!(
        matches!(
            bare_res,
            Err(sdk_engine::error::SdkError::InvalidArgument(_))
        ),
        "Bare name without .lez must return InvalidArgument"
    );

    // Unregistered name on testnet must return NameResolutionError
    let unreg_res = engine
        .resolve_name(&session_id, "definitely-unregistered-xyz999.lez")
        .await;
    assert!(
        matches!(
            unreg_res,
            Err(sdk_engine::error::SdkError::NameResolutionError(_))
        ),
        "Unregistered name must return NameResolutionError"
    );

    // 2. Transfer to Unregistered LNS Name Must Be Rejected Before Transaction Creation
    engine
        .grant_account(&session_id, alice, GrantKind::Propose)
        .await
        .expect("grant propose");

    let transfer_unreg = engine
        .propose_native_transfer(
            &session_id,
            alice,
            "definitely-unregistered-xyz999.lez".to_string(),
            100,
            None,
        )
        .await;
    assert!(
        matches!(
            transfer_unreg,
            Err(sdk_engine::error::SdkError::NameResolutionError(_))
        ),
        "Transfer to unregistered .lez name must fail without proposing a transaction"
    );
}

#[test]
fn test_lns_ffi_c_abi_safety() {
    let ffi_env = TestEnv::new();
    let path_str = CString::new(ffi_env.data_dir.to_str().unwrap()).unwrap();
    let ffi_engine = unsafe { e_wallet_engine_new(path_str.as_ptr()) };
    assert!(!ffi_engine.is_null());

    let name = CString::new("alice.lez").unwrap();
    let sess = CString::new("mock_session_id").unwrap();

    // 1. Null engine pointer safety
    let null_ptr_res =
        unsafe { e_wallet_resolve_name(std::ptr::null_mut(), sess.as_ptr(), name.as_ptr()) };
    let null_res_str = unsafe { CStr::from_ptr(null_ptr_res) }.to_str().unwrap();
    let null_res_json: serde_json::Value = serde_json::from_str(null_res_str).unwrap();
    assert_eq!(null_res_json["ok"], false);
    assert_eq!(null_res_json["error"], "Null engine handle");
    unsafe { e_wallet_free_string(null_ptr_res) };

    // 2. Null parameter pointers safety
    let null_param_res =
        unsafe { e_wallet_resolve_name(ffi_engine, std::ptr::null(), name.as_ptr()) };
    let null_param_str = unsafe { CStr::from_ptr(null_param_res) }.to_str().unwrap();
    let null_param_json: serde_json::Value = serde_json::from_str(null_param_str).unwrap();
    assert_eq!(null_param_json["ok"], false);
    unsafe { e_wallet_free_string(null_param_res) };

    // 3. Invalid/unauthorized session call safety
    let uninit_res = unsafe { e_wallet_resolve_name(ffi_engine, sess.as_ptr(), name.as_ptr()) };
    let uninit_str = unsafe { CStr::from_ptr(uninit_res) }.to_str().unwrap();
    let uninit_json: serde_json::Value = serde_json::from_str(uninit_str).unwrap();
    assert_eq!(uninit_json["ok"], false);
    unsafe { e_wallet_free_string(uninit_res) };

    unsafe { e_wallet_engine_free(ffi_engine) };
}

#[tokio::test]
async fn test_engine_transfers_and_holdings() {
    let env = TestEnv::new();
    let handler = Arc::new(MockApprovalHandler::new(ApprovalDecision::Approve));
    let engine =
        SdkEngine::open_or_init(env.data_dir.clone(), "testpass123", handler).expect("engine");

    let _mnemonic = engine.create_wallet("testpass123").await.expect("wallet");
    let alice = engine
        .create_account_public(Some("Alice".into()))
        .await
        .expect("alice");
    let bob = engine
        .create_account_public(Some("Bob".into()))
        .await
        .expect("bob");

    let session_id = engine
        .connect("dapp.test".into(), 1)
        .await
        .expect("connect");
    engine
        .grant_account(&session_id, alice, GrantKind::Propose)
        .await
        .expect("grant");

    // 1. Guard clause: zero amount must fail
    let err_zero = engine
        .propose_native_transfer(&session_id, alice, bob.to_string(), 0, None)
        .await;
    assert!(err_zero.is_err(), "Zero transfer must fail");

    // 2. Guard clause: self-transfer with same privacy must fail
    let err_self = engine
        .propose_native_transfer(&session_id, alice, alice.to_string(), 100, None)
        .await;
    assert!(err_self.is_err(), "Self-transfer must fail");

    // 3. Token holdings management
    let def_id = lee::AccountId::default();
    let holding_id = alice;
    engine
        .register_token_holding(
            holding_id,
            def_id,
            "TEST".into(),
            "Test Token".into(),
            false,
        )
        .await
        .expect("register token holding");

    let holdings_owner = engine.list_all_token_holdings().await.expect("list all");
    assert_eq!(holdings_owner.len(), 1);
    assert_eq!(holdings_owner[0].symbol, "TEST");
    assert_eq!(holdings_owner[0].holding_id, holding_id);

    let holdings_dapp = engine
        .list_token_holdings(&session_id)
        .await
        .expect("list dapp");
    assert_eq!(holdings_dapp.len(), 1);
    assert_eq!(holdings_dapp[0].symbol, "TEST");

    // 4. Contract call proposal
    let req_id = engine
        .propose_contract_call(
            &session_id,
            def_id,
            alice,
            vec![alice],
            vec![1, 2, 3],
            Some("Test Call".into()),
        )
        .await
        .expect("propose contract call");

    let status = engine.get_request_status(&req_id).await.expect("status");
    assert!(matches!(
        status,
        sdk_engine::request::RequestStatus::PendingApproval
            | sdk_engine::request::RequestStatus::Approved
            | sdk_engine::request::RequestStatus::Failed { .. }
    ));
}

#[test]
fn test_ffi_contract_calls_and_holdings() {
    let env = TestEnv::new();
    let ffi_data_dir = CString::new(env.data_dir.to_str().unwrap()).unwrap();
    let ffi_engine = unsafe { e_wallet_engine_new(ffi_data_dir.as_ptr()) };
    assert!(!ffi_engine.is_null());

    let h_str = CString::new(lee::AccountId::default().to_string()).unwrap();
    let d_str = CString::new(lee::AccountId::default().to_string()).unwrap();
    let sym_str = CString::new("FFITOKEN").unwrap();
    let name_str = CString::new("FFI Token").unwrap();

    let reg_res = unsafe {
        e_wallet_register_token_holding(
            ffi_engine,
            h_str.as_ptr(),
            d_str.as_ptr(),
            sym_str.as_ptr(),
            name_str.as_ptr(),
            false,
        )
    };
    let reg_str = unsafe { CStr::from_ptr(reg_res) }.to_str().unwrap();
    let reg_json: serde_json::Value = serde_json::from_str(reg_str).unwrap();
    assert_eq!(reg_json["ok"], true);
    unsafe { e_wallet_free_string(reg_res) };

    let list_res = unsafe { e_wallet_list_all_token_holdings(ffi_engine) };
    let list_str = unsafe { CStr::from_ptr(list_res) }.to_str().unwrap();
    let list_json: serde_json::Value = serde_json::from_str(list_str).unwrap();
    assert_eq!(list_json["ok"], true);
    assert_eq!(list_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(list_json["data"][0]["symbol"], "FFITOKEN");
    unsafe { e_wallet_free_string(list_res) };

    unsafe { e_wallet_engine_free(ffi_engine) };
}

fn to_hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn rpc_post(
    sequencer_url: &str,
    method: &str,
    params: serde_json::Value,
) -> Option<serde_json::Value> {
    use std::io::{Read, Write};
    let host_port = sequencer_url
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let mut stream = std::net::TcpStream::connect(host_port).ok()?;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    })
    .to_string();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).ok()?;
    let (_, resp_body) = resp.split_once("\r\n\r\n")?;
    serde_json::from_str::<serde_json::Value>(resp_body)
        .ok()?
        .get("result")
        .cloned()
}

async fn wait_for_submitted_tx(
    engine: &SdkEngine,
    request_id: &sdk_engine::request::RequestId,
    label: &str,
) -> String {
    use sdk_engine::request::RequestStatus;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let status = engine
            .get_request_status(request_id)
            .await
            .expect("get_request_status failed");
        match status {
            RequestStatus::Submitted { tx_hash } | RequestStatus::Confirmed { tx_hash, .. } => {
                let raw: [u8; 32] = tx_hash.into();
                return to_hex_lower(&raw);
            }
            RequestStatus::Failed { reason } => {
                panic!("{label} failed: {reason}");
            }
            RequestStatus::Rejected | RequestStatus::Cancelled | RequestStatus::TimedOut => {
                panic!("{label} ended with {status:?}");
            }
            RequestStatus::PendingApproval | RequestStatus::Approved => {
                if tokio::time::Instant::now() >= deadline {
                    panic!("{label} timed out waiting for submission (last={status:?})");
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }
    }
}

async fn wait_for_confirmed_tx(engine: &SdkEngine, tx_hash_hex: &str, label: &str) -> u64 {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
    loop {
        let st = engine
            .get_transaction_status(tx_hash_hex)
            .await
            .expect("get_transaction_status failed");
        if st.status == "confirmed" {
            return st
                .block_id
                .expect("confirmed transaction should report block_id");
        }
        assert!(
            !st.status.starts_with("failed") && st.status != "reverted",
            "{label} tx {tx_hash_hex} ended with status: {}",
            st.status
        );
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{label} tx {tx_hash_hex} timed out waiting for confirmation (last={})",
                st.status
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

fn log_tx_receipt_summary(sequencer_url: &str, label: &str, tx_hash_hex: &str) {
    let receipt = rpc_post(
        sequencer_url,
        "getTransactionReceipt",
        serde_json::json!([tx_hash_hex]),
    );
    let fee_state = rpc_post(sequencer_url, "getFeeState", serde_json::json!([]));
    println!("[{label}] receipt={receipt:?} | head_fee_state={fee_state:?}");
}

/// Live end-to-end verification against the standalone LEZ v0.4.0 sequencer (`http://127.0.0.1:3040`).
/// Exercises PoW mining + `channel_deposit` (bridge disbursement) and all four native transfer
/// routes (`Public -> Public`, `Public -> Private`, `Private -> Private`, `Private -> Public`).
///
/// Run explicitly with:
/// `RISC0_DEV_MODE=1 cargo test -p integration-tests --test e2e_tests test_live_sequencer_transfer_routes_and_bridge -- --ignored --nocapture`
#[tokio::test]
#[ignore = "requires a running standalone LEZ v0.4.0 sequencer (e.g. http://127.0.0.1:3040)"]
async fn test_live_sequencer_transfer_routes_and_bridge() {
    let sequencer_url =
        std::env::var("LEZ_SEQUENCER_URL").unwrap_or_else(|_| "http://127.0.0.1:3040".to_string());
    let env = TestEnv::new();

    // Point WalletConfig in TestEnv to the running standalone sequencer before engine init.
    let wallet_cfg = serde_json::json!({
        "sequencer": {
            "sequencer_addr": sequencer_url,
        },
        "seq_poll_timeout": "30s",
        "seq_tx_poll_max_blocks": 20,
        "seq_poll_max_retries": 10,
        "seq_block_poll_max_amount": 100
    });
    std::fs::write(
        env.data_dir.join("wallet_config.json"),
        serde_json::to_string_pretty(&wallet_cfg).unwrap(),
    )
    .expect("write wallet_config.json");

    let handler = Arc::new(MockApprovalHandler::new(ApprovalDecision::Approve));
    let engine =
        SdkEngine::open_or_init(env.data_dir.clone(), "live_test_pass_123", handler).expect("init");
    let _mnemonic = engine
        .create_wallet("live_test_pass_123")
        .await
        .expect("create_wallet");
    let session = engine
        .connect("internal_wallet_ui".to_string(), 1)
        .await
        .expect("connect internal_wallet_ui");

    let pub_a = engine
        .create_account_public(Some("PubA".into()))
        .await
        .expect("pub_a");
    let pub_b = engine
        .create_account_public(Some("PubB".into()))
        .await
        .expect("pub_b");
    let priv_a = engine
        .create_account_private(Some("PrivA".into()))
        .await
        .expect("priv_a");
    let priv_b = engine
        .create_account_private(Some("PrivB".into()))
        .await
        .expect("priv_b");

    // 0. PoW mining + Bridge channel_deposit (Genesis Alice -> PubA)
    // Mine 5 tickets (50_000_000 units) so PubA and PrivA comfortably cover all 4 routes
    // including two subsequent private-paid txs whose reserves (~8.4M each at base_fee=8)
    // account for `PROOF_BYTES` (223_047 bytes) and headroom.
    let _ = engine.pow_start_mining(1).await.expect("pow_start_mining");
    let mine_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(35);
    loop {
        let st = engine.pow_status().await.expect("pow_status");
        if st.blocks_mined >= 5 {
            println!(
                "[PoW] mined blocks={}, unbridged={}",
                st.blocks_mined, st.unbridged_balance
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < mine_deadline,
            "PoW mining timed out waiting for 5 tickets (got {})",
            st.blocks_mined
        );
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    let _ = engine.pow_stop_mining().await.expect("pow_stop_mining");

    let deposit = engine
        .channel_deposit(&pub_a.to_string(), "50000000")
        .await
        .expect("channel_deposit failed");
    let dep_block = wait_for_confirmed_tx(&engine, &deposit.tx_hash, "channel_deposit").await;
    log_tx_receipt_summary(&sequencer_url, "channel_deposit", &deposit.tx_hash);
    let pub_a_bal_0 = engine
        .get_balance(&session, &pub_a)
        .await
        .expect("bal pub_a");
    assert_eq!(pub_a_bal_0, 50_000_000);
    println!(
        "[channel_deposit] tx={} block={} amount={} pub_a_after={}",
        deposit.tx_hash, dep_block, deposit.amount_deposited, pub_a_bal_0
    );

    // 1. Public -> Public (PubA -> PubB: 500_000)
    let pub_pub_amount = 500_000u128;
    let req_1 = engine
        .propose_native_transfer(
            &session,
            pub_a,
            format!("Public/{pub_b}"),
            pub_pub_amount,
            Some(AccountPrivacy::Public),
        )
        .await
        .expect("propose Public -> Public");
    let tx_1_hex = wait_for_submitted_tx(&engine, &req_1, "Public -> Public").await;
    let blk_1 = wait_for_confirmed_tx(&engine, &tx_1_hex, "Public -> Public").await;
    log_tx_receipt_summary(&sequencer_url, "Public -> Public", &tx_1_hex);

    let pub_a_bal_1 = engine.get_balance(&session, &pub_a).await.expect("pub_a_1");
    let pub_b_bal_1 = engine.get_balance(&session, &pub_b).await.expect("pub_b_1");
    assert_eq!(pub_b_bal_1, pub_pub_amount);
    assert!(pub_a_bal_1 <= pub_a_bal_0 - pub_pub_amount);
    println!(
        "[Public -> Public] tx={} block={} pub_a: {} -> {} (fee={}), pub_b: 0 -> {}",
        tx_1_hex,
        blk_1,
        pub_a_bal_0,
        pub_a_bal_1,
        pub_a_bal_0 - pub_pub_amount - pub_a_bal_1,
        pub_b_bal_1
    );

    // 2. Public -> Private (Shield: PubA -> PrivA: 25_000_000)
    let shield_amount = 25_000_000u128;
    let req_2 = engine
        .propose_native_transfer(
            &session,
            pub_a,
            format!("Private/{priv_a}"),
            shield_amount,
            Some(AccountPrivacy::Public),
        )
        .await
        .expect("propose Public -> Private");
    let tx_2_hex = wait_for_submitted_tx(&engine, &req_2, "Public -> Private").await;
    let blk_2 = wait_for_confirmed_tx(&engine, &tx_2_hex, "Public -> Private").await;
    log_tx_receipt_summary(&sequencer_url, "Public -> Private", &tx_2_hex);

    let pub_a_bal_2 = engine.get_balance(&session, &pub_a).await.expect("pub_a_2");
    let priv_a_bal_2 = engine
        .get_balance(&session, &priv_a)
        .await
        .expect("priv_a_2");
    assert!(
        priv_a_bal_2 > 0 && priv_a_bal_2 <= shield_amount,
        "unexpected priv_a balance after shield: {priv_a_bal_2}"
    );
    println!(
        "[Public -> Private] tx={} block={} pub_a: {} -> {}, priv_a: 0 -> {} (fee_deducted={})",
        tx_2_hex,
        blk_2,
        pub_a_bal_1,
        pub_a_bal_2,
        priv_a_bal_2,
        (pub_a_bal_1 - pub_a_bal_2) - priv_a_bal_2
    );

    // 3. Private -> Private (PrivA -> PrivB: 4_000_000)
    let priv_priv_amount = 4_000_000u128;
    let req_3 = engine
        .propose_native_transfer(
            &session,
            priv_a,
            format!("Private/{priv_b}"),
            priv_priv_amount,
            Some(AccountPrivacy::Private),
        )
        .await
        .expect("propose Private -> Private");
    let tx_3_hex = wait_for_submitted_tx(&engine, &req_3, "Private -> Private").await;
    let blk_3 = wait_for_confirmed_tx(&engine, &tx_3_hex, "Private -> Private").await;
    log_tx_receipt_summary(&sequencer_url, "Private -> Private", &tx_3_hex);

    let priv_a_bal_3 = engine
        .get_balance(&session, &priv_a)
        .await
        .expect("priv_a_3");
    let priv_b_bal_3 = engine
        .get_balance(&session, &priv_b)
        .await
        .expect("priv_b_3");
    assert_eq!(priv_b_bal_3, priv_priv_amount);
    assert!(priv_a_bal_3 <= priv_a_bal_2 - priv_priv_amount);
    println!(
        "[Private -> Private] tx={} block={} priv_a: {} -> {} (private_fee={}), priv_b: 0 -> {}",
        tx_3_hex,
        blk_3,
        priv_a_bal_2,
        priv_a_bal_3,
        priv_a_bal_2 - priv_priv_amount - priv_a_bal_3,
        priv_b_bal_3
    );

    // 4. Private -> Public (Unshield: PrivA -> PubB: 2_000_000)
    let unshield_amount = 2_000_000u128;
    let req_4 = engine
        .propose_native_transfer(
            &session,
            priv_a,
            format!("Public/{pub_b}"),
            unshield_amount,
            Some(AccountPrivacy::Private),
        )
        .await
        .expect("propose Private -> Public");
    let tx_4_hex = wait_for_submitted_tx(&engine, &req_4, "Private -> Public").await;
    let blk_4 = wait_for_confirmed_tx(&engine, &tx_4_hex, "Private -> Public").await;
    log_tx_receipt_summary(&sequencer_url, "Private -> Public", &tx_4_hex);

    let priv_a_bal_4 = engine
        .get_balance(&session, &priv_a)
        .await
        .expect("priv_a_4");
    let pub_b_bal_4 = engine.get_balance(&session, &pub_b).await.expect("pub_b_4");
    assert_eq!(pub_b_bal_4, pub_b_bal_1 + unshield_amount);
    assert!(priv_a_bal_4 <= priv_a_bal_3 - unshield_amount);
    println!(
        "[Private -> Public] tx={} block={} priv_a: {} -> {} (private_fee={}), pub_b: {} -> {}",
        tx_4_hex,
        blk_4,
        priv_a_bal_3,
        priv_a_bal_4,
        priv_a_bal_3 - unshield_amount - priv_a_bal_4,
        pub_b_bal_1,
        pub_b_bal_4
    );
}
