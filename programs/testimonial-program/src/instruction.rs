use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// Instructions accepted by the Testimonial Program.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum TestimonialInstruction {
    /// Submit an on-chain testimonial.
    ///
    /// - `submission_id`: The 32-byte prize submission identifier.
    /// - `text`: Testimonial content (max 280 bytes).
    /// - `username`: Optional pseudonym or handle (max 64 bytes).
    SubmitTestimonial {
        submission_id: [u8; 32],
        text: String,
        username: Option<String>,
    },
}

impl TestimonialInstruction {
    /// Helper to serialize instruction into Borsh bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, std::io::Error> {
        borsh::to_vec(self)
    }

    /// Helper to deserialize instruction from Borsh bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, std::io::Error> {
        borsh::from_slice(bytes)
    }
}
