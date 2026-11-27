## Changelog

All notable changes to the Evice Wallet SDK are documented in this file.

## [v0.3.0] - 2026-11-27

### LEZ v0.4.0 & Testnet 0.4 Compatibility
- **Upstream LEZ v0.4.0 Upgrade:** Pinned `logos-execution-zone` crates (`wallet`, `lee`, `lee_core`, `common`, `sequencer_service_rpc`, `testnet_initial_state`, `bridge_core`, `program_loader_core`) to official release tag `v0.4.0` (`rev a255b42c0fc80d1cb219454a3a1f5ec3b5d92115`), aligned `risc0-binfmt = "3.0.6"` and `risc0-zkos-v1compat = "=2.2.3"`, and removed obsolete `keycard_wallet` and `libpcsclite` dependencies.
- **SDK Engine & Bridge Fixes (`crates/sdk-engine`):**
  - Migrated account/signing APIs to `PublicAccountSecret`/`pub_account_secret`, `wallet::DEFAULT_GAS_LIMIT` (`10,000,000`), `bridge_core::{DepositMetadata, DepositRecipient}`, `Authority::Signer`/`Authority::None`, and `WitnessSet::sign`.
  - Fixed dual-market (`exec` + `stor` `charged_bytes()`) `max_fee` sizing in `pow.rs::execute_on_chain_bridge_disbursement` verified against a live standalone v0.4.0 sequencer.
  - Updated `sdk_info().network` to `"testnet-0.4"`, removed pre-v0.4.0 RPC `-32601` workarounds, and deduplicated engine/client helpers.
- **C-ABI FFI (`crates/sdk-ffi`, `include/e_wallet_sdk.h`):**
  - Restored `#[unsafe(no_mangle)]` on `e_wallet_get_gas_limit` and regenerated `include/e_wallet_sdk.h` via `cbindgen` to include `e_wallet_get_gas_limit` and `e_wallet_get_transaction_status`.
- **CLI, Dispatcher, Dev-Env & CI:**
  - Derived standalone/dispatcher genesis Alice directly from `testnet_initial_state::initial_pub_accounts_private_keys()` and aligned `dev-env/sequencer_config.json`, `dev-env/genesis_accounts.json`, `Dockerfile` (`r0vm 3.1.0`, `--branch v0.4.0`), `Justfile` (`--tag v0.4.0`), and GitHub Actions workflows.
  - Added ignored live E2E test `test_live_sequencer_transfer_routes_and_bridge` covering PoW mining + `channel_deposit` and all four transfer routes (`Public -> Public`, `Public -> Private`, `Private -> Private`, `Private -> Public`).
- **Release Version Bump:** Promoted workspace SDK version from `0.3.0-rc1` to `0.3.0`.

## [v0.3.0-rc1] - 2026-11-26

### Modular Refactoring & Dependency Alignment (PR #24)
- **Workspace & Legacy Cleanup (`3d3cab3`):** Removed `crates/sdk-engine/src/v03_compat.rs`, updated `logos-execution-zone` pins, aligned `risc0` dependencies in `Cargo.toml`, and simplified `crates/sdk-engine/src/client.rs`, `crates/sdk-engine/src/pow.rs`, and `crates/cli/src/main.rs`.
- **Modular SDK Engine & Nonce-Drop Detection (`5792f01`):** Decomposed `crates/sdk-engine/src/api.rs` into domain modules (`api/mod.rs`, `api/dapp.rs`, `api/engine.rs`, `api/owner.rs`) and added `submitted_tx_nonces` tracking to report `"dropped"` transactions in `get_transaction_status`.
- **Modular C-ABI FFI (`bd20600`):** Split `crates/sdk-ffi/src/lib.rs` into domain modules (`approval.rs`, `dapp.rs`, `dto.rs`, `identity.rs`, `pow.rs`, `wallet.rs`).
- **Documentation (`e5c0f2f`):** Updated `README.md`, `docs/ARCHITECTURE.md`, and `docs/sdk-api-reference.md`.

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
