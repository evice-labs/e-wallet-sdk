use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum TestimonialError {
    #[error("Testimonial text cannot be empty")]
    EmptyText,
    #[error("Testimonial text exceeds maximum length of 280 bytes (got {0} bytes)")]
    TextTooLong(usize),
    #[error("Username exceeds maximum length of 64 bytes (got {0} bytes)")]
    UsernameTooLong(usize),
    #[error("Username contains invalid characters (must be alphanumeric or underscore)")]
    InvalidUsernameCharacters,
    #[error("Account has already submitted a testimonial for this submission ID")]
    AlreadySubmitted,
    #[error("State deserialization failed")]
    DeserializationFailed,
    #[error("State serialization failed")]
    SerializationFailed,
    #[error("Unauthorized transaction: signer required")]
    Unauthorized,
}
