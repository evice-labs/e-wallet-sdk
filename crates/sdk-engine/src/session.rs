//! Session management .
//!
//! Each dApp connection opens a **session**. A session carries:
//! - a unique [`SessionId`],
//! - the caller's module identity (or a fallback hash),
//! - per-account grants (`read`, `propose`),
//! - an expiry timestamp.
//!
//! Sessions are stored in-memory only (never persisted to disk).
//! The owner API can list and revoke sessions.

use lee::AccountId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Unique session identifier (UUID v4 string).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Grant kind for a specific account within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GrantKind {
    Read,
    Propose,
    IdentityRead,
    IdentityPropose,
}

/// A grant ties a [`GrantKind`] to a specific account.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionGrant {
    pub account: AccountId,
    pub kind: GrantKind,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub caller_identity: String,
    pub api_version: u32,
    pub grants: HashSet<SessionGrant>,
    pub created_at: Instant,
    pub ttl: Duration,
}

impl Session {
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.ttl
    }

    /// Check whether this session has a grant of `kind` for `account`.
    pub fn has_grant(&self, account: &AccountId, kind: GrantKind) -> bool {
        // `Propose` implies `Read`.
        self.grants.contains(&SessionGrant {
            account: *account,
            kind,
        }) || (kind == GrantKind::Read
            && self.grants.contains(&SessionGrant {
                account: *account,
                kind: GrantKind::Propose,
            }))
    }

    /// Check whether this session has identity permissions.
    /// `IdentityPropose` implies `IdentityRead`.
    pub fn has_identity_grant(&self, kind: GrantKind) -> bool {
        self.grants.iter().any(|g| {
            g.kind == kind
                || (kind == GrantKind::IdentityRead && g.kind == GrantKind::IdentityPropose)
        })
    }

    /// Add an identity permission grant to this session.
    pub fn add_identity_grant(&mut self, kind: GrantKind) -> bool {
        self.grants.insert(SessionGrant {
            account: AccountId::default(),
            kind,
        })
    }

    /// Returns whether the grant was newly inserted.
    pub fn add_grant(&mut self, account: AccountId, kind: GrantKind) -> bool {
        self.grants.insert(SessionGrant { account, kind })
    }

    pub fn revoke_grant(&mut self, account: &AccountId, kind: GrantKind) -> bool {
        self.grants.remove(&SessionGrant {
            account: *account,
            kind,
        })
    }

    /// List accounts this session can access (any grant kind).
    pub fn granted_accounts(&self) -> Vec<AccountId> {
        self.grants
            .iter()
            .map(|g| g.account)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }
}

/// Thread-safe session store.
#[derive(Debug, Clone)]
pub struct SessionStore {
    sessions: Arc<RwLock<HashMap<SessionId, Session>>>,
    default_ttl: Duration,
}

impl SessionStore {
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_ttl,
        }
    }

    pub async fn create(&self, caller_identity: String, api_version: u32) -> Session {
        let session = Session {
            id: SessionId::new(),
            caller_identity,
            api_version,
            grants: HashSet::new(),
            created_at: Instant::now(),
            ttl: self.default_ttl,
        };
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());
        session
    }

    /// Get a session by id, returning `None` if not found or expired.
    pub async fn get(&self, id: &SessionId) -> Option<Session> {
        let sessions = self.sessions.read().await;
        sessions.get(id).and_then(|s| {
            if s.is_expired() {
                None
            } else {
                Some(s.clone())
            }
        })
    }

    /// Get a mutable reference to a session (for adding grants).
    pub async fn update<F, R>(&self, id: &SessionId, f: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut sessions = self.sessions.write().await;
        sessions
            .get_mut(id)
            .and_then(|s| if s.is_expired() { None } else { Some(f(s)) })
    }

    pub async fn revoke(&self, id: &SessionId) -> bool {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id).is_some()
    }

    pub async fn list(&self) -> Vec<Session> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| !s.is_expired())
            .cloned()
            .collect()
    }

    pub async fn purge_expired(&self) -> usize {
        let mut sessions = self.sessions.write().await;
        let before = sessions.len();
        sessions.retain(|_, s| !s.is_expired());
        before - sessions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_account_id() -> AccountId {
        AccountId::default()
    }

    #[tokio::test]
    async fn session_lifecycle() {
        let store = SessionStore::new(Duration::from_secs(3600));
        let session = store.create("test-module".into(), 1).await;

        // Session exists
        assert!(store.get(&session.id).await.is_some());

        // No grants initially
        let s = store.get(&session.id).await.unwrap();
        assert!(!s.has_grant(&test_account_id(), GrantKind::Read));

        // Add grant
        store
            .update(&session.id, |s| {
                s.add_grant(test_account_id(), GrantKind::Read);
            })
            .await;
        let s = store.get(&session.id).await.unwrap();
        assert!(s.has_grant(&test_account_id(), GrantKind::Read));

        // Propose implies Read
        store
            .update(&session.id, |s| {
                s.add_grant(test_account_id(), GrantKind::Propose);
            })
            .await;
        let s = store.get(&session.id).await.unwrap();
        assert!(s.has_grant(&test_account_id(), GrantKind::Read));
        assert!(s.has_grant(&test_account_id(), GrantKind::Propose));

        // Revoke
        assert!(store.revoke(&session.id).await);
        assert!(store.get(&session.id).await.is_none());
    }

    #[tokio::test]
    async fn expired_session_not_returned() {
        let store = SessionStore::new(Duration::from_millis(1));
        let session = store.create("test".into(), 1).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(store.get(&session.id).await.is_none());
    }
}
