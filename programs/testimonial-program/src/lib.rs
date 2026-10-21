pub mod error;
pub mod instruction;
pub mod state;

pub use error::TestimonialError;
pub use instruction::TestimonialInstruction;
pub use state::{derive_testimonial_pda_seed, TestimonialRecord, TestimonialState};

use lee_core::program::{ReceiveInput, Response};

/// Processes a testimonial instruction against an in-memory state.
pub fn process_instruction(
    state: &mut TestimonialState,
    instruction: TestimonialInstruction,
    submitter: [u8; 32],
    timestamp: u64,
) -> Result<(), TestimonialError> {
    match instruction {
        TestimonialInstruction::SubmitTestimonial {
            submission_id,
            text,
            username,
        } => {
            let trimmed_text = text.trim();
            if trimmed_text.is_empty() {
                return Err(TestimonialError::EmptyText);
            }
            if text.len() > 280 {
                return Err(TestimonialError::TextTooLong(text.len()));
            }

            if let Some(ref uname) = username {
                if uname.is_empty() || uname.len() > 64 {
                    return Err(TestimonialError::UsernameTooLong(uname.len()));
                }
                if !uname
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
                {
                    return Err(TestimonialError::InvalidUsernameCharacters);
                }
            }

            // Enforce uniqueness: one testimonial per (submitter, submission_id)
            if state
                .records
                .iter()
                .any(|r| r.submitter == submitter && r.submission_id == submission_id)
            {
                return Err(TestimonialError::AlreadySubmitted);
            }

            state.records.push(TestimonialRecord {
                submission_id,
                submitter,
                text,
                username,
                timestamp,
            });

            Ok(())
        }
    }
}

/// Standard LEZ actor message handler returning a Response directly.
pub fn handle_message(input: &ReceiveInput, instruction: TestimonialInstruction) -> Response {
    let mut state: TestimonialState = if input.pre_state.is_empty() {
        TestimonialState::default()
    } else {
        borsh::from_slice(&input.pre_state).expect("Failed to deserialize state")
    };

    let submitter = *input.receiver.account_id.value();
    process_instruction(&mut state, instruction, submitter, 0).expect("Invalid instruction");

    let post_state_bytes = borsh::to_vec(&state).expect("Failed to serialize state");
    Response::set_state(post_state_bytes)
}

/// Checked version of the handler returning Result.
pub fn handle_message_checked(
    input: &ReceiveInput,
    instruction: TestimonialInstruction,
) -> Result<Response, TestimonialError> {
    let mut state: TestimonialState = if input.pre_state.is_empty() {
        TestimonialState::default()
    } else {
        borsh::from_slice(&input.pre_state).map_err(|_| TestimonialError::DeserializationFailed)?
    };

    let submitter = *input.receiver.account_id.value();
    process_instruction(&mut state, instruction, submitter, 0)?;

    let post_state_bytes =
        borsh::to_vec(&state).map_err(|_| TestimonialError::SerializationFailed)?;
    Ok(Response::set_state(post_state_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submit_valid_testimonial() {
        let mut state = TestimonialState::default();
        let submitter = [1u8; 32];
        let submission_id = [42u8; 32];

        let instruction = TestimonialInstruction::SubmitTestimonial {
            submission_id,
            text: "I use Evice Wallet SDK! It is fast and secure.".into(),
            username: Some("alice_builder".into()),
        };

        let res = process_instruction(&mut state, instruction, submitter, 1700000000);
        assert!(res.is_ok());
        assert_eq!(state.records.len(), 1);
        assert_eq!(state.count_distinct_submitters(&submission_id), 1);
    }

    #[test]
    fn test_reject_empty_text() {
        let mut state = TestimonialState::default();
        let submitter = [1u8; 32];
        let submission_id = [42u8; 32];

        let instruction = TestimonialInstruction::SubmitTestimonial {
            submission_id,
            text: "   ".into(),
            username: None,
        };

        let res = process_instruction(&mut state, instruction, submitter, 0);
        assert_eq!(res.unwrap_err(), TestimonialError::EmptyText);
    }

    #[test]
    fn test_reject_text_exceeding_280_bytes() {
        let mut state = TestimonialState::default();
        let submitter = [1u8; 32];
        let submission_id = [42u8; 32];

        let long_text = "a".repeat(281);
        let instruction = TestimonialInstruction::SubmitTestimonial {
            submission_id,
            text: long_text,
            username: None,
        };

        let res = process_instruction(&mut state, instruction, submitter, 0);
        assert_eq!(res.unwrap_err(), TestimonialError::TextTooLong(281));
    }

    #[test]
    fn test_reject_username_exceeding_64_bytes() {
        let mut state = TestimonialState::default();
        let submitter = [1u8; 32];
        let submission_id = [42u8; 32];

        let long_username = "u".repeat(65);
        let instruction = TestimonialInstruction::SubmitTestimonial {
            submission_id,
            text: "Valid text".into(),
            username: Some(long_username),
        };

        let res = process_instruction(&mut state, instruction, submitter, 0);
        assert_eq!(res.unwrap_err(), TestimonialError::UsernameTooLong(65));
    }

    #[test]
    fn test_reject_duplicate_testimonial_same_submitter() {
        let mut state = TestimonialState::default();
        let submitter = [1u8; 32];
        let submission_id = [42u8; 32];

        let instruction = TestimonialInstruction::SubmitTestimonial {
            submission_id,
            text: "First review".into(),
            username: Some("alice".into()),
        };
        assert!(process_instruction(&mut state, instruction.clone(), submitter, 0).is_ok());

        // Duplicate attempt
        let res = process_instruction(&mut state, instruction, submitter, 1);
        assert_eq!(res.unwrap_err(), TestimonialError::AlreadySubmitted);
    }

    #[test]
    fn test_distinct_submitters_same_submission_id() {
        let mut state = TestimonialState::default();
        let submitter1 = [1u8; 32];
        let submitter2 = [2u8; 32];
        let submission_id = [42u8; 32];

        assert!(process_instruction(
            &mut state,
            TestimonialInstruction::SubmitTestimonial {
                submission_id,
                text: "Review 1".into(),
                username: Some("alice".into()),
            },
            submitter1,
            0,
        )
        .is_ok());

        assert!(process_instruction(
            &mut state,
            TestimonialInstruction::SubmitTestimonial {
                submission_id,
                text: "Review 2".into(),
                username: Some("bob".into()),
            },
            submitter2,
            0,
        )
        .is_ok());

        assert_eq!(state.count_distinct_submitters(&submission_id), 2);
    }

    #[test]
    fn test_pda_derivation_deterministic() {
        let submitter = [5u8; 32];
        let submission_id = [9u8; 32];

        let seed1 = derive_testimonial_pda_seed(&submitter, &submission_id);
        let seed2 = derive_testimonial_pda_seed(&submitter, &submission_id);

        assert_eq!(seed1, seed2);
        assert_ne!(seed1, [0u8; 32]);
    }
}
