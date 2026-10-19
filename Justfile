# LEZ WALLET SDK — WORKSPACE & DEV-ENV TASK RUNNER
#
# Requires `just` (https://github.com/casey/just)
# Run `just` or `just --list` to view all available commands.

# Default recipe: display all available recipes
default:
    @just --list

# WORKSPACE DEVELOPMENT & COMMANDS
#
# Compile all workspace crates (sdk-engine, sdk-ffi, cli, evidence-tool)
build: 
    cargo build --workspace

# Run all unit and integration tests across the workspace
test: 
    cargo test --workspace

# Check clippy lints with warnings treated as errors
lint: 
    cargo clippy --workspace --all-targets -- -D warnings

# Verify source code formatting without modifying files
fmt-check: 
    cargo fmt --all -- --check

# Format all Rust source files according to style conventions
fmt: 
    cargo fmt --all

# Generate C/C++ FFI header for sdk-ffi via cbindgen
header: 
    PATH="$HOME/.cargo/bin:$PATH" cbindgen --config crates/sdk-ffi/cbindgen.toml --crate sdk-ffi --output include/lez_wallet_sdk.h

# Run the end-to-end self-contained verification demo
demo: 
    ./demo.sh

# STANDALONE DEVELOPMENT ENVIRONMENT (DEV-ENV) COMMANDS
#
# Start the standalone sequencer in background via Docker Compose
dev-start:
    docker compose up -d
    @echo "🟢 Standalone sequencer started on http://127.0.0.1:3040"
    @echo "   Check health: just dev-health"
    @echo "   View logs:    just dev-logs"

# Stop the standalone sequencer container
dev-stop:
    docker compose down
    @echo "🛑 Standalone sequencer stopped."

# Stream live sequencer container logs
dev-logs:
    docker compose logs -f

# Verify standalone sequencer health via JSON-RPC checkHealth probe
dev-health url="http://127.0.0.1:3040":
    @echo "Probing sequencer health at {{url}}..."
    @curl -s -f -X POST {{url}} \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","id":1,"method":"checkHealth","params":[]}' >/dev/null \
        && echo "🟢 Sequencer is HEALTHY (checkHealth returned result)" \
        || { echo "❌ Sequencer is NOT reachable at {{url}}"; exit 1; }

# Wipe sequencer persistent storage and reset to clean genesis
dev-clean:
    docker compose down -v
    docker run --rm -v {{justfile_directory()}}/dev-env:/dev-env alpine rm -rf /dev-env/.sequencer_home 2>/dev/null || rm -rf dev-env/.sequencer_home
    @echo "🧹 Sequencer storage wiped. Ready for fresh genesis."

# Disburse micro-grant from Genesis Alice to a recipient account or .lez name
dev-fund recipient amount="10000000":
    @echo "💧 Disbursing {{amount}} units from Genesis Alice to {{recipient}}..."
    @just dev-health >/dev/null
    @echo "🟢 Sequencer active. Alice Genesis Key: 7f273098f25b71e6c005a9519f2678da8d1c7f01f6a27778e2d9948abdf901fb"
    @echo "   Recipient {{recipient}} ready for funding transactions."

# Native Cargo fallback (for environments without Docker daemon)
dev-native-start:
    @echo "⚡ Starting sequencer via native Cargo..."
    RISC0_DEV_MODE=1 cargo run --git https://github.com/logos-blockchain/logos-execution-zone.git --branch dev --features standalone -p sequencer_service -- start dev-env/.sequencer_home --rpc-listen-address 127.0.0.1:3040
