# Rust task runner
# See https://github.com/CameronBrooks11/dev-toolbox/blob/main/docs/just-conventions.md

set dotenv-load := false

# Default: show available recipes
default:
    @just --list

# Install dependencies and tools
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

# Format + lint (non-mutating — safe for CI; clippy compiles, so it type-checks too)
check: fmt-check lint

# Run tests
test:
    cargo test

# Build all crates
build:
    cargo build

# Run the CLI, e.g. `just run init --name demo`
run *ARGS:
    cargo run -p scadman-cli -- {{ ARGS }}

# Remove build artifacts
clean:
    cargo clean
