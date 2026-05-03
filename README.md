# Carrel

A reading and curation tool for the open web — and for books, papers, podcasts, and everything else worth reading. Local-first, peer-to-peer, no servers we run, no engagement metrics, no growth loops.

In spirit: a successor to Google Reader's social layer. In practice: a desktop app that owns your reading life under your own keys.

## Status

Early. Pre-alpha. Not yet a usable reader.

This repository is the home of the project's design documents, code, and (eventually) releases. The design documents are stable enough to guide implementation, and the foundation crates are now taking shape: the Cargo workspace, Cozo schema, identity primitives, keystore, CLI, and first feed ingestion path are implemented. The desktop app is still a placeholder, and the CLI remains a development surface rather than end-user software.

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
just dev     # run the desktop app stub
just cli     # run CLI commands
```

The desktop app is only a placeholder binary until the Tauri and Leptos shell is implemented. The CLI can initialize a local data directory, print install info, run schema migrations, execute raw Cozo queries, subscribe to feeds, and manually fetch RSS/Atom/JSON Feed subscriptions; it is the main dogfooding surface while the lower layers come online.

## License

Carrel is licensed under the GNU Affero General Public License v3.0 or later. See [`LICENSE`](LICENSE).

## Contributing

The project is currently being built by a small group for a small group. Once there's something to use, we'll think about contribution. In the meantime, the design documents are the artifact; reading them and disagreeing with them is the most useful contribution.
