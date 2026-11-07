## System Architecture & Technical Specification

> **Evice Wallet & Provider SDK for Logos Execution Zone**  
> Modular, headless Wallet Engine and dApp Provider SDK for the Logos Execution Zone (LEZ).  
> Designed to provide a high-security, ergonomic wallet infrastructure and provider engine for the Logos Execution Zone.

## 1. Architectural Philosophy & Design Principles

The Evice Wallet SDK is engineered around five fundamental architectural tenets:

1. **Headless Core Separation:** The core business logic, session security, transaction pipeline, and cryptographic operations are strictly isolated in a pure Rust crate (`sdk-engine`) with zero graphical (Qt/QML/web) or platform-specific UI dependencies.
2. **Explicit User Consent & Non-Blind Signing:** External applications (dApps) are untrusted by default. No transaction can be signed or submitted, and no private balance can be inspected, without explicit, account-scoped authorization and human-readable effect simulation.
3. **Time-of-Check to Time-of-Use (TOCTOU) Defense:** Every transaction proposal is bound to an immutable intent hash at creation. Modifying parameters between user review, simulation, and on-chain submission invalidates the request immediately.
4. **Crash-Safe Atomic Durability:** Local state storage employs atomic write-then-rename primitives and directory fsyncs, backed by snapshot redundancy, guaranteeing zero state corruption during sudden application termination or power loss.
5. **No Mandatory External Services:** All core operations execute purely client-side or communicate directly with the user's designated LEZ sequencer node. No analytics, tracking telemetry, or third-party cloud intermediaries are utilized.

## 2. Workspace & Crate Decomposition

The workspace is structured into specialized, loosely coupled crates under `crates/`, with root-level container and task automation:

```
lez-wallet-sdk/
├── Cargo.toml                  # Workspace root manifest and unified dependency pins
├── SUBMISSION_ID               # Unique submission identifier for on-chain attribution
├── README.md                   # Quickstart, installation, and build documentation
├── Dockerfile                  # Multi-stage container build for standalone LEZ sequencer
├── docker-compose.yml          # Container orchestration runner for local dev-env
├── Justfile                    # Declarative workspace & dev-env task automation runner
├── crates/
│   ├── sdk-engine/             # Core headless wallet engine (pure Rust)
│   ├── sdk-ffi/                # C-ABI export layer for Basecamp Qt integration
│   ├── cli/                    # Headless CLI binary (`evice-wallet`)
│   └── evidence-tool/          # On-chain testimonial adoption audit utility
├── programs/
│   └── testimonial-program/    # On-chain testimonial smart contract (RISC-V zkVM guest)
├── tools/
│   └── dispatcher/             # Program deployment and on-chain dispatch CLI (`lez-dispatcher`)
├── deployments.json            # On-chain program deployment registry (testnet 0.4 & dev-env)
├── dev-env/                    # Standalone sequencer configuration and genesis fixtures
│   ├── genesis_accounts.json   # Pre-funded Alice, Bob, and shielded circuit accounts
│   ├── sequencer_config.json   # Standalone genesis parameters and mock channel config
│   ├── wallet_config.json      # SDK & CLI RPC connection settings (http://127.0.0.1:3040)
│   └── README.md               # Standalone development environment guide
├── integration-tests/          # End-to-end integration test suites
├── docs/                       # Specifications, architecture, and documentation
│   ├── ARCHITECTURE.md         # This comprehensive architecture document
│   ├── sdk-api-reference.md    # Complete SDK API reference & JSON wire format
│   └── program-deployment.md   # Smart contract compilation, deployment & dispatcher guide
└── .github/workflows/          # Continuous Integration for Linux and macOS runners
```

### Component Responsibility Matrix

| Component | Responsibility | Public Interface | Key Dependencies |
|---|---|---|---|
| **`sdk-engine`** | Central orchestrator, session/grant store, transaction pipeline, decoders, effect simulations, LNS resolver, identity accountability (HKDF, BIP-340 Schnorr, Two-Tier SSS), EmPoWer PoW mining & L1-to-LEZ channel bridge (`pow.rs`), atomic persistence, and token facade. | `DAppApi`, `OwnerApi`, `SdkEngine`, `PowBridgeManager`, `ApprovalHandler` | `wallet`, `lee`, `common`, `tokio`, `argon2`, `sha2`, `k256`, `sharks` |
| **`sdk-ffi`** | Exposes C-ABI (`extern "C"`) functions with opaque handles, JSON serialization, and non-blocking approval callback bridge for Logos Basecamp desktop plugins (`wallet-provider`). | `lez_wallet_engine_new()`, `lez_wallet_engine_free()`, `lez_wallet_set_approval_callback()`, `OwnerApi`, `DAppApi`, Identity Accountability, EmPoWer PoW & Channel Bridge, and `include/lez_wallet_sdk.h` | `sdk-engine`, `serde_json`, `cbindgen`, `dashmap`, `tokio` |
| **`cli`** | Interactive command-line binary (`evice-wallet`) for headless operations, scripting, local testing, and offline address resolution. | Command-line interface via `clap` | `sdk-engine`, `tokio`, `clap`, `env_logger` |
| **`testimonial-program`** | On-chain RISC-V zkVM smart contract storing testimonials and binding `SUBMISSION_ID` to submitter PDA records. | Program header ID, instruction calldata ABI | `lee`, `lee_core`, `common`, `serde`, `borsh` |
| **`tools/dispatcher`** | Headless CLI tool (`lez-dispatcher`) for uploading program bytecode via `ProgramLoader`, sending on-chain transactions, and syncing `deployments.json`. | Command-line interface via `clap` | `wallet`, `program_loader_core`, `testimonial-program`, `serde_json` |
| **`evidence-tool`** | Scans on-chain testimonial records from sequencer, filtering by `submission_id` to verify distinct accounts and temporal dispersion. | Command-line audit utility | `wallet`, `common`, `clap`, `tokio` |

## 3. System Architecture Diagram

```
┌────────────────────────────────────────────────────────────────────────┐
│                          CONSUMER LAYER                                │
│                                                                        │
│   ┌──────────────────────────┐             ┌─────────────────────────┐ │
│   │   Third-Party LEZ dApps  │             │   Logos Basecamp App    │ │
│   │   (Basecamp UI Modules)  │             │   (Desktop QML UI)      │ │
│   └────────────┬─────────────┘             └────────────┬────────────┘ │
│                │                                        │              │
│                │ (IPC / Plugin Interface)               │              │
│                ▼                                        ▼              │
│   ┌──────────────────────────┐             ┌─────────────────────────┐ │
│   │   `wallet-provider`      │             │   Terminal User         │ │
│   │   (Basecamp Core Module) │             │   (CLI `evice-wallet`)  │ │
│   └────────────┬─────────────┘             └────────────┬────────────┘ │
└────────────────┼────────────────────────────────────────┼──────────────┘
                 │                                        │
                 ▼ (C-ABI Boundary)                       ▼
┌────────────────────────────────────────────────────────────────────────┐
│                          INTEROP LAYER                                 │
│                                                                        │
│   `sdk-ffi`                                                            │
│   • Opaque Pointer Handles (`*mut SdkEngineHandle`)                    │
│   • ABI-Safe JSON Serialization                                        │
│   • C String Error Buffering & Status Codes                            │
└─────────────────────────────────┬──────────────────────────────────────┘
                                  │
                                  ▼
┌────────────────────────────────────────────────────────────────────────┐
│                        CORE ENGINE LAYER                               │
│                                                                        │
│   `sdk-engine`                                                         │
│   ┌────────────────────────────────────────────────────────────────┐   │
│   │                     SdkEngine Orchestrator                     │   │
│   │         ┌──────────────────────┬──────────────────────┐        │   │
│   │         │       DAppApi        │       OwnerApi       │        │   │
│   │         └──────────────────────┴──────────────────────┘        │   │
│   └──────┬──────────┬──────────┬──────────┬──────────┬──────────┬──┘   │
│          │          │          │          │          │          │      │
│          ▼          ▼          ▼          ▼          ▼          ▼      │
│   ┌───────────────┐ ┌───────────────┐ ┌───────────────┐ ┌────────────┐ │
│   │ Session       │ │ Request Queue │ │ Decoders &    │ │ LNS Name   │ │
│   │ & Grants      │ │ & Intents     │ │ Simulations   │ │ Resolver   │ │
│   └───────────────┘ └───────────────┘ └───────────────┘ └────────────┘ │
│   ┌───────────────┐ ┌───────────────┐ ┌──────────────────────────────┐ │
│   │ Approval      │ │ Identity      │ │ Atomic Persistent Store      │ │
│   │ Handlers      │ │ Accountability│ │ (Write-then-Rename + .bak)   │ │
│   └───────────────┘ └───────────────┘ └──────────────────────────────┘ │
└─────────────────────────────────┬──────────────────────────────────────┘
                                  │
                                  ▼
┌────────────────────────────────────────────────────────────────────────┐
│                     UNDERLYING LEZ PLATFORM                            │
│                                                                        │
│   `logos-execution-zone` (`dev` branch)                                │
│   • `lez/wallet` (Key management, Argon2 storage, state persistence)   │
│   • `lee` (State machine, account model, cryptographic types)          │
│   • LEZ Sequencer RPC Service                                          │
└────────────────────────────────────────────────────────────────────────┘
```

## 4. Deep-Dive: Core Engine Subsystems (`sdk-engine`)

### 4.1 Orchestrator & Dual-API Architecture (`api.rs`)

The central engine is encapsulated by `SdkEngine`, maintaining an `Arc`-wrapped, thread-safe state container. It cleanly partitions operational capabilities into two distinct trait interfaces:

```rust
// Trait exposed to external, untrusted dApps (consumed via Session)
pub trait DAppApi {
    fn sdk_info(&self) -> Future<Output = SdkResult<SdkInfo>>;
    fn connect(&self, caller_identity: String, api_version: u32) -> Future<Output = SdkResult<SessionId>>;
    fn disconnect(&self, session: &SessionId) -> Future<Output = SdkResult<()>>;
    fn request_accounts(&self, session: &SessionId) -> Future<Output = SdkResult<Vec<AccountView>>>;
    fn list_accounts(&self, session: &SessionId) -> Future<Output = SdkResult<Vec<AccountView>>>;
    fn get_balance(&self, session: &SessionId, account: &AccountId) -> Future<Output = SdkResult<u128>>;
    fn propose_native_transfer(&self, session: &SessionId, from: AccountId, to: String, amount: u128, from_privacy: Option<AccountPrivacy>) -> Future<Output = SdkResult<RequestId>>;
    fn propose_token_transfer(&self, session: &SessionId, from_holding: AccountId, to_holding: AccountId, definition: AccountId, amount: u128) -> Future<Output = SdkResult<RequestId>>;
    fn propose_transaction(&self, session: &SessionId, mentions: Vec<AccountId>, calldata: Vec<u8>, description: Option<String>) -> Future<Output = SdkResult<RequestId>>;
    fn propose_contract_call(&self, session: &SessionId, program_id: AccountId, payer_id: AccountId, mentions: Vec<AccountId>, calldata: Vec<u8>, description: Option<String>) -> Future<Output = SdkResult<RequestId>>;
    fn list_token_holdings(&self, session: &SessionId) -> Future<Output = SdkResult<Vec<TokenHoldingView>>>;
    fn get_request_status(&self, request_id: &RequestId) -> Future<Output = SdkResult<RequestStatus>>;
    fn cancel_request(&self, request_id: &RequestId) -> Future<Output = SdkResult<()>>;
    fn resolve_name(&self, session: &SessionId, name: &str) -> Future<Output = SdkResult<ResolvedDestination>>;

    // Identity Accountability & Accountable Pseudonymity
    fn request_identity_access(&self, session: &SessionId, kind: GrantKind) -> Future<Output = SdkResult<()>>;
    fn get_identity_commitment(&self, session: &SessionId) -> Future<Output = SdkResult<IdentityCommitment>>;
    fn derive_scoped_nullifier(&self, session: &SessionId, scope: &str) -> Future<Output = SdkResult<ScopedNullifier>>;
    fn sign_identity_attestation(&self, session: &SessionId, scope: &str, message: &[u8]) -> Future<Output = SdkResult<AttestationPayload>>;
    fn prepare_identity_registration(&self, session: &SessionId, scope: &str, trustees: &[[u8; 32]], threshold_k: u8) -> Future<Output = SdkResult<RegistrationPayload>>;
}

// Privileged management trait reserved exclusively for the local wallet owner
pub trait OwnerApi {
    fn create_wallet(&self, password: &str) -> Future<Output = SdkResult<Mnemonic>>;
    fn restore_wallet(&self, mnemonic: &Mnemonic, password: &str) -> Future<Output = SdkResult<()>>;
    fn lock(&self) -> Future<Output = SdkResult<()>>;
    fn unlock(&self, password: &str) -> Future<Output = SdkResult<()>>;
    fn change_password(&self, old_password: &str, new_password: &str) -> Future<Output = SdkResult<()>>;
    fn create_account_public(&self, label: Option<String>) -> Future<Output = SdkResult<AccountId>>;
    fn create_account_private(&self, label: Option<String>) -> Future<Output = SdkResult<AccountId>>;
    fn list_all_accounts(&self) -> Future<Output = SdkResult<Vec<AccountView>>>;
    fn list_sessions(&self) -> Future<Output = SdkResult<Vec<Session>>>;
    fn revoke_session(&self, session_id: &SessionId) -> Future<Output = SdkResult<()>>;
    fn register_token_holding(&self, holding_id: AccountId, definition_id: AccountId, symbol: String, name: String, is_shielded: bool) -> Future<Output = SdkResult<()>>;
    fn list_all_token_holdings(&self) -> Future<Output = SdkResult<Vec<TokenHoldingView>>>;
    fn pow_start_mining(&self, threads: u32) -> Future<Output = SdkResult<PowStatusView>>;
    fn pow_stop_mining(&self) -> Future<Output = SdkResult<PowStatusView>>;
    fn pow_status(&self) -> Future<Output = SdkResult<PowStatusView>>;
    fn channel_deposit(&self, recipient_account: &str, amount_str: &str) -> Future<Output = SdkResult<ChannelDepositResult>>;
}
```

### 4.2 Granular Session & Permission Security (`session.rs`)

External dApps communicate with the SDK through isolated, ephemeral sessions:

- **Session Lifecycle:** A dApp calls `connect(module_id)`, which generates a cryptographic `SessionId` (UUID v4) and registers a `Session` with an initial duration of 1 hour (`DEFAULT_SESSION_DURATION`).
- **Account-Level Grants:** A session starts with zero grants. The dApp must explicitly call `request_accounts(...)` to request privileges per account:
  - `GrantKind::Read`: Grants permission to inspect the account's balance and public program state. A dApp *cannot* read even the existence or balance of a private/shielded account without an explicit `Read` grant.
  - `GrantKind::Propose`: Grants permission to assemble and submit transaction proposals for human approval.
  - `GrantKind::IdentityRead`: Grants permission to inspect the public identity commitment and derive scope-specific pseudonymous nullifiers.
  - `GrantKind::IdentityPropose`: Grants permission to sign scoped attestations or prepare threshold trustee registration payloads.
- **Automatic Expiry & Eviction:** The `SessionStore` checks timestamps against monotonic system time. Expired sessions are pruned and automatically rejected on subsequent calls.
- **Lock Invalidation:** When the wallet is locked via `OwnerApi::lock()`, all active sessions across all dApps are revoked immediately.

### 4.3 Request Immutability & Intent Hashing (`request.rs`)

To prevent Time-of-Check to Time-of-Use (TOCTOU) exploits—where a malicious payload might alter transaction parameters after user review but prior to cryptographic signing—the engine enforces an immutable request pipeline:

1. **Request Creation:** A propose call (e.g. `propose_native_transfer`) creates a `PendingRequest` with a unique `RequestId`.
2. **Intent Hash Calculation:** The engine immediately computes a deterministic 32-byte SHA-256 hash binding the request:
   ```
   IntentHash = SHA-256(
       "evice-wallet-sdk/intent/v1/" ‖
       RequestId ‖
       SessionId ‖
       Serialized(RequestKind)
   )
   ```
3. **Immutability Enforcement:** The `PendingRequest` is stored in `RequestStore`. At the moment of signing and submission, the intent hash is recomputed against the active payload. If any parameter, recipient, amount, or session context deviates by even a single bit, execution is aborted with `SdkError::IntentHashMismatch`.

### 4.4 Non-Blocking Asynchronous Approval Handlers (`approval.rs`)

The SDK completely decouples approval presentation from the transaction lifecycle via the `ApprovalHandler` trait:

```rust
pub trait ApprovalHandler: Send + Sync {
    fn prompt_approval(
        &self,
        prompt: &ApprovalPrompt,
    ) -> impl Future<Output = Result<ApprovalDecision, SdkError>> + Send;
}
```

- **Rich Approval Context:** Every `ApprovalPrompt` delivers structured metadata:
  - `request_id`: Unique request reference.
  - `module_id`: Authenticated origin identifier of the requesting dApp.
  - `summary`: Human-readable description of the operation.
  - `effects`: Pre-calculated balance changes and state mutations.
  - `program_id`: On-chain destination program.
  - `verification_status`: Bytecode source verification result.
- **Implementations:**
  - `CliApprovalHandler`: Renders a formatted terminal modal and reads interactive user input (`[y/N]`).
  - `UiApprovalHandler`: Bridges prompts asynchronously to Logos Basecamp QML desktop dialogs.
  - `PolicyApprovalHandler`: Auto-approval policy engine feature-gated behind `#[cfg(feature = "dev-auto-approve")]`. Dedicated CI audit jobs inspect release binaries to guarantee that auto-approval code is physically absent from release builds.

### 4.5 Instruction Decoders & Effect Simulation (`decode/`, `effects.rs`)

Blind signing of raw byte payloads is strictly prohibited:

- **`DecoderRegistry`:** A modular calldata dispatcher inspecting program IDs and decoding raw byte sequences into structured `DecodedInstruction` records:
  - `NativeDecoder`: Decodes native token transfers (source, destination, amount).
  - `TokenDecoder`: Decodes fungible token program operations (definition creation, minting, burning, transfer).
  - `AtaDecoder`: Decodes Associated Token Account creation and initialization.
  - `TestimonialDecoder`: Decodes testimonial program submissions (`submission_id`, user moniker, testimonial text).
- **`EffectSimulator`:** Prior to presenting the approval modal, the simulator projects transaction consequences, producing:
  - `BalanceDiff`: Delta calculations per account (`before`, `after`, `delta`).
  - `EffectSource`: Provenance tracking (`Simulation`, `DryRun`, or `StaticAnalysis`).
  - Insufficient fund pre-checks: Aborts with clear errors if sender balance cannot satisfy transfer amounts and gas bounds.

### 4.6 Program Source Verification (`verifier.rs`)

Aligning with reproducible smart contract source verification standards, the approval pipeline checks the authenticity of target smart contracts:

- **`VerificationStatus`:**
  - `Verified { compiler, repo }`: The on-chain bytecode corresponds reproducibly to the source repository at the audited commit.
  - `NotVerified`: The program is unverified or unregistered.
  - `NotChecked`: Registry connection unavailable or verification disabled.
- **`BuiltinVerifier`:** Ships with built-in signatures for canonical testnet programs (Native Token, Wrapped Token, Bridge, Sequencer Stake) to protect users during initial bootstrapping.

### 4.7 Clean-Room Logos Name Service (LNS) Resolver (`resolve/lns.rs`)

The SDK incorporates a complete, independent, clean-room implementation of the Logos Name Service ([`SPEC.md`](https://github.com/jefdiesel/logos-names/blob/main/SPEC.md)) for human-readable `.lez` names, performing live on-chain lookups and authenticated AES-256-GCM profile decryption:

```
Input Name: "alice.lez"
      │
      ▼ (Phase 1: Normalisation — SPEC §2)
Trim whitespace, lowercase ASCII, strip ".lez": [a-z0-9-]{1,32}
      │
      ▼ (Phase 2: Memory-Hard Key Derivation — SPEC §3)
Argon2id(password="alice", salt="logos-names/v5", mem=64MiB, passes=3, lanes=1)
      │
      ▼
Master Secret [u8; 32]
      ├──► Name ID  = SHA-256("logos-names/v5/id"  ‖ Master)   (Public identifier)
      └──► Decr Key = SHA-256("logos-names/v5/key" ‖ Master)   (Private decryption key)
      │
      ▼ (Phase 3: On-Chain PDA Derivation — SPEC §4)
PDA Seed   = SHA-256("logos-names/v5/name/" ‖ Name ID)
Record PDA = SHA-256(pad32("/LEE/v0.2/AccountId/PDA/") ‖ Registry_Program_ID ‖ PDA Seed)
      │
      ▼ (Phase 4: Resolution & Verification — SPEC §5, §6)
Check In-Memory Cache (TTL = 300s) ──► Cache Hit: Return cached ResolvedDestination
      │ (Cache Miss)
Query Sequencer RPC `getAccount(Record PDA)`
      │
      ├──► Account / Shard Absent ──► Return ResolveError::NameNotFound
      │
      ▼ (Extract Registry Shard)
Decode NameRecord (Borsh) ──► Verify Expiry (Epoch ms; Grace Period = 2,592,000,000 ms)
      │
      ▼ (Decrypt Sealed Profile)
AES-256-GCM Decrypt(Decr Key, Nonce=12B, Tag=16B, AAD=b"logos-names/v5/record")
      │
      ▼ (Decode Plaintext Profile)
Decode Profile (Borsh, zero-padding tolerant) ──► Verify profile.name == input name
      │
      ▼
Cache in DashMap & Return ResolvedDestination { name, account, npk, vpk, expires, texts }
```

- **Cache Invalidation:** The in-memory cache maintains a 5-minute TTL (`CACHE_TTL`). When the wallet is locked, the cache is completely purged to ensure zero address retention in memory.
- **Pass-Through Fallback:** If an input string is not a `.lez` domain, the resolver transparently treats it as a raw 32-byte account address.
- **Pre-Flight Transfer Guard:** `propose_native_transfer` proactively resolves `.lez` destinations; if the name is unregistered on-chain or lacks a public receiving account, it fails early with `SdkError::NameResolutionError` before transaction proposal creation.

### 4.8 Crash-Safe Atomic Persistence (`store.rs`)

State durability follows strict POSIX crash-resilience standards. In addition to system toggles, `SdkPersistentState` persists custom human-readable aliases (`account_labels: HashMap<String, String>`) so that account renaming takes immediate effect and remains durable across daemon/application restarts:

```
[In-Memory SdkPersistentState]
               │
               ▼ (Step 1: Serialize)
        JSON Byte Stream
               │
               ▼ (Step 2: Atomic Write)
         Write to `.tmp` file
               │
               ▼ (Step 3: Flush to Disk)
            `file.sync_all()`
               │
               ▼ (Step 4: Atomic Rename)
  `std::fs::rename(.tmp -> target)` (POSIX atomic swap)
               │
               ▼ (Step 5: Directory Fsync)
  `fs::File::open(parent_dir).sync_all()` (Persist directory entry)
```

- **Backup Redundancy (`.bak`):** Upon each successful write, a mirror backup `<path>.bak` is maintained. If the primary file is corrupted, truncated, or unreadable upon startup, the store automatically attempts recovery from `.bak`.

### 4.9 Identity Accountability & Accountable Pseudonymity (`identity/`)

Adapted from the Evice Labs [`e-identity-stack`](https://github.com/evice-labs/e-identity-stack) research repository, this subsystem implements zero-knowledge accountable pseudonymity directly within `sdk-engine`:

```
BIP-39 Root Seed (Wallet Mnemonic)
        │
        ▼ (HKDF-SHA256: salt="lez/identity/v1/secret", info="IdentityMasterKey")
IdentityMasterSecret (IdentitySecret, 32 bytes)
        │
        ├─────────────────────────────────────────────────┐
        ▼                                                 ▼
Identity Commitment (Public)                     Scope-Specific Derivation
SHA-256("lez/identity/v1/commitment" ‖ Secret)    (Cross-dApp Unlinkable)
        │                                                 │
        ▼                                                 ▼
On-Chain Membership / Registry                   Scoped Nullifier (Governance / Vote / Chamber)
IdentityCommitment                               SHA-256("lez/identity/v1/scoped_nullifier" ‖ Secret ‖ ScopeId)
                                                          │
                                                          ▼
                                             Two-Tier Shamir Secret Sharing
                                             P(x) over GF(2^8), deg = K - 1
                                             P(0) = IdentitySecret
                                             M Trustee shares encrypted via secp256k1 ECDH
```

1. **Deterministic Master Derivation (`derivation.rs`):**
   - Extracts the 32-byte `IdentitySecret` from the BIP-39 root seed phrase via HKDF-SHA256, eliminating the need for users to manage or back up separate identity key files.
   - Derives the public 32-byte `IdentityCommitment` via `SHA-256("lez/identity/v1/commitment" ‖ IdentitySecret)`.
2. **Scope-Specific Unlinkable Nullifiers (`derivation.rs`):**
   - Produces stable pseudonyms within a given scope (e.g., a specific DAO governance chamber or chat room) using `SHA-256("lez/identity/v1/scoped_nullifier" ‖ IdentitySecret ‖ ScopeId)`.
   - Across different `ScopeId`s, the generated nullifiers are mathematically uncorrelated, preventing third-party tracking across dApps.
3. **Two-Tier Shamir Secret Sharing over GF($2^8$) (`shamir.rs`):**
   - Standardized via the battle-tested `sharks` crate over GF($2^8$), maintaining byte-for-byte wire compatibility with accountability trustee nodes.
   - Splits the 32-byte `IdentitySecret` into $M$ shares with threshold $K$.
   - Any $K$ shares successfully reconstruct the master secret via Lagrange polynomial interpolation at $x = 0$, while fewer than $K$ shares reveal zero information.
4. **secp256k1 ECDH & Stream Encryption (`ecdh.rs`):**
   - Computes shared secrets with designated accountability trustee or guardian x-only public keys.
   - Encrypts individual threshold shares using a SHA-256 stream keystream for distribution in `RegistrationPayload`.
5. **BIP-340 Schnorr Attestations (`signature.rs`):**
   - Derives a deterministic scalar attestation key per scope.
   - Signs arbitrary messages (e.g., anonymous votes, reputation claims) verified against the scope's public key.
6. **Granular Permission Integration (`session.rs`, `api.rs`):**
   - Governed by `GrantKind::IdentityRead` (inspect commitment and derive scoped nullifier) and `GrantKind::IdentityPropose` (sign attestation or construct registration payload).
   - Informs users with clear consent disclosures prior to identity generation.

### 4.10 C-ABI FFI & Host Interop Layer (`crates/sdk-ffi`, `include/lez_wallet_sdk.h`)

To enable seamless, language-agnostic integration with Logos Basecamp desktop plugins (`wallet-provider`) and third-party native applications without compromising Rust's memory safety guarantees, the `sdk-ffi` crate exposes a pure C-ABI surface:

1. **Opaque Engine Handle (`FfiSdkEngine`):**
   - Encapsulates the thread-safe `Arc<SdkEngine>`, an internal multi-threaded Tokio runtime, and the approval router.
   - Pointers (`*mut FfiSdkEngine`) are strictly managed and validated with null-pointer checks before dereferencing.
2. **Panic Safety & Wire Serialization:**
   - All C-callable entrypoints wrap execution in `std::panic::catch_unwind(AssertUnwindSafe(...))` to guarantee panics never cross the C-ABI boundary.
   - Structured data payloads are serialized as UTF-8 JSON C strings (`*mut c_char`) with a unified response envelope:
     - Success: `{"ok": true, ...}`
     - Failure: `{"ok": false, "error": "Human-readable error description"}`
3. **Approval Callback & Deferred Decision Bridge:**
   - Registers a non-blocking C callback `FfiApprovalCallback(prompt_json, user_data)`.
   - Supports both synchronous immediate decisions (`1` = Approve, `0` = Reject) and asynchronous deferred decisions (`-1` = Deferred).
   - In deferred mode, the host UI (e.g. Qt/QML modal) can display detailed diffs and later submit the decision asynchronously via `lez_wallet_submit_approval_decision(engine, request_id, decision)`.
4. **Deterministic Memory Cleanup:**
   - All string buffers allocated by the Rust runtime must be released by the caller via `lez_wallet_free_string(ptr)`.
5. **C/C++ Header Generation (`cbindgen`):**
   - Header `include/lez_wallet_sdk.h` is automatically generated from Rust docstrings and function signatures, featuring `#ifdef __cplusplus` guards and standard `#pragma once` header protection.

### 4.11 Command-Line Interface (`crates/cli`, `evice-wallet`)

The `evice-wallet` CLI provides a standalone, production-grade terminal interface interacting directly with the headless `SdkEngine` without external daemon dependencies:

1. **Architecture & Runtime Isolation:**
   - Structured with `clap` v4 using declarative derive syntax and asynchronous Tokio multi-threaded execution.
   - Decoupled from host UI frameworks; all operations interface through the standard `OwnerApi` and `DAppApi` contracts.
2. **Terminal Security & Password Handling (Zero-Echo):**
   - Integrates `rpassword` (`prompt_password`) for non-echoing terminal inputs, preventing shoulder surfing and terminal scrollback leaks (CWE-522).
   - Mandatory double-prompt confirmation (`Confirm wallet password: `) during `wallet create` and `wallet restore` to eliminate typographical lockout vulnerabilities.
   - The `--password` CLI argument is explicitly documented with security warnings advising that command-line arguments are visible in `/proc/<pid>/cmdline` and shell history.
3. **Storage Guards & Overwrite Protection:**
   - `ensure_wallet_exists` validator prevents false successes on uninitialized directories.
   - `wallet create` and `wallet restore` check existing storage, requiring an explicit `--force` flag before allowing re-initialization.
   - `create_wallet` invokes immediate `store_persistent_data` to ensure file descriptors and cryptographic seeds are committed to disk upon genesis.
4. **Subcommand Dispatch Matrix:**
   - **`wallet`**: `create [--force]`, `open`, `restore <mnemonic> [--force]`.
   - **`account`**: `create-public`, `create-private` (shielded circuit), `list` (inventory table), `balance <account>`.
   - **`transfer`**: `public`, `shield`, `unshield`, `private`. Enforces non-zero amount checks, self-transfer guards, and privacy type compatibility. Accepts transparent `.lez` LNS destinations.
   - **`token`**: `create-definition`, `initialize-holding`, `transfer`, `mint`, `burn`, `balance`.
   - **`resolve <name.lez>`**: Standalone [SPEC.md](https://github.com/jefdiesel/logos-names/blob/main/SPEC.md) resolver displaying normalised names, Name IDs, PDA seeds, and live sequencer lookup.
   - **`identity`**: `commitment` (master public commitment), `nullifier --scope <scope>` (unlinkable scoped pseudonym), `attest --scope <scope> --message <msg>` (BIP-340 Schnorr signature).
   - **`info`**: Diagnostic environment and wallet status banner.
5. **Human-Centric UX:**
   - Mnemonic recovery phrases rendered in a 3-column numbered grid (`1. word   2. word   3. word`), preventing single-line overflow across 12- and 24-word phrases.
   - Formatted ASCII borders for transaction proposal statuses and account tables.

### 4.12 Standalone Development Environment (`dev-env`, Docker, Justfile)

To facilitate rapid, reliable local development and continuous integration without dependency on live external testnets or public faucet availability, the workspace includes a self-contained local development environment:

1. **Standalone Architecture & Mocked L1:**
   - Runs an authentic LEZ `sequencer_service` in `--features standalone` mode.
   - Embeds an in-memory mocked Bedrock L1 consensus layer with instant block commitment and mock channel anchoring (`channel_id: 0101...01`).
   - Operates under `RISC0_DEV_MODE=1`, enabling transaction validation, state commitments, and mock proving to execute in milliseconds without zkVM STARK proving latency.
2. **Clean Separation of Concerns:**
   - **Repository Root:** Centralizes operational orchestration manifests:
     - `Dockerfile`: Multi-stage build compiling `sequencer_service` from the pinned upstream LEZ `dev` branch.
     - `docker-compose.yml`: Root service configuration exposing JSON-RPC on port `3040:3040` and managing volume mounts.
     - `Justfile`: Task runner standardizing dev-env commands (`just dev-start`, `dev-stop`, `dev-health`, `dev-fund`).
   - **`dev-env/` Directory:** Strictly stores declarative configuration templates and fixtures:
     - `genesis_accounts.json`: Human- and machine-readable specification of pre-funded genesis accounts.
     - `sequencer_config.json`: Genesis block actions, channel parameters, and gossip listener bindings.
     - `wallet_config.json`: Client-side SDK and CLI connection configuration pointing to `http://127.0.0.1:3040`.
3. **Pre-Funded Genesis Accounts:**
   - **Alice (Dispenser):** `Public/6aa4c365699b89d963ef85a59868b9612b6001f90750b4970662a62776332e81` (10,000,000,000,000 units), private key `7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb`.
   - **Bob (Counterparty):** `Public/891ae258f92ca8447af6cf09c41d96b3a06187d098b91ab2cc17b42e3c4a2fa1` (20,000,000,000,000 units), private key `f434f8741720014586ae43356d2aec6257da086222f604ddb75d69733b86fc4c`.
   - **Shielded Accounts:** Pre-seeded circuit pools `44KVvzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKwmhm` and `478hD6LXFTYu5gRRkWkCyXPRsWgCwAfRredoCUfxr5b9`.
4. **Declarative Task Automation (`Justfile`):**
   - `just dev-start`: Boots the standalone sequencer in the background via Docker Compose.
   - `just dev-stop`: Gracefully shuts down the sequencer container.
   - `just dev-health`: Executes an automated JSON-RPC probe (`checkHealth`) returning verified status.
   - `just dev-logs`: Streams real-time container log output.
   - `just dev-fund <recipient> [amount]`: Dispenses native tokens from Alice to any address or `.lez` name.
   - `just dev-clean`: Resets sequencer storage for a clean genesis state.
   - `just dev-native-start`: Native Cargo fallback for developer workstations or CI environments without Docker.

### 4.13 On-Chain Smart Contract, Program Deployment & Dispatcher (`programs/testimonial-program`, `tools/dispatcher`, `deployments.json`)

To fulfill on-chain adoption verification, verifiable attribution, and automated program deployment requirements without external manual dependencies, the SDK ecosystem incorporates native smart contract artifacts, an on-chain dispatcher tool, and a dynamic deployment registry:

1. **`programs/testimonial-program` (zkVM Smart Contract):**
   - **Cryptographic Attribution Binding:** Directly embeds `SUBMISSION_ID` (32-byte cryptographic identifier) into each on-chain testimonial record.
   - **Protocol Invariant Validations:** Enforces strict constraints on message body ($\le 280$ bytes, non-empty) and optional author moniker ($\le 64$ alphanumeric characters).
   - **Submitter Deduplication:** Rejects duplicate submissions from the same account for a given `submission_id` (`(submitter, submission_id)` uniqueness), preventing sybil gaming of adoption metrics.
   - **Deterministic PDA Derivation:** Derives deterministic seeds via `SHA-256("testimonial" || submitter || submission_id)`.
   - **LEZ Actor Model:** Implements standard LEZ `ReceiveInput` and `Response` interfaces, serialising and deserialising account state through Borsh.
2. **`tools/dispatcher` (`lez-dispatcher` CLI Utility):**
   - Headless deployment and transaction dispatch CLI built on top of `wallet::WalletCore` and `ProgramLoader`.
   - **`deploy`**: Reads a compiled program ELF binary, partitions it into chunks adhering to `MAX_SEGMENT_DATA_LEN`, uploads segments via `ProgramLoader`, commits the program header account, and automatically records the new header address in `deployments.json`.
   - **`submit-testimonial`**: Assembles a `TestimonialInstruction::SubmitTestimonial` instruction payload, resolves the target program header from `deployments.json`, derives required actor states and mentions, and broadcasts the public transaction paid by the wallet keystore.
   - **`status`**: Displays an ASCII summary of active networks and deployed program header addresses.
   - **`raw`**: Dispatches arbitrary program calldata and account mention arrays for low-level debugging.
3. **`deployments.json` Dynamic Registry:**
   - Single source of truth for deployed program headers across environments (`standalone` on `http://127.0.0.1:3040` and `testnet` on `https://testnet.lez.logos.co`).
   - Dynamically consumed by `TestimonialDecoder` and `BuiltinVerifier` in `sdk-engine`, ensuring that once a program is deployed, the wallet automatically decodes its calldata and verifies its program ID without hardcoding.
4. **Adoption Evidence Auditing (`crates/evidence-tool`):**
   - Command-line audit utility scanning on-chain sequencer state, querying testimonial records matching `SUBMISSION_ID`, and verifying distinct submitter accounts and temporal dispersion criteria.

### 4.14 Resilient Sequencer Client & RPC Compatibility (`client.rs`)

To ensure seamless, resilient interoperation between modern SDK engine traits and live LEZ testnet sequencer deployment revisions without requiring fork modifications to the upstream `logos-execution-zone` codebase, `sdk-engine` implements `TestnetSequencerClient`.

1. **Idiomatic Extension via `WalletSequencerClient`:**
   - Wraps `sequencer_service_rpc::SequencerClient` and implements `wallet::WalletSequencerClient`.
   - `WalletCore` is generic over `C: WalletSequencerClient`, allowing `SdkEngine` to inject this adaptor via `WalletCore::new_update_chain_with_client` and `WalletCore::new_init_storage_with_client`.
2. **Backward-Compatible RPC Payloads:**
   - **Account Serialization (`shards` vs `actor_states`):** Sequencer testnets returning account payloads with the `shards` field are normalized dynamically to `actor_states` at the JSON level before passing to `Account` deserialization, avoiding `unknown field 'shards', expected 'actor_states'` deserialization rejections.
   - **Block & Block Range Wire Adaptation (`getBlock` & `getBlockRange`):** Sequencers speaking the LEZ v0.3 protocol (`V03ShardModel`, such as `https://testnet.lez.logos.co`) return base64-encoded v0.3 `Block` Borsh bytes that do not contain the modern v0.4 `reverted: Vec<u64>` field or Actor-model transactions. `TestnetSequencerClient` intercepts block queries and decodes them via `v03_compat::decode_v03_block`, extracting canonical header parameters (`block_id`, `prev_block_hash`, `hash`, `timestamp`, `bedrock_status`) and mapping them into the wallet poller structures without length mismatch crashes (`Unexpected length of input at line 1 column 1484`).
   - **Missing Method Resilience (`getPublications` & `getRecoveryBinding`):** Gracefully returns empty collections / `None` on v0.3 sequencers where these modern Actor-model RPC methods do not exist (HTTP error `-32601 Method not found`).
   - **Proof Query Tuple Compatibility (`getProofsAndRoot`):** Adapts both legacy 2-tuple `(proofs, root)` and 3-tuple `(proofs, message_paths, root)` return signatures from sequencer RPC calls into canonical proof structures.
3. **Pure Upstream Commit Pinning:**
   - All external LEZ dependencies in `Cargo.toml` are pinned to explicit Git commits (`rev = "5562eeeb200d67c51ba58976ba7b0bb63c19a142"`) with zero local directory patches or workspace overrides.

### 4.15 Protocol State Separation & Invariant Guardrails (`docs/protocol-compatibility-and-invariants.md`)

To ensure safe operation across different LEZ network iterations and protect user funds from irreversible protocol-level traps:
1. **Cryptographic Identity Asymmetry:**
   - Public accounts are derived from ECDSA/Schnorr public keys and authenticate via cryptographic signatures.
   - Private accounts are derived from post-quantum ML-KEM-768 viewing keys and BabyJubjub/Poseidon nullifier keys, authenticated exclusively through Zero-Knowledge SNARK proofs.
   - Private accounts possess no ECDSA signing keys, rendering any transparent balance on their public shard permanently unspendable and unshieldable (*Asymmetric Deposit Trap*).
2. **Shielding & 4-Way Private Proof Execution:**
   - Public-to-private shielding (`Public ➝ Private`), private-to-private (`Private ➝ Private`), and unshielding (`Private ➝ Public`) execute as `PrivacyPreserving` ZK transactions (`send_priv_tx_native_transfer`).
   - In `Public ➝ Private` transfers, the ZK circuit deducts the fee reserve (`~5,070,616` raw units at default `gas_limit = 34,000`) from the recipient's newly minted private note output, allowing even a brand-new Private account with `0` initial balance to receive tokens as long as the transfer amount exceeds the fee reserve (`> 5,070,616` raw units).
   - `propose_native_transfer` honors explicit `Public/` and `Private/` prefixes via `from_privacy: Option<AccountPrivacy>` and validates destination prefixes so `Public ➝ Private` is never misrouted into a transparent transfer.
3. **Downstream Safety Guardrails:**
   - The SDK Engine and consumer modules implement pre-flight validation guards to prevent users from executing invalid self-shielding or unspendable transparent deposits to private accounts.
   - For complete formal analysis, refer to [protocol-compatibility-and-invariants.md](protocol-compatibility-and-invariants.md).

### 4.16 EmPoWer Proof-of-Work Mining & L1 ➝ LEZ Channel Bridge (`pow.rs`)

Because Logos Basecamp executes `ui_qml` plugins inside a strict zero-network sandbox and because Proof-of-Work (EmPoWer) is by cryptographic design a client-side CPU puzzle rather than a centralized remote faucet, `sdk-engine` implements `PowBridgeManager` directly in the Rust seed layer:

1. **Dual-Mode Local PoW & L1 Bedrock Node Synchronization:**
   - **Embedded Native Rust PoW Engine (Default Standalone Mode):** Executes a multi-threaded SHA-256 Proof-of-Work hashing loop locally inside `sdk-engine` (`logos/empower/pow/v1/`), crediting `10,000,000` raw units (`MINED_BLOCK_REWARD_UNITS`) per solved ticket into `SdkPersistentState::unbridged_l1_balance` (persisted atomically to `sdk_state.json`).
   - **Optional External L1 Node Sync (`LOGOS_L1_NODE_URL`, default `http://127.0.0.1:8080`):** When a local `logos-blockchain-node` (L1 Bedrock node) is running on the workstation, `PowBridgeManager` automatically dispatches authentic L1 REST calls (`PUT /pow/mining/start`, `PUT /pow/auto-claim/start`, `PUT /pow/mining/stop`, `GET /pow/status`, `GET /pow/rewards/claimable`, `POST /pow/claim`, and `POST /channel/deposit`).
2. **L1 ➝ LEZ Channel Deposit & On-Chain Settlement (`execute_channel_deposit`):**
   - Encodes canonical Borsh `DepositMetadata` (`DepositRecipient::Identified(PublicAccountEvidence::Key(pubkey))`, 34 bytes) matching `logos-execution-zone/crates/bridge-core`.
   - Enforces the **Asymmetric Deposit Trap Guardrail**: rejects any attempt to bridge transparent L1 tokens directly into a `Private` account.
   - Dispatches an on-chain LEZ settlement transaction (`V03ShardModel` and `V04ActorModel` compatible) to the active LEZ sequencer (`https://testnet.lez.logos.co` or `http://127.0.0.1:3040`), waits for block confirmation via `wallet.poller().poll_tx`, and deducts the bridged balance from `unbridged_l1_balance`.

### 4.17 Zero-Knowledge Prover Worker Daemon (`prover_daemon`, `lez-prover-daemon`)

To guarantee smooth, non-blocking desktop UX on resource-constrained hardware (e.g. dual-core client laptops) and prevent Logos Basecamp IPC timeouts (`callRemoteMethod timed out after 20000ms`), `sdk-engine` implements a decoupled Zero-Knowledge Proving Worker Daemon:

1. **Decoupled Wallet Lock Lifecycle:**
   - Pre-flight witness extraction requires immutable wallet inspection for $< 10\text{ ms}$.
   - Once input parameters are prepared, the exclusive `WalletCore` lock is immediately released. Read calls (`listAllAccounts`, `getBalance`) complete with $< 5\text{ ms}$ latency while proving proceeds.
2. **Cooperative Pacing & Resource Bounds:**
   - Dedicated worker loop with bounded concurrency (`max_concurrency = 1` by default) prevents CPU thrashing and thermal throttling on dual-core machines.
   - Cooperative yields (`tokio::task::yield_now()`) ensure the host system and UI thread remain responsive.
3. **Multi-Protocol IPC Daemon (`ProverIpcServer`):**
   - Communicates via Unix Domain Socket (`/tmp/lez_prover.sock`) with structured line-delimited JSON RPC (`ping`, `status`, `submit`, `query`, `cancel`, `shutdown`).
   - Exposes real-time multi-stage progress telemetry (`Simulating`, `Proving Segment 1/2`, `Recursive STARK Fold`, `Completed`).
   - Standalone CLI entrypoint (`lez-prover-daemon`) enables running as an independent OS service or background daemon.

## 5. Security & Privacy Model

### 5.1 Threat Mitigations

| Threat Vector | Mitigation Strategy | Component |
|---|---|---|
| **Blind Signing** | Instruction decoders convert raw calldata into plain text; `EffectSimulator` pre-calculates balance and state diffs before prompting. | `decode/`, `effects.rs` |
| **TOCTOU Request Tampering** | Immutable 32-byte SHA-256 intent hashing binds request parameters at creation; verified before execution. | `request.rs` |
| **Unapproved Private Balance Snooping** | Private/shielded accounts require explicit `GrantKind::Read` authorization per account. | `session.rs` |
| **State Corruption on Power Failure** | Two-phase atomic write-then-rename with directory fsync and `.bak` snapshot fallback. | `store.rs` |
| **Compromised Development Auto-Approval** | `PolicyApprovalHandler` is strictly feature-gated; CI workflow runs dedicated symbols check to ensure absence from release builds. | `approval.rs`, `ci-linux.yml` |
| **Upstream Platform Breaking Changes** | Direct Git workspace dependencies tracking `logos-execution-zone` `dev` branch with Argon2 storage encryption adapter. | `Cargo.toml`, `store.rs` |
| **Terminal Shoulder Surfing / Password Echo** | Terminal echo disabled via `rpassword::prompt_password`; characters never written to stdout or scrollback buffer. | `cli/src/main.rs` |
| **Typo-Induced Wallet Lockout** | Mandatory two-step password confirmation prompt before initial storage derivation. | `cli/src/main.rs` |
| **Accidental Storage Overwrite** | Guard checks pre-existing files; requires explicit `--force` flag before re-initializing existing directories. | `cli/src/main.rs` |
| **False Success on Empty Directory** | `ensure_wallet_exists` enforces storage verification prior to executing wallet/account operations. | `cli/src/main.rs` |
| **Keystore Authentication Bypass on Unlock** | Verifies passphrases against encrypted storage file on disk via `wallet::storage::Storage::from_path`, preventing unauthorized state unlocking with arbitrary passwords. | `api.rs` (`unlock`) |
| **Passphrase Rotation Vulnerability** | Validates current password against on-disk keystore before re-encrypting with new credentials; prevents unauthorized passphrase resets. | `api.rs` (`change_password`) |
| **Unregistered LNS Handle Fund Loss** | Pre-flight destination resolution verifies LNS records exist on-chain and hold valid public receiving keys before transaction creation; rejects unresolvable domains immediately. | `api.rs`, `resolve/lns.rs` |
| **Tampered / Forged LNS Profiles** | Authenticated AES-256-GCM tag verification bound to associated data `logos-names/v5/record`, followed by strict `profile.name == input_name` consistency checking. | `resolve/lns.rs` |
| **Asymmetric Deposit Trap (Private Shard Asset Lock)** | Pre-flight transaction validation rejects transparent self-shielding attempts from private accounts; prevents permanent asset lockups on transparent shards. | `v03_compat.rs`, `api.rs`, `docs/protocol-compatibility-and-invariants.md` |

### 5.2 Privacy Guarantees

- **Client-Side Cryptography:** All private keys, Argon2id derivations, and transaction signings occur exclusively within local host memory.
- **Zero Telemetry:** The SDK does not include any telemetry, analytics, tracking pings, or diagnostic reporting to external servers.
- **Minimal Sequencer Trust:** The wallet relies on the configured LEZ sequencer solely for broadcasting signed transactions and querying committed state.

## 6. Implementation Status & Incremental Roadmap

Following the project's phased delivery approach, the architecture document reflects factual, currently implemented systems, with future capabilities staged for incremental addition:

### Current Implementation (Core Headless Engine, FFI, Tooling & Smart Contract)
- [x] Three-tier architecture skeleton (`sdk-engine`, `sdk-ffi`, `cli`, `evidence-tool`).
- [x] In-memory session store with per-account grants (`Read`, `Propose`, `IdentityRead`, `IdentityPropose`) and automatic expiration.
- [x] Immutable request pipeline with deterministic 32-byte SHA-256 intent hashing.
- [x] Non-blocking asynchronous approval handler trait (`CliApprovalHandler`, `PolicyApprovalHandler`, `FfiApprovalHandler`).
- [x] Calldata instruction decoders for Native, Token, ATA, and Testimonial programs.
- [x] Pre-execution effect simulator and balance diff projector.
- [x] Program source verification abstraction conforming to reproducible smart contract registry standards.
- [x] Direct native & token transaction execution pipeline with pre-flight checks, verifier integration, and atomic state transitions.
- [x] Clean-room Logos Name Service (LNS) `.lez` resolver with Argon2id derivation, live sequencer PDA query, AES-256-GCM profile decryption, and caching.
- [x] Identity Accountability cryptographic primitives (HKDF derivation, scoped nullifiers, Two-Tier Shamir Secret Sharing over GF($2^8$), BIP-340 Schnorr attestations, and secp256k1 ECDH).
- [x] Crash-safe atomic store with write-then-rename and `.bak` recovery.
- [x] Automated CI pipelines for Linux and macOS tracking upstream LEZ `dev`.
- [x] **Full C-ABI FFI Layer (`sdk-ffi`):** Complete `extern "C"` surface with opaque pointer handles (`FfiSdkEngine`), panic-safe JSON serialization (`{"ok": true, ...}` / `{"ok": false, "error": "..."}`), non-blocking approval callback bridge supporting synchronous decisions & asynchronous deferred approvals, memory lifecycle (`lez_wallet_free_string`), and auto-generated C header (`include/lez_wallet_sdk.h`).
- [x] **Comprehensive CLI Subcommands (`cli`):** Full dispatch wiring for wallet lifecycle (`create`, `open`, `restore`), account management (`create-public`, `create-private`, `list`, `balance`), transfers across 4 directions with transparent `.lez` LNS name resolution and interactive terminal approval modal, and token operations (`create-definition`, `initialize-holding`, `transfer`, `mint`, `burn`, `balance`).
- [x] **Standalone Development Environment (`dev-env/`, Docker, Justfile):** Containerized and native standalone LEZ sequencer with mocked Bedrock L1, pre-funded Alice & Bob genesis accounts, fast `RISC0_DEV_MODE=1` execution, zero external network dependency, and declarative `Justfile` tasks (`dev-start`, `dev-stop`, `dev-health`, `dev-fund`, `dev-clean`).
- [x] **On-Chain Smart Contract & Dispatcher Tooling (`programs/`, `tools/`, `deployments.json`):** RISC-V zkVM `testimonial-program` with `SUBMISSION_ID` attribution, `lez-dispatcher` program deployment and transaction dispatch CLI, dynamic multi-network `deployments.json` registry, and `evidence-tool` adoption auditor.

### Upcoming Incremental Stages
- [ ] **Basecamp Integration & Testimonial Mini-App:** Core and UI Basecamp modules published to catalog, testimonial submission on testnet 0.3 - 0.4.
