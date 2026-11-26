use crate::*;

/// Request identity permission grants (`IdentityRead` or `IdentityPropose`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_request_identity_access(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
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
        let kind_str = match unsafe { parse_c_str(grant_kind) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };

        let sid = SessionId(sess_str.to_string());
        let kind = match kind_str.to_lowercase().as_str() {
            "identityread" | "read" => GrantKind::IdentityRead,
            "identitypropose" | "propose" => GrantKind::IdentityPropose,
            _ => {
                return error_json(
                    "Invalid identity grant kind (must be 'IdentityRead' or 'IdentityPropose')",
                );
            }
        };

        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.request_identity_access(&sid, kind))
        {
            Ok(_) => success_json(),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Retrieve the user's 32-byte public identity commitment as a hex string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_get_identity_commitment(
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
        match ffi
            .runtime
            .block_on(ffi.engine.get_identity_commitment(&sid))
        {
            Ok(commitment) => {
                let json = serde_json::json!({
                    "ok": true,
                    "commitment": hex::encode(commitment.0)
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Copy the 32-byte identity commitment into a raw buffer.
/// Returns `0` on success, or non-zero on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_get_identity_commitment_bytes(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    out_32: *mut u8,
) -> i32 {
    catch_ffi(-1, || {
        if engine.is_null() || out_32.is_null() {
            return -1;
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(_) => return -2,
        };
        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.get_identity_commitment(&sid))
        {
            Ok(commitment) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(commitment.0.as_ptr(), out_32, 32);
                }
                0
            }
            Err(_) => -3,
        }
    })
}

/// Derive a scope-specific pseudonymous nullifier hex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_derive_scoped_nullifier(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    scope_id: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let scope_str = match unsafe { parse_c_str(scope_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.derive_scoped_nullifier(&sid, scope_str))
        {
            Ok(nullifier) => {
                let json = serde_json::json!({
                    "ok": true,
                    "scope_id": scope_str,
                    "nullifier": hex::encode(nullifier.0)
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Sign an arbitrary message using the scoped identity credential.
/// `message_hex`: Hex-encoded byte payload to sign.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_sign_identity_attestation(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    scope_id: *const c_char,
    message_hex: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let scope_str = match unsafe { parse_c_str(scope_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let msg_str = match unsafe { parse_c_str(message_hex) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let clean_msg = msg_str
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        let msg_bytes = match hex::decode(clean_msg) {
            Ok(b) => b,
            Err(e) => return error_json(&format!("Invalid message hex: {:?}", e)),
        };

        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(
            ffi.engine
                .sign_identity_attestation(&sid, scope_str, &msg_bytes),
        ) {
            Ok(payload) => {
                let json = serde_json::json!({
                    "ok": true,
                    "scope_id": payload.scope_id,
                    "commitment": hex::encode(payload.commitment.0),
                    "nullifier": hex::encode(payload.nullifier.0),
                    "message": hex::encode(&payload.message),
                    "signature": hex::encode(&payload.signature)
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Prepare on-chain registration payload with Two-Tier Shamir Secret Sharing threshold shares.
/// `node_pubkeys_json`: JSON array of 32-byte hex public keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_prepare_identity_registration(
    engine: *mut FfiSdkEngine,
    session_id: *const c_char,
    scope_id: *const c_char,
    node_pubkeys_json: *const c_char,
    threshold_k: u8,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let sess_str = match unsafe { parse_c_str(session_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let scope_str = match unsafe { parse_c_str(scope_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let pks_str = match unsafe { parse_c_str(node_pubkeys_json) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };

        let raw_pks: Vec<String> = match serde_json::from_str(pks_str) {
            Ok(p) => p,
            Err(e) => return error_json(&format!("Invalid pubkeys JSON array: {:?}", e)),
        };

        let mut node_pubkeys = Vec::new();
        for pk_hex in raw_pks {
            let bytes = match hex::decode(&pk_hex) {
                Ok(b) if b.len() == 32 => b,
                _ => return error_json(&format!("Invalid 32-byte pubkey hex '{}'", pk_hex)),
            };
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            node_pubkeys.push(arr);
        }

        let sid = SessionId(sess_str.to_string());
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.prepare_identity_registration(
                &sid,
                scope_str,
                &node_pubkeys,
                threshold_k,
            )) {
            Ok(reg) => {
                let shares: Vec<serde_json::Value> = reg
                    .encrypted_shares
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "node_pubkey": hex::encode(s.node_pubkey),
                            "ephemeral_pk": hex::encode(s.ephemeral_pk),
                            "ciphertext": hex::encode(&s.ciphertext)
                        })
                    })
                    .collect();

                let json = serde_json::json!({
                    "ok": true,
                    "commitment": hex::encode(reg.commitment.0),
                    "scope_id": reg.scope_id,
                    "threshold_k": reg.threshold_k,
                    "total_m": reg.encrypted_shares.len(),
                    "encrypted_shares": shares
                });
                to_c_string(&json.to_string())
            }
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}
