# Carrel

A reading and curation tool for the open web — and for books, papers, podcasts, and everything else worth reading. Local-first, peer-to-peer, no servers we run, no engagement metrics, no growth loops.

In spirit: a successor to Google Reader's social layer. In practice: a desktop app that owns your reading life under your own keys.

## Status

Early. Pre-alpha. Not yet a usable reader.

This repository is the home of the project's design documents, code, and (eventually) releases. The design documents are stable enough to guide implementation, and the foundation crates are now taking shape: the Cargo workspace, Cozo schema, identity primitives, keystore, CLI, feed ingestion, readable article extraction, content-shape detection, and the first Tauri + Leptos desktop shell are implemented. The CLI remains a development surface rather than end-user software.

## Documents

- [`docs/VISION.md`](docs/VISION.md) — what Carrel is, why it exists, what it will never do
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — how the system is put together
- [`docs/SCHEMA.md`](docs/SCHEMA.md) — the data model
- [`docs/DESIGN.md`](docs/DESIGN.md) — visual and interaction design

If you're trying to understand the project, read VISION first.

## Development

Carrel is a Rust workspace. The common development commands are:

```bash
just check   # format check, clippy, tests
just test    # run all tests
just dev     # run the Tauri desktop app in dev mode
just build   # build the desktop app bundle
just cli     # run CLI commands
```

The desktop app currently boots a Tauri 2 shell with a Leptos webview, bundled reading fonts, design tokens, sidebar chrome, a status strip, and a Today route backed by typed Tauri commands. The CLI can initialize a local data directory, print install info, run schema migrations, execute raw Cozo queries, subscribe to feeds, manually fetch RSS/Atom/JSON Feed subscriptions, cache readable article HTML and images, inspect content-shape facts, recompute derived shape facts, and show a text preview of cached items; it remains the main dogfooding surface while the reader UI comes online.

## License

Carrel is licensed under the GNU Affero General Public License v3.0 or later. See [`LICENSE`](LICENSE).

## Contributing

The project is currently being built by a small group for a small group. Once there's something to use, we'll think about contribution. In the meantime, the design documents are the artifact; reading them and disagreeing with them is the most useful contribution.
