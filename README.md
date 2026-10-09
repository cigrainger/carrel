# Carrel

A reading and curation tool for the open web — and for books, papers, podcasts, and everything else worth reading. Local-first, peer-to-peer, no servers we run, no engagement metrics, no growth loops.

In spirit: a successor to Google Reader's social layer. In practice: a desktop app that owns your reading life under your own keys.

## Status

Early. Pre-alpha. Not yet a usable reader.

This repository is the home of the project's design documents, code, and (eventually) releases. The design documents are stable enough to guide implementation, and the foundation crates are now taking shape: the Cargo workspace, Cozo schema, identity primitives, keystore, CLI, feed ingestion, readable article extraction, content-shape detection, and the first Tauri + Leptos desktop reader surfaces are implemented. The CLI remains a development surface rather than end-user software.

## Documents

- [`docs/VISION.md`](docs/VISION.md) — what Carrel is, why it exists, what it will never do
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — how the system is put together
- [`docs/SCHEMA.md`](docs/SCHEMA.md) — the data model
- [`docs/DESIGN.md`](docs/DESIGN.md) — visual and interaction design

If you're trying to understand the project, read VISION first.

## Development

Carrel is a Rust workspace requiring Rust 1.95 or later. The desktop frontend uses Leptos 0.8 and Tauri 2, with stable dependency versions recorded in `Cargo.lock`. Install `just`, `trunk`, and `cargo-tauri` for the development commands, and add the WASM standard library to your Rust toolchain:

```bash
rustup target add wasm32-unknown-unknown
```

The common development commands are:

```bash
just check   # format check, native + WASM clippy, tests
just test    # run all tests
just dev     # run the Tauri desktop app in dev mode
just build   # build the desktop app bundle
just cli     # run CLI commands
```

The native frontend tests use Leptos's SSR feature. `just frontend-check` separately compiles and lints the CSR/WASM code used by the real desktop webview, and is included in `just check` and CI.

Normal development and test builds retain line-level debugging for Carrel's
code, omit dependency debug symbols, and disable incremental caches to keep
disk use manageable. For full debugger inspection, use `cargo build --profile
debugging` or `cargo test --profile debugging`. Release builds are unaffected.

The desktop app currently boots a Tauri 2 shell with a Leptos webview, bundled reading fonts, design tokens, sidebar chrome, a status strip, feed management, reading view, and local library surfaces for Today, Library, Unread, Reading, Starred, and per-feed item lists. The CLI can initialize a local data directory, print install info, run schema migrations, execute raw Cozo queries, subscribe to feeds, manually fetch RSS/Atom/JSON Feed subscriptions, cache readable article HTML and images, inspect content-shape facts, recompute derived shape facts, and show a text preview of cached items; it remains a dogfooding and development surface while the desktop reader comes online.

## License

Carrel is licensed under the GNU Affero General Public License v3.0 or later. See [`LICENSE`](LICENSE).

## Contributing

The project is currently being built by a small group for a small group. Once there's something to use, we'll think about contribution. In the meantime, the design documents are the artifact; reading them and disagreeing with them is the most useful contribution.
