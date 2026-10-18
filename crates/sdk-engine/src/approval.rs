//! Async approval handlers.
//!
//! The [`ApprovalHandler`] trait is the pluggable extension point that decides
//! what happens when a dApp proposes a transaction. Implementations:
//!
//! - **`UiApprovalHandler`** — forwards to the Basecamp wallet UI .
//! - **`CliApprovalHandler`** — interactive terminal prompt for the CLI.
//! - **`PolicyApprovalHandler`** — feature-gated (`dev-auto-approve`), MUST
//!   NOT ship in release builds. CI checks for absent symbol.

use async_trait::async_trait;
use std::time::Duration;

use crate::request::{PendingRequest, RequestKind};

/// The user's decision on a pending request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    Reject,
}

/// Summary shown to the user during approval.
#[derive(Debug, Clone)]
pub struct ApprovalPrompt {
    pub request: PendingRequest,
    pub summary: String,
    pub dapp_identity: String,
    pub effects: Option<String>,
    pub verification_status: Option<String>,
    pub warnings: Vec<String>,
}

impl ApprovalPrompt {
    /// Build a summary string from the request kind.
    pub fn auto_summary(request: &PendingRequest) -> String {
        match &request.kind {
            RequestKind::NativeTransfer { from, to, amount } => {
                format!(
                    "Transfer {} native tokens from {} to {}",
                    amount,
                    hex::encode(from.to_bytes()),
                    hex::encode(to.to_bytes()),
                )
            }
            RequestKind::TokenTransfer {
                from_holding,
                to_holding,
                definition: _,
                amount,
            } => {
                format!(
                    "Transfer {} tokens from {} to {}",
                    amount,
                    hex::encode(from_holding.to_bytes()),
                    hex::encode(to_holding.to_bytes()),
                )
            }
            RequestKind::GenericCall { description, .. } => description
                .clone()
                .unwrap_or_else(|| "Generic program call".to_string()),
        }
    }
}

/// Pluggable async approval handler.
///
/// The engine calls `request_approval` when a dApp proposes a transaction.
/// The implementation decides how to present the prompt to the user and
/// returns the user's decision.
///
/// **Contract**: the handler MUST NOT block the UI thread. It is called from
/// the engine's async runtime and may take arbitrarily long (within the
/// timeout) to resolve.
#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    /// Present the approval prompt and return the user's decision.
    ///
    /// The implementation should return within `timeout`. If it does not,
    /// the engine treats the request as timed out.
    async fn request_approval(&self, prompt: ApprovalPrompt, timeout: Duration)
    -> ApprovalDecision;
}

/// CLI approval handler — interactive terminal prompt.
pub struct CliApprovalHandler;

#[async_trait]
impl ApprovalHandler for CliApprovalHandler {
    async fn request_approval(
        &self,
        prompt: ApprovalPrompt,
        _timeout: Duration,
    ) -> ApprovalDecision {
        println!("\n╔══════════════════════════════════════════╗");
        println!("║        TRANSACTION APPROVAL REQUEST      ║");
        println!("╠══════════════════════════════════════════╣");
        println!("║ dApp: {:<34} ║", prompt.dapp_identity);
        println!("║ Request: {:<31} ║", prompt.request.id);
        println!("╠══════════════════════════════════════════╣");
        println!("║ {:<40} ║", prompt.summary);
        if let Some(effects) = &prompt.effects {
            println!("║ Effects: {:<31} ║", effects);
        }
        if let Some(status) = &prompt.verification_status {
            println!("║ Verification: {:<26} ║", status);
        }
        for warning in &prompt.warnings {
            println!("║ ⚠ {:<38} ║", warning);
        }
        println!("╚══════════════════════════════════════════╝");

        // Read from stdin
        println!("Approve? [y/N]: ");
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let trimmed = input.trim().to_lowercase();
            if trimmed == "y" || trimmed == "yes" {
                return ApprovalDecision::Approve;
            }
        }
        ApprovalDecision::Reject
    }
}

/// Auto-approve handler for development and testing ONLY.
///
/// This handler is feature-gated behind `dev-auto-approve` and MUST NEVER
/// be compiled into a release build. CI checks for the absence of the
/// `PolicyApprovalHandler` symbol in release artifacts.
#[cfg(feature = "dev-auto-approve")]
pub struct PolicyApprovalHandler;

#[cfg(feature = "dev-auto-approve")]
#[async_trait]
impl ApprovalHandler for PolicyApprovalHandler {
    async fn request_approval(
        &self,
        prompt: ApprovalPrompt,
        _timeout: Duration,
    ) -> ApprovalDecision {
        log::warn!(
            "AUTO-APPROVING request {} (dev-auto-approve feature enabled!)",
            prompt.request.id
        );
        ApprovalDecision::Approve
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{PendingRequest, RequestKind};
    use crate::session::SessionId;

    #[test]
    fn auto_summary_native_transfer() {
        let req = PendingRequest::new(
            SessionId("test".into()),
            RequestKind::NativeTransfer {
                from: lee::AccountId::default(),
                to: lee::AccountId::default(),
                amount: 1000,
            },
        );
        let summary = ApprovalPrompt::auto_summary(&req);
        assert!(summary.contains("1000"));
        assert!(summary.contains("native tokens"));
    }
}
