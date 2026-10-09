default: check

# Run all checks: format, native and webview lint, tests
check: fmt-check clippy test frontend-check

# Run all tests
test:
    cargo test --workspace

# Lint
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Compile and lint the actual client-side webview (native checks use SSR)
frontend-check:
    cargo clippy -p carrel-app-frontend --target wasm32-unknown-unknown --no-default-features --features csr -- -D warnings

# Format check
fmt-check:
    cargo fmt --check

# Apply formatting
fmt:
    cargo fmt

# Run the desktop app in dev mode
dev:
    cd crates/carrel-app && cargo tauri dev

# Build the desktop app bundle
build:
    cd crates/carrel-app && cargo tauri build

# Run the CLI
cli *ARGS:
    cargo run -p carrel-cli -- {{ARGS}}
