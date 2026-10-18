## Evice Wallet SDK

> **LEZ Wallet & Provider SDK** — A modular, high-performance wallet engine and dApp integration SDK for the Logos Execution Zone (LEZ).

## Overview

Provides the foundational wallet infrastructure, developer SDK, and integration tooling for the Logos Execution Zone (LEZ).

It solves the core challenges of asset custody, dApp communication, and transaction security on LEZ:
- **Full LEZ Asset & Account Management:** Unified handling of native and token-program assets across both public and private (shielded) accounts.
- **Wallet Provider SDK:** Robust, asynchronous dApp-facing API enabling external applications to connect, request account access, read balances, and propose transactions.
- **Informed Consent & Approval UX:** Every transaction proposal produces human-readable instruction summaries, simulated balance/state diffs, and bytecode source verification status before signing.
- **Privacy-Preserving Name Resolution:** Built-in clean-room resolver for human-readable `.lez` names (Logos Name Service) using off-chain Argon2id key derivation and on-chain PDA lookups.
- **Decoupled Three-Layer Architecture:** Pure Rust core logic (`sdk-engine`) with zero GUI dependencies, accessible via C-ABI (`sdk-ffi`) for Logos Basecamp modules and via CLI.

## Workspace Structure

The workspace is structured into specialized, decoupled crates under `crates/`:

```
e-wallet-sdk/
├── Cargo.toml                  # Workspace manifest and pinned dependencies
├── SUBMISSION_ID               # Unique submission identifier for on-chain testimonials
├── LICENSE-MIT / LICENSE-APACHE
├── crates/
│   ├── sdk-engine/             # Core wallet engine (pure Rust, no Qt)
│   ├── sdk-ffi/                # C-ABI FFI layer for Basecamp module binding
│   ├── cli/                    # Command-line interface (`lez-wallet`)
│   └── evidence-tool/          # Adoption verification tool for on-chain testimonials
├── dev-env/                    # Local development sequencer and genesis accounts
├── integration-tests/          # End-to-end integration test suites
├── docs/                       # Technical specifications and API documentation
└── .github/workflows/          # Multi-platform CI workflows (Linux & macOS)
```

### Crate Roles

| Crate | Responsibility | Dependencies |
|---|---|---|
| **`sdk-engine`** | Central orchestrator: sessions, permissions, transaction queue, decoders, effect simulations, LNS resolver, and atomic persistence. | `wallet`, `lee`, `common`, standard crates |
| **`sdk-ffi`** | Exposes C-ABI (`extern "C"`) functions with opaque handles and JSON serialization, consumed by Basecamp Qt plugins (`wallet-provider`). | `sdk-engine` |
| **`cli`** | Interactive command-line binary (`lez-wallet`) for headless operations, testing, and scriptable management. | `sdk-engine`, `clap`, `tokio` |
| **`evidence-tool`** | Scans on-chain testimonial records and outputs adoption statistics filtered by submission identifier. | `wallet`, `common`, `clap` |

## Implemented Architecture & Features

#### 1. Granular Session & Permission Management (`session.rs`)
- Connections from external dApps open an isolated, ephemeral in-memory `Session`.
- Grants are assigned per-account with specific privileges:
  - `GrantKind::Read`: Permission to inspect balances and program states.
  - `GrantKind::Propose`: Permission to submit transaction proposals for user approval.
- Sessions include automatic timestamped expiration (`DEFAULT_SESSION_DURATION`).

#### 2. Immutable Requests & Intent Hashing (`request.rs`)
- Propose calls (`proposeNativeTransfer`, `proposeTokenTransfer`, `proposeTransaction`) immediately return a `RequestId`.
- The engine computes a deterministic 32-byte SHA-256 intent hash (`lez-wallet-sdk/intent/v1/`) binding the request ID, session ID, and serialised parameters.
- The hash is verified before execution, eliminating Time-of-Check to Time-of-Use (TOCTOU) tampering.

#### 3. Asynchronous Non-Blocking Approvals (`approval.rs`)
- The pluggable `ApprovalHandler` trait allows user authorization without blocking UI or async runtimes:
  - `CliApprovalHandler`: Formatted terminal modal prompt reading interactive user input.
  - `UiApprovalHandler`: Bridge routing prompts to Basecamp desktop QML modals.
  - `PolicyApprovalHandler`: Auto-approval policy feature-gated behind `dev-auto-approve` (audited by CI to ensure absence from release builds).

#### 4. Instruction Decoders & Effect Simulation (`decode/`, `effects.rs`)
- The `DecoderRegistry` automatically inspects program calldata and converts raw bytes into human-readable operations:
  - Native token transfers.
  - Token program operations (holding creation, mint, burn, transfer).
  - Associated Token Accounts (ATA).
  - Testimonial program submissions.
- `EffectSimulator` pre-calculates balance changes and state mutations (`EffectSource::Simulation`), preventing blind signing.

#### 5. Program Source Verification (`verifier.rs`)
- Integrates source-code reproducibility verification based on the LP-0023 registry abstraction.
- Displays `VerificationStatus::Verified`, `NotVerified`, or `NotChecked` alongside compiler and source repository metadata.

#### 6. Clean-Room LNS Resolver (`resolve/lns.rs`)
- Full, independent implementation of SPEC.md v2 for `.lez` domain resolution.
- Normalises names (`[a-z0-9-]{1,32}`), derives master keys via Argon2id (64 MiB, 3 passes), and calculates PDA seeds without depending on external binaries.
- In-memory cache with 5-minute TTL, cleared automatically upon wallet lock.
- User-controllable toggle (`lns_enabled`) with graceful fallback to raw 32-byte account IDs.

#### 7. Resilient Storage (`store.rs`)
- Atomic persistence using write-then-rename (`.tmp` → target file) preventing state corruption during unexpected crashes or power loss.
- Automatic `.bak` backup retention and fallback recovery.

## Quick Start

### Prerequisites
- **Rust Toolchain:** Pinned to Rust 1.98.1+ via `rust-toolchain.toml`.
- **LEZ Source:** Local checkout of `logos-execution-zone` accessible in the workspace parent directory.

### Building & Checking

```bash
# Clone the repository
git clone https://github.com/evice-labs/e-wallet-sdk.git
cd e-wallet-sdk

# Compile all workspace crates
cargo build

# Run the complete test suite (all 18 unit tests)
cargo test --workspace

# Check clippy lints
cargo clippy --workspace -- -D warnings
```

### Running the CLI

```bash
# Inspect SDK and environment version
cargo run --bin lez-wallet -- info

# Test offline .lez name resolution (normalisation, Argon2id derivation & PDA calculation)
cargo run --bin lez-wallet -- resolve alice.lez
```

### Self-Contained Demo

Execute the demo script to verify build integrity, CLI diagnostics, offline name resolution, and submission identifier output:

```bash
./demo.sh
```

## Continuous Integration & Quality Assurance

Automated GitHub Actions workflows are configured in `.github/workflows/`:
- **`ci-linux.yml`:** Runs on `ubuntu-latest` validating `cargo check`, `cargo test`, `cargo clippy`, and verifies the complete absence of test auto-approval symbols (`PolicyApprovalHandler`) in release binaries.
- **`ci-macos.yml`:** Runs on `macos-latest` ensuring full macOS platform compatibility.

## Security & Privacy Guarantees

- **No Telemetry:** Zero analytics, metrics, or telemetry are collected or transmitted.
- **No Mandatory External Services:** All core wallet functions, key management, and transactions communicate solely with the user's configured LEZ sequencer node.
- **Client-Side Cryptography:** All private key derivations, Argon2id computations, and transaction signings occur exclusively on the local machine.
- **Explicit User Consent:** A dApp cannot sign transactions, read private balances, or mutate state without explicit, per-account user authorization.

## Acknowledgments & Attribution

- **Logos Name Service (LNS):** We gratefully acknowledge **Jef** ([@jefdiesel](https://github.com/jefdiesel)) for designing and publishing the open protocol specification for the Logos Name Service ([SPEC.md](https://github.com/jefdiesel/logos-names/blob/main/SPEC.md)) on the Logos Execution Zone. This SDK includes an independent, clean-room Rust implementation of the client-side name resolution protocol (`sdk-engine::resolve::lns`).
- **Identity Accountability Architecture:** The accountable pseudonymity model, deterministic identity commitments, and threshold accountability primitives are designed by **Evice Labs**, adapted from the [`e-identity-stack`](https://github.com/evice-labs/e-identity-stack) repository for native integration into the Logos Execution Zone.

## License

Dual-licensed under either:
- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE))

at your option.
