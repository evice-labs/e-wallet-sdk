## Evice Wallet SDK — On-Chain Smart Contract Deployment & Dispatcher Guide

> **Official Developer Guide** for compiling, deploying, and dispatching transactions to on-chain smart contracts within the Logos Execution Zone (LEZ) ecosystem using the Evice Wallet Provider SDK.

## 1. Architectural Overview

The Evice Wallet SDK includes native on-chain smart contract artifacts, a headless program dispatcher tool, and a persistent deployment registry. This architecture provides seamless development and testing of smart contract interactions locally (offline) and on the public LEZ testnet.

```
┌────────────────────────────────────────────────────────────────────────┐
│                       EVICE WALLET SDK ECOSYSTEM                       │
│                                                                        │
│   ┌───────────────────────────┐      ┌─────────────────────────────┐   │
│   │ programs/testimonial-prog │      │   tools/dispatcher          │   │
│   │ (zkVM Guest Actor)        │      │   (`lez-dispatcher` CLI)    │   │
│   └─────────────┬─────────────┘      └──────────────┬──────────────┘   │
│                 │                                   │                  │
│                 ▼                                   ▼                  │
│      Compiled Program ELF / Bin             ProgramLoader::deploy      │
│                 │                                   │                  │
│                 └─────────────────► ◄───────────────┘                  │
│                                     │                                  │
│                                     ▼                                  │
│                       ┌───────────────────────────┐                    │
│                       │     deployments.json      │                    │
│                       │   (Environment Registry)  │                    │
│                       └─────────────┬─────────────┘                    │
│                                     │                                  │
│             ┌───────────────────────┴───────────────────────┐          │
│             ▼                                               ▼          │
│   ┌───────────────────┐                           ┌───────────────────┐│
│   │ sdk-engine        │                           │ crates/           ││
│   │ • TestimonialDec  │                           │ evidence-tool     ││
│   │ • BuiltinVerifier │                           │ • Adoption Audit  ││
│   └───────────────────┘                           └───────────────────┘│
└────────────────────────────────────────────────────────────────────────┘
```

### Key Components

1. **`programs/testimonial-program`:**
   - On-chain RISC-V zkVM guest actor contract.
   - Manages immutable testimonial records attributed to a unique `SUBMISSION_ID`.
2. **`tools/dispatcher` (`lez-dispatcher` CLI):**
   - Headless deployment and transaction dispatch CLI built with `wallet::WalletCore` and `program_facades::program_loader::ProgramLoader`.
   - Automates segment partitioning (`MAX_SEGMENT_DATA_LEN`), header initialization, and multi-network registry management.
3. **`deployments.json`:**
   - Version-controlled JSON registry mapping active networks (`standalone` vs. `testnet`) to sequencer RPC endpoints and deployed program header account IDs.
   - Dynamically consumed by the SDK engine's [`BuiltinVerifier`](../crates/sdk-engine/src/verifier.rs) and [`TestimonialDecoder`](../crates/sdk-engine/src/decode/testimonial.rs).
4. **`crates/evidence-tool`:**
   - Audit utility scanning on-chain sequencer storage to verify the 150 distinct testimonial accounts required for milestone adoption evidence.

## 2. Testimonial Smart Contract Specification

The testimonial smart contract binds user attestations to a cryptographic `SUBMISSION_ID` (32-byte SHA-256 digest of the root `SUBMISSION_ID` file).

### Data Structures

```rust
// Testimonial instruction calldata
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum TestimonialInstruction {
    SubmitTestimonial {
        submission_id: [u8; 32],
        text: String,
        username: Option<String>,
    },
}

// On-chain state record
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct TestimonialRecord {
    pub submission_id: [u8; 32],
    pub submitter: [u8; 32],
    pub text: String,
    pub username: Option<String>,
    pub timestamp: u64,
}
```

### Protocol Invariants & Validations

1. **Text Constraints:** Testimonial body must not be empty or whitespace-only, and cannot exceed 280 UTF-8 bytes.
2. **Moniker Constraints:** Optional username/handle must not exceed 64 ASCII alphanumeric characters (`[a-zA-Z0-9_.-]`).
3. **Submitter Deduplication:** Only one testimonial per `(submitter, submission_id)` tuple is accepted. Duplicate attempts are rejected with `TestimonialError::AlreadySubmitted`.
4. **Deterministic PDA Derivation:**
   ```rust
   let pda_seed = derive_testimonial_pda_seed(&submitter, &submission_id);
   // SHA-256("testimonial" || submitter || submission_id)
   ```

## 3. Compilation & Guest ELF Generation

Smart contracts for LEZ run as guest actors inside the RISC Zero zkVM (`riscv32im-risc0-zkvm-elf`).

### Compiling for RISC-V zkVM

```bash
# Using Just task runner:
just build-program

# Or using the native RISC0 toolchain directly:
RUSTFLAGS='--cfg getrandom_backend="custom"' cargo +risc0 build --release \
    --manifest-path programs/testimonial-program/Cargo.toml \
    --target riscv32im-risc0-zkvm-elf
```

The optimized ELF executable is generated at:
`target/riscv32im-risc0-zkvm-elf/release/testimonial` (298 KB).

### Development & Simulation Mode

When testing in the standalone local sequencer, the sequencer operates under `RISC0_DEV_MODE=1`. In this mode, mock ELF binaries or program bytecodes can be deployed and executed in milliseconds without generating full STARK proofs.

## 4. Deployment Lifecycle

### Environment 1: Standalone Sequencer (Local Dev-Env)

The standalone sequencer provides an authentic, isolated LEZ environment pre-seeded with Genesis Alice (10T units) and Bob (20T units):

```bash
# 1. Start local sequencer in background
just dev-start
# (or: docker compose up -d)

# 2. Verify health probe
just dev-health
# Expected: 🟢 Sequencer is HEALTHY (checkHealth returned result)

# 3. Deploy compiled program binary to standalone sequencer
just deploy-program <path-to-bin> standalone
# (or: cargo run -p lez-dispatcher -- deploy --bin <path-to-bin> --network standalone)
```

**What Happens Under the Hood:**
1. `lez-dispatcher` initializes `WalletCore` pointing to `http://127.0.0.1:3040`.
2. The binary is divided into chunks conforming to `MAX_SEGMENT_DATA_LEN`.
3. Segment accounts and a program header account (`Public/...`) are allocated.
4. `ProgramLoader::deploy` writes each segment to sequencer state and finalizes the header.
5. `deployments.json` is updated with the resulting `Public/<PROGRAM_ID>`.

### Environment 2: Logos Public Testnet (`https://testnet.lez.logos.co`)

Deploying to the public testnet requires funding the deployer keystore account with testnet gas tokens:

```bash
# Deploy to public testnet
cargo run -p lez-dispatcher -- deploy --bin <path-to-bin> --network testnet
```

The resulting program header is committed to the `"testnet"` object in `deployments.json`.

> [!TIP]
> For the complete verifiable on-chain record (including exact transaction hashes, sequencer block inclusion numbers `43457`–`43466`, block `43565` testimonial, and raw terminal receipts), see [Testnet Deployment Evidence](testnet-deployment-evidence.md).

## 5. Dispatching Transactions & Submitting Testimonials

### Submitting a Testimonial

```bash
# Using Just task runner
just submit-testimonial "Evice Wallet SDK provides instant, private asset transfers!" "alice"

# Or using lez-dispatcher directly
cargo run -p lez-dispatcher -- submit-testimonial \
    --text "Evice Wallet SDK provides instant, private asset transfers!" \
    --username "alice"
```

The dispatcher automatically:
- Resolves the active program header from `deployments.json`.
- Reads `SUBMISSION_ID` from the repository root.
- Serializes `TestimonialInstruction::SubmitTestimonial` using Borsh.
- Broadcasts the transaction paid by the local wallet keystore.

### Checking Deployment Status

```bash
just status
# or: cargo run -p lez-dispatcher -- status
```

Output:
```text
╔══════════════════════════════════════════════════════════╗
║                 LEZ PROGRAM DEPLOYMENTS                  ║
╠══════════════════════════════════════════════════════════╣
║ Active Network:   standalone                             ║
║ Sequencer RPC:    http://127.0.0.1:3040                  ║
║ Program Header:   Public/6aa4c365699b89d963ef85a59868b961║
╚══════════════════════════════════════════════════════════╝
```

### Low-Level Raw Instruction Dispatch

For contract testing and arbitrary instruction calldata:

```bash
cargo run -p lez-dispatcher -- raw \
    --program Public/<PROGRAM_ID> \
    --accounts Public/<ACC1> Public/<ACC2> \
    --data-hex "002a000000..."
```

## 6. Adoption Evidence Auditing (`evidence-tool`)

The `evidence-tool` verifies the milestone adoption criteria by querying on-chain testimonial records attributed to `SUBMISSION_ID`:

```bash
just audit-evidence
# or: cargo run -p evidence-tool
```

The audit tool inspects:
- **Total Valid Testimonials:** Verifies records with non-empty text and valid `SUBMISSION_ID`.
- **Distinct Submitter Accounts:** Confirms at least 150 unique account addresses.
- **Temporal Dispersion:** Ensures submissions occurred over required operational windows.

## 7. Dynamic Integration with SDK Engine

The SDK Engine is designed to seamlessly interoperate with `deployments.json`:

1. **`BuiltinVerifier` ([`crates/sdk-engine/src/verifier.rs`](../crates/sdk-engine/src/verifier.rs)):**
   - Automatically recognizes `NATIVE_TOKEN_PROGRAM_ID` and `PROGRAM_LOADER_ACCOUNT_ID`.
   - Dynamically loads and verifies active program headers listed in `deployments.json`.
2. **`TestimonialDecoder` ([`crates/sdk-engine/src/decode/testimonial.rs`](../crates/sdk-engine/src/decode/testimonial.rs)):**
   - Inspects outgoing transactions to the deployed testimonial program.
   - Automatically decodes Borsh calldata into readable `DecodedInstruction` fields in the informed consent approval modal.
