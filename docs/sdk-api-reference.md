## Evice Wallet SDK — API Reference Manual

> **Version:** Draft v0.2  
> **Target Audience:** dApp Developers, Core Module (`wallet-provider`) Integrators, and Native Client Engineers.

## 1. Architectural Overview & API Surfaces

The Evice Wallet SDK exposes two distinct high-level interfaces on top of the headless `SdkEngine`:

1. **`DAppApi` (Untrusted External Surface):**  
   Session-scoped interface accessed by dApps or external consumers. All methods require a valid `SessionId` and enforce granular, per-account privilege checks (`GrantKind`).
2. **`OwnerApi` (Privileged Wallet Controller Surface):**  
   Privileged management interface intended for the wallet host environment (CLI, Desktop QML UI, or Core Module). Governs key generation, storage locking, account derivation, and access grant issuance.
3. **`C-ABI FFI Layer` (`crates/sdk-ffi`, `include/e_wallet_sdk.h`):**  
   C-compatible linkage layer with opaque engine handles, ABI-stable JSON serialization, panic boundaries (`catch_unwind`), and non-blocking approval callback dispatch.

```
┌────────────────────────────────────────────────────────────────────────┐
│                        CONSUMER APPLICATIONS                           │
│   ┌───────────────────────────┐      ┌─────────────────────────────┐   │
│   │   External LEZ dApp       │      │   Logos Basecamp / Desktop  │   │
│   └─────────────┬─────────────┘      └──────────────┬──────────────┘   │
└─────────────────┼───────────────────────────────────┼──────────────────┘
                  │ Session-Scoped IPC                │ Privileged Host
                  ▼                                   ▼
┌────────────────────────────────────────────────────────────────────────┐
│                      C-ABI INTEROPERABILITY LAYER                      │
│   `crates/sdk-ffi` (Header: `include/e_wallet_sdk.h`)                 │
│   • Opaque Pointer: `*mut FfiSdkEngine`                                │
│   • Memory Lifecycle: `e_wallet_free_string(char*)`                  │
│   • Non-Blocking Bridge: `FfiApprovalCallback` + Deferred Submission   │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   ▼
┌────────────────────────────────────────────────────────────────────────┐
│                          CORE SDK ENGINE                               │
│   `crates/sdk-engine`                                                  │
│   ┌────────────────────────────────────────────────────────────────┐   │
│   │                      DAppApi (Session Guard)                   │   │
│   ├────────────────────────────────────────────────────────────────┤   │
│   │                      OwnerApi (Privileged)                     │   │
│   ├────────────────────────────────────────────────────────────────┤   │
│   │ Subsystems: Decoders | Verifiers | LNS Resolver | SSS Identity │   │
│   └────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────────┘
```

## 2. Wire Format & Response Envelope

All structured data across the C-ABI FFI boundary is formatted as UTF-8 encoded JSON. Responses follow a standardized envelope:

### Success Response
```json
{
  "ok": true,
  "...": "<method-specific payload>"
}
```

### Error Response
```json
{
  "ok": false,
  "error": "Descriptive error message string"
}
```

### Memory Management Rule
Any `*mut c_char` returned by `e_wallet_*` functions is allocated by Rust using standard heap allocators. The caller **must** deallocate it by passing the pointer to `e_wallet_free_string(ptr)`. Passing a null pointer is safe (no-op).

## 3. `DAppApi` Reference Specification

### 3.1 `sdk_info`
Retrieves engine metadata, API compatibility version, and runtime status.
- **FFI Signature:** `e_wallet_sdk_info(engine: *mut FfiSdkEngine) -> *mut c_char`
- **Session Required:** No.
- **Response Format:**
```json
{
  "ok": true,
  "sdk_version": "0.2.0",
  "api_version": 1,
  "network": "standalone-local",
  "wallet_initialized": true,
  "wallet_locked": false
}
```

### 3.2 `connect`
Initializes an isolated, timestamped dApp session with protocol version handshake.
- **FFI Signature:** `e_wallet_connect(engine: *mut FfiSdkEngine, caller_identity: *const c_char, api_version: u32) -> *mut c_char`
- **Parameters:**
  - `caller_identity`: Human-readable identifier of the calling dApp (e.g. `"uniswap.lez"` or `"portal-app"`).
  - `api_version`: Expected protocol version (currently `1`).
- **Response Format:**
```json
{
  "ok": true,
  "session_id": "sess_01J98X..."
}
```

### 3.3 `disconnect`
Explicitly closes an active session, invalidating all associated ephemeral grants.
- **FFI Signature:** `e_wallet_disconnect(engine: *mut FfiSdkEngine, session_id: *const c_char) -> *mut c_char`
- **Response Format:** `{"ok": true}`

### 3.4 `request_accounts` & `list_accounts`
Returns accounts authorized for the given session.
- **FFI Signatures:**
  - `e_wallet_request_accounts(engine: *mut FfiSdkEngine, session_id: *const c_char) -> *mut c_char`
  - `e_wallet_list_accounts(engine: *mut FfiSdkEngine, session_id: *const c_char) -> *mut c_char`
  - `e_wallet_list_all_accounts(engine: *mut FfiSdkEngine) -> *mut c_char` *(Privileged Owner API)*
- **Privilege Required:** `request_accounts` & `list_accounts` require active session grants. `list_all_accounts` is an owner call querying all derived accounts.
- **Response Format (AccountViewDto):**
```json
{
  "ok": true,
  "accounts": [
    {
      "account_id": "44KVvzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKwmhm",
      "canonical_address": "Public/44KVvzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKwmhm",
      "privacy": "public",
      "is_private": false,
      "label": "Primary Treasury",
      "balance": "10000000000",
      "formatted_balance": "10,000,000,000.00"
    },
    {
      "account_id": "87MNwzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKzxyz",
      "canonical_address": "Private/87MNwzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKzxyz",
      "privacy": "private",
      "is_private": true,
      "label": "Shielded Vault",
      "balance": "5000000000",
      "formatted_balance": "5,000,000,000.00"
    }
  ]
}
```

### 3.5 `get_balance`
Fetches the balance of an authorized account in native atomic units.
- **FFI Signature:** `e_wallet_get_balance(engine: *mut FfiSdkEngine, session_id: *const c_char, account_id: *const c_char) -> *mut c_char`
- **Privilege Required:** `GrantKind::Read`.
- **Factual Balance Semantics:**
  - For **Transparent/Public** accounts: Queries live sequencer account state and returns on-chain native balance.
  - For **Private/Shielded** accounts: Factual shielded balance representing zero-knowledge circuit note commitments decryptable by the account's viewing key.
- **Response Format:**
```json
{
  "ok": true,
  "balance": 500000000
}
```

### 3.6 `propose_native_transfer`
Proposes a native token transfer. Computes an immutable intent hash and dispatches for user approval.
- **FFI Signature:**
  ```c
  char* e_wallet_propose_native_transfer(
      FfiSdkEngine* engine,
      const char* session_id,
      const char* from_account,
      const char* to_account_or_name,
      const char* amount_str
  );
  ```
- **Privilege Required:** `GrantKind::Propose` on `from_account`.
- **Destination Support:** Accepts standard Base58 `AccountId` or human-readable `.lez` names (e.g. `"alice.lez"`).
- **Response Format:**
```json
{
  "ok": true,
  "request_id": "req_01J98Y...",
  "status": "pending_approval"
}
```

### 3.7 `propose_token_transfer`
Proposes a transfer of custom fungible tokens defined under the LEZ token program.
- **FFI Signature:**
  ```c
  char* e_wallet_propose_token_transfer(
      FfiSdkEngine* engine,
      const char* session_id,
      const char* from_account,
      const char* to_account_or_name,
      const char* token_id,
      const char* amount_str
  );
  ```
- **Privilege Required:** `GrantKind::Propose` on `from_account`.
- **Response Format:**
```json
{
  "ok": true,
  "request_id": "req_01J98Z...",
  "status": "pending_approval"
}
```

### 3.8 `get_request_status` & `cancel_request`
Monitors or cancels the lifecycle of an immutable request proposal.
- **FFI Signatures:**
  - `e_wallet_get_request_status(engine: *mut FfiSdkEngine, request_id: *const c_char) -> *mut c_char`
  - `e_wallet_cancel_request(engine: *mut FfiSdkEngine, request_id: *const c_char) -> *mut c_char`
- **Possible Statuses:**
  - `pending_approval`: Waiting for user confirmation.
  - `approved`: Confirmed by user, preparing on-chain submission.
  - `submitted`: Dispatched to sequencer with `tx_hash`.
  - `rejected`: Explicitly rejected by user or expired via timeout.
  - `failed`: On-chain execution failure with error reason.
- **Response Format:**
```json
{
  "ok": true,
  "status": "submitted",
  "tx_hash": "a1b2c3d4..."
}
```

### 3.8a `propose_contract_call`
Proposes an explicit smart contract execution specifying target program ID, payer account ID, account mentions, calldata, and human-readable description.
- **FFI Signature:** `e_wallet_propose_contract_call(engine: *mut FfiSdkEngine, session_id: *const c_char, program_id: *const c_char, payer_id: *const c_char, mentions_json: *const c_char, calldata_hex: *const c_char, description: *const c_char) -> *mut c_char`
- **Parameters:**
  - `session_id`: Active session identifier.
  - `program_id`: Target smart contract Base58 AccountId.
  - `payer_id`: Account authorizing transaction execution and paying fees.
  - `mentions_json`: JSON array of string AccountIds mentioned in transaction calldata.
  - `calldata_hex`: Hex-encoded instruction calldata bytes.
  - `description`: Optional human-readable transaction summary.
- **Behavior:**
  - Validates that `payer_id` has been granted `GrantKind::Propose`.
  - Configures target program actor states for signers (`AccountIdentity::Public` or `AccountIdentity::PrivateOwned`) and non-signing mentions (`AccountIdentity::PublicNoSign`).
  - Routes through approval handler and dispatches via `send_pub_tx_paid_by`.
- **Response Format:**
```json
{
  "ok": true,
  "request_id": "e4b1fc9a-728b-4c9c-9e45-f290eac667ee",
  "status": "pending_approval"
}
```

### 3.8b Token Program Holdings API
Queries and registers fungible token program asset holdings across sessions:
- **`list_token_holdings(session_id)`:**
  Lists tracked token holdings accessible to the session.
  - **FFI Signature:** `e_wallet_list_token_holdings(engine, session_id) -> *mut c_char`
  - **Response:** `{"ok": true, "tokens": [{"holding_id": "...", "definition_id": "...", "symbol": "LOGOS", "name": "Logos", "is_shielded": false}]}`
- **`list_all_token_holdings()`:**
  Privileged Owner API returning all token holdings tracked by the engine keystore.
  - **FFI Signature:** `e_wallet_list_all_token_holdings(engine) -> *mut c_char`
- **`register_token_holding(holding_id, definition_id, symbol, name, is_shielded)`:**
  Privileged Owner API registering a new token holding, persisting it atomically to disk (`token_holdings: Vec<TokenHoldingEntry>`).
  - **FFI Signature:** `e_wallet_register_token_holding(engine, holding_id, definition_id, symbol, name, is_shielded) -> *mut c_char`

### 3.9 `resolve_name`
Resolves a human-readable `.lez` domain name using the clean-room Logos Name Service (LNS) client against live on-chain registry state.
- **FFI Signature:** `e_wallet_resolve_name(engine: *mut FfiSdkEngine, session_id: *const c_char, name: *const c_char) -> *mut c_char`
- **Parameters:**
  - `session_id`: Active authorized session identifier.
  - `name`: Target `.lez` domain name (e.g. `"alice.lez"` or `"jef.lez"`).
- **Resolution Protocol (SPEC.md):**
  1. **Normalisation:** Trims whitespace, lowers ASCII case, verifies `[a-z0-9-]{1,32}` without leading/trailing hyphens.
  2. **Key Derivation:** Computes `Argon2id(name, "logos-names/v5", 64MiB, 3, 1)` -> `id` (`SHA-256("logos-names/v5/id" || master)`) and `key` (`SHA-256("logos-names/v5/key" || master)`).
  3. **PDA Calculation:** Derives `seed = SHA-256("logos-names/v5/name/" || id)` and public PDA `SHA-256(pad32("/LEE/v0.2/AccountId/PDA/") || registry || seed)`.
  4. **State Query:** Fetches on-chain account state from LEZ Sequencer JSON-RPC (`getAccount`), extracting the registry shard.
  5. **Borsh Decoding & Expiry:** Decodes `NameRecord` and checks expiration (epoch ms; deposit v5 never lapses, active terms checked against 30-day grace period of 2,592,000,000 ms).
  6. **AES-256-GCM Decryption:** Opens sealed profile payload with 12-byte nonce, 16-byte authentication tag, and Associated Data `b"logos-names/v5/record"`.
  7. **Profile Deserialization:** Decodes Borsh `Profile` (ignoring zero-padding bytes) and verifies `profile.name == input_name`.
- **Response Format:**
```json
{
  "ok": true,
  "name": "alice",
  "account": "44KVvzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKwmhm",
  "npk": null,
  "vpk": null,
  "expires": 1893456000000,
  "texts": [
    ["chat", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"]
  ]
}
```

### 3.10 Identity Accountability API
Client-side primitives for accountable pseudonymity:
- **`request_identity_access`:** Requests `IdentityRead` or `IdentityPropose` grants for a session.
- **`get_identity_commitment`:** Generates deterministic master identity commitment `SHA-256("lez/identity/v1/commitment" || secret)`.
- **`derive_scoped_nullifier`:** Computes cross-dApp unlinkable nullifier `SHA-256("lez/identity/v1/scoped_nullifier" || secret || scope_id)`.
- **`sign_identity_attestation`:** Produces a 64-byte BIP-340 Schnorr signature over custom payloads.
- **`prepare_identity_registration`:** Prepares Two-Tier Shamir Secret Sharing (SSS) threshold encrypted recovery shares over GF(2^8).

### 3.11 `resolve_account`
Resolves an arbitrary account identifier string into structured metadata:
- **FFI Signature:** `e_wallet_resolve_account(engine: *mut FfiSdkEngine, input: *const c_char) -> *mut c_char`
- **Supported Formats:**
  1. **LNS Domain Names:** `*.lez` (e.g. `"alice.lez"`) via live on-chain name resolution.
  2. **Prefixed Public Addresses:** `"Public/<base58_address>"` or `"public/<base58_address>"`.
  3. **Prefixed Private Addresses:** `"Private/<base58_address>"` or `"private/<base58_address>"`.
  4. **Raw Base58 Addresses:** Identifies local keystore privacy type or defaults to transparent public.
- **Response Format:**
```json
{
  "ok": true,
  "account_id": "44KVvzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKwmhm",
  "privacy": "public",
  "canonical_address": "Public/44KVvzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKwmhm",
  "label": "alice.lez",
  "is_local": false
}
```

## 4. `OwnerApi` Reference Specification

Privileged interface restricted to the wallet owner / host environment:

| Method Signature (C-ABI) | Description | Parameters |
|---|---|---|
| `e_wallet_create_wallet(engine, password)` | Generates a new master BIP-39 mnemonic phrase and initializes encrypted storage. | `password`: Non-empty master passphrase. |
| `e_wallet_restore_wallet(engine, mnemonic, password)` | Restores an existing wallet from a 12- or 24-word recovery phrase. | `mnemonic`: Normalized space-delimited phrase; `password`: New passphrase. |
| `e_wallet_lock(engine)` | Immediately zeroes key material in memory and clears cached session states. | None. |
| `e_wallet_unlock(engine, password)` | Re-derives encryption keys and unlocks local storage. | `password`: Master passphrase. |
| `e_wallet_change_password(engine, old_password, new_password)` | Re-encrypts keystore storage and updates the master passphrase. | `old_password`: Current passphrase; `new_password`: New passphrase. |
| `e_wallet_create_account_public(engine, label)` | Derives a new transparent public account (`Public/...`). | `label`: Optional UTF-8 display alias. |
| `e_wallet_create_account_private(engine, label)` | Derives a new shielded zero-knowledge private account. | `label`: Optional UTF-8 display alias. |
| `e_wallet_list_all_accounts(engine)` | Lists all registered accounts with labels, types, and balances. | None. |
| `e_wallet_grant_account(engine, session_id, account_id, grant_kind)` | Issues an explicit privilege grant to an active dApp session. | `grant_kind`: `"read"` or `"propose"`. |
| `e_wallet_revoke_session(engine, session_id)` | Immediately revokes an authorized session and all its grants. | `session_id`: Target session identifier. |
| `e_wallet_set_account_label(engine, account_id, label)` | Renames or sets a human-readable alias on an account, persisting it atomically to override old aliases. | `account_id`: Target account; `label`: New alias string. |
| `e_wallet_pow_start_mining(engine)` | Starts the EmPoWer Proof-of-Work mining daemon and L1 auto-claim (`PUT /pow/mining/start`, `PUT /pow/auto-claim/start`) alongside the local SHA-256 PoW worker. | None. |
| `e_wallet_pow_stop_mining(engine)` | Stops the EmPoWer Proof-of-Work mining daemon (`PUT /pow/mining/stop`) and halts the local SHA-256 PoW worker. | None. |
| `e_wallet_pow_status(engine)` | Queries live EmPoWer mining telemetry (`GET /pow/status`, `GET /pow/rewards/claimable`) and persistent unbridged L1 reward balance (`unbridged_l1_balance`). | None. |
| `e_wallet_channel_deposit(engine, recipient_account, amount_str)` | Bridges unbridged L1 PoW rewards to a **Transparent/Public** LEZ account via L1 `/pow/claim` + `/channel/deposit` (`bridge_core::DepositMetadata`) and on-chain LEZ settlement. Enforces the Asymmetric Deposit Trap guardrail (rejects `Private` destinations). | `recipient_account`: Destination Public AccountId; `amount_str`: Raw atomic units string. |
| `e_wallet_get_transaction_status(engine, tx_hash)` | Queries on-chain inclusion status of a transaction (`pending` or `confirmed` with `block_id`) via direct sequencer JSON-RPC `getTransaction` without payload deserialization. | `tx_hash`: 32-byte hex string (with or without `0x` prefix). |

### 4.1 Vault Passphrase Rotation (`change_password`)
Allows the wallet owner to change the master encryption passphrase protecting local keystore storage:
- **FFI Signature:** `e_wallet_change_password(engine: *mut FfiSdkEngine, old_password: *const c_char, new_password: *const c_char) -> *mut c_char`
- **Authentication:** Validates `old_password` against current storage credentials before re-encrypting.
- **Success Response:** `{"ok": true}`
- **Error Response:** `{"ok": false, "error": "InvalidPassword"}` when `old_password` fails validation.

### 4.2 Account Alias & Label Management (`set_account_label`)
Associates a friendly display name with an account:
- **FFI Signature:** `e_wallet_set_account_label(engine: *mut FfiSdkEngine, account_id: *const c_char, label: *const c_char) -> *mut c_char`
- **Persistence:** Saved atomically into `SdkPersistentState` (`account_labels`), ensuring that renaming an account replaces previously bound labels and survives process restarts.
- **Success Response:** `{"ok": true}`

### 4.3 On-Chain Transaction Status Query (`get_transaction_status`)
Queries the current lifecycle / inclusion status of a transaction directly from the LEZ sequencer:
- **FFI Signature:** `e_wallet_get_transaction_status(engine: *mut FfiSdkEngine, tx_hash: *const c_char) -> *mut c_char`
- **Parameters:** `tx_hash`: 32-byte transaction hash (64 hex characters, optional `0x` prefix).
- **Inclusion Semantics:**
  - When the sequencer includes the transaction in a block, returns `"status": "confirmed"` and `"block_id": <u64>`.
  - When the transaction is in mempool or pending sequencer batching, returns `"status": "pending"` and `"block_id": null`.
  - Bypasses transaction payload protobuf deserialization, preventing decoding failures across protocol versions (v0.3 vs v0.4).
- **Response Format (Confirmed):**
```json
{
  "ok": true,
  "tx_hash": "0x5a1b...",
  "status": "confirmed",
  "block_id": 142
}
```
- **Response Format (Pending):**
```json
{
  "ok": true,
  "tx_hash": "0x5a1b...",
  "status": "pending",
  "block_id": null
}
```

## 5. Non-Blocking Approval Modal Bridge

To eliminate UI freezes and support asynchronous desktop dialogs (e.g. Qt/QML modals in Basecamp), the SDK implements a callback bridge:

### Step 1: Register Callback
```c
int on_approval_prompt(const char* prompt_json, void* user_data);

e_wallet_set_approval_callback(engine, on_approval_prompt, host_context);
```

### Step 2: Callback Return Semantics
When an action requiring user consent occurs (e.g. `propose_native_transfer`):
- Return `1`: **Synchronous Approve** (immediate confirmation).
- Return `0`: **Synchronous Reject** (immediate denial).
- Return `-1`: **Asynchronous / Deferred** (host opens a modal dialog; resolution occurs later via `e_wallet_submit_approval_decision`).

### Step 3: Deferred Resolution
Once the user confirms or dismisses the UI dialog:
```c
// decision: 1 for Approve, 0 for Reject
e_wallet_submit_approval_decision(engine, request_id, 1);
```

## 6. Error Codes & Diagnostics

Errors returned inside `{"ok": false, "error": "..."}` adhere to the following categories:

| Error Category | Identifier / Prefix | Description | Remediation |
|---|---|---|---|
| **Authentication** | `WalletLocked` | Operation attempted while wallet core is locked. | Call `unlock(password)` before invoking the method. |
| **Session** | `SessionNotFound` / `SessionExpired` | The supplied `session_id` does not exist or has exceeded its TTL. | Re-authenticate via `connect()`. |
| **Authorization** | `PermissionDenied` | The session lacks required `GrantKind` for the account. | User must authorize access via `grant_account()`. |
| **Validation** | `InvalidArgument` | Invalid account address, malformed Base58 string, or zero amount. | Correct input parameter format. |
| **Name Resolution** | `NameResolutionError` | `.lez` domain is unregistered or expired. | Verify domain registration status on LNS. |
| **TOCTOU Guard** | `IntentHashMismatch` | Transaction parameters were mutated between proposal and signing. | Reject request; regenerate fresh proposal. |
| **Sequencer** | `SequencerRpcError` | Upstream LEZ sequencer node is unreachable or returned RPC error. | Verify local container via `just dev-health`. |
