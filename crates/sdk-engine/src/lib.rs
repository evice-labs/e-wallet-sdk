//! # LEZ Wallet SDK Engine.
//!
//! Foundational engine crate providing LNS resolution and error types.

pub mod error;
pub mod resolve;

pub use error::SdkError;
pub use resolve::lns::{LnsConfig, LnsResolver};
pub use resolve::NameResolver;

pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const API_VERSION: u32 = 1;
