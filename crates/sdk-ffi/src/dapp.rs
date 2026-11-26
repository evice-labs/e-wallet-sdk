use crate::*;

/// Connect an untrusted caller module and open a session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_connect(
    engine: *mut FfiSdkEngine,
    caller_identity: *const c_char,
    api_version: u32,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let caller_str = match unsafe { parse_c_str(caller_identity) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.connect(caller_str.to_string(), api_version))
        {
            Ok(session_id) => {
                let json = serde_json::json!({
                    "ok": true,
                    "session_id": session_id.0
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Disconnect and terminate a dApp session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_disconnect(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.disconnect(&sid)) {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Request accounts to share with a session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_request_accounts(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.request_accounts(&sid)) {
            Ok(views) => {
                let dtos: Vec<AccountViewDto> = views.iter().map(AccountViewDto::from).collect();
                let json = serde_json::json!({
                    "ok": true,
                    "accounts": dtos
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// List accounts permitted for a session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_list_accounts(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.list_accounts(&sid)) {
            Ok(views) => {
                let dtos: Vec<AccountViewDto> = views.iter().map(AccountViewDto::from).collect();
                let json = serde_json::json!({
                    "ok": true,
                    "accounts": dtos
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Grant account access permission (`"read"` or `"propose"`) to a session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_grant_account(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    account_id: *const c_char,
    grant_kind: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let acc_str = match unsafe { parse_c_str(account_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let kind_str = match unsafe { parse_c_str(grant_kind) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };

        let sid = SessionId(sess_str.to_string());
        let acc = match AccountId::from_str(acc_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid account ID: {:?}", e)),
        };
        let kind = match kind_str.to_lowercase().as_str() {
            "read" => GrantKind::Read,
            "propose" => GrantKind::Propose,
            _ => return error_json("Invalid grant kind (must be 'read' or 'propose')"),
        };

        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.grant_account(&sid, acc, kind))
        {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Get native balance for a specific granted account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_get_balance(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    account_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let acc_str = match unsafe { parse_c_str(account_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sid = SessionId(sess_str.to_string());
        let acc = match AccountId::from_str(acc_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid account ID: {:?}", e)),
        };

        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.get_balance(&sid, &acc)) {
            Ok(balance) => {
                let json = serde_json::json!({
                    "ok": true,
                    "balance": balance.to_string()
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Propose a native token transfer (accepts .lez names as `to`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_propose_native_transfer(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    from_account: *const c_char,
    to_account_or_name: *const c_char,
    amount_str: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let from_str = match unsafe { parse_c_str(from_account) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let to_str = match unsafe { parse_c_str(to_account_or_name) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let amt_str = match unsafe { parse_c_str(amount_str) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };

        let sid = SessionId(sess_str.to_string());
        let clean_from = from_str
            .trim()
            .strip_prefix("Public/")
            .or_else(|| from_str.trim().strip_prefix("public/"))
            .or_else(|| from_str.trim().strip_prefix("Private/"))
            .or_else(|| from_str.trim().strip_prefix("private/"))
            .unwrap_or(from_str.trim());
        let from = match AccountId::from_str(clean_from) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid from account ID: {:?}", e)),
        };
        let amount = match amt_str.parse::<u128>() {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid amount integer: {:?}", e)),
        };

        let from_lower = from_str.trim().to_lowercase();
        let from_privacy = if from_lower.starts_with("public/") {
            Some(sdk_engine::api::AccountPrivacy::Public)
        } else if from_lower.starts_with("private/") {
            Some(sdk_engine::api::AccountPrivacy::Private)
        } else {
            None
        };

        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.propose_native_transfer(
            &sid,
            from,
            to_str.to_string(),
            amount,
            from_privacy,
        )) {
            Ok(req_id) => {
                let json = serde_json::json!({
                    "ok": true,
                    "request_id": req_id.0,
                    "status": "pending_approval"
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Propose a fungible token program transfer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_propose_token_transfer(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    from_holding: *const c_char,
    to_holding: *const c_char,
    definition_id: *const c_char,
    amount_str: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let from_str = match unsafe { parse_c_str(from_holding) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let to_str = match unsafe { parse_c_str(to_holding) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let def_str = match unsafe { parse_c_str(definition_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let amt_str = match unsafe { parse_c_str(amount_str) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };

        let sid = SessionId(sess_str.to_string());
        let from = match AccountId::from_str(from_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid from_holding: {:?}", e)),
        };
        let to = match AccountId::from_str(to_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid to_holding: {:?}", e)),
        };
        let def = match AccountId::from_str(def_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid definition_id: {:?}", e)),
        };
        let amount = match amt_str.parse::<u128>() {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid amount: {:?}", e)),
        };

        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(
            ffi.engine
                .propose_token_transfer(&sid, from, to, def, amount),
        ) {
            Ok(req_id) => {
                let json = serde_json::json!({
                    "ok": true,
                    "request_id": req_id.0,
                    "status": "pending_approval"
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Propose a generic smart contract transaction.
/// `mentions_json`: JSON array of AccountId strings.
/// `calldata_hex`: Hex string of instruction calldata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_propose_transaction(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    mentions_json: *const c_char,
    calldata_hex: *const c_char,
    description: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let mentions_str = match unsafe { parse_c_str(mentions_json) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let calldata_str = match unsafe { parse_c_str(calldata_hex) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let desc_opt = unsafe { parse_c_str_optional(description) }
            .ok()
            .flatten()
            .map(|s| s.to_string());

        let raw_mentions: Vec<String> = match serde_json::from_str(mentions_str) {
            Ok(m) => m,
            Err(e) => return error_json(&format!("Invalid mentions JSON array: {:?}", e)),
        };

        let mut mentions = Vec::new();
        for m in raw_mentions {
            match AccountId::from_str(&m) {
                Ok(a) => mentions.push(a),
                Err(e) => {
                    return error_json(&format!("Invalid mention account ID '{}': {:?}", m, e));
                }
            }
        }

        let calldata = match hex::decode(calldata_str) {
            Ok(b) => b,
            Err(e) => return error_json(&format!("Invalid calldata hex: {:?}", e)),
        };

        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(
            ffi.engine
                .propose_transaction(&sid, mentions, calldata, desc_opt),
        ) {
            Ok(req_id) => {
                let json = serde_json::json!({
                    "ok": true,
                    "request_id": req_id.0,
                    "status": "pending_approval"
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Propose an explicit smart contract call with program_id and payer_id.
/// `mentions_json`: JSON array of AccountId strings.
/// `calldata_hex`: Hex string of instruction calldata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_propose_contract_call(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    program_id: *const c_char,
    payer_id: *const c_char,
    mentions_json: *const c_char,
    calldata_hex: *const c_char,
    description: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let prog_str = match unsafe { parse_c_str(program_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let payer_str = match unsafe { parse_c_str(payer_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let mentions_str = match unsafe { parse_c_str(mentions_json) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let calldata_str = match unsafe { parse_c_str(calldata_hex) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let desc_opt = match unsafe { parse_c_str(description) } {
            Ok(s) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        };

        let sid = SessionId(sess_str.to_string());
        let clean_prog = prog_str
            .trim()
            .trim_start_matches("Public/")
            .trim_start_matches("Private/");
        let program = match AccountId::from_str(clean_prog) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid program_id: {:?}", e)),
        };
        let clean_payer = payer_str
            .trim()
            .trim_start_matches("Public/")
            .trim_start_matches("Private/");
        let payer = match AccountId::from_str(clean_payer) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid payer_id: {:?}", e)),
        };

        let raw_mentions: Vec<String> = match serde_json::from_str(mentions_str) {
            Ok(v) => v,
            Err(e) => return error_json(&format!("Invalid mentions_json array: {:?}", e)),
        };
        let mut mentions = Vec::with_capacity(raw_mentions.len());
        for m in raw_mentions {
            let clean_m = m
                .trim()
                .trim_start_matches("Public/")
                .trim_start_matches("Private/");
            match AccountId::from_str(clean_m) {
                Ok(a) => mentions.push(a),
                Err(e) => return error_json(&format!("Invalid mention account: {:?}", e)),
            }
        }

        let clean_calldata = calldata_str
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        let calldata = match hex::decode(clean_calldata) {
            Ok(b) => b,
            Err(e) => return error_json(&format!("Invalid calldata hex: {:?}", e)),
        };

        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(
            ffi.engine
                .propose_contract_call(&sid, program, payer, mentions, calldata, desc_opt),
        ) {
            Ok(req_id) => {
                let json = serde_json::json!({
                    "ok": true,
                    "request_id": req_id.0,
                    "status": "pending_approval"
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// List token holdings accessible to the session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_list_token_holdings(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.list_token_holdings(&sid)) {
            Ok(holdings) => {
                let dtos: Vec<TokenHoldingDto> =
                    holdings.iter().map(TokenHoldingDto::from).collect();
                ok_json(&dtos)
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Privileged Owner API: List all tracked token holdings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_list_all_token_holdings(
    engine: *mut FfiSdkEngine,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.list_all_token_holdings()) {
            Ok(holdings) => {
                let dtos: Vec<TokenHoldingDto> =
                    holdings.iter().map(TokenHoldingDto::from).collect();
                ok_json(&dtos)
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Privileged Owner API: Register or update a tracked token holding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_register_token_holding(
    engine: *mut FfiSdkEngine,
    holding_id: *const c_char,
    definition_id: *const c_char,
    symbol: *const c_char,
    name: *const c_char,
    is_shielded: bool,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let h_str = match unsafe { parse_c_str(holding_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let d_str = match unsafe { parse_c_str(definition_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sym_str = match unsafe { parse_c_str(symbol) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let name_str = match unsafe { parse_c_str(name) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };

        let hid = match AccountId::from_str(h_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid holding_id: {:?}", e)),
        };
        let did = match AccountId::from_str(d_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid definition_id: {:?}", e)),
        };

        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.register_token_holding(
            hid,
            did,
            sym_str.to_string(),
            name_str.to_string(),
            is_shielded,
        )) {
            Ok(()) => ok_json(&serde_json::json!({ "registered": true })),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Get the current status of a proposed transaction request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_get_request_status(
    engine: *mut FfiSdkEngine,
    request_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let req_str = match unsafe { parse_c_str(request_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let rid = RequestId(req_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.get_request_status(&rid)) {
            Ok(status) => ok_json(&RequestStatusDto::from(&status)),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Query the on-chain status of a transaction by its 32-byte hex hash.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_get_transaction_status(
    engine: *mut FfiSdkEngine,
    tx_hash: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let hash_str = match unsafe { parse_c_str(tx_hash) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.get_transaction_status(hash_str))
        {
            Ok(view) => ok_json(&view),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Cancel a pending transaction request before approval.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_cancel_request(
    engine: *mut FfiSdkEngine,
    request_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let req_str = match unsafe { parse_c_str(request_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let rid = RequestId(req_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.cancel_request(&rid)) {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Resolve a `.lez` human-readable domain name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_resolve_name(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    name: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let name_str = match unsafe { parse_c_str(name) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.resolve_name(&sid, name_str))
        {
            Ok(dest) => ok_json(&ResolvedDestinationDto::from(&dest)),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Resolve an account identifier (raw Base58, prefixed Public/..., prefixed Private/..., or *.lez name).
/// Identifies privacy type, canonical address format, and user label.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_resolve_account(
    engine: *mut FfiSdkEngine,
    input: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let input_str = match unsafe { parse_c_str(input) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.resolve_account(input_str)) {
            Ok(info) => {
                let json = serde_json::json!({
                    "ok": true,
                    "account_id": info.account_id.to_string(),
                    "privacy": match info.privacy {
                        AccountPrivacy::Public => "public",
                        AccountPrivacy::Private => "private",
                    },
                    "canonical_address": info.canonical_address,
                    "label": info.label,
                    "is_local": info.is_local,
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}
