# Rust task runner
# Mirrors the dev-toolbox just-conventions; will reconcile with the canonical
# dev-toolbox Rust profile once it lands.

set dotenv-load := false

# Default: show available recipes
default:
    @just --list

# Install toolchain components
setup:
    rustup component add rustfmt clippy

# Format code (mutates working tree — use locally)
fmt:
    cargo fmt

# Verify formatting (non-mutating — use in CI)
fmt-check:
    cargo fmt --check

# Run linters
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Format-check + lint (non-mutating — safe for CI; clippy also type-checks)
check: fmt-check lint

# Run tests
test:
    cargo test

# Build all crates
build:
    cargo build

# Run the CLI, e.g. `just run init --name demo`
run *ARGS:
    cargo run -p scadman-cli -- {{ARGS}}

# Remove build artifacts
clean:
    cargo clean
