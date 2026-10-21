//! Immutable pending requests and intent hashing.
//!
//! Every `propose*` call creates an immutable [`PendingRequest`]. The request
//! captures the full intent at proposal time so that execution later cannot
//! silently change parameters.
//!
//! The canonical **intent hash** binds: sender, destination, amount, program,
//! calldata, nonce-at-proposal-time, and the session that created it.

use common::HashType;
use lee::AccountId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::session::SessionId;

/// Unique request identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(pub String);

impl RequestId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The kind of operation a request represents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestKind {
    NativeTransfer {
        from: AccountId,
        to: AccountId,
        amount: u128,
    },
    TokenTransfer {
        from_holding: AccountId,
        to_holding: AccountId,
        definition: AccountId,
        amount: u128,
    },
    GenericCall {
        mentions: Vec<AccountId>,
        calldata: Vec<u8>,
        description: Option<String>,
    },
}

/// Lifecycle status of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestStatus {
    PendingApproval,
    Approved,
    Rejected,
    Submitted { tx_hash: HashType },
    Confirmed { tx_hash: HashType, block_id: u64 },
    Failed { reason: String },
    Cancelled,
    TimedOut,
}

/// An immutable request created by a `propose*` call.
///
/// Once created, **no field may be mutated**. The intent hash is computed at
/// creation time and is what the execution path uses to verify that nothing
/// changed between proposal and execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub id: RequestId,
    pub session_id: SessionId,
    pub kind: RequestKind,
    pub intent_hash: [u8; 32],
    #[serde(skip)]
    pub created_at: Option<Instant>,
}

impl PendingRequest {
    /// Create a new pending request and compute the intent hash.
    pub fn new(session_id: SessionId, kind: RequestKind) -> Self {
        let id = RequestId::new();
        let intent_hash = Self::compute_intent_hash(&id, &session_id, &kind);
        Self {
            id,
            session_id,
            kind,
            intent_hash,
            created_at: Some(Instant::now()),
        }
    }

    /// Compute the canonical intent hash.
    ///
    /// The hash binds: request_id, session_id, and the full serialized kind.
    /// This ensures that execution of the request will always operate on
    /// exactly the parameters the user approved.
    fn compute_intent_hash(id: &RequestId, session_id: &SessionId, kind: &RequestKind) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"evice-wallet-sdk/intent/v1/");
        hasher.update(id.0.as_bytes());
        hasher.update(session_id.0.as_bytes());
        // Serialize the kind deterministically.
        let kind_bytes = serde_json::to_vec(kind).expect("RequestKind must be serializable");
        hasher.update(&kind_bytes);
        hasher.finalize().into()
    }

    /// Verify the intent hash matches the request contents.
    /// Used before execution to detect any tampering.
    pub fn verify_intent(&self) -> bool {
        let expected = Self::compute_intent_hash(&self.id, &self.session_id, &self.kind);
        self.intent_hash == expected
    }
}

/// Thread-safe request store.
#[derive(Debug, Clone)]
pub struct RequestStore {
    requests: Arc<RwLock<HashMap<RequestId, PendingRequest>>>,
    statuses: Arc<RwLock<HashMap<RequestId, RequestStatus>>>,
}

impl RequestStore {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            statuses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a new request.
    pub async fn insert(&self, request: PendingRequest) {
        let id = request.id.clone();
        let mut requests = self.requests.write().await;
        let mut statuses = self.statuses.write().await;
        requests.insert(id.clone(), request);
        statuses.insert(id, RequestStatus::PendingApproval);
    }

    /// Get a request by id.
    pub async fn get(&self, id: &RequestId) -> Option<PendingRequest> {
        let requests = self.requests.read().await;
        requests.get(id).cloned()
    }

    /// Get the status of a request.
    pub async fn get_status(&self, id: &RequestId) -> Option<RequestStatus> {
        let statuses = self.statuses.read().await;
        statuses.get(id).cloned()
    }

    /// Update the status of a request.
    pub async fn set_status(&self, id: &RequestId, status: RequestStatus) -> bool {
        let mut statuses = self.statuses.write().await;
        if statuses.contains_key(id) {
            statuses.insert(id.clone(), status);
            true
        } else {
            false
        }
    }

    /// List all requests for a given session.
    pub async fn list_for_session(
        &self,
        session_id: &SessionId,
    ) -> Vec<(PendingRequest, RequestStatus)> {
        let requests = self.requests.read().await;
        let statuses = self.statuses.read().await;
        requests
            .values()
            .filter(|r| &r.session_id == session_id)
            .map(|r| {
                let status = statuses
                    .get(&r.id)
                    .cloned()
                    .unwrap_or(RequestStatus::PendingApproval);
                (r.clone(), status)
            })
            .collect()
    }
}

impl Default for RequestStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_hash_is_deterministic() {
        let sid = SessionId("test-session".into());
        let kind = RequestKind::NativeTransfer {
            from: AccountId::default(),
            to: AccountId::default(),
            amount: 1000,
        };
        let r1 = PendingRequest::new(sid.clone(), kind.clone());
        // Recompute with same id
        let hash = PendingRequest::compute_intent_hash(&r1.id, &sid, &kind);
        assert_eq!(r1.intent_hash, hash);
        assert!(r1.verify_intent());
    }

    #[tokio::test]
    async fn request_store_lifecycle() {
        let store = RequestStore::new();
        let sid = SessionId("s1".into());
        let req = PendingRequest::new(
            sid.clone(),
            RequestKind::NativeTransfer {
                from: AccountId::default(),
                to: AccountId::default(),
                amount: 500,
            },
        );
        let rid = req.id.clone();

        store.insert(req).await;
        assert_eq!(
            store.get_status(&rid).await,
            Some(RequestStatus::PendingApproval)
        );

        store.set_status(&rid, RequestStatus::Approved).await;
        assert_eq!(store.get_status(&rid).await, Some(RequestStatus::Approved));

        let list = store.list_for_session(&sid).await;
        assert_eq!(list.len(), 1);
    }
}
