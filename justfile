# T-KOMA Development Commands
# Use: just fmt (format all), just check (verify build), just ci (full CI)

# Default recipe: show available commands
default:
    @just --list

# Format all Rust code
fmt-rust:
    cargo fmt --all

# Format all markdown, SQL, JSON, and TOML files using dprint
fmt-other:
    dprint fmt

# Format everything (Rust + Markdown/SQL/JSON/TOML)
fmt: fmt-rust fmt-other

# Check Rust formatting without fixing
check-fmt-rust:
    cargo fmt --all -- --check

# Check markdown/SQL/JSON/TOML formatting without fixing
check-fmt-other:
    dprint check

# Check all formatting without fixing
check-fmt: check-fmt-rust check-fmt-other

# Run cargo check
check:
    cargo check --all-features --all-targets

# Run cargo clippy
clippy:
    cargo clippy --all-features --all-targets

# Run tests (excluding live tests)
test:
    cargo test

# Run all checks (format, clippy, test)
ci: check-fmt check clippy test

# Clean build artifacts
clean:
    cargo clean
    rm -rf target/

# Build in release mode
build-release:
    cargo build --release --all-features

# Run the gateway
run-gateway:
    cargo run --bin t-koma-gateway

# Run the CLI
run-cli:
    cargo run --bin t-koma-cli
