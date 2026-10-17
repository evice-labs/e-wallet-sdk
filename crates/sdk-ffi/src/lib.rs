//! C-ABI FFI layer for the LEZ Wallet SDK .
//!
//! This crate exposes the `sdk-engine` API as `extern "C"` functions
//! consumed by the Basecamp core module (`wallet-provider`) via cbindgen.
//!
//! TODO: Implement the full C-ABI surface with:
//! - JSON serialization for complex types across the FFI boundary
//! - Opaque handle for SdkEngine
//! - Error codes + message buffer pattern
//! - cbindgen.toml for header generation

/// Placeholder: returns the SDK version as a C string.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the program.
#[unsafe(no_mangle)]
pub extern "C" fn lez_sdk_version() -> *const std::ffi::c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const std::ffi::c_char
}

/// Placeholder: returns the API version number.
#[unsafe(no_mangle)]
pub extern "C" fn lez_sdk_api_version() -> u32 {
    sdk_engine::API_VERSION
}
