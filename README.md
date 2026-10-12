# Carrel

A reading and curation tool for the open web — and for books, papers, podcasts, and everything else worth reading. Local-first, peer-to-peer, no servers we run, no engagement metrics, no growth loops.

In spirit: a successor to Google Reader's social layer. In practice: a desktop app that owns your reading life under your own keys.

## Status

Early. Pre-alpha. The local reading loop, archive, offline search, annotation
review and article URL capture work. Two-device peer-to-peer self-sync is
implemented and integration tested; a physical two-machine check remains.
Private article sharing with a trusted person is implemented and tested with
isolated local peers. Physical two-machine networking remains a manual check.

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

The desktop app adopts the Reading Room revision of Field: neutral Paper and
forest Dark surfaces, quieter date gutters, grotesk titles, serif excerpts and
a reading column for Highlights. Sepia and Black remain available; existing
Light preferences use Paper. It has bundled fonts, a collapsible
navigation and source sidebar (Cmd+B), explicit reading completion, saved position restoration,
highlights and notes, and local lists for Today, Reading, Library, Unread,
Starred, individual feeds and source folders. **Load older** extends lists through the archive.
**Import OPML** and **Export OPML** in Settings → Sources use native file dialogs;
importing keeps existing subscription settings. Nested OPML folders become readable folder
paths, exported as named groups. Assign an optional folder to each source in
Settings → Sources; blank leaves it unfiled. Sidebar folder names open combined
article lists, and their disclosure buttons collapse the source rows.
Source and folder lists show the newest publication first, using arrival time
only for undated posts. Each article list can search titles and cached
text offline; `/` focuses search and `\` clears it. Every word matches a word
beginning, within the current filter. The query and loaded results survive
opening an article and returning. Highlights supports full note review, note
editing, deliberate removal, and a complete Markdown export through a native
save dialog. **Save article** in the context bar or its app menu (Cmd+N) archives an article
URL and its images without subscribing to the site. Saving preserves the current
view and offers **Read article** or **View in Library** afterward. Existing
articles retain their identity, archive, highlights and reading state. A failed
capture keeps the URL available for retry; partial image caching is stated.
**Add source** is available in the sidebar, Settings → Sources and the app menu.
Two-device self-sync is described in
[`docs/SELF_SYNC_PLAN.md`](docs/SELF_SYNC_PLAN.md).

For private sharing, set up your identity in **Settings → Devices**, then use
**Settings → People**. Exchange contact codes directly, review the full public
key, and explicitly follow each other. A bare public key can be saved before
connection information arrives. In the reader, **Share article** (`c` or `C`)
opens a person picker with an optional note; saved shares wait locally until
the desktops can connect. Only the chosen article, its cached images and that
note are shared. Private highlights, notes and reading state stay private.
**Library → Shared with you** shows received articles, with their vouches also
available in reader Info. Settings → People lets you retry a connection, remove
a person or withdraw a sent vouch. Withdrawal preserves an article already
received. This initial flow connects one desktop per person; contact pins and
sent history belong to that desktop.
People offers Copy public key / Copy contact code, with a complete-text fallback
if clipboard access is unavailable. Sent articles extend with Load older; retry
and withdrawal keep the loaded history. The first two-desktop friend trial is
described in [`docs/PRIVATE_SHARING_PLAN.md`](docs/PRIVATE_SHARING_PLAN.md#first-friend-trial).

One context bar holds list or reader controls. **Aa** in the reader offers theme,
four bundled reading fonts, text size, line width and spacing. **Settings** (Cmd+,)
contains Reading, Sources, People, Devices and Keyboard sections, including Editorial/Dense
article lists. Preferences are remembered on this installation and changing
layout preserves the visible passage or row. Closing Settings resumes the same
article or list. The context bar's app menu keeps commands reachable with the
sidebar hidden. Cmd+K finds working commands, sources and articles in the local
archive; `?` shows the effective shortcuts with native modal focus behavior.
**Settings → Sources** and **Source activity**, opened through a failure's **Details**
control or the command palette, explain incomplete updates and retry individual
sources without interrupting the reader. Cmd+R refreshes feeds and Cmd+Shift+R
opens Source activity.

The CLI can initialize a local data directory, print install info, run schema
migrations, execute raw Cozo queries, subscribe to feeds, fetch RSS/Atom/JSON
Feed subscriptions, cache readable article HTML and images, inspect and
recompute content-shape facts, and show a text preview of cached items. It
remains a development surface.

## License

Carrel is licensed under the GNU Affero General Public License v3.0 or later. See [`LICENSE`](LICENSE).

## Contributing

The project is currently being built by a small group for a small group. Once there's something to use, we'll think about contribution. In the meantime, the design documents are the artifact; reading them and disagreeing with them is the most useful contribution.
