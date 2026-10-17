# Development

Clone [cigrainger/carrel](https://github.com/cigrainger/carrel). Read
[VISION](https://github.com/cigrainger/carrel/blob/main/docs/VISION.md),
[ARCHITECTURE](https://github.com/cigrainger/carrel/blob/main/docs/ARCHITECTURE.md)
and [AGENTS.md](https://github.com/cigrainger/carrel/blob/main/AGENTS.md) before
non-trivial changes. The
[code map](https://github.com/cigrainger/carrel/blob/main/docs/CODE_MAP.md)
connects behavior to source and regression tests.

## Toolchain

Use Rust 1.95 or newer and a C/C++ compiler. On macOS, install Xcode Command Line
Tools. Packaging and release scripts use Python 3.11 or newer. On Ubuntu 24.04,
the CI/release environment installs:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libclang-dev \
  libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf
```

Install the webview target and the pinned desktop tools:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo install tauri-cli --version 2.10.0 --locked
```

Installer builds also need `cargo install cargo-about --version 0.9.2 --locked
--features cli`. The production hook generates dependency license notices before
building the webview. Unresolved licenses fail packaging; the generated HTML is
ignored locally and included in the native license resources.

These versions are recorded build tools, not a claim about the latest upstream
release. There is no Node/npm build step. The Tauri hooks invoke Trunk with the
CSR feature explicitly; a default native frontend build uses SSR for tests.

## Daily commands

With just installed, run these from the repository root:

```sh
just dev          # native app and frontend hot reload
just check        # format, native + WASM lint, workspace tests
just fmt          # cargo fmt --all
just build        # production frontend, app and platform packages
just cli --help   # development/diagnostic CLI
```

Without just, `cargo tauri dev` and `cargo tauri build` run from
`crates/carrel-app`. Workspace tests and checks run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo clippy -p carrel-app-frontend --target wasm32-unknown-unknown \
  --no-default-features --features csr --locked -- -D warnings
```

Use `CARREL_DATA_DIR` and `CARREL_CONFIG_DIR` to isolate a development profile.
Normal development/test profiles keep line tables for Carrel, omit dependency
debug symbols and disable incremental caches. The optional `debugging` profile
retains full dependency debugging when you need it.

## Documentation

Install `mdbook` 0.5.4 with `cargo install mdbook --version 0.5.4 --locked`.
`just docs` builds this guide into ignored `site/book`; `just docs-serve`
previews it. Guide sources are ordinary Markdown under `site/src`, with one
small stylesheet. GitHub Pages deploys that output; there is no separate
website repository or application service.

The engineering documents under `docs/` remain the source of truth for product,
architecture, schema and interaction decisions. Keep implemented guarantees
distinct from future plans, and update docs in the same change as the code.
