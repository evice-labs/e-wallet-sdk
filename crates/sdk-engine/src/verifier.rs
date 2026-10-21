//! Program source verification (LP-0023).
//!
//! The [`ProgramVerifier`] trait abstracts how the engine checks whether a
//! program's bytecode is reproducibly built from known source. The approval
//! prompt shows the verification status.
//!
//! The built-in verifier maintains a hardcoded list of known-good program ids.
//! When LP-0023's on-chain or HTTP registry is available, a second
//! implementation can query it.

use async_trait::async_trait;
use lee::AccountId;
use std::collections::HashSet;
use std::str::FromStr;

/// Verification status for a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationStatus {
    Verified { source: String },
    VerifiedExternal { source: String, registry: String },
    Unverified,
    NotChecked { reason: String },
    Unavailable,
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Verified { source } => write!(f, "[✓] Verified ({})", source),
            Self::VerifiedExternal { source, registry } => {
                write!(f, "[✓] Verified ({}, via {})", source, registry)
            }
            Self::Unverified => write!(f, "[!] Unverified"),
            Self::NotChecked { reason } => write!(f, "[i] Not checked: {}", reason),
            Self::Unavailable => write!(f, "[✗] Unavailable"),
        }
    }
}

/// Trait for program verification backends.
#[async_trait]
pub trait ProgramVerifier: Send + Sync {
    async fn verify(&self, program_id: &AccountId) -> VerificationStatus;
}

/// Built-in verifier with a hardcoded allow-list of known program ids.
pub struct BuiltinVerifier {
    known_programs: HashSet<AccountId>,
    external_enabled: bool,
}

impl BuiltinVerifier {
    pub fn new(external_enabled: bool) -> Self {
        let mut known = HashSet::new();

        // Native token program is always known.
        known.insert(lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID);

        // Native program loader is always known.
        known.insert(lee_core::program::PROGRAM_LOADER_ACCOUNT_ID);

        // Dynamically load deployed program IDs from deployments.json if available.
        if let Ok(content) = std::fs::read_to_string("deployments.json")
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
        {
            if let Some(hdr) = v.get("testimonial_program_header").and_then(|h| h.as_str()) {
                let clean = hdr.trim_start_matches("Public/");
                if let Ok(id) = AccountId::from_str(clean) {
                    known.insert(id);
                }
            }
            if let Some(networks) = v.get("networks").and_then(|n| n.as_object()) {
                for (_net, entry) in networks {
                    if let Some(hdr) = entry
                        .get("testimonial_program_header")
                        .and_then(|h| h.as_str())
                    {
                        let clean = hdr.trim_start_matches("Public/");
                        if let Ok(id) = AccountId::from_str(clean) {
                            known.insert(id);
                        }
                    }
                }
            }
        }

        Self {
            known_programs: known,
            external_enabled,
        }
    }

    /// Add a program id to the known-good list.
    pub fn add_known(&mut self, program_id: AccountId) {
        self.known_programs.insert(program_id);
    }
}

#[async_trait]
impl ProgramVerifier for BuiltinVerifier {
    async fn verify(&self, program_id: &AccountId) -> VerificationStatus {
        if self.known_programs.contains(program_id) {
            return VerificationStatus::Verified {
                source: "built-in known programs list".into(),
            };
        }

        if !self.external_enabled {
            return VerificationStatus::NotChecked {
                reason: "external verification disabled by user".into(),
            };
        }

        // Query LP-0023 registry when available on-chain.
        VerificationStatus::NotChecked {
            reason: "LP-0023 registry not yet implemented".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_verify_native_token_and_loader() {
        let verifier = BuiltinVerifier::new(false);

        let status_token = verifier
            .verify(&lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID)
            .await;
        assert!(matches!(status_token, VerificationStatus::Verified { .. }));

        let status_loader = verifier
            .verify(&lee_core::program::PROGRAM_LOADER_ACCOUNT_ID)
            .await;
        assert!(matches!(status_loader, VerificationStatus::Verified { .. }));

        let unknown_id = AccountId::new([99u8; 32]);
        let status_unknown = verifier.verify(&unknown_id).await;
        assert!(matches!(
            status_unknown,
            VerificationStatus::NotChecked { .. }
        ));
    }

    #[tokio::test]
    async fn test_add_known_program() {
        let mut verifier = BuiltinVerifier::new(false);
        let custom_id = AccountId::new([77u8; 32]);

        assert!(matches!(
            verifier.verify(&custom_id).await,
            VerificationStatus::NotChecked { .. }
        ));

        verifier.add_known(custom_id);
        assert!(matches!(
            verifier.verify(&custom_id).await,
            VerificationStatus::Verified { .. }
        ));
    }
}
