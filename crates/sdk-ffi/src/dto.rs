use crate::*;

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

pub const NATIVE_TOKEN_SYMBOL: &str = "LGO";

#[derive(Serialize)]
pub struct AccountViewDto {
    pub account_id: String,
    pub canonical_address: String,
    pub privacy: String,
    pub is_private: bool,
    pub label: Option<String>,
    pub balance: Option<String>,
    pub formatted_balance: Option<String>,
    pub unit: &'static str,
}

pub fn format_thousand_separator(amount: u128) -> String {
    let s = amount.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    for (i, c) in chars.into_iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

impl From<&AccountView> for AccountViewDto {
    fn from(view: &AccountView) -> Self {
        let id_str = view.account_id.to_string();
        let (privacy_str, prefix, is_priv) = match view.privacy {
            AccountPrivacy::Public => ("public", "Public", false),
            AccountPrivacy::Private => ("private", "Private", true),
        };
        Self {
            account_id: id_str.clone(),
            canonical_address: format!("{}/{}", prefix, id_str),
            privacy: privacy_str.to_string(),
            is_private: is_priv,
            label: view.label.clone(),
            balance: view.balance.map(|b| b.to_string()),
            formatted_balance: view.balance.map(format_thousand_separator),
            unit: NATIVE_TOKEN_SYMBOL,
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
                RequestKind::ContractCall { .. } => "contract_call".to_string(),
            },
            effects: p.effects.clone(),
            verification_status: p.verification_status.clone(),
            warnings: p.warnings.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct TokenHoldingDto {
    pub holding_id: String,
    pub definition_id: String,
    pub symbol: String,
    pub name: String,
    pub is_shielded: bool,
    pub balance: Option<String>,
    pub formatted_balance: Option<String>,
}

impl From<&sdk_engine::api::TokenHoldingView> for TokenHoldingDto {
    fn from(v: &sdk_engine::api::TokenHoldingView) -> Self {
        Self {
            holding_id: v.holding_id.to_string(),
            definition_id: v.definition_id.to_string(),
            symbol: v.symbol.clone(),
            name: v.name.clone(),
            is_shielded: v.is_shielded,
            balance: v.balance.map(|b| b.to_string()),
            formatted_balance: v.balance.map(format_thousand_separator),
        }
    }
}

// INTERNAL HELPER FUNCTIONS

pub(crate) fn to_c_string(s: &str) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

pub(crate) fn ok_json<T: Serialize>(val: &T) -> *mut c_char {
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

pub(crate) fn success_json() -> *mut c_char {
    to_c_string("{\"ok\":true}")
}

pub(crate) fn error_json(msg: &str) -> *mut c_char {
    let wrapped = serde_json::json!({
        "ok": false,
        "error": msg
    });
    to_c_string(&wrapped.to_string())
}

pub(crate) fn parse_account_id_str(raw: &str) -> Result<AccountId, String> {
    let trimmed = raw.trim();
    let clean = trimmed
        .strip_prefix("Public/")
        .or_else(|| trimmed.strip_prefix("public/"))
        .or_else(|| trimmed.strip_prefix("Private/"))
        .or_else(|| trimmed.strip_prefix("private/"))
        .unwrap_or(trimmed);
    AccountId::from_str(clean).map_err(|e| format!("{:?}", e))
}

pub(crate) unsafe fn parse_c_str<'a>(ptr: *const c_char) -> Result<&'a str, &'static str> {
    if ptr.is_null() {
        return Err("Null pointer passed as string argument");
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "Invalid UTF-8 in C string")
}

pub(crate) unsafe fn parse_c_str_optional<'a>(
    ptr: *const c_char,
) -> Result<Option<&'a str>, &'static str> {
    if ptr.is_null() {
        return Ok(None);
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "Invalid UTF-8 in C string")?;
    if s.is_empty() { Ok(None) } else { Ok(Some(s)) }
}

pub(crate) fn catch_ffi<F, T>(default: T, f: F) -> T
where
    F: FnOnce() -> T,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(default)
}
