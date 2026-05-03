default: check

# Run all checks: format, clippy, tests
check: fmt-check clippy test

# Run all tests
test:
    cargo test --workspace

# Lint
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Format check
fmt-check:
    cargo fmt --check

# Apply formatting
fmt:
    cargo fmt

# Run the desktop app in dev mode
dev:
    cargo run -p carrel-app

# Run the CLI
cli *ARGS:
    cargo run -p carrel-cli -- {{ARGS}}
