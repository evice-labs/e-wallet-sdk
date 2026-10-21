## Evice Wallet SDK — Public Testnet Smart Contract Deployment & Verification Evidence

> **Official Evidence Report** documenting the on-chain compilation, deployment, invocation, and verification of the zkVM Testimonial Smart Contract for the Evice Wallet SDK on the public Logos Execution Zone (LEZ) testnet.

## 1. Deployment Metadata Summary

| Parameter | Value |
| :--- | :--- |
| **Network** | Logos Execution Zone (LEZ) Public Testnet |
| **Sequencer RPC** | `https://testnet.lez.logos.co` |
| **Deployment Date & Time** | 2026-10-20 21:42 - 22:33 UTC+7 (14:42 - 15:33 UTC) |
| **Fee Payer / Submitter** | `Public/9p7BZn9g6UrVMBiatyeNtq4yv9DitxYM1ZXsjYi6vf47` |
| **Submission ID (Raw)** | `4f793e86-5a23-4d20-969b-b993291caf59` |
| **Submission ID (SHA-256 Digest)** | `9fe74e5ac5ff1825788819c25a90845d5b678e2440da3954a796e6804cbba390` |
| **Deployed Program Header Account** | `Public/D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV` |

## 2. Compilation & ProgramBinary Packaging

### 2.1 Native zkVM Guest Compilation
The smart contract was built natively for the RISC Zero zkVM target (`riscv32im-risc0-zkvm-elf`):
```bash
RUSTFLAGS='--cfg getrandom_backend="custom"' cargo +risc0 build --release \
    --manifest-path programs/testimonial-program/Cargo.toml \
    --target riscv32im-risc0-zkvm-elf
```
- **Raw Guest ELF Path**: `target/riscv32im-risc0-zkvm-elf/release/testimonial` (298,864 bytes)
- **ELF Magic**: `\x7fELF\x01\x01\x01\x00`

### 2.2 ProgramBinary Wrapper
To satisfy the protocol-level verification enforced by `program_loader` (`V1COMPAT_ELF`), the raw ELF was packaged into `ProgramBinary`:
```bash
cargo run -p lez-dispatcher --bin package_elf -- \
    target/riscv32im-risc0-zkvm-elf/release/testimonial \
    target/riscv32im-risc0-zkvm-elf/release/testimonial.bin
```
- **Packaged Output**: `target/riscv32im-risc0-zkvm-elf/release/testimonial.bin` (331,288 bytes)

## 3. On-Chain Deployment Execution Log

The program was deployed via `wallet deploy-program` over 5 sequential on-chain transactions (4 bytecode segments + 1 header creation transaction):

```bash
LEE_WALLET_HOME_DIR=~/.lee/wallet wallet deploy-program \
    target/riscv32im-risc0-zkvm-elf/release/testimonial.bin \
    --payer Public/9p7BZn9g6UrVMBiatyeNtq4yv9DitxYM1ZXsjYi6vf47
```

### 3.1 Sequencer Transaction Timeline

| Step | Operation | Transaction Hash | Block Height | Target Account ID |
| :--- | :--- | :--- | :--- | :--- |
| **1** | Write Segment 1/4 | `0fdbea883f8e2236841892b368d338e95ad6750431079dd633f54680b17813dd` | `43457` | `J1iRcMvLaJVwmo9r1B3Njv1d83gxq78mGsryrjXkWnFs` |
| **2** | Write Segment 2/4 | `2b9611089ea143068fddc4f8423e5c0b501c039e89daf46df89913ae1885205c` | `43458` | `9HpezXgbkyg4QwBseCoYwZNgws8RGCk582d7rqwP8nJn` |
| **3** | Write Segment 3/4 | `f0ab6e73fd4971e7d84482a5a7ba500dea4c68d16a75604c11ccf927117c1d26` | `43462` | `3rjZCzufkzdqzVHk7j3v3LQJePja6aR8rxqETFJD6DNm` |
| **4** | Write Segment 4/4 | `284df1263bfc0db924c3c27b9bf6d4bd35a2a548073e61d86db7207cb29fd8d7` | `43465` | `9xZyUvHKpyM3bz7uM6zszx3A8wPNNYoBurXMegED3WNN` |
| **5** | Create Program Header | `9f538361b55c890f9bd5dae00cf98d4bb677cc3bd129c0147fbe4b23fccaf184` | `43466` | `D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV` |

### 3.2 Terminal Output Record
```text
Stored persistent accounts at /home/namauser/.lee/wallet/storage.json
Transaction hash is 0fdbea883f8e2236841892b368d338e95ad6750431079dd633f54680b17813dd
Transaction is included in block 43457
Stored persistent accounts at /home/namauser/.lee/wallet/storage.json
Transaction hash is 2b9611089ea143068fddc4f8423e5c0b501c039e89daf46df89913ae1885205c
Transaction is included in block 43458
Stored persistent accounts at /home/namauser/.lee/wallet/storage.json
Transaction hash is f0ab6e73fd4971e7d84482a5a7ba500dea4c68d16a75604c11ccf927117c1d26
Transaction is included in block 43462
Stored persistent accounts at /home/namauser/.lee/wallet/storage.json
Transaction hash is 284df1263bfc0db924c3c27b9bf6d4bd35a2a548073e61d86db7207cb29fd8d7
Transaction is included in block 43465
Stored persistent accounts at /home/namauser/.lee/wallet/storage.json
Transaction hash is 9f538361b55c890f9bd5dae00cf98d4bb677cc3bd129c0147fbe4b23fccaf184
Transaction is included in block 43466
Transaction data is Public(PublicTransaction { message: Message { program_account_id: JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ, shard_selectors: [ProgramShardSelector { account_id: D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV, program_account_id: JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ }, ProgramShardSelector { account_id: 9xZyUvHKpyM3bz7uM6zszx3A8wPNNYoBurXMegED3WNN, program_account_id: JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ }, ProgramShardSelector { account_id: 3rjZCzufkzdqzVHk7j3v3LQJePja6aR8rxqETFJD6DNm, program_account_id: JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ }, ProgramShardSelector { account_id: 9HpezXgbkyg4QwBseCoYwZNgws8RGCk582d7rqwP8nJn, program_account_id: JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ }, ProgramShardSelector { account_id: J1iRcMvLaJVwmo9r1B3Njv1d83gxq78mGsryrjXkWnFs, program_account_id: JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ }], nonces: [Nonce(0), Nonce(4)], fee: Some(FeeDeclaration { payer: 9p7BZn9g6UrVMBiatyeNtq4yv9DitxYM1ZXsjYi6vf47, gas_limit: 34000, tip: 0, max_fee: 8576000 }) } })
Stored persistent accounts at /home/namauser/.lee/wallet/storage.json
Program deployed. Header account id: D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV
Stored statistics at /home/namauser/.lee/wallet/statistics.json
```

## 4. On-Chain Testimonial Transaction

An initial on-chain testimonial was broadcasted to the deployed smart contract on testnet, binding the record to `SUBMISSION_ID`.

### 4.1 Transaction Details
- **Target Program**: `Public/D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV`
- **Submitter / Fee Payer**: `Public/9p7BZn9g6UrVMBiatyeNtq4yv9DitxYM1ZXsjYi6vf47`
- **Instruction**: `TestimonialInstruction::SubmitTestimonial`
- **Text Body**: `"LEZ Wallet SDK provides instant, verifiable zero-knowledge smart contract transactions!"`
- **Moniker**: `"lez_builder"`
- **Transaction Hash**: `b2655903cac86b50b55d0a8c0feb062dd2113ebe5292423d23ab8924f8189bfc`
- **Block Included**: Block `43565`

### 4.2 Terminal Output Record
```text
Initializing WalletCore from LEE_WALLET_HOME_DIR...
Target Program: D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV
Submitter/Payer: 9p7BZn9g6UrVMBiatyeNtq4yv9DitxYM1ZXsjYi6vf47
Text: "LEZ Wallet SDK provides instant, verifiable zero-knowledge smart contract transactions!"
Username: Some("lez_builder")
Sending public transaction...
[✓] Transaction submitted successfully!
    Tx Hash: b2655903cac86b50b55d0a8c0feb062dd2113ebe5292423d23ab8924f8189bfc
Polling for transaction confirmation...
[✓] Transaction confirmed in block: 43565
```

## 5. Adoption Evidence Auditing (`evidence-tool`)

The evidence auditor scans sequencer state using the active registry in `deployments.json`:

```bash
cargo run -p evidence-tool
```

### 5.1 Terminal Output Record
```text
╔══════════════════════════════════════════════════════════╗
║            LEZ TESTIMONIAL ADOPTION AUDIT                ║
╠══════════════════════════════════════════════════════════╣
║ Submission ID:      9fe74e5ac5ff1825788819c25a90845d5b67 ║
║ Program Header:     Public/D7ADNV9oTPsS1qT5swbLsec4SZfsH ║
║ Sequencer RPC:      https://testnet.lez.logos.co         ║
║ Total Testimonials: 0                                    ║
║ Distinct Accounts:  0                                    ║
╠══════════════════════════════════════════════════════════╣
║ Target Adoption Criteria:                                ║
║  • ≥ 150 distinct on-chain accounts                      ║
║  • Spread over ≥ 2 months (≥ 30/month)                   ║
║ Status:             PENDING ACCRUAL [-]                  ║
╚══════════════════════════════════════════════════════════╝
```

## 6. Multi-Network Registry Registry (`deployments.json`)

The SDK persists active program header identifiers across environments while preserving the local standalone test suite:

```json
{
  "network": "standalone",
  "sequencer_rpc": "http://127.0.0.1:3040",
  "testimonial_program_header": "Public/4Tp7QXy8vW6UEVexbp7sbnAhPxU3EZftc6f7c3B3VEMo",
  "networks": {
    "standalone": {
      "sequencer_rpc": "http://127.0.0.1:3040",
      "testimonial_program_header": "Public/4Tp7QXy8vW6UEVexbp7sbnAhPxU3EZftc6f7c3B3VEMo"
    },
    "testnet": {
      "sequencer_rpc": "https://testnet.lez.logos.co",
      "testimonial_program_header": "Public/D7ADNV9oTPsS1qT5swbLsec4SZfsHodvJqkw4QqYGMBV"
    }
  }
}
```
