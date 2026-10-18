//! Program source verification (LP-0023).
//!
//! The [`ProgramVerifier`] trait abstracts how the engine checks whether a
//! program's bytecode is reproducibly built from known source. The approval
//! prompt shows the verification status.
//!
//! The built-in verifier maintains a hardcoded list of known-good program ids.
//! When LP-0023's on-chain or HTTP registry is available, a second
//! implementation can query it.

use std::collections::HashSet;

use async_trait::async_trait;
use lee::AccountId;

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
            Self::Verified { source } => write!(f, "✅ Verified ({})", source),
            Self::VerifiedExternal { source, registry } => {
                write!(f, "✅ Verified ({}, via {})", source, registry)
            }
            Self::Unverified => write!(f, "⚠️ Unverified"),
            Self::NotChecked { reason } => write!(f, "ℹ️ Not checked: {}", reason),
            Self::Unavailable => write!(f, "❌ Unavailable"),
        }
    }
}

/// Trait for program verification backends.
#[async_trait]
pub trait ProgramVerifier: Send + Sync {
    /// Check the verification status of a program.
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

        // TODO: Add token_program, testimonial_program, program_loader ids.

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

        // TODO: Query LP-0023 registry when available.
        VerificationStatus::NotChecked {
            reason: "LP-0023 registry not yet implemented".into(),
        }
    }
}
