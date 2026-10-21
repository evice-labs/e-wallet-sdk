use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// An on-chain record representing an individual user testimonial.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TestimonialRecord {
    pub submission_id: [u8; 32],
    pub submitter: [u8; 32],
    pub text: String,
    pub username: Option<String>,
    pub timestamp: u64,
}

/// The actor state for a testimonial instance or registry.
#[derive(
    BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default,
)]
pub struct TestimonialState {
    pub records: Vec<TestimonialRecord>,
}

impl TestimonialState {
    /// Finds a testimonial submitted by a specific submitter for a given submission ID.
    pub fn find_record(
        &self,
        submitter: &[u8; 32],
        submission_id: &[u8; 32],
    ) -> Option<&TestimonialRecord> {
        self.records
            .iter()
            .find(|r| &r.submitter == submitter && &r.submission_id == submission_id)
    }

    /// Counts distinct submitter accounts for a given submission ID.
    pub fn count_distinct_submitters(&self, submission_id: &[u8; 32]) -> usize {
        let mut accounts = std::collections::HashSet::new();
        for r in &self.records {
            if &r.submission_id == submission_id {
                accounts.insert(r.submitter);
            }
        }
        accounts.len()
    }
}

/// Derives deterministic PDA seed for an individual testimonial.
///
/// Formula: SHA-256("testimonial" ‖ submitter ‖ submission_id)
pub fn derive_testimonial_pda_seed(submitter: &[u8; 32], submission_id: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"testimonial");
    hasher.update(submitter);
    hasher.update(submission_id);
    hasher.finalize().into()
}
