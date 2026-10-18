use super::{DecodedInstruction, ProgramDecoder};
use lee::AccountId;

pub struct AtaDecoder;

// TODO: Implement actual decoding from program instruction bytes.
// For now, stub that returns None so the registry falls through to raw display.

impl AtaDecoder {
    #[allow(dead_code)]
    pub fn name() -> &'static str {
        "ata"
    }
}

impl ProgramDecoder for AtaDecoder {
    fn program_ids(&self) -> Vec<AccountId> {
        // TODO: Return the actual program id(s) for ata.
        vec![]
    }

    fn decode(&self, _program_id: &AccountId, _data: &[u8]) -> Option<DecodedInstruction> {
        // TODO: Implement real decoding.
        None
    }
}
