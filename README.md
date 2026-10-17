# Carrel

**A reading life of your own.**

Carrel is a desktop reader for the open web. Subscribe to feeds, save articles,
read offline, keep passages and notes, and send something worth reading to a
person you trust. Your archive lives on your computer, under your own keys.

It takes inspiration from Google Reader's small circle of trusted curators,
and from the habit of keeping a commonplace book. Reading comes first.

[Guide](https://cigrainger.github.io/carrel/) ·
[Releases](https://github.com/cigrainger/carrel/releases) ·
[Vision](docs/VISION.md) · [Contributing](CONTRIBUTING.md)

## What you can do

- **Read comfortably.** A compact context bar, stable source sidebar, four
  reading fonts, Paper/Sepia/Dark/Black themes, and Editorial or Dense lists.
  Keyboard navigation and return-to-list restoration keep your place.
- **Build an archive.** RSS, Atom and JSON Feed subscriptions; discovery from
  website addresses; arbitrary article URLs; cached text and images; offline
  title/body search; optional source folders; OPML import and export.
- **Keep what you notice.** Durable highlights and notes, full annotation review,
  deliberate conflict resolution, and Markdown export.
- **Share by choice.** Follow a person by public key and share one archived
  article with an optional note. Personal notes, highlights and reading state
  stay private. Your own devices can synchronize through a separate pairing flow.

Completion is explicit. Opening an article, scrolling to the end or going to the
next one does not mark it read. Lists are chronological, with explicit **Load
older**. There are no engagement metrics, recommendations, notifications or
telemetry, and no Carrel account or service we operate.

## Status

**Pre-alpha: `0.1.0-alpha.1`.** The local reader, archive, annotations, self-sync
engine and first private-sharing flow are implemented. Automated tests cover
isolated stores and real in-process peers, including audience separation,
revocation and conflicting offline edits. The frontend has separate native and
CSR/WASM checks.

Physical two-computer sharing, live native file-picker selection and broader
platform usability remain manual release checks. This is an early tool for
careful trials, with no independent security audit. See [security](SECURITY.md)
and the [first-friend checklist](docs/PRIVATE_SHARING_PLAN.md#first-friend-trial).

Release automation builds native Apple Silicon and Intel Mac packages, plus
Linux x86-64 packages, with SHA-256 checksums. A version tag creates a **draft
prerelease** for review. Mac trial builds are ad-hoc signed and not notarized;
see the [installation guide](site/src/getting-started.md). Published builds, when
available, are on [GitHub Releases](https://github.com/cigrainger/carrel/releases).

Books/papers metadata, Pocket import, send-to-ereader, mobile and public sharing
are future work. The [vision](docs/VISION.md) describes the destination; this
README describes what exists.

## Run from source

You need Rust **1.95 or newer**, a C/C++ toolchain, and the
[Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/).
On macOS, install Xcode Command Line Tools. Linux needs the WebKitGTK 4.1 and
other development packages listed in the [development guide](site/src/development.md).

```sh
git clone https://github.com/cigrainger/carrel.git
cd carrel
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo install tauri-cli --version 2.10.0 --locked
```

With [just](https://just.systems/) installed:

```sh
just dev          # desktop app with hot reload
just check        # formatting, native + WASM lint, workspace tests
just build        # release app and platform packages
```

Without just, run `cargo tauri dev` from `crates/carrel-app`. The Tauri build
hook builds the Leptos CSR frontend with Trunk; you do not need Node or npm.
The first build compiles native dependencies and can take a while.

Close the development app before opening an installed copy: they use the same
local archive by default. On macOS it lives in
`~/Library/Application Support/Carrel`; on Linux, `~/.local/share/carrel`
(or under XDG_DATA_HOME). `CARREL_DATA_DIR` selects an isolated archive for
development. Keymap configuration uses the platform's configuration directory,
or `CARREL_CONFIG_DIR`. See [data and recovery](site/src/data-and-recovery.md)
before moving an archive.

## Find your way around

The engine is all Rust: Tauri and Leptos at the desktop boundary, Cozo as the
source of truth, and Iroh for peer transport and verified replication.

| Crate | Responsibility |
| --- | --- |
| `carrel-core` | Fact model, signatures, causal state, keymaps and reading rules |
| `carrel-store` | Cozo transactions, local archive, search and key persistence |
| `carrel-feeds` | Polite HTTP, feed discovery/parsing and sanitized extraction |
| `carrel-sync` | Identity authorization, Iroh mirror, private device and friend protocols |
| `carrel-app` | Native commands/workers and Leptos webview |
| `carrel-cli` | Headless development and diagnostics |

Read [VISION](docs/VISION.md) first, then [ARCHITECTURE](docs/ARCHITECTURE.md).
[CODE_MAP](docs/CODE_MAP.md) connects behaviors to source and tests.
[SCHEMA](docs/SCHEMA.md) defines stored facts and trust rules; [DESIGN](docs/DESIGN.md)
defines the reading interface. [AGENTS.md](AGENTS.md) records project practice for
human and AI contributors. The [engineering review](docs/ENGINEERING_REVIEW.md)
records the publication audit and its limits.

## License

Carrel is free software under **AGPL-3.0-or-later**. Distributed derivatives stay
under strong copyleft; modified versions made available for network interaction
must also offer their corresponding source as the license requires. This fits a
reader whose users should be able to inspect, modify and keep their tools.
See [COPYRIGHT](COPYRIGHT), the full [license](LICENSE), and the
[GNU explanation](https://www.gnu.org/licenses/why-affero-gpl.html).

Bundled fonts and icons retain their own licenses; see [THIRD_PARTY](THIRD_PARTY.md).
Contributions are welcome within the project's small, deliberate scope.
