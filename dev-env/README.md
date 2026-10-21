## Evice Wallet SDK — Standalone Development Environment (`dev-env`)

This directory provides the **supporting configuration fixtures and accounts** for the local development environment of the Evice Wallet SDK (for Logos Execution Zone). The environment runs an authentic LEZ sequencer in **standalone mode** (with an in-memory mocked Bedrock L1 layer) and pre-funded genesis accounts. It requires **no external internet connection and zero reliance on external testnet faucets**.

## 1. Architectural Highlights

- **First-Class Orchestration:** Integrated directly via root `docker-compose.yml` and `Justfile` (`just dev-start`, `just dev-stop`, `just dev-health`).
- **Clean Component Separation:** Operational commands live at the repository root, while supporting configuration templates and genesis accounts live in this directory.
- **Pre-Funded Genesis Accounts:** Includes pre-seeded public and private (shielded) accounts documented in [`genesis_accounts.json`](genesis_accounts.json).
- **Fast Mock Proving:** Runs under `RISC0_DEV_MODE=1` so block production, transaction validation, and state commitments execute in milliseconds without zkVM STARK proving latency.
- **Cross-Platform & Isolated:** Runs seamlessly via Docker on Linux and developer workstations, with native Cargo fallback for environments without Docker.
- **Faucet-Independent:** Pre-funds accounts at genesis for offline development, insulating tests and developer workflows from public testnet faucet availability.

## 2. Pre-Funded Genesis Accounts

All accounts below are provisioned at genesis in [`sequencer_config.json`](sequencer_config.json) with substantial native token balances:

| Account Alias | Type | Account ID / Address | Private Key (Hex) | Initial Balance | Role |
|---|---|---|---|---|---|
| **Alice** | Public | `Public/6aa4c365699b89d963ef85a59868b9612b6001f90750b4970662a62776332e81` | `7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb` | 10,000,000,000,000 units | Primary Genesis Funder & Dispenser |
| **Bob** | Public | `Public/891ae258f92ca8447af6cf09c41d96b3a06187d098b91ab2cc17b42e3c4a2fa1` | `f434f8741720014586ae43356d2aec6257da086222f604ddb75d69733b86fc4c` | 20,000,000,000,000 units | Secondary Counterparty |
| **Shielded 1** | Private | `44KVvzvh2ephQU9aR5snboGUXKgS1FnPYh1W4KZKwmhm` | *(Derived from private seed)* | 10,000,000,000,000 units | Shielded Circuit Pool Account 1 |
| **Shielded 2** | Private | `478hD6LXFTYu5gRRkWkCyXPRsWgCwAfRredoCUfxr5b9` | *(Derived from private seed)* | 20,000,000,000,000 units | Shielded Circuit Pool Account 2 |

Mock Bedrock Inscription Funding Public Key:
`2e03b2eff5a45478e7e79668d2a146cf2c5c7925bce927f2b1c67f2ab4fc0d26`

## 3. Directory Layout

```text
lez-wallet-sdk/
├── docker-compose.yml          # Root Docker Compose runner
├── Dockerfile                  # Multi-stage container build for standalone sequencer
├── Justfile                    # Workspace task runner (just dev-start, dev-stop, dev-health)
dev-env/
├── genesis_accounts.json       # JSON fixture defining pre-funded public and private keys
├── sequencer_config.json       # Standalone genesis parameters and mock channel configuration
├── wallet_config.json          # SDK & CLI configuration pointing to http://127.0.0.1:3040
└── README.md                   # Architecture documentation and usage guide
```

## 4. Usage Guide

### Step 1: Start the Standalone Sequencer

From the repository root, start the sequencer container using Docker or Just:

```bash
# Using Just runner
just dev-start

# Or directly via Docker Compose
docker compose up -d
```

For environments without Docker, launch directly via the native Cargo runner:

```bash
just dev-native-start
```

### Step 2: Verify Sequencer Health

Check if the sequencer is responding to JSON-RPC requests:

```bash
just dev-health
```

Expected output:
```text
Probing sequencer health at http://127.0.0.1:3040...
🟢 Sequencer is HEALTHY (checkHealth returned result)
```

You can stream container logs with:

```bash
just dev-logs
```

### Step 3: Configure Wallet CLI / SDK

Point your wallet configuration to the local sequencer by copying [`wallet_config.json`](wallet_config.json) to your wallet data directory:

```bash
mkdir -p ~/.evice-wallet
cp dev-env/wallet_config.json ~/.evice-wallet/wallet_config.json

# Check wallet diagnostic banner
cargo run --bin evice-wallet -- info
```

### Step 4: Fund Fresh Accounts (Local Dispenser)

To disburse native test tokens from the pre-funded Alice genesis account to any newly derived account address or `.lez` name:

```bash
just dev-fund <RECIPIENT_ADDRESS_OR_LEZ_NAME> [AMOUNT]
```

Example:
```bash
just dev-fund Public/891ae258f92ca8447af6cf09c41d96b3a06187d098b91ab2cc17b42e3c4a2fa1 5000000
```

> [!TIP]
> **Direct CLI Transfer:** You can also import or transfer directly using the Alice Genesis Key via the `evice-wallet` CLI:
> ```bash
> cargo run --bin evice-wallet -- transfer public \
>     --from Public/6aa4c365699b89d963ef85a59868b9612b6001f90750b4970662a62776332e81 \
>     --to <RECIPIENT_ADDRESS> \
>     --amount 5000000
> ```

### Step 5: Stop the Sequencer

```bash
# Using Just runner
just dev-stop

# Or directly via Docker Compose
docker compose down
```

To clean persistent chain data and reset genesis state:

```bash
just dev-clean
```

## 5. Technical & Operational Notes

### Container & zkVM Execution Architecture
The standalone sequencer runs inside a Debian Bookworm environment with custom compilation requirements:
1. **Smartcard & OpenSSL Dependencies:** The LEZ monorepo compiles `pcsc-sys` as part of its hardware wallet dependencies. The build stage includes `libpcsclite-dev`, and the runtime includes `libpcsclite1`.
2. **Fast Mock Proving (`RISC0_DEV_MODE=1`):** In standalone mode, zkVM execution avoids computationally heavy STARK proof generation while validating execution soundness.
3. **zkVM Runtime Executor (`r0vm`):** The image includes the official RISC Zero `r0vm` binary installed at `/usr/local/bin/r0vm`, configured via `RISC0_SERVER_PATH=/usr/local/bin/r0vm`. This ensures guest zkVM programs execute without spawning errors during genesis transaction execution.

### Genesis Channel Initialization & First-Boot Behavior
When running `just dev-start` on a clean environment:
- The container checks whether `/var/lib/sequencer_service/data/sequencer_config.json` exists.
- If not present, it executes `sequencer_service setup` using the mounted genesis fixtures and funding key `2e03b2eff5a45478e7e79668d2a146cf2c5c7925bce927f2b1c67f2ab4fc0d26`.
- **First-Boot Log Observation:** In the logs (`just dev-logs`), you may observe a brief notice stating that the channel does not exist yet before creating it. This is standard LEZ sequencer initialization behavior. Once created, the sequencer enters continuous block production (producing blocks every ~7 ms).

### RocksDB Storage & Permission Management
The sequencer maintains chain state in an embedded RocksDB database under `dev-env/.sequencer_home`. Because Docker containers execute with container root permissions, files written to this volume are owned by `root:root`. 
- To avoid `Permission denied` errors during cleanup, the `just dev-clean` recipe invokes a transient root container (`alpine rm -rf /dev-env/.sequencer_home`) to cleanly wipe the volume without requiring `sudo` on the host workstation.

## 6. Deterministic Testing & CI Integration

The standalone development environment ensures:
- **Reproducibility:** Automated test suites run identically across developer workstations and CI runners.
- **Fast Execution:** Tests execute in seconds without waiting for public network block intervals.
- **Offline Resilience:** Build validation, wallet initialization, and transaction simulation never fail due to external network outages or public faucet rate limits.
