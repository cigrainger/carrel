# Carrel

A reading and curation tool for the open web — and for books, papers, podcasts, and everything else worth reading. Local-first, peer-to-peer, no servers we run, no engagement metrics, no growth loops.

In spirit: a successor to Google Reader's social layer. In practice: a desktop app that owns your reading life under your own keys.

## Status

Early. Pre-alpha. The local reading loop, archive, offline search, and annotation
review work. Two-device peer-to-peer self-sync is the next milestone.

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

The desktop app has an editorial reading layout, bundled fonts, a collapsible
reader sidebar (Cmd+B), explicit reading completion, saved position restoration,
highlights and notes, and local lists for Today, Reading, Library, Unread,
Starred, and individual feeds. **Load older** extends lists through the archive.
**Import OPML** and **Export OPML** use native file dialogs; importing keeps
existing subscription settings. Nested OPML folders become readable folder
paths, exported as named groups. Each article list can search titles and cached
text offline; `/` focuses search and `\` clears it. Every word matches a word
beginning, within the current filter. The query and loaded results survive
opening an article and returning. Highlights supports full note review, note
editing, deliberate removal, and a complete Markdown export through a native
save dialog. Two-device self-sync follows in
[`docs/SELF_SYNC_PLAN.md`](docs/SELF_SYNC_PLAN.md).

The CLI can initialize a local data directory, print install info, run schema
migrations, execute raw Cozo queries, subscribe to feeds, fetch RSS/Atom/JSON
Feed subscriptions, cache readable article HTML and images, inspect and
recompute content-shape facts, and show a text preview of cached items. It
remains a development surface.

## License

Carrel is licensed under the GNU Affero General Public License v3.0 or later. See [`LICENSE`](LICENSE).

## Contributing

The project is currently being built by a small group for a small group. Once there's something to use, we'll think about contribution. In the meantime, the design documents are the artifact; reading them and disagreeing with them is the most useful contribution.
