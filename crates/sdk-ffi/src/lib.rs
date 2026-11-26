//! C-ABI FFI layer for the Evice Wallet SDK (Logos Execution Zone).
//!
//! This crate exposes the `sdk-engine` API as `extern "C"` functions
//! consumed by the Basecamp core module (`wallet-provider`) and third-party
//! native integrations via cbindgen.
//!
//! # Conventions
//! - Opaque handle structs wrap inner Rust types (`FfiSdkEngine`).
//! - Structured payloads are exchanged as UTF-8 JSON strings (`*mut c_char`),
//!   allocated by Rust and freed using [`e_wallet_free_string`].
//! - All JSON responses include `{"ok": true, ...}` on success or
//!   `{"ok": false, "error": "..."}` on failure.
//! - All functions perform null-pointer checks before dereferencing and
//!   catch internal panics via `catch_unwind`.

#![allow(clippy::missing_safety_doc)]

use async_trait::async_trait;
use dashmap::DashMap;
use lee::AccountId;
use sdk_engine::SdkEngine;
use sdk_engine::api::{AccountPrivacy, AccountView, DAppApi, OwnerApi, SdkInfo};
use sdk_engine::approval::{ApprovalDecision, ApprovalHandler, ApprovalPrompt};
use sdk_engine::request::{RequestId, RequestKind, RequestStatus};
use sdk_engine::session::{GrantKind, SessionId};
use serde::Serialize;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

/// Opaque handle representing an active Evice Wallet Engine instance.
pub struct FfiSdkEngine {
    pub(crate) engine: Arc<SdkEngine>,
    pub(crate) approval_handler: Arc<FfiApprovalHandler>,
    pub(crate) runtime: tokio::runtime::Runtime,
}

// PUBLIC C-ABI INTERFACE

/// Free any string previously returned by a `e_wallet_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

/// Returns the SDK version as a static C string.
#[unsafe(no_mangle)]
pub extern "C" fn lez_sdk_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

/// Returns the SDK API capability version number.
#[unsafe(no_mangle)]
pub extern "C" fn lez_sdk_api_version() -> u32 {
    sdk_engine::API_VERSION
}

/// Create an SdkEngine handle from a data directory path.
///
/// Check whether a persistent wallet storage file already exists in the given directory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_exists(data_dir: *const c_char) -> bool {
    catch_ffi(false, || {
        let dir_str = match unsafe { parse_c_str(data_dir) } {
            Ok(s) => s,
            Err(_) => return false,
        };
        SdkEngine::wallet_exists(std::path::Path::new(dir_str))
    })
}

/// Returns `NULL` on fatal initialization error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_engine_new(data_dir: *const c_char) -> *mut FfiSdkEngine {
    unsafe { e_wallet_engine_new_with_password(data_dir, std::ptr::null()) }
}

/// Create an SdkEngine handle with an optional storage password.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_engine_new_with_password(
    data_dir: *const c_char,
    password: *const c_char,
) -> *mut FfiSdkEngine {
    catch_ffi(std::ptr::null_mut(), || {
        let dir_str = match unsafe { parse_c_str(data_dir) } {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let pass_str = unsafe { parse_c_str_optional(password) }
            .ok()
            .flatten()
            .unwrap_or("");

        let path = PathBuf::from(dir_str);
        let approval = Arc::new(FfiApprovalHandler::new());

        let engine = match SdkEngine::open_or_init(path, pass_str, approval.clone()) {
            Ok(e) => Arc::new(e),
            Err(e) => {
                log::error!("Failed to initialize SdkEngine: {:?}", e);
                return std::ptr::null_mut();
            }
        };

        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::error!("Failed to start Tokio runtime: {:?}", e);
                return std::ptr::null_mut();
            }
        };

        Box::into_raw(Box::new(FfiSdkEngine {
            engine,
            approval_handler: approval,
            runtime,
        }))
    })
}

/// Free an SdkEngine handle and release associated resources.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_engine_free(engine: *mut FfiSdkEngine) {
    if !engine.is_null() {
        unsafe {
            drop(Box::from_raw(engine));
        }
    }
}

mod approval;
mod dapp;
mod dto;
mod identity;
mod pow;
mod wallet;

pub use approval::*;
pub use dapp::*;
pub use dto::*;
pub use identity::*;
pub use pow::*;
pub use wallet::*;

// UNIT TESTS

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn test_version_exports() {
        let v_ptr = lez_sdk_version();
        assert!(!v_ptr.is_null());
        let v_str = unsafe { CStr::from_ptr(v_ptr) }.to_str().unwrap();
        assert_eq!(v_str, env!("CARGO_PKG_VERSION"));

        let api_v = lez_sdk_api_version();
        assert_eq!(api_v, sdk_engine::API_VERSION);
    }

    #[test]
    fn test_ffi_engine_lifecycle_and_owner_flows() {
        let tmp_dir = std::env::temp_dir().join(format!("test_e_wallet_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path_str = CString::new(tmp_dir.to_str().unwrap()).unwrap();

        let engine = unsafe { e_wallet_engine_new(path_str.as_ptr()) };
        assert!(!engine.is_null());

        // Create wallet
        let pass_str = CString::new("testpass123").unwrap();
        let res_ptr = unsafe { e_wallet_create_wallet(engine, pass_str.as_ptr()) };
        assert!(!res_ptr.is_null());
        let res_str = unsafe { CStr::from_ptr(res_ptr) }.to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(res_str).unwrap();
        assert_eq!(parsed["ok"], true);
        assert!(parsed["mnemonic"].as_str().is_some());
        unsafe { e_wallet_free_string(res_ptr) };

        // Create public account
        let label_str = CString::new("test_public").unwrap();
        let acc_ptr = unsafe { e_wallet_create_account_public(engine, label_str.as_ptr()) };
        assert!(!acc_ptr.is_null());
        let acc_str = unsafe { CStr::from_ptr(acc_ptr) }.to_str().unwrap();
        let acc_json: serde_json::Value = serde_json::from_str(acc_str).unwrap();
        assert_eq!(acc_json["ok"], true);
        let pub_account_id = acc_json["account_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(acc_ptr) };

        // List accounts
        let list_ptr = unsafe { e_wallet_list_all_accounts(engine) };
        assert!(!list_ptr.is_null());
        let list_str = unsafe { CStr::from_ptr(list_ptr) }.to_str().unwrap();
        let list_json: serde_json::Value = serde_json::from_str(list_str).unwrap();
        assert_eq!(list_json["ok"], true);
        assert!(!list_json["accounts"].as_array().unwrap().is_empty());
        unsafe { e_wallet_free_string(list_ptr) };

        // Connect dApp session
        let caller_str = CString::new("basecamp_demo").unwrap();
        let conn_ptr = unsafe { e_wallet_connect(engine, caller_str.as_ptr(), 1) };
        assert!(!conn_ptr.is_null());
        let conn_str = unsafe { CStr::from_ptr(conn_ptr) }.to_str().unwrap();
        let conn_json: serde_json::Value = serde_json::from_str(conn_str).unwrap();
        assert_eq!(conn_json["ok"], true);
        let session_id = conn_json["session_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(conn_ptr) };

        let sess_cstr = CString::new(session_id.clone()).unwrap();
        let acc_cstr = CString::new(pub_account_id.clone()).unwrap();
        let grant_cstr = CString::new("read").unwrap();

        // Grant read access
        let grant_res_ptr = unsafe {
            e_wallet_grant_account(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                grant_cstr.as_ptr(),
            )
        };
        let grant_res_str = unsafe { CStr::from_ptr(grant_res_ptr) }.to_str().unwrap();
        let grant_json: serde_json::Value = serde_json::from_str(grant_res_str).unwrap();
        assert_eq!(grant_json["ok"], true);
        unsafe { e_wallet_free_string(grant_res_ptr) };

        // Request Identity Grant
        let id_grant_cstr = CString::new("IdentityRead").unwrap();
        let id_req_ptr = unsafe {
            e_wallet_request_identity_access(engine, sess_cstr.as_ptr(), id_grant_cstr.as_ptr())
        };
        let id_req_str = unsafe { CStr::from_ptr(id_req_ptr) }.to_str().unwrap();
        let id_req_json: serde_json::Value = serde_json::from_str(id_req_str).unwrap();
        assert_eq!(id_req_json["ok"], true);
        unsafe { e_wallet_free_string(id_req_ptr) };

        // Get Identity Commitment
        let comm_ptr = unsafe { e_wallet_get_identity_commitment(engine, sess_cstr.as_ptr()) };
        let comm_str = unsafe { CStr::from_ptr(comm_ptr) }.to_str().unwrap();
        let comm_json: serde_json::Value = serde_json::from_str(comm_str).unwrap();
        assert_eq!(comm_json["ok"], true);
        assert!(comm_json["commitment"].as_str().is_some());
        unsafe { e_wallet_free_string(comm_ptr) };

        // Derive Scoped Nullifier
        let scope_cstr = CString::new("forum_gov_alpha").unwrap();
        let null_ptr = unsafe {
            e_wallet_derive_scoped_nullifier(engine, sess_cstr.as_ptr(), scope_cstr.as_ptr())
        };
        let null_str = unsafe { CStr::from_ptr(null_ptr) }.to_str().unwrap();
        let null_json: serde_json::Value = serde_json::from_str(null_str).unwrap();
        assert_eq!(null_json["ok"], true);
        assert_eq!(null_json["scope_id"], "forum_gov_alpha");
        assert!(null_json["nullifier"].as_str().is_some());
        unsafe { e_wallet_free_string(null_ptr) };

        // Cleanup
        unsafe { e_wallet_engine_free(engine) };
    }

    unsafe extern "C" fn mock_sync_reject_cb(
        _prompt_json: *const c_char,
        _user_data: *mut std::ffi::c_void,
    ) -> i32 {
        0 // reject immediately
    }

    unsafe extern "C" fn mock_deferred_cb(
        _prompt_json: *const c_char,
        _user_data: *mut std::ffi::c_void,
    ) -> i32 {
        -1 // deferred modal
    }

    #[test]
    fn test_ffi_approval_callback_sync_rejection() {
        let tmp_dir =
            std::env::temp_dir().join(format!("test_e_wallet_sync_rej_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path_str = CString::new(tmp_dir.to_str().unwrap()).unwrap();

        let engine = unsafe { e_wallet_engine_new(path_str.as_ptr()) };
        assert!(!engine.is_null());

        let cb_res = unsafe {
            e_wallet_set_approval_callback(engine, mock_sync_reject_cb, std::ptr::null_mut())
        };
        unsafe { e_wallet_free_string(cb_res) };

        let pass_str = CString::new("testpass123").unwrap();
        let _ = unsafe { e_wallet_create_wallet(engine, pass_str.as_ptr()) };
        let label = CString::new("alice").unwrap();
        let acc_ptr = unsafe { e_wallet_create_account_public(engine, label.as_ptr()) };
        let acc_str = unsafe { CStr::from_ptr(acc_ptr) }.to_str().unwrap();
        let acc_json: serde_json::Value = serde_json::from_str(acc_str).unwrap();
        let from_acc = acc_json["account_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(acc_ptr) };

        let label_bob = CString::new("bob").unwrap();
        let bob_ptr = unsafe { e_wallet_create_account_public(engine, label_bob.as_ptr()) };
        let bob_str = unsafe { CStr::from_ptr(bob_ptr) }.to_str().unwrap();
        let bob_json: serde_json::Value = serde_json::from_str(bob_str).unwrap();
        let to_acc = bob_json["account_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(bob_ptr) };

        let caller = CString::new("test_dapp").unwrap();
        let conn_ptr = unsafe { e_wallet_connect(engine, caller.as_ptr(), 1) };
        let conn_str = unsafe { CStr::from_ptr(conn_ptr) }.to_str().unwrap();
        let conn_json: serde_json::Value = serde_json::from_str(conn_str).unwrap();
        let sid = conn_json["session_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(conn_ptr) };

        let sess_cstr = CString::new(sid).unwrap();
        let acc_cstr = CString::new(from_acc).unwrap();
        let grant_cstr = CString::new("propose").unwrap();
        let g_res = unsafe {
            e_wallet_grant_account(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                grant_cstr.as_ptr(),
            )
        };
        unsafe { e_wallet_free_string(g_res) };

        let to_cstr = CString::new(to_acc).unwrap();
        let amt_cstr = CString::new("500").unwrap();
        let prop_ptr = unsafe {
            e_wallet_propose_native_transfer(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                to_cstr.as_ptr(),
                amt_cstr.as_ptr(),
            )
        };
        let prop_str = unsafe { CStr::from_ptr(prop_ptr) }.to_str().unwrap();
        let prop_json: serde_json::Value = serde_json::from_str(prop_str).unwrap();
        assert_eq!(prop_json["ok"], true);
        let req_id = prop_json["request_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(prop_ptr) };

        std::thread::sleep(std::time::Duration::from_millis(50));

        let req_cstr = CString::new(req_id).unwrap();
        let status_ptr = unsafe { e_wallet_get_request_status(engine, req_cstr.as_ptr()) };
        let status_str = unsafe { CStr::from_ptr(status_ptr) }.to_str().unwrap();
        let status_json: serde_json::Value = serde_json::from_str(status_str).unwrap();
        assert_eq!(status_json["ok"], true);
        assert_eq!(status_json["status"], "rejected");
        unsafe { e_wallet_free_string(status_ptr) };

        unsafe { e_wallet_engine_free(engine) };
    }

    #[test]
    fn test_ffi_approval_callback_async_deferred_resolution() {
        let tmp_dir =
            std::env::temp_dir().join(format!("test_e_wallet_async_def_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path_str = CString::new(tmp_dir.to_str().unwrap()).unwrap();

        let engine = unsafe { e_wallet_engine_new(path_str.as_ptr()) };
        assert!(!engine.is_null());

        let cb_res = unsafe {
            e_wallet_set_approval_callback(engine, mock_deferred_cb, std::ptr::null_mut())
        };
        unsafe { e_wallet_free_string(cb_res) };

        let pass_str = CString::new("testpass123").unwrap();
        let _ = unsafe { e_wallet_create_wallet(engine, pass_str.as_ptr()) };
        let label = CString::new("alice").unwrap();
        let acc_ptr = unsafe { e_wallet_create_account_public(engine, label.as_ptr()) };
        let acc_str = unsafe { CStr::from_ptr(acc_ptr) }.to_str().unwrap();
        let acc_json: serde_json::Value = serde_json::from_str(acc_str).unwrap();
        let from_acc = acc_json["account_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(acc_ptr) };

        let label_bob = CString::new("bob").unwrap();
        let bob_ptr = unsafe { e_wallet_create_account_public(engine, label_bob.as_ptr()) };
        let bob_str = unsafe { CStr::from_ptr(bob_ptr) }.to_str().unwrap();
        let bob_json: serde_json::Value = serde_json::from_str(bob_str).unwrap();
        let to_acc = bob_json["account_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(bob_ptr) };

        let caller = CString::new("test_dapp").unwrap();
        let conn_ptr = unsafe { e_wallet_connect(engine, caller.as_ptr(), 1) };
        let conn_str = unsafe { CStr::from_ptr(conn_ptr) }.to_str().unwrap();
        let conn_json: serde_json::Value = serde_json::from_str(conn_str).unwrap();
        let sid = conn_json["session_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(conn_ptr) };

        let sess_cstr = CString::new(sid).unwrap();
        let acc_cstr = CString::new(from_acc).unwrap();
        let grant_cstr = CString::new("propose").unwrap();
        let g_res = unsafe {
            e_wallet_grant_account(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                grant_cstr.as_ptr(),
            )
        };
        unsafe { e_wallet_free_string(g_res) };

        let to_cstr = CString::new(to_acc).unwrap();
        let amt_cstr = CString::new("1000").unwrap();
        let prop_ptr = unsafe {
            e_wallet_propose_native_transfer(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                to_cstr.as_ptr(),
                amt_cstr.as_ptr(),
            )
        };
        let prop_str = unsafe { CStr::from_ptr(prop_ptr) }.to_str().unwrap();
        let prop_json: serde_json::Value = serde_json::from_str(prop_str).unwrap();
        assert_eq!(prop_json["ok"], true);
        let req_id = prop_json["request_id"].as_str().unwrap().to_string();
        unsafe { e_wallet_free_string(prop_ptr) };

        std::thread::sleep(std::time::Duration::from_millis(50));

        let req_cstr = CString::new(req_id.clone()).unwrap();
        let status_ptr = unsafe { e_wallet_get_request_status(engine, req_cstr.as_ptr()) };
        let status_str = unsafe { CStr::from_ptr(status_ptr) }.to_str().unwrap();
        let status_json: serde_json::Value = serde_json::from_str(status_str).unwrap();
        assert_eq!(status_json["ok"], true);
        assert_eq!(status_json["status"], "pending_approval");
        unsafe { e_wallet_free_string(status_ptr) };

        // Host submits decision: 0 (Reject)
        let dec_ptr = unsafe { e_wallet_submit_approval_decision(engine, req_cstr.as_ptr(), 0) };
        let dec_str = unsafe { CStr::from_ptr(dec_ptr) }.to_str().unwrap();
        let dec_json: serde_json::Value = serde_json::from_str(dec_str).unwrap();
        assert_eq!(dec_json["ok"], true);
        unsafe { e_wallet_free_string(dec_ptr) };

        std::thread::sleep(std::time::Duration::from_millis(50));

        let status_ptr2 = unsafe { e_wallet_get_request_status(engine, req_cstr.as_ptr()) };
        let status_str2 = unsafe { CStr::from_ptr(status_ptr2) }.to_str().unwrap();
        let status_json2: serde_json::Value = serde_json::from_str(status_str2).unwrap();
        assert_eq!(status_json2["ok"], true);
        assert_eq!(status_json2["status"], "rejected");
        unsafe { e_wallet_free_string(status_ptr2) };

        unsafe { e_wallet_engine_free(engine) };
    }
}
