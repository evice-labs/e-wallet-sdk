use crate::*;

// OPAQUE ENGINE HANDLE & APPROVAL HANDLER

/// Callback type for approval prompts in C/C++.
///
/// Return values:
/// - `1`: Approve immediately
/// - `0`: Reject immediately
/// - `-1`: Deferred / async prompt (host will call `e_wallet_submit_approval_decision`)
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

/// Register a C-ABI approval callback for interactive approval prompt handling.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn e_wallet_set_approval_callback(
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
pub unsafe extern "C" fn e_wallet_submit_approval_decision(
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
