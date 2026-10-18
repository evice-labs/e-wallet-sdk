use super::{DecodedInstruction, ProgramDecoder};
use lee::AccountId;

pub struct TestimonialDecoder;

// TODO: Implement actual decoding from program instruction bytes.
// For now, stub that returns None so the registry falls through to raw display.

impl TestimonialDecoder {
    #[allow(dead_code)]
    pub fn name() -> &'static str {
        "testimonial"
    }
}

impl ProgramDecoder for TestimonialDecoder {
    fn program_ids(&self) -> Vec<AccountId> {
        // TODO: Return the actual program id(s) for testimonial.
        vec![]
    }

    fn decode(&self, _program_id: &AccountId, _data: &[u8]) -> Option<DecodedInstruction> {
        // TODO: Implement real decoding.
        None
    }
}
