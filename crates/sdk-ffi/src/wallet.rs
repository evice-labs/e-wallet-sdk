use crate::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_get_gas_limit(engine: *mut FfiSdkEngine) -> u64 {
    catch_ffi(sdk_engine::api::DEFAULT_WALLET_GAS_LIMIT, || {
        if engine.is_null() {
            return sdk_engine::api::DEFAULT_WALLET_GAS_LIMIT;
        }
        let ffi = unsafe { &*engine };
        ffi.runtime.block_on(ffi.engine.get_gas_limit())
    })
}

/// Set the gas limit seed for transactions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_set_gas_limit(
    engine: *mut FfiSdkEngine,
    limit: u64,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.set_gas_limit(limit)) {
            Ok(()) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Create a fresh wallet encrypted under `password`.
/// Returns JSON: `{"ok":true, "mnemonic":"word1 word2 ..."}` or `{"ok":false, "error":"..."}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_create_wallet(
    engine: *mut FfiSdkEngine,
    password: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let pass_str = match unsafe { parse_c_str(password) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.create_wallet(pass_str)) {
            Ok(mnemonic) => {
                let json = serde_json::json!({
                    "ok": true,
                    "mnemonic": mnemonic.to_string(),
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Restore a wallet from mnemonic and set password.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_restore_wallet(
    engine: *mut FfiSdkEngine,
    mnemonic: *const c_char,
    password: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let mnem_str = match unsafe { parse_c_str(mnemonic) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let pass_str = match unsafe { parse_c_str(password) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let parsed_mnem = match bip39::Mnemonic::from_str(mnem_str) {
            Ok(m) => m,
            Err(e) => return error_json(&format!("Invalid mnemonic: {:?}", e)),
        };

        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.restore_wallet(&parsed_mnem, pass_str))
        {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Lock the wallet, revoking all active sessions and purging secret keys from memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_lock(engine: *mut FfiSdkEngine) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.lock()) {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Unlock the wallet with password.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_unlock(
    engine: *mut FfiSdkEngine,
    password: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let pass_str = match unsafe { parse_c_str(password) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.unlock(pass_str)) {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Change the wallet encryption password.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_change_password(
    engine: *mut FfiSdkEngine,
    old_password: *const c_char,
    new_password: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let old_pass_str = match unsafe { parse_c_str(old_password) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let new_pass_str = match unsafe { parse_c_str(new_password) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.change_password(old_pass_str, new_pass_str))
        {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Create a new public account with an optional label.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_create_account_public(
    engine: *mut FfiSdkEngine,
    label: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let label_opt = unsafe { parse_c_str_optional(label) }
            .ok()
            .flatten()
            .map(|s| s.to_string());
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.create_account_public(label_opt))
        {
            Ok(account_id) => {
                let json = serde_json::json!({
                    "ok": true,
                    "account_id": account_id.to_string(),
                    "privacy": "public"
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Create a new private account with an optional label.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_create_account_private(
    engine: *mut FfiSdkEngine,
    label: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let label_opt = unsafe { parse_c_str_optional(label) }
            .ok()
            .flatten()
            .map(|s| s.to_string());
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.create_account_private(label_opt))
        {
            Ok(account_id) => {
                let json = serde_json::json!({
                    "ok": true,
                    "account_id": account_id.to_string(),
                    "privacy": "private"
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// List all accounts in the wallet with privacy and balances.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_list_all_accounts(engine: *mut FfiSdkEngine) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.list_all_accounts()) {
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

/// List all active dApp sessions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_list_sessions(engine: *mut FfiSdkEngine) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.list_sessions()) {
            Ok(sessions) => {
                let dtos: Vec<SessionDto> = sessions.iter().map(SessionDto::from).collect();
                let json = serde_json::json!({
                    "ok": true,
                    "sessions": dtos
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Revoke a specific dApp session by session ID.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_revoke_session(
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
        match ffi.runtime.block_on(ffi.engine.revoke_session(&sid)) {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Set a user-friendly label for an account.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_set_account_label(
    engine: *mut FfiSdkEngine,
    account_id: *const c_char,
    label: *const c_char,
    is_private: bool,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let acc_str = match unsafe { parse_c_str(account_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let label_str = match unsafe { parse_c_str(label) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let acc = match AccountId::from_str(acc_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid account ID: {:?}", e)),
        };
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.set_account_label(
            acc,
            label_str.to_string(),
            is_private,
        )) {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

// DAPP PROVIDER API (DAppApi)

/// Query SDK and network information.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_sdk_info(engine: *mut FfiSdkEngine) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.sdk_info()) {
            Ok(info) => ok_json(&SdkInfoDto::from(&info)),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}
