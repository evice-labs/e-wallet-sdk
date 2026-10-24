//! Name resolution .
//!
//! The [`NameResolver`] trait provides a pluggable interface for resolving
//! human-readable names (e.g. `.lez` names) to LEZ account identifiers
//! or private receiving keys.
//!
//! The [`LnsResolver`] in the `lns` submodule implements the LNS protocol
//! (SPEC.md) via clean-room engineering — no code from
//! `jefdiesel/logos-names` is imported.

pub mod lns;

use async_trait::async_trait;
use lee::AccountId;
use serde::{Deserialize, Serialize};

/// Resolved destination from a `.lez` name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedDestination {
    pub name: String,
    pub account: Option<AccountId>,
    pub npk: Option<[u8; 32]>,
    pub vpk: Option<Vec<u8>>,
    pub expires: u64,
    pub texts: Vec<(String, String)>,
}

/// Error type for name resolution.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolveError {
    #[error("name not found: {0}")]
    NameNotFound(String),
    #[error("name expired: {name} (expired at {expires})")]
    NameExpired { name: String, expires: u64 },
    #[error("name in grace period: {name} (expired at {expires}, within 30-day grace)")]
    NameInGracePeriod { name: String, expires: u64 },
    #[error("decryption failed for name: {0}")]
    DecryptionFailed(String),
    #[error("profile name mismatch: expected {expected}, got {actual}")]
    ProfileMismatch { expected: String, actual: String },
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("name resolution is disabled")]
    Disabled,
    #[error("invalid name format: {0}")]
    InvalidFormat(String),
}

/// Pluggable name resolver trait.
///
/// The engine calls `resolve()` before interpreting a destination as a raw
/// `AccountId`. If the input is not a name the resolver handles (e.g. not
/// ending in `.lez`), it returns `Ok(None)` to let the caller fall through
/// to raw `AccountId` parsing.
#[async_trait]
pub trait NameResolver: Send + Sync {
    /// Resolve a name to a destination.
    ///
    /// - `Ok(None)` — input is not a name handled by this resolver (passthrough).
    /// - `Ok(Some(dest))` — resolution succeeded.
    /// - `Err(e)` — resolution failed (name not found, expired, network, etc.).
    async fn resolve(&self, input: &str) -> Result<Option<ResolvedDestination>, ResolveError>;

    /// Resolve a name directly from on-chain state, bypassing any cached entry.
    async fn resolve_fresh(
        &self,
        input: &str,
    ) -> Result<Option<ResolvedDestination>, ResolveError> {
        self.resolve(input).await
    }

    /// Clear all cached resolution entries.
    fn clear_cache(&self) {}

    /// Evict a specific name from the resolution cache immediately.
    fn evict(&self, _name: &str) {}
}
