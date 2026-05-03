# Architecture

## Purpose of this document

This document describes how Carrel is put together: what the major components are, how they communicate, what data flows look like, and what cross-cutting concerns we've committed to. It's the document a contributor (human or AI) reads when about to make a change that touches more than one file.

It is *not* the place for the data model itself (see `SCHEMA.md`), the visual and interaction design (see `DESIGN.md`), or the why-we-built-this-at-all (see `VISION.md`). Where those documents disagree with this one, they win for their domain. This document is for the structural decisions that hold across all of them.

This is also a living document. When an architectural decision changes, this document changes — ideally in the same change as the code. If you find drift between this document and the code, treat it as a bug and fix the document.

## The shape of the system

Carrel is a desktop application that runs on the user's own computer, holds all of the user's reading data locally, and synchronizes a subset of that data peer-to-peer with people the user has chosen to follow. There is no server we run, no centralized service, no account system. The user's identity is a cryptographic keypair held on their own machine.

At the highest level:

```
┌─────────────────────────────────────────────────────────────────┐
│ Carrel desktop application (single Tauri process)               │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ Application core (Rust, async)                           │  │
│  │                                                          │  │
│  │  ┌────────────┐  ┌─────────────┐  ┌────────────────┐   │  │
│  │  │ Local      │  │ Sync bridge │  │ Background     │   │  │
│  │  │ store      │←→│ (Iroh docs  │  │ workers        │   │  │
│  │  │ (Cozo)     │  │  + blobs)   │  │ (feeds, OPDS,  │   │  │
│  │  │            │  │             │  │  sync loops)   │   │  │
│  │  └─────┬──────┘  └──────┬──────┘  └────────┬───────┘   │  │
│  │        │                │                   │            │  │
│  │        └────────────────┴───────────────────┘            │  │
│  │                         │                                │  │
│  │                  ┌──────┴───────┐                        │  │
│  │                  │ Command and  │                        │  │
│  │                  │ event API    │                        │  │
│  │                  └──────┬───────┘                        │  │
│  └─────────────────────────┼────────────────────────────────┘  │
│                            │ tauri::invoke                     │
│                            │ tauri::emit                       │
│  ┌─────────────────────────┴────────────────────────────────┐  │
│  │ Webview (Leptos WASM)                                    │  │
│  │  - Reactive UI                                           │  │
│  │  - Reading view, lists, highlights, settings             │  │
│  │  - Calls into core via commands                          │  │
│  │  - Subscribes to events for reactive updates             │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                            ↕ QUIC / Iroh relay
                  ┌─────────────────────────┐
                  │ Other users' Carrel     │
                  │ instances, syncing      │
                  │ shared facts P2P        │
                  └─────────────────────────┘
```

The deep idea is that *all real work happens in the Rust core*. The webview is a thin reactive shell that renders state and dispatches commands. This keeps the UI fast (no business logic in WASM), keeps the data layer testable in isolation, and means the same core can power a CLI, headless tests, and any future companion clients without rewriting business logic.

## Workspace layout

Carrel is a Cargo workspace. The crate boundaries reflect real layering and let us recompile, test, and reason about each layer independently.

```
carrel/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── carrel-core/            # data model, fact types, pure logic
│   ├── carrel-store/           # Cozo wrapper, fact persistence
│   ├── carrel-feeds/           # RSS/Atom/JSON, readability extraction
│   ├── carrel-sync/            # Iroh bridge, peer management
│   ├── carrel-app/             # Tauri shell + Leptos frontend
│   └── carrel-cli/             # headless CLI for development and power users
├── docs/                       # this and other docs
└── design/                     # design tokens, fonts, sample data
```

Layering rules, strictly enforced:

- `carrel-core` depends on nothing in the workspace. It is pure logic: fact types, identity, signing, validation. It has no I/O. It can be tested in microseconds.
- `carrel-store` depends only on `carrel-core`. It owns Cozo. It exposes a typed API for reading and writing facts. It does not know about networks, files outside the database, or UI.
- `carrel-feeds` depends only on `carrel-core`. It does HTTP, parsing, and content extraction. It exposes pure functions: given input, produce items. It does not know about Cozo or Iroh.
- `carrel-sync` depends on `carrel-core` and `carrel-store`. It owns Iroh. It bridges between Iroh-doc fact streams and the local Cozo store.
- `carrel-app` depends on everything. It owns Tauri, the webview, the Leptos frontend, and the orchestration of background workers. UI changes recompile only this crate.
- `carrel-cli` depends on everything except `carrel-app`. It exposes a terminal interface to the same operations the GUI does.

Anti-pattern: any crate calling "up" the dependency graph. If `carrel-store` ever needs to talk to `carrel-sync`, the right answer is to extract an interface into `carrel-core` that both implement, or to have `carrel-app` orchestrate the interaction.

## Data flows

These are the load-bearing flows. Understanding them is the fastest way to understand the system. Each is described in terms of what crate owns each step.

### 1. Writing a local fact (e.g., starring an item)

```
[Webview] user presses 's'
   ↓ tauri::invoke("star_item", {id})
[carrel-app::commands] star_item(id) handler
   ↓
[carrel-app::core] core.star_item(id, true)
   ↓
[carrel-store] writes fact: [item_id, :starred, true, validity, sig?]
   ↓
[carrel-sync] bridge observes the write, mirrors to appropriate iroh-doc
   ↓ (for self-only facts: doc:self; for shared facts: doc:public or audience doc)
[carrel-app::events] emit("fact-changed", {kind: "star", entity: id})
   ↓
[Webview] subscribed components refetch or update local signals
```

The principle: a fact is written exactly once at the source of truth (Cozo), and all other consumers learn about it via emitted events. The bridge to Iroh is asynchronous and observational; the user's action does not block on sync.

### 2. Fetching and ingesting a feed

```
[carrel-app::background::feed_loop] tick
   ↓ query Cozo for feeds due for fetch
[carrel-store] returns due feeds
   ↓
[carrel-feeds] fetch_feed(url, etag, last_modified)
   ↓ HTTP with conditional headers
[carrel-feeds] parse with feed-rs → list of entries
   ↓ for each new entry:
[carrel-feeds] extract readable content (Rust readability, fallback to trafilatura)
   ↓ sanitize HTML, rewrite image URLs to local blobs
[carrel-sync::blobs] cache article HTML and images as iroh-blobs
   ↓
[carrel-store] write item facts + identifier facts + content reference
   ↓
[carrel-app::events] emit("fact-changed", {kind: "item", entity: id})
   ↓
[Webview] "Today" view updates
```

Every step is restartable. If we crash mid-fetch, the next tick re-fetches (the conditional headers prevent re-downloading unchanged content). If we crash mid-extract, the entry isn't yet in Cozo, so we just retry. Idempotency is achieved by deduplication on stable identifiers (feed GUID, URL, content hash) at the Cozo write layer.

### 3. Sync with a peer

```
[carrel-app::background::sync_loop] for each followed peer:
   ↓
[carrel-sync] open or reuse iroh-net connection to peer's NodeId
   ↓
[carrel-sync] subscribe to peer's relevant docs (public, granted-audience docs)
   ↓ iroh-doc range reconciliation runs in the background
[carrel-sync::bridge] new entries arrive from peer
   ↓ for each:
[carrel-sync::bridge] verify signature against peer's known pubkey
   ↓ check fact type is acceptable from peers (e.g., shares yes, read-state no)
[carrel-store] write fact to local Cozo
   ↓ if fact references blobs we don't have:
[carrel-sync::blobs] fetch missing blobs from peer
   ↓
[carrel-app::events] emit("fact-changed", {...})
```

The sync layer is unconditionally trust-minimizing. Every fact arriving from a peer is signature-verified against that peer's pubkey before being written. Any fact type not on the "acceptable from peers" allow-list is dropped. Even from someone we follow, we only accept the facts we asked for.

### 4. Rendering an article in the reading view

```
[Webview] user navigates to /items/{id}
[Leptos] route component mounts, calls use_query("get_item_detail", {id})
   ↓ tauri::invoke
[carrel-app::commands] get_item_detail(id)
   ↓
[carrel-store] Cozo query: item + identifiers + content reference + tags + highlights
   ↓
[carrel-app::commands] for content reference, fetch blob bytes from iroh-blobs
   ↓ return ItemDetail struct (item metadata + readable HTML + highlights)
[Webview] renders ItemHeader + ItemBody + ItemFooter
   ↓ ItemBody applies highlights inline by walking the DOM and matching position selectors
   ↓ subscribes to "fact-changed" events filtered to this item, refetches on relevant changes
```

The latency budget for this flow is sub-frame for warm content and "as fast as the disk read" for cold content. There should never be a spinner.

### 5. Publishing a public share

```
[Webview] user shares an item with audience set including "public"
   ↓
[carrel-store] writes share fact with audience: ["public", ...]
   ↓
[carrel-app::events] emit("fact-changed", {kind: "share"})
   ↓
[carrel-app::background::publisher] observes the event (debounced)
   ↓ queries Cozo for all current public shares
[carrel-app::publisher] generates JSON Feed + Atom feed from shares
   ↓ writes to user-configured output path (a folder in a git repo, an rsync target, etc.)
[user's existing publishing pipeline] picks up the file change
   ↓ syncs/deploys/commits as the user has configured externally
```

The publisher writes a complete feed file each time, not deltas. Feed files are small (kilobytes), and the simplicity is worth more than the savings. The user's existing tooling (cron jobs that push to git, automatic deploys, rsync watchers) handles distribution.

## Identity and trust

Identity in Carrel is a single ed25519 keypair. The public key is the user's stable handle on the network. The private key never leaves the user's devices.

Each device the user owns generates a sub-key derived from a master keypair (or shares the same keypair, depending on backup choices the user makes). All sub-keys count as "you" for the purposes of self-sync. The user can revoke a device by removing its sub-key from their authorized list.

The master key is persisted as a passphrase-encrypted key file: Argon2id derives a local encryption key and XChaCha20-Poly1305 encrypts the Ed25519 seed. Device keys are persisted in plain local JSON in v1 because they are used continuously and the device filesystem is the trust boundary. The master key is the recovery secret; the device key is an operational sub-key.

When user A follows user B:
1. A obtains B's "card": a signed JSON object containing B's pubkey, NodeId(s), display name, the doc IDs they publish to, and B's signature over the contents.
2. A verifies the signature.
3. A stores B as a known peer in the local Cozo store.
4. A's sync layer connects to B's NodeId and subscribes to B's `doc:public` (and any audience docs B has granted A access to via capability tokens).
5. Facts begin flowing one-way from B to A.

Cards are exchanged out of band: pasted as URLs, scanned as QR codes, or imported from a canonical directory site (which is just a static aggregation of self-hosted card files). The directory has no authority; it's a phone book, not a phone company.

Trust is never inferred. There is no "friend of friend," no automatic trust propagation, no global graph. If user A wants to follow user C because B follows C, A must explicitly do so.

## The local store

Carrel uses Cozo (with the RocksDB backend) as its single source of truth for all facts. Cozo was chosen for three reasons: it's embedded (no separate process), it speaks real Datalog (recursive queries, which we use for connection graphs and trust paths), and it has built-in `Validity` for time-traveling fact semantics, which is how we get "latest fact wins" without rolling our own.

The schema is documented separately in `SCHEMA.md`. The principle here is just that Cozo is *the* store. Other components do not maintain their own caches or shadow databases of facts. If a piece of state matters to the application, it lives in Cozo.

The exceptions, deliberately scoped:

- **Iroh-blobs** for bulky payloads: cached article HTML, images, EPUBs, PDFs, transcripts. These are content-addressed, so their hashes appear in Cozo facts but their bodies live in the blob store.
- **Iroh-docs** for the sync layer: a mirror of the *shareable subset* of facts, reformatted for sync. The bridge keeps these in sync with Cozo. Cozo remains authoritative.
- **Webview state**: ephemeral UI state (cursor position, what's selected, what's expanded) lives in Leptos signals and is not persisted across app launches except where explicitly designed to be (e.g., last-read position per item, which *is* a fact and lives in Cozo).

Migrations are handled via versioned schema. Breaking changes write a new schema version with a migration step that reads from old relations and writes to new. We never silently mutate user data.

## The sync bridge

The sync bridge is the most architecturally subtle part of Carrel and deserves explicit treatment.

The problem: Cozo is a query store; iroh-docs is a sync store. They have different shapes. Cozo wants typed relations and rich queries. Iroh-docs wants opaque key-value entries that can be range-reconciled efficiently. Neither is a substitute for the other.

The solution: a thin bridge layer that mirrors *outgoing* shareable facts from Cozo into the appropriate iroh-doc, and *incoming* facts from iroh-docs into Cozo, with verification.

The bridge maintains a small set of iroh-docs per user:

- `doc:self` — private state, syncs only between the user's own devices. Read-state, private tags, drafts, configuration.
- `doc:public` — facts the user shares with all followers. Their public shares, public highlights, public reactions.
- `doc:audience:{id}` — facts shared with a specific audience. One per audience.

Each fact, when written to Cozo, is examined for shareability. If the fact is shareable, it's also written to the appropriate iroh-doc with a proper signature. If the fact is local-only, it stays in Cozo.

When facts arrive from a peer's iroh-doc, the bridge:

1. Verifies the cryptographic signature against the claimed author's pubkey.
2. Checks that the user is authorized to subscribe to this doc (capability check).
3. Checks that the fact type is on the "acceptable from peers" allow-list.
4. Writes the fact to Cozo, attributing it to the remote author.
5. Schedules any referenced blobs for fetching.

The "acceptable from peers" allow-list is critical. Some facts only make sense locally (your own read-state, your own tags). The allow-list enforces this: even if a peer somehow pushed a "you starred this" fact into a doc you read, the bridge would refuse to write it.

A note on garbage collection: facts accumulate. We will eventually need a compaction strategy that keeps "current" facts plus a configurable history window. This is not implemented in v1 but the design accommodates it: iroh-docs supports prefix deletion and recreation, and Cozo's Validity model lets us identify "current" trivially.

## Frontend

The frontend is Leptos compiled to WASM, running in Tauri's webview. The decision to go all-Rust (rather than a JS frontend) is documented in ADR-0003.

The frontend has three layers:

1. **The data layer**: a `use_query` primitive that wraps a Tauri command + event invalidation pattern. Components declare what data they need; the data layer handles fetching, caching, and reactive invalidation.
2. **The keymap layer**: a stack-based keymap system where components register their own bindings on mount and they pop off on unmount. The top of the stack wins; misses fall through. Documented further in `DESIGN.md`.
3. **The view layer**: pure rendering, with optimistic mutations for actions that should feel instant (star, mark read, highlight).

Leptos signals are the reactive primitive. Server-side state (Cozo facts) is reflected into client-side resources via `use_query`. Local UI state (cursor, selection, expanded panes) lives in component-scoped signals.

The frontend never does business logic. It composes commands and renders results. This is enforced by code review, not by the type system, and is the most important UI architectural rule we have.

## Mobile

Carrel is desktop-first in v1. Mobile is a v2 commitment, not v1, but the architecture is designed so adding mobile is purely additive rather than requiring a rewrite. This section documents the architectural assumptions that keep that path open.

The honest constraint mobile imposes: **iOS in particular is structurally hostile to long-running peer-to-peer nodes in the background.** A Carrel mobile app cannot maintain QUIC connections, accept incoming sync, or be reachable for peers indefinitely the way a desktop node can. Android is more permissive but battery and OS optimization make "always-on peer" still a poor fit.

The architectural pattern that resolves this is the **foreground peer**: mobile Carrel acts as a full peer, but only when the user has the app open. When the app foregrounds, it connects to followed peers and to the user's other devices, syncs, and stays current while open. When the app backgrounds, sync stops; writes the user makes offline queue locally and propagate next time the app is open.

This works because reading is a deliberate activity. A 5-15 second sync at app launch is acceptable cost for the architectural simplicity. Most users open their reading app when they want to read, not because it pinged them; the foreground-peer model fits this naturally.

For users who want their mobile to feel "always synced," the v2 path is a **personal always-on node**: the user's desktop, a home server, or a small device they own runs Carrel as a 24/7 peer. Mobile then syncs primarily with that node (which is part of their identity) rather than with each followed peer directly. This preserves the no-servers-we-run principle: the always-on node is the user's own infrastructure.

What this means for the v1 architecture:

- The `carrel-core`, `carrel-store`, `carrel-feeds`, and `carrel-sync` crates must compile for mobile targets (iOS via the Apple toolchain, Android via NDK). Iroh and Cozo both support these targets.
- These crates must not assume desktop characteristics: not always-online, not large disk, not privileged network access, not unlimited compute. A peer that connects briefly, syncs a delta, and disconnects must be a first-class case, not a degraded one.
- The Tauri shell is platform-specific. `carrel-app` is the desktop shell; `carrel-mobile` (when it exists) is the mobile shell. They share the engine but have different UI shells, different input models (keyboard vs touch), different navigation patterns.
- A future `carrel-ui` crate can hold genuinely shared Leptos components (item rendering, typography, highlight markup) that are pure functions of structured data. Page-level layouts and input handling diverge by platform.

Identity across devices uses the master keypair + device sub-key pattern documented in "Identity and trust." Mobile sign-in is QR-code provisioning from an existing device, or generating a new master keypair on the mobile (becoming the user's root device). The same `doc:self` syncs between all devices tied to a single master key; peers see facts attributed to the user regardless of which device originated them.

The discipline to maintain right now, while building v1, is the layering rule: **nothing in the data, sync, or feeds layers may assume desktop-only context.** If a piece of code wants "always running" semantics, that's a sign it belongs in a worker that the platform-specific shell controls, not in the engine.

## Background workers

The Tauri app launches several long-running async tasks at startup:

- **Feed fetcher**: every 60 seconds, queries Cozo for feeds due for fetch, fetches them politely (per-host concurrency limits, conditional headers, backoff on failures), and ingests new items.
- **Sync orchestrator**: maintains iroh-net connections to followed peers, runs sync loops for each subscribed doc, handles reconnection on network changes.
- **Blob fetcher**: services queued blob requests (incoming references that we don't have local content for).
- **OPDS server**: a tiny HTTP server bound to a configurable address (default localhost) serving the user's reading queue as an OPDS catalog for ereader devices.
- **Public publisher**: watches for facts marked as belonging to the special `public` audience and emits them as a JSON Feed (and Atom feed) to a user-configured output path. See "Public syndication" below.
- **Compactor** (future): periodic schema-aware compaction of Cozo and iroh-doc storage.

Each worker is a `tokio::task` that owns its own state and communicates with the rest of the system via the shared `carrel-store`. They do not share mutable state directly; they share the database.

## Public syndication

Carrel has two distinct output channels for shared facts: peer-to-peer sync (via iroh-docs to followers) and public syndication (via standard feed formats to a published file).

The trust model treats `public` as a special audience that any Carrel node can compute against without authorization. Facts shared to the `public` audience flow through a different path: rather than being signed and published to an iroh-doc, they are emitted as entries in a JSON Feed (and Atom feed, for compatibility) that the user publishes via their own infrastructure.

The publisher worker watches the local Cozo store for facts in the public audience and (re)generates the feed file on a debounced schedule. The output path is configured by the user: a folder in a git repository, an rsync target, a Dropbox-synced folder, anywhere the user already publishes static content. Carrel does not host anything; it produces files.

The deliberate consequences of this design:

- **Anyone with an RSS reader** — Carrel or otherwise — can subscribe to a user's public shares. This is the same protocol every blog uses; we add no new requirements on the consumer.
- **Other Carrel users discover via the open web**: a user can post their public feed URL on their blog, on Mastodon, in a directory, anywhere. Subscribers find them via the same mechanism they find any feed. There's no key exchange, no card import, no social handshake.
- **Public sharing is opt-in per share**: a share's audience set is its own piece of metadata. A user can share to "tech friends" privately and to "public" simultaneously, or to either alone, or to neither (a private save). The choice is per-action.
- **The static file is the API**: anyone can build on top of the JSON Feed. A blog widget that shows recent shares. An aggregator. A search engine over a known set of public feeds. The data is open; the tooling is downstream.

Anti-pattern: routing public shares through any infrastructure Carrel runs. The publisher writes to a path the user controls, and that's where Carrel's responsibility ends. Distribution is the user's problem and uses their existing infrastructure.

## Cross-cutting concerns

### Errors

We use `thiserror` for typed errors at crate boundaries and `anyhow` only at the application's outermost layer (Tauri command handlers). A typed error includes enough context to be actionable; the outer layer translates into user-visible messages.

The UI never shows raw error chains. The "system status" panel surfaces structured error information for users who want to look. Errors from background workers are logged but never modal.

Anti-pattern: using `anyhow` inside library crates. The cost of the typed-error discipline is real (more enum variants), but the benefit is that consumers can pattern-match on failure modes and decide.

### Logging

We use `tracing` throughout. Spans are used for operations that span async boundaries (fetching a feed, syncing with a peer, handling a command). Log levels:

- `error`: something the user might need to know about
- `warn`: something the developer should investigate eventually
- `info`: significant lifecycle events (started, connected, ingested N items)
- `debug`: enough detail to reconstruct what happened in a session
- `trace`: too much for normal operation

In development, all levels go to stderr. In production, logs go to a per-session file with rotation; users can find them via the "system status" panel.

We never log fact contents. We log fact metadata (kind, id, length) but not bodies, because logs may be shared during debugging and we treat user data as confidential by default.

### Async runtime

Tokio is the runtime. The Tauri app launches it explicitly. All I/O goes through async APIs. Synchronous work that takes more than a few milliseconds runs on `tokio::task::spawn_blocking` to avoid blocking the runtime threads.

### Configuration

User configuration lives in two places:

- **The keymap and theme settings** in `~/.config/carrel/` (or platform-equivalent), as TOML files the user can edit directly. We watch them for changes and reload.
- **Application state** (peers, audiences, fetch intervals, device configurations) in Cozo, edited through the UI.

This split exists because keymaps and themes are *texts the user maintains*, like a `.vimrc`; everything else is *state the app maintains for the user*. Texts go in files; state goes in the database.

### Testing

We use three kinds of tests:

- **Unit tests** for pure logic in `carrel-core` and parsing/extraction in `carrel-feeds`. These run in milliseconds; we have hundreds of them.
- **Integration tests** for `carrel-store` and `carrel-sync` against in-memory or temp-directory backends. These run in seconds; we have tens of them.
- **Multi-instance tests** for `carrel-sync` that spin up two or more in-process peers and verify sync semantics. These are slower but irreplaceable for verifying convergence properties.

We do not have UI integration tests in v1. This is a deliberate choice: UI tests are expensive to write and fragile, and we get most of the value from a well-tested core. We may add them later via Playwright against a real Tauri build.

Property-based tests (via `proptest`) are used heavily in `carrel-core` to verify monotonicity invariants of the fact model.

### Build and deployment

`cargo build --release` produces a Tauri binary. We sign and distribute via:

- **macOS**: notarized .dmg via Apple Developer credentials.
- **Linux**: AppImage and Flatpak.
- **Windows**: signed .msi via Authenticode (later).

Continuous builds run on every push; releases are tagged and built reproducibly. We do not auto-update silently; updates are user-initiated.

## Anti-patterns

These are the architectural mistakes that would erode the system. Each is here because it's tempting and we've decided against it:

**Putting business logic in the webview.** The webview renders state and dispatches commands. It does not query, validate, transform, or compute. If a piece of logic appears in Leptos that should run in Rust, move it to Rust and call it via a command.

**Bypassing the bridge for sync.** Code outside `carrel-sync` does not write directly to iroh-docs. The bridge is the single point where Cozo facts become syncable entries.

**Storing state outside Cozo.** With the deliberate exceptions noted above, state goes in Cozo. Side files, in-memory caches, JSON dumps — these are all signs of a missing relation or a missed schema design.

**Adding a server.** Carrel is local-first and peer-to-peer. If we ever find ourselves designing a service that has to run for the network to function, we have made a wrong turn and need to back up.

**Adding analytics.** No telemetry. Not "anonymous" telemetry. Not "opt-in" telemetry. Crash reports require explicit user action and are sent only when the user clicks a button.

**Making the UI block on network.** The UI is responsive to local state, always. Network operations happen in the background and the UI updates when their results arrive. Spinners are forbidden for any operation that touches only local data.

**Using global mutable state.** Each background worker owns its own state. They communicate via the database (for facts) and channels (for control flow). No `lazy_static!{}` mutable singletons.

**Adding a feature without updating docs.** If a change adds a new fact type, `SCHEMA.md` is part of the change. If it changes UX, `DESIGN.md` is part of the change. If it changes architecture, this document is part of the change.

**Conflating public syndication with private sync.** Public publishing emits to user-controlled files via standard feed formats; private sync uses signed iroh-doc replication between authorized peers. These are different transports for different trust models and they should not borrow from each other. A "public" share never gets pushed via iroh; a "private" share never lands in a JSON Feed file. The audience set on a share determines which channel(s) it flows through.

## Tradeoffs we've accepted

Honest accounting of what's hard about this architecture, so future-us doesn't get confused.

**The Rust compile-time tax.** All-Rust means slower builds than a JS frontend would have. We accept this for the type-safety and single-language benefits. Mitigations: split crates, sccache, watch-mode for dev.

**The two-store cognitive load.** Cozo is the source of truth; iroh-docs is the sync mirror. Bridging them is real work and the mental model has to be maintained. We accept this because the alternatives (using only iroh-docs, or rolling our own sync over Cozo) are worse.

**Cold-start sync cost.** Following someone with years of history means downloading all their historical facts. v1 does this naively; v2 will scope by time window. Accepted because we're starting with users we know and small histories.

**No mobile in v1, by design.** P2P on mobile is genuinely hard for OS-policy reasons (background restrictions, especially on iOS). We accept this in v1 and design the layer boundaries so that adding a mobile shell is purely additive. See "Mobile" above for the architectural commitments that keep that path open.

**Garbage collection deferred.** Facts grow unbounded. We accept this for v1 because at our scale (one user, hundreds of feeds, thousands of items per year) it doesn't matter for years.

**Identity recovery is unforgiving.** Lose your private key, lose your identity. We accept this and mitigate via passphrase-encrypted backups, optional Shamir splitting, and cross-device sync. We do not add a recovery server.

## What we don't know yet

Documenting our uncertainty so we can revisit:

- Whether iroh-docs' performance characteristics will hold at the scale we eventually want (tens of thousands of facts, dozens of peers). We have benchmarks for "small" cases. The first time we hit a wall, we'll know.
- Whether the two-store bridge will become a bottleneck or a source of bugs. So far the bridge is small (low hundreds of lines) and idempotent. If it grows to thousands of lines, that's a sign we've made it do too much.
- Whether the "facts only" model handles every product case we care about. So far it handles all cases we've designed for, but reality has a way of surfacing exceptions. If we find a case that doesn't fit, we revisit before papering over.
- Whether iroh-willow (the partial-sync evolution) will be ready when we need it. If yes, we migrate. If no, we segment iroh-docs by time window manually.

When any of these resolve, this section gets updated.

## A note on changing this document

If you're reading this and you're about to make an architectural change, the discipline is:

1. Read this document. If your change conflicts with what's here, that's a real decision, not a refactor.
2. If it's a real decision, write an ADR documenting what you're changing and why.
3. Update this document in the same change as the code.
4. If you're unsure whether your change is "architectural," err toward writing it down. The cost of a small ADR is trivial; the cost of architectural drift is high.

We treat this document as a living artifact. It is allowed to change — but only deliberately.
