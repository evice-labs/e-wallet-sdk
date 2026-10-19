//! C-ABI FFI layer for the LEZ Wallet SDK.
//!
//! This crate exposes the `sdk-engine` API as `extern "C"` functions
//! consumed by the Basecamp core module (`wallet-provider`) and third-party
//! native integrations via cbindgen.
//!
//! # Conventions
//! - Opaque handle structs wrap inner Rust types (`FfiSdkEngine`).
//! - Structured payloads are exchanged as UTF-8 JSON strings (`*mut c_char`),
//!   allocated by Rust and freed using [`lez_wallet_free_string`].
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

// OPAQUE ENGINE HANDLE & APPROVAL HANDLER

/// Callback type for approval prompts in C/C++.
///
/// Return values:
/// - `1`: Approve immediately
/// - `0`: Reject immediately
/// - `-1`: Deferred / async prompt (host will call `lez_wallet_submit_approval_decision`)
pub type FfiApprovalCallback =
    unsafe extern "C" fn(prompt_json: *const c_char, user_data: *mut std::ffi::c_void) -> i32;

/// Async approval handler bridging Tokio requests to C callbacks and UI dialogs.
pub struct FfiApprovalHandler {
    callback: Mutex<Option<(FfiApprovalCallback, usize)>>,
    pending: Arc<DashMap<String, tokio::sync::oneshot::Sender<ApprovalDecision>>>,
}

impl FfiApprovalHandler {
    pub fn new() -> Self {
        Self {
            callback: Mutex::new(None),
            pending: Arc::new(DashMap::new()),
        }
    }

    pub fn set_callback(&self, cb: FfiApprovalCallback, user_data: *mut std::ffi::c_void) {
        let mut guard = self.callback.lock().unwrap();
        *guard = Some((cb, user_data as usize));
    }

    pub fn resolve_decision(&self, request_id: &str, decision: ApprovalDecision) -> bool {
        if let Some((_, sender)) = self.pending.remove(request_id) {
            sender.send(decision).is_ok()
        } else {
            false
        }
    }
}

impl Default for FfiApprovalHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ApprovalHandler for FfiApprovalHandler {
    async fn request_approval(
        &self,
        prompt: ApprovalPrompt,
        timeout: Duration,
    ) -> ApprovalDecision {
        let request_id = prompt.request.id.0.clone();
        let dto = ApprovalPromptDto::from(&prompt);
        let prompt_json = serde_json::to_string(&dto).unwrap_or_else(|_| "{}".to_string());

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.insert(request_id.clone(), tx);

        let cb_opt = {
            let guard = self.callback.lock().unwrap();
            *guard
        };

        if let Some((cb, user_data_raw)) = cb_opt {
            let c_str = CString::new(prompt_json).unwrap_or_default();
            let res = unsafe { cb(c_str.as_ptr(), user_data_raw as *mut std::ffi::c_void) };
            if res == 1 {
                self.pending.remove(&request_id);
                return ApprovalDecision::Approve;
            } else if res == 0 {
                self.pending.remove(&request_id);
                return ApprovalDecision::Reject;
            }
            // -1: deferred, wait for host resolution
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => decision,
            _ => {
                self.pending.remove(&request_id);
                ApprovalDecision::Reject
            }
        }
    }
}

/// Opaque handle representing an active LEZ Wallet Engine instance.
pub struct FfiSdkEngine {
    pub(crate) engine: Arc<SdkEngine>,
    pub(crate) approval_handler: Arc<FfiApprovalHandler>,
    pub(crate) runtime: tokio::runtime::Runtime,
}

// DATA TRANSFER OBJECTS (DTOs)

#[derive(Serialize)]
pub struct SdkInfoDto {
    pub sdk_version: String,
    pub api_version: u32,
    pub network: String,
    pub wallet_initialized: bool,
    pub wallet_locked: bool,
}

impl From<&SdkInfo> for SdkInfoDto {
    fn from(info: &SdkInfo) -> Self {
        Self {
            sdk_version: info.sdk_version.clone(),
            api_version: info.api_version,
            network: info.network.clone(),
            wallet_initialized: info.wallet_initialized,
            wallet_locked: info.wallet_locked,
        }
    }
}

#[derive(Serialize)]
pub struct AccountViewDto {
    pub account_id: String,
    pub privacy: String,
    pub label: Option<String>,
    pub balance: Option<String>,
}

impl From<&AccountView> for AccountViewDto {
    fn from(view: &AccountView) -> Self {
        Self {
            account_id: view.account_id.to_string(),
            privacy: match view.privacy {
                AccountPrivacy::Public => "public".to_string(),
                AccountPrivacy::Private => "private".to_string(),
            },
            label: view.label.clone(),
            balance: view.balance.map(|b| b.to_string()),
        }
    }
}

#[derive(Serialize)]
pub struct SessionGrantDto {
    pub account_id: String,
    pub kind: String,
}

#[derive(Serialize)]
pub struct SessionDto {
    pub session_id: String,
    pub caller_identity: String,
    pub api_version: u32,
    pub grants: Vec<SessionGrantDto>,
    pub identity_grants: Vec<String>,
    pub elapsed_secs: u64,
    pub is_expired: bool,
}

impl From<&sdk_engine::session::Session> for SessionDto {
    fn from(s: &sdk_engine::session::Session) -> Self {
        let mut grants = Vec::new();
        let mut id_grants = Vec::new();
        for g in &s.grants {
            match g.kind {
                GrantKind::Read => grants.push(SessionGrantDto {
                    account_id: g.account.to_string(),
                    kind: "read".to_string(),
                }),
                GrantKind::Propose => grants.push(SessionGrantDto {
                    account_id: g.account.to_string(),
                    kind: "propose".to_string(),
                }),
                GrantKind::IdentityRead => id_grants.push("IdentityRead".to_string()),
                GrantKind::IdentityPropose => id_grants.push("IdentityPropose".to_string()),
            }
        }
        Self {
            session_id: s.id.0.clone(),
            caller_identity: s.caller_identity.clone(),
            api_version: s.api_version,
            grants,
            identity_grants: id_grants,
            elapsed_secs: s.created_at.elapsed().as_secs(),
            is_expired: s.is_expired(),
        }
    }
}

#[derive(Serialize)]
pub struct RequestStatusDto {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl From<&RequestStatus> for RequestStatusDto {
    fn from(st: &RequestStatus) -> Self {
        match st {
            RequestStatus::PendingApproval => Self {
                status: "pending_approval".to_string(),
                tx_hash: None,
                reason: None,
            },
            RequestStatus::Approved => Self {
                status: "approved".to_string(),
                tx_hash: None,
                reason: None,
            },
            RequestStatus::Submitted { tx_hash } => Self {
                status: "submitted".to_string(),
                tx_hash: Some(hex::encode(tx_hash)),
                reason: None,
            },
            RequestStatus::Rejected => Self {
                status: "rejected".to_string(),
                tx_hash: None,
                reason: None,
            },
            RequestStatus::Cancelled => Self {
                status: "cancelled".to_string(),
                tx_hash: None,
                reason: None,
            },
            RequestStatus::Confirmed { tx_hash, .. } => Self {
                status: "confirmed".to_string(),
                tx_hash: Some(hex::encode(tx_hash)),
                reason: None,
            },
            RequestStatus::TimedOut => Self {
                status: "timed_out".to_string(),
                tx_hash: None,
                reason: Some("Approval request timed out".to_string()),
            },
            RequestStatus::Failed { reason } => Self {
                status: "failed".to_string(),
                tx_hash: None,
                reason: Some(reason.clone()),
            },
        }
    }
}

#[derive(Serialize)]
pub struct ResolvedDestinationDto {
    pub name: String,
    pub account: Option<String>,
    pub npk: Option<String>,
    pub vpk: Option<String>,
    pub expires: u64,
    pub texts: Vec<(String, String)>,
}

impl From<&sdk_engine::resolve::ResolvedDestination> for ResolvedDestinationDto {
    fn from(d: &sdk_engine::resolve::ResolvedDestination) -> Self {
        Self {
            name: d.name.clone(),
            account: d.account.map(|a| a.to_string()),
            npk: d.npk.map(hex::encode),
            vpk: d.vpk.as_ref().map(hex::encode),
            expires: d.expires,
            texts: d.texts.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct ApprovalPromptDto {
    pub request_id: String,
    pub session_id: String,
    pub dapp_identity: String,
    pub summary: String,
    pub request_kind: String,
    pub effects: Option<String>,
    pub verification_status: Option<String>,
    pub warnings: Vec<String>,
}

impl From<&ApprovalPrompt> for ApprovalPromptDto {
    fn from(p: &ApprovalPrompt) -> Self {
        Self {
            request_id: p.request.id.0.clone(),
            session_id: p.request.session_id.0.clone(),
            dapp_identity: p.dapp_identity.clone(),
            summary: p.summary.clone(),
            request_kind: match &p.request.kind {
                RequestKind::NativeTransfer { .. } => "native_transfer".to_string(),
                RequestKind::TokenTransfer { .. } => "token_transfer".to_string(),
                RequestKind::GenericCall { .. } => "generic_call".to_string(),
            },
            effects: p.effects.clone(),
            verification_status: p.verification_status.clone(),
            warnings: p.warnings.clone(),
        }
    }
}

// INTERNAL HELPER FUNCTIONS

fn to_c_string(s: &str) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

fn ok_json<T: Serialize>(val: &T) -> *mut c_char {
    let mut obj = serde_json::to_value(val).unwrap_or(serde_json::Value::Null);
    if let serde_json::Value::Object(ref mut map) = obj {
        map.insert("ok".to_string(), serde_json::Value::Bool(true));
        to_c_string(&serde_json::to_string(&obj).unwrap_or_default())
    } else {
        let wrapped = serde_json::json!({
            "ok": true,
            "data": obj
        });
        to_c_string(&wrapped.to_string())
    }
}

fn success_json() -> *mut c_char {
    to_c_string("{\"ok\":true}")
}

fn error_json(msg: &str) -> *mut c_char {
    let wrapped = serde_json::json!({
        "ok": false,
        "error": msg
    });
    to_c_string(&wrapped.to_string())
}

unsafe fn parse_c_str<'a>(ptr: *const c_char) -> Result<&'a str, &'static str> {
    if ptr.is_null() {
        return Err("Null pointer passed as string argument");
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "Invalid UTF-8 in C string")
}

unsafe fn parse_c_str_optional<'a>(ptr: *const c_char) -> Result<Option<&'a str>, &'static str> {
    if ptr.is_null() {
        return Ok(None);
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "Invalid UTF-8 in C string")?;
    if s.is_empty() { Ok(None) } else { Ok(Some(s)) }
}

fn catch_ffi<F, T>(default: T, f: F) -> T
where
    F: FnOnce() -> T,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(default)
}

// PUBLIC C-ABI INTERFACE

/// Free any string previously returned by a `lez_wallet_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_free_string(ptr: *mut c_char) {
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
/// Returns `NULL` on fatal initialization error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_engine_new(data_dir: *const c_char) -> *mut FfiSdkEngine {
    unsafe { lez_wallet_engine_new_with_password(data_dir, std::ptr::null()) }
}

/// Create an SdkEngine handle with an optional storage password.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_engine_new_with_password(
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
pub unsafe extern "C" fn lez_wallet_engine_free(engine: *mut FfiSdkEngine) {
    if !engine.is_null() {
        unsafe {
            drop(Box::from_raw(engine));
        }
    }
}

/// Register a C-ABI approval callback for interactive approval prompt handling.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_set_approval_callback(
    engine: *mut FfiSdkEngine,
    callback: FfiApprovalCallback,
    user_data: *mut std::ffi::c_void,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let ffi = unsafe { &*engine };
        ffi.approval_handler.set_callback(callback, user_data);
        success_json()
    })
}

/// Submit an approval decision (`1` for Approve, `0` for Reject) for a pending request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_submit_approval_decision(
    engine: *mut FfiSdkEngine,
    request_id: *const c_char,
    decision: i32,
) -> *mut c_char {
    catch_ffi(error_json("FFI panic"), || {
        if engine.is_null() {
            return error_json("Null engine handle");
        }
        let req_str = match unsafe { parse_c_str(request_id) } {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let ffi = unsafe { &*engine };
        let app_decision = if decision == 1 {
            ApprovalDecision::Approve
        } else {
            ApprovalDecision::Reject
        };

        if ffi.approval_handler.resolve_decision(req_str, app_decision) {
            success_json()
        } else {
            error_json("Pending request not found or already resolved")
        }
    })
}

// PRIVILEGED OWNER API (OwnerApi)

/// Create a fresh wallet encrypted under `password`.
/// Returns JSON: `{"ok":true, "mnemonic":"word1 word2 ..."}` or `{"ok":false, "error":"..."}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_create_wallet(
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
pub unsafe extern "C" fn lez_wallet_restore_wallet(
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
pub unsafe extern "C" fn lez_wallet_lock(engine: *mut FfiSdkEngine) -> *mut c_char {
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
pub unsafe extern "C" fn lez_wallet_unlock(
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

/// Create a new public account with an optional label.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_create_account_public(
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
pub unsafe extern "C" fn lez_wallet_create_account_private(
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
pub unsafe extern "C" fn lez_wallet_list_all_accounts(engine: *mut FfiSdkEngine) -> *mut c_char {
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
pub unsafe extern "C" fn lez_wallet_list_sessions(engine: *mut FfiSdkEngine) -> *mut c_char {
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
pub unsafe extern "C" fn lez_wallet_revoke_session(
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
pub unsafe extern "C" fn lez_wallet_set_account_label(
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
pub unsafe extern "C" fn lez_wallet_sdk_info(engine: *mut FfiSdkEngine) -> *mut c_char {
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

/// Connect an untrusted caller module and open a session.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_connect(
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
pub unsafe extern "C" fn lez_wallet_disconnect(
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
pub unsafe extern "C" fn lez_wallet_request_accounts(
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
pub unsafe extern "C" fn lez_wallet_list_accounts(
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
pub unsafe extern "C" fn lez_wallet_grant_account(
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
pub unsafe extern "C" fn lez_wallet_get_balance(
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
pub unsafe extern "C" fn lez_wallet_propose_native_transfer(
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
        let from = match AccountId::from_str(from_str) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid from account ID: {:?}", e)),
        };
        let amount = match amt_str.parse::<u128>() {
            Ok(a) => a,
            Err(e) => return error_json(&format!("Invalid amount integer: {:?}", e)),
        };

        let ffi = unsafe { &*engine };
        match ffi.runtime.block_on(ffi.engine.propose_native_transfer(
            &sid,
            from,
            to_str.to_string(),
            amount,
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
pub unsafe extern "C" fn lez_wallet_propose_token_transfer(
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
pub unsafe extern "C" fn lez_wallet_propose_transaction(
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

/// Get the current status of a proposed transaction request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_get_request_status(
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

/// Cancel a pending transaction request before approval.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_cancel_request(
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
pub unsafe extern "C" fn lez_wallet_resolve_name(
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

// IDENTITY ACCOUNTABILITY API

/// Request identity permission grants (`IdentityRead` or `IdentityPropose`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lez_wallet_request_identity_access(
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
pub unsafe extern "C" fn lez_wallet_get_identity_commitment(
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
pub unsafe extern "C" fn lez_wallet_get_identity_commitment_bytes(
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
pub unsafe extern "C" fn lez_wallet_derive_scoped_nullifier(
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
pub unsafe extern "C" fn lez_wallet_sign_identity_attestation(
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
        let msg_bytes = match hex::decode(msg_str) {
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
pub unsafe extern "C" fn lez_wallet_prepare_identity_registration(
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
        let tmp_dir =
            std::env::temp_dir().join(format!("test_lez_wallet_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path_str = CString::new(tmp_dir.to_str().unwrap()).unwrap();

        let engine = unsafe { lez_wallet_engine_new(path_str.as_ptr()) };
        assert!(!engine.is_null());

        // Create wallet
        let pass_str = CString::new("testpass123").unwrap();
        let res_ptr = unsafe { lez_wallet_create_wallet(engine, pass_str.as_ptr()) };
        assert!(!res_ptr.is_null());
        let res_str = unsafe { CStr::from_ptr(res_ptr) }.to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(res_str).unwrap();
        assert_eq!(parsed["ok"], true);
        assert!(parsed["mnemonic"].as_str().is_some());
        unsafe { lez_wallet_free_string(res_ptr) };

        // Create public account
        let label_str = CString::new("test_public").unwrap();
        let acc_ptr = unsafe { lez_wallet_create_account_public(engine, label_str.as_ptr()) };
        assert!(!acc_ptr.is_null());
        let acc_str = unsafe { CStr::from_ptr(acc_ptr) }.to_str().unwrap();
        let acc_json: serde_json::Value = serde_json::from_str(acc_str).unwrap();
        assert_eq!(acc_json["ok"], true);
        let pub_account_id = acc_json["account_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(acc_ptr) };

        // List accounts
        let list_ptr = unsafe { lez_wallet_list_all_accounts(engine) };
        assert!(!list_ptr.is_null());
        let list_str = unsafe { CStr::from_ptr(list_ptr) }.to_str().unwrap();
        let list_json: serde_json::Value = serde_json::from_str(list_str).unwrap();
        assert_eq!(list_json["ok"], true);
        assert!(!list_json["accounts"].as_array().unwrap().is_empty());
        unsafe { lez_wallet_free_string(list_ptr) };

        // Connect dApp session
        let caller_str = CString::new("basecamp_demo").unwrap();
        let conn_ptr = unsafe { lez_wallet_connect(engine, caller_str.as_ptr(), 1) };
        assert!(!conn_ptr.is_null());
        let conn_str = unsafe { CStr::from_ptr(conn_ptr) }.to_str().unwrap();
        let conn_json: serde_json::Value = serde_json::from_str(conn_str).unwrap();
        assert_eq!(conn_json["ok"], true);
        let session_id = conn_json["session_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(conn_ptr) };

        let sess_cstr = CString::new(session_id.clone()).unwrap();
        let acc_cstr = CString::new(pub_account_id.clone()).unwrap();
        let grant_cstr = CString::new("read").unwrap();

        // Grant read access
        let grant_res_ptr = unsafe {
            lez_wallet_grant_account(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                grant_cstr.as_ptr(),
            )
        };
        let grant_res_str = unsafe { CStr::from_ptr(grant_res_ptr) }.to_str().unwrap();
        let grant_json: serde_json::Value = serde_json::from_str(grant_res_str).unwrap();
        assert_eq!(grant_json["ok"], true);
        unsafe { lez_wallet_free_string(grant_res_ptr) };

        // Request Identity Grant
        let id_grant_cstr = CString::new("IdentityRead").unwrap();
        let id_req_ptr = unsafe {
            lez_wallet_request_identity_access(engine, sess_cstr.as_ptr(), id_grant_cstr.as_ptr())
        };
        let id_req_str = unsafe { CStr::from_ptr(id_req_ptr) }.to_str().unwrap();
        let id_req_json: serde_json::Value = serde_json::from_str(id_req_str).unwrap();
        assert_eq!(id_req_json["ok"], true);
        unsafe { lez_wallet_free_string(id_req_ptr) };

        // Get Identity Commitment
        let comm_ptr = unsafe { lez_wallet_get_identity_commitment(engine, sess_cstr.as_ptr()) };
        let comm_str = unsafe { CStr::from_ptr(comm_ptr) }.to_str().unwrap();
        let comm_json: serde_json::Value = serde_json::from_str(comm_str).unwrap();
        assert_eq!(comm_json["ok"], true);
        assert!(comm_json["commitment"].as_str().is_some());
        unsafe { lez_wallet_free_string(comm_ptr) };

        // Derive Scoped Nullifier
        let scope_cstr = CString::new("forum_gov_alpha").unwrap();
        let null_ptr = unsafe {
            lez_wallet_derive_scoped_nullifier(engine, sess_cstr.as_ptr(), scope_cstr.as_ptr())
        };
        let null_str = unsafe { CStr::from_ptr(null_ptr) }.to_str().unwrap();
        let null_json: serde_json::Value = serde_json::from_str(null_str).unwrap();
        assert_eq!(null_json["ok"], true);
        assert_eq!(null_json["scope_id"], "forum_gov_alpha");
        assert!(null_json["nullifier"].as_str().is_some());
        unsafe { lez_wallet_free_string(null_ptr) };

        // Cleanup
        unsafe { lez_wallet_engine_free(engine) };
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
            std::env::temp_dir().join(format!("test_lez_wallet_sync_rej_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path_str = CString::new(tmp_dir.to_str().unwrap()).unwrap();

        let engine = unsafe { lez_wallet_engine_new(path_str.as_ptr()) };
        assert!(!engine.is_null());

        let cb_res = unsafe {
            lez_wallet_set_approval_callback(engine, mock_sync_reject_cb, std::ptr::null_mut())
        };
        unsafe { lez_wallet_free_string(cb_res) };

        let pass_str = CString::new("testpass123").unwrap();
        let _ = unsafe { lez_wallet_create_wallet(engine, pass_str.as_ptr()) };
        let label = CString::new("alice").unwrap();
        let acc_ptr = unsafe { lez_wallet_create_account_public(engine, label.as_ptr()) };
        let acc_str = unsafe { CStr::from_ptr(acc_ptr) }.to_str().unwrap();
        let acc_json: serde_json::Value = serde_json::from_str(acc_str).unwrap();
        let from_acc = acc_json["account_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(acc_ptr) };

        let label_bob = CString::new("bob").unwrap();
        let bob_ptr = unsafe { lez_wallet_create_account_public(engine, label_bob.as_ptr()) };
        let bob_str = unsafe { CStr::from_ptr(bob_ptr) }.to_str().unwrap();
        let bob_json: serde_json::Value = serde_json::from_str(bob_str).unwrap();
        let to_acc = bob_json["account_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(bob_ptr) };

        let caller = CString::new("test_dapp").unwrap();
        let conn_ptr = unsafe { lez_wallet_connect(engine, caller.as_ptr(), 1) };
        let conn_str = unsafe { CStr::from_ptr(conn_ptr) }.to_str().unwrap();
        let conn_json: serde_json::Value = serde_json::from_str(conn_str).unwrap();
        let sid = conn_json["session_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(conn_ptr) };

        let sess_cstr = CString::new(sid).unwrap();
        let acc_cstr = CString::new(from_acc).unwrap();
        let grant_cstr = CString::new("propose").unwrap();
        let g_res = unsafe {
            lez_wallet_grant_account(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                grant_cstr.as_ptr(),
            )
        };
        unsafe { lez_wallet_free_string(g_res) };

        let to_cstr = CString::new(to_acc).unwrap();
        let amt_cstr = CString::new("500").unwrap();
        let prop_ptr = unsafe {
            lez_wallet_propose_native_transfer(
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
        unsafe { lez_wallet_free_string(prop_ptr) };

        std::thread::sleep(std::time::Duration::from_millis(50));

        let req_cstr = CString::new(req_id).unwrap();
        let status_ptr = unsafe { lez_wallet_get_request_status(engine, req_cstr.as_ptr()) };
        let status_str = unsafe { CStr::from_ptr(status_ptr) }.to_str().unwrap();
        let status_json: serde_json::Value = serde_json::from_str(status_str).unwrap();
        assert_eq!(status_json["ok"], true);
        assert_eq!(status_json["status"], "rejected");
        unsafe { lez_wallet_free_string(status_ptr) };

        unsafe { lez_wallet_engine_free(engine) };
    }

    #[test]
    fn test_ffi_approval_callback_async_deferred_resolution() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "test_lez_wallet_async_def_{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path_str = CString::new(tmp_dir.to_str().unwrap()).unwrap();

        let engine = unsafe { lez_wallet_engine_new(path_str.as_ptr()) };
        assert!(!engine.is_null());

        let cb_res = unsafe {
            lez_wallet_set_approval_callback(engine, mock_deferred_cb, std::ptr::null_mut())
        };
        unsafe { lez_wallet_free_string(cb_res) };

        let pass_str = CString::new("testpass123").unwrap();
        let _ = unsafe { lez_wallet_create_wallet(engine, pass_str.as_ptr()) };
        let label = CString::new("alice").unwrap();
        let acc_ptr = unsafe { lez_wallet_create_account_public(engine, label.as_ptr()) };
        let acc_str = unsafe { CStr::from_ptr(acc_ptr) }.to_str().unwrap();
        let acc_json: serde_json::Value = serde_json::from_str(acc_str).unwrap();
        let from_acc = acc_json["account_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(acc_ptr) };

        let label_bob = CString::new("bob").unwrap();
        let bob_ptr = unsafe { lez_wallet_create_account_public(engine, label_bob.as_ptr()) };
        let bob_str = unsafe { CStr::from_ptr(bob_ptr) }.to_str().unwrap();
        let bob_json: serde_json::Value = serde_json::from_str(bob_str).unwrap();
        let to_acc = bob_json["account_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(bob_ptr) };

        let caller = CString::new("test_dapp").unwrap();
        let conn_ptr = unsafe { lez_wallet_connect(engine, caller.as_ptr(), 1) };
        let conn_str = unsafe { CStr::from_ptr(conn_ptr) }.to_str().unwrap();
        let conn_json: serde_json::Value = serde_json::from_str(conn_str).unwrap();
        let sid = conn_json["session_id"].as_str().unwrap().to_string();
        unsafe { lez_wallet_free_string(conn_ptr) };

        let sess_cstr = CString::new(sid).unwrap();
        let acc_cstr = CString::new(from_acc).unwrap();
        let grant_cstr = CString::new("propose").unwrap();
        let g_res = unsafe {
            lez_wallet_grant_account(
                engine,
                sess_cstr.as_ptr(),
                acc_cstr.as_ptr(),
                grant_cstr.as_ptr(),
            )
        };
        unsafe { lez_wallet_free_string(g_res) };

        let to_cstr = CString::new(to_acc).unwrap();
        let amt_cstr = CString::new("1000").unwrap();
        let prop_ptr = unsafe {
            lez_wallet_propose_native_transfer(
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
        unsafe { lez_wallet_free_string(prop_ptr) };

        std::thread::sleep(std::time::Duration::from_millis(50));

        let req_cstr = CString::new(req_id.clone()).unwrap();
        let status_ptr = unsafe { lez_wallet_get_request_status(engine, req_cstr.as_ptr()) };
        let status_str = unsafe { CStr::from_ptr(status_ptr) }.to_str().unwrap();
        let status_json: serde_json::Value = serde_json::from_str(status_str).unwrap();
        assert_eq!(status_json["ok"], true);
        assert_eq!(status_json["status"], "pending_approval");
        unsafe { lez_wallet_free_string(status_ptr) };

        // Host submits decision: 0 (Reject)
        let dec_ptr = unsafe { lez_wallet_submit_approval_decision(engine, req_cstr.as_ptr(), 0) };
        let dec_str = unsafe { CStr::from_ptr(dec_ptr) }.to_str().unwrap();
        let dec_json: serde_json::Value = serde_json::from_str(dec_str).unwrap();
        assert_eq!(dec_json["ok"], true);
        unsafe { lez_wallet_free_string(dec_ptr) };

        std::thread::sleep(std::time::Duration::from_millis(50));

        let status_ptr2 = unsafe { lez_wallet_get_request_status(engine, req_cstr.as_ptr()) };
        let status_str2 = unsafe { CStr::from_ptr(status_ptr2) }.to_str().unwrap();
        let status_json2: serde_json::Value = serde_json::from_str(status_str2).unwrap();
        assert_eq!(status_json2["ok"], true);
        assert_eq!(status_json2["status"], "rejected");
        unsafe { lez_wallet_free_string(status_ptr2) };

        unsafe { lez_wallet_engine_free(engine) };
    }
}
