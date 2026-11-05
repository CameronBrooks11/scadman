# Rust task runner

set dotenv-load := false

# Default: show available recipes
default:
    @just --list

# Install dependencies and tools, and the git hooks from .pre-commit-config.yaml
setup:
    rustup component add rustfmt clippy
    @command -v pre-commit >/dev/null 2>&1 || { echo "pre-commit is not installed, so the git hooks cannot be installed. Install it (e.g. 'uv tool install pre-commit' or 'pipx install pre-commit'), then re-run 'just setup'." >&2; exit 1; }
    pre-commit install

# Format code (mutates working tree — use locally)
fmt:
    cargo fmt

# Verify formatting (non-mutating — use in CI)
fmt-check:
    cargo fmt --check

# Run linters
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Type-check without linting. CI gates on this rather than on clippy, so a new
# lint in a new Rust release reports without failing the build.
typecheck:
    cargo check --all-targets --all-features

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
