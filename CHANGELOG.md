## Changelog

All notable changes to the Evice Wallet SDK are documented in this file.

## [v0.2.2] - 2026-11-11

### Features & Cross-Platform Compatibility
- **Windows Cross-Compilation Support (`x86_64-pc-windows-gnu`):**
  - Added conditional compilation attributes (`#[cfg(unix)]` / `#[cfg(not(unix))]`) across `crates/sdk-engine/src/client.rs` for IPC client and daemon communication routines, safely isolating Unix domain sockets from Windows targets.
  - Added `crates/sdk-ffi/build.rs` configuring linker search flags and system libraries for Windows (`ws2_32`, `userenv`, `ntdll`, `bcrypt`, `crypt32`, `secur32`, `winscard`).
  - Compiled and vendored 64-bit Windows dynamic and static C-ABI libraries (`e_wallet_sdk.dll`, `libe_wallet_sdk.dll.a`, `e_wallet_sdk.lib`).

### Workspace & Manifest Cleanup
- **Cargo Workspace Member Pruning:**
  - Streamlined `Cargo.toml` `workspace.members` to contain only active crates on `main` (`crates/sdk-engine` and `crates/sdk-ffi`), eliminating broken references to non-existent crate paths.
  - Bumped workspace version to `0.2.2` across `Cargo.toml`, child crates, and documentation.

## [v0.2.1] - 2026-11-10

### Features & Reliability
- **Protocol-Adaptive Sequencer Status Reconciliation:**
  - Implemented `OwnerApi::get_transaction_status` in `crates/sdk-engine/src/api.rs` querying sequencer JSON-RPC `getTransaction`.
  - Directly returns `TransactionStatusView { status: "confirmed", block_id: Some(block_id) }` upon block commitment and `status: "pending"` with `block_id: None` while in mempool.
  - Bypasses protobuf payload deserialization differences across protocol versions (v0.3 shard model vs v0.4 actor model).
- **C-ABI Export:**
  - Exported `e_wallet_get_transaction_status` in `crates/sdk-ffi/src/lib.rs` returning JSON status payload for host environments.

## [v0.2.0] - 2026-11-09

### Initial Stable Release
- Modular wallet engine (`sdk-engine`) and C-ABI bridge (`sdk-ffi`).
- Argon2id encrypted keystore with AES-256-GCM cipher suite.
- Public and Shielded zero-knowledge account derivation on LEZ.
- Client-side Logos Name Service (LNS) offline and on-chain resolver.
- Identity Accountability (master commitment, scoped nullifiers, BIP-340 Schnorr attestations).
