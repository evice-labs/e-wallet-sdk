use crate::*;

// EMPOWER POW MINING & L1 -> LEZ CHANNEL BRIDGE API

/// Start EmPoWer PoW mining across `threads` worker threads and notify L1 node (`PUT /pow/mining/start`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_pow_start_mining(
    engine: *mut FfiSdkEngine,
    threads: u32,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.pow_start_mining(threads)) {
            Ok(status) => ok_json(&status),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Stop EmPoWer PoW mining and notify L1 node (`PUT /pow/mining/stop`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_pow_stop_mining(engine: *mut FfiSdkEngine) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.pow_stop_mining()) {
            Ok(status) => ok_json(&status),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Query current EmPoWer PoW mining telemetry and unbridged L1 reward balance (`GET /pow/status`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_pow_status(engine: *mut FfiSdkEngine) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.pow_status()) {
            Ok(status) => ok_json(&status),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}

/// Execute an L1 ➝ LEZ `channelDeposit` for a transparent `Public` account (`POST /channel/deposit`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_channel_deposit(
    engine: *mut FfiSdkEngine,
    recipient_account_id: *const c_char,
    amount_str: *const c_char,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let rec_str = match unsafe { parse_c_str(recipient_account_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let amt_str = match unsafe { parse_c_str(amount_str) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        match ffi
            .runtime
            .block_on(ffi.engine.channel_deposit(rec_str, amt_str))
        {
            Ok(res) => ok_json(&res),
            Err(e) => error_json(&format!("{:?}", e)),
        }
    })
}
