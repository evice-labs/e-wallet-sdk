# LEZ Protocol State Invariants & Compatibility Specification
## Architectural Analysis of Account Derivation, Invariant Rules, and Testnet Version Drift

| Document Metadata | Value |
| :--- | :--- |
| **Title** | LEZ Protocol State Invariants & Compatibility Specification |
| **Target Engine** | `lez-wallet-sdk` (`sdk-engine`, `sdk-ffi`, `cli`) |
| **Upstream References** | `logos-blockchain/logos-execution-zone` (`lee`, `lez/wallet`) |
| **Standard Specification** | LP-0021 (Universal LEZ Wallet Architecture) |
| **Status** | Active Technical Specification |

---

### 1. Executive Summary

This document specifies the protocol state separation invariants, cryptographic account derivation rules, zero-knowledge execution constraints, and wire-format evolutions between LEZ Testnet v0.3 and v0.4. It serves as the authoritative technical reference for developers building against the Evice LEZ Wallet SDK and integrating client applications.

---

### 2. Dual Cryptographic Account Derivation Models

In the Logos Execution Zone, account identities are derived via two distinct cryptographic mechanisms that enforce strict architectural boundaries:

#### 2.1 Public Accounts (Transparent Ledger)
- **Derivation Function:**
  $$\text{AccountId} = \text{lee::AccountId::from}(\&\text{PublicKey})$$
- **Cryptographic Basis:** Elliptic-curve cryptography (secp256k1 or Ed25519).
- **Authentication Model:** State transitions that debit from an account's public balance (`Actor::native_balance(account_id)`) require a valid digital signature produced by the account's private signing key (`sk`).
- **State Representation:** Public accounts maintain transparent state recorded directly on the sequencer ledger.

#### 2.2 Private Accounts (Shielded ZK Pool)
- **Derivation Function:**
  $$\text{AccountId} = \text{AccountId::for\_private\_account}(\&\text{nullifier\_public\_key}, \&\text{viewing\_public\_key}, \text{kind})$$
- **Cryptographic Basis:** 
  - Post-Quantum Lattice Encapsulation: **ML-KEM-768** (Viewing Public Key, `vpk`, 1,184 bytes).
  - Algebraic SNARK Curves: **BabyJubjub / Poseidon** (Nullifier Public Key, `npk`, 32 bytes).
- **Authentication Model:** Private accounts possess **no ECDSA or Schnorr signing key**. State transitions are authenticated exclusively through Zero-Knowledge proofs that demonstrate ownership of unspent note commitments and valid nullifier derivations without revealing the underlying spending key (`ask`).
- **State Representation:** Encrypted note commitments in the state Merkle tree, obscured behind ephemeral viewing encryptions.

---

### 3. Protocol Invariants & The Asymmetric Deposit Trap

#### 3.1 The Shard Separation Invariant
The LEZ ledger allocates state shards per `AccountId`. When an external sender executes a transparent transfer targeting the Base58 string of a private account (`Public/<private_account_id>`), the native balance is credited to the account's transparent shard:
$$\text{Actor}::\text{native\_balance}(\text{AccountId}_{\text{private}}) \mathrel{+}= \text{amount}$$

#### 3.2 The Signature Catch-22 (Asymmetric Deposit Trap)
Because private accounts are generated without an elliptic-curve keypair, the wallet keystore holds only the ZK nullifier secret key and ML-KEM decapsulation key. 

When attempting to spend or self-shield funds residing on the public shard:
1. The transaction entry requires an authorized public signer to debit the balance and pay execution fees.
2. The wallet engine cannot produce an ECDSA signature for a private account identity.
3. The transaction constructor aborts with:
   $$\text{TransactionBuildError}(\text{InvalidInput}(\text{"Public transaction has no signing account to pay its fees"}))$$

**Implication for Developers:** Under the current protocol, transparent funds deposited into a private account's public shard cannot be retrieved or shielded by that private account. Applications must prevent users from initiating transparent deposits to private account identifiers.

---

### 4. Shielding & Private Note Transfer Support

#### 4.1 Shielding & Private Transfer Mechanics
In LEZ, transactions interacting with private state (shielding `Public ➔ Private`, private transfers `Private ➔ Private`, or deshielding `Private ➔ Public`) are processed via Zero-Knowledge note proofs (`send_proven`). When shielding from a transparent account, the wallet constructs note commitments and nullifiers that credit the shielded pool of the recipient private account.

#### 4.2 Resolution of Historical `WitnessNotInRoot` Error
In earlier development iterations, client binaries compiled prior to upstream commit `4ba9f988` (October 14, 2026) encountered an error during fee sponsorship:
$$\text{ExecutionError::WitnessNotInRoot}$$
This was resolved in commit `4ba9f988` by refactoring private actor simulation and decoupling fee-payer witnesses. Toolchains and client binaries tracking this commit or later (including the `names` 0.10 and updated SDK toolchains) fully support public-to-private shielding and private note transfers on LEZ Testnet v0.3.

---

### 5. Wire-Format Serialization & Version Alignment

| Protocol Dimension | LEZ Testnet v0.3 | LEZ Upstream v0.4 (Commit `5562eee`) |
| :--- | :--- | :--- |
| **State Organization** | Positional Shard Model (`shard_selectors`) | Transactional Actor Model (`actor_states`) |
| **Serialization** | Strict length-dependent Borsh (v0.3 structs) | Padded actor structures and dynamic boundary steps |
| **Fee Settlement** | Direct in-circuit reserve checking | Decoupled `fee_inbox` and headroom pricing |
| **RPC Wire Failure** | `-32602 Invalid params ("Unexpected length of input")` when deserializing v0.4 transactions | Native actor state simulation and execution |

---

### 6. Architectural Guidelines for SDK Integrations

1. **Strict Pool Separation:**
   - Public accounts should exclusively operate within the transparent public pool.
   - Private accounts should exclusively spend from unspent note commitments in the shielded pool.
2. **Preventing Self-Shielding Pitfalls:**
   - In v0.3 environments, client applications must guard against self-shielding flows originating from a private account's transparent shard.
3. **Dynamic Gas Reserves:**
   - Always query `get_fee_state()` from the sequencer RPC to dynamically calculate headroom reserves rather than applying static, sub-allocated gas limits.
4. **Error Data Preservation:**
   - When wrapping RPC responses, clients must preserve `err_obj.data()` to ensure positional deserialization mismatches and circuit constraint failures are diagnosable.

