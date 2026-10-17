use super::{DecodedInstruction, ProgramDecoder};
use lee::AccountId;

pub struct TokenDecoder;

// TODO: Implement actual decoding from program instruction bytes.
// For now, stub that returns None so the registry falls through to raw display.

impl TokenDecoder {
    #[allow(dead_code)]
    pub fn name() -> &'static str {
        "token"
    }
}

impl ProgramDecoder for TokenDecoder {
    fn program_ids(&self) -> Vec<AccountId> {
        // TODO: Return the actual program id(s) for token.
        vec![]
    }

    fn decode(&self, _program_id: &AccountId, _data: &[u8]) -> Option<DecodedInstruction> {
        // TODO: Implement real decoding.
        None
    }
}
