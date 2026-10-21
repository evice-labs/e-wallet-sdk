use super::{DecodedInstruction, ProgramDecoder};
use lee::AccountId;
use std::str::FromStr;
use testimonial_program::TestimonialInstruction;

/// Decoder for testimonial program calldata.
pub struct TestimonialDecoder {
    known_programs: Vec<AccountId>,
}

impl Default for TestimonialDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestimonialDecoder {
    /// Create decoder reading deployed program headers from deployments.json if available.
    pub fn new() -> Self {
        let mut known = Vec::new();
        if let Ok(content) = std::fs::read_to_string("deployments.json")
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
        {
            if let Some(hdr) = v.get("testimonial_program_header").and_then(|h| h.as_str()) {
                let clean = hdr.trim_start_matches("Public/");
                if let Ok(id) = AccountId::from_str(clean) {
                    known.push(id);
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
                            if !known.contains(&id) {
                                known.push(id);
                            }
                        }
                    }
                }
            }
        }
        Self {
            known_programs: known,
        }
    }

    /// Add an explicit program ID to decode.
    pub fn with_program_id(mut self, id: AccountId) -> Self {
        self.known_programs.push(id);
        self
    }

    pub fn name() -> &'static str {
        "testimonial"
    }
}

impl ProgramDecoder for TestimonialDecoder {
    fn program_ids(&self) -> Vec<AccountId> {
        self.known_programs.clone()
    }

    fn decode(&self, _program_id: &AccountId, data: &[u8]) -> Option<DecodedInstruction> {
        let instruction = TestimonialInstruction::from_bytes(data).ok()?;
        match instruction {
            TestimonialInstruction::SubmitTestimonial {
                submission_id,
                text,
                username,
            } => Some(DecodedInstruction {
                program: "testimonial".into(),
                operation: "SubmitTestimonial".into(),
                fields: vec![
                    ("Submission ID".into(), hex::encode(submission_id)),
                    ("Text".into(), text),
                    (
                        "Username".into(),
                        username.unwrap_or_else(|| "Anonymous".into()),
                    ),
                ],
                raw: data.to_vec(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_submit_testimonial() {
        let decoder = TestimonialDecoder::new();
        let instruction = TestimonialInstruction::SubmitTestimonial {
            submission_id: [7u8; 32],
            text: "Hello from LEZ Testimonial!".into(),
            username: Some("bob_builder".into()),
        };
        let bytes = instruction.to_bytes().unwrap();
        let dummy_program = AccountId::new([9u8; 32]);

        let decoded = decoder.decode(&dummy_program, &bytes);
        assert!(decoded.is_some());
        let d = decoded.unwrap();
        assert_eq!(d.operation, "SubmitTestimonial");
        assert_eq!(d.fields[1].1, "Hello from LEZ Testimonial!");
        assert_eq!(d.fields[2].1, "bob_builder");
    }
}
