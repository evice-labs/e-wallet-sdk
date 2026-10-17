#!/usr/bin/env bash
set -euo pipefail

# LEZ Wallet SDK — Demo Script
# Verifies build, CLI info, offline LNS name resolution, and submission identifier.
# Must succeed from a clean clone, unmodified, on Linux and macOS.

echo "=== LEZ Wallet SDK Demo ==="
echo ""

# Step 1: Build
echo "[1/4] Building..."
cargo build --bin lez-wallet 2>&1 | tail -1

# Step 2: Show info
echo "[2/4] SDK info:"
cargo run --quiet --bin lez-wallet -- info

# Step 3: Resolve a .lez name (offline — shows normalisation + derivation)
echo "[3/4] Name resolution (offline):"
cargo run --quiet --bin lez-wallet -- resolve alice.lez

# Step 4: Show submission ID
echo "[4/4] Submission ID:"
cat SUBMISSION_ID

echo ""
echo "=== Demo complete ==="
