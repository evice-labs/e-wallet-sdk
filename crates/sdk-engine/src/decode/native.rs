use super::{DecodedInstruction, ProgramDecoder};
use lee::AccountId;

pub struct NativeDecoder;

// TODO: Implement actual decoding from program instruction bytes.
// For now, stub that returns None so the registry falls through to raw display.

impl NativeDecoder {
    #[allow(dead_code)]
    pub fn name() -> &'static str {
        "native"
    }
}

impl ProgramDecoder for NativeDecoder {
    fn program_ids(&self) -> Vec<AccountId> {
        // TODO: Return the actual program id(s) for native.
        vec![]
    }

    fn decode(&self, _program_id: &AccountId, _data: &[u8]) -> Option<DecodedInstruction> {
        // TODO: Implement real decoding.
        None
    }
}
