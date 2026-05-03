# Carrel

A reading and curation tool for the open web — and for books, papers, podcasts, and everything else worth reading. Local-first, peer-to-peer, no servers we run, no engagement metrics, no growth loops.

In spirit: a successor to Google Reader's social layer. In practice: a desktop app that owns your reading life under your own keys.

## Status

Early. Pre-alpha. Not yet usable.

This repository is the home of the project's design documents, code, and (eventually) releases. The design documents are stable; the code does not yet exist in any meaningful form.

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
just cli     # run the CLI stub
```

The desktop app is only a placeholder binary until the Tauri and Leptos shell is implemented.

## License

To be determined. The project's commitments — no acquisition, no relicensing into a closed product, data and protocols belong to the user — argue for a copyleft license; AGPL is likely. Until a license is chosen, all rights are reserved.

## Contributing

The project is currently being built by a small group for a small group. Once there's something to use, we'll think about contribution. In the meantime, the design documents are the artifact; reading them and disagreeing with them is the most useful contribution.
