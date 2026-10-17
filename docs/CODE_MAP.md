# Code map

Use this map to locate an invariant before editing it. The linked modules and
tests are authoritative for behavior; this is a navigation aid.

## Follow a reading action

1. `carrel-app/frontend/src/routes/` owns route lifetime, selection and queries.
   `components/` renders controls. `api.rs` contains the native command boundary.
2. `carrel-app/src/commands/` composes native work and display projections.
   It delegates durable mutations and validation to the engine crates.
3. `carrel-store` commits source facts, projections and durable outbox work
   together. `carrel-core` owns signatures and conflict resolution rules.
4. `carrel-sync::bridge` mirrors verified portable facts. Cozo remains the source
   of truth. Incoming peer data never becomes an arbitrary database query.
5. App-owned observation refreshes mounted views. Frontend `navigation.rs`,
   `progress.rs` and `writes.rs` preserve location and queued local intent.

## Important boundaries

Paths below are relative to `crates/`. Integration tests live in each crate's
`tests/` directory; protocol tests also use sibling `tests.rs` modules.

| Behavior | Implementation | Regression coverage |
| --- | --- | --- |
| URL identity / source metadata | Core `article.rs`; feeds `article.rs` | Core unit tests, feeds parsing/discovery |
| Polite and bounded HTTP | Feeds `fetch.rs`, `politeness.rs` | Feeds `tests/fetch.rs`, fetch unit tests |
| Cached HTML / images | Feeds `readability/`; store `blobs.rs` | Readability, blob integrity and app render-boundary tests |
| Archive filters / cursors / search | Store `library.rs`, `search.rs` | Store library tests; app item-projection tests |
| OPML / source folders | Feeds `opml.rs`; store `feeds.rs`; app `subscriptions.rs` | OPML, subscription and command tests |
| Note branches / explicit intent | Core `annotation/note.rs`, `private_state.rs`; store `notes.rs`, `private_state.rs` | Property tests, store notes/state tests |
| Identity / key files | Core `identity/`; store `keystore.rs`, `identity.rs` | Keystore/identity tests and fixed compatibility fixture |
| Private device authorization | Sync `authorization.rs`, `transport.rs`, `pairing.rs` | Real-peer transport/pairing, revocation and restart tests |
| Friend audience isolation | Core / store / sync `sharing.rs` | Adversarial/property tests, receipts and three-peer exchange |
| Close / pending writes | App `close.rs`; frontend `progress.rs`, `writes.rs`, `requests.rs` | Unit tests and bounded native/browser review |
| Keyboard / modal focus | Core `keymap/`; frontend `keymap/`, `components/modal.rs` | Keymap unit tests and actual webview review |

Test key seeds and encrypted compatibility fixtures are deterministic test
material, not developer identities.

## Ownership rules

Dependencies flow down the workspace graph. Core has no workspace dependencies;
its OS-random key-generation convenience is isolated from pure signing and
resolution operations. Store knows no Tauri or Iroh APIs. Only sync owns Iroh
writes and incoming verification. The webview cannot query Cozo directly.

Store clones share mutation locks, not independent snapshots. Compound writes
acquire private-state, annotation, identity and social locks in their existing
order, omitting locks they do not need. Do not acquire an earlier lock while
holding a later one or await network I/O while holding a store lock. The
transaction, not a wake-up event or UI signal, is the durable record.

Native and frontend command DTOs currently have separate serde declarations.
Field changes must be checked on both sides and in the real CSR build. Browser
mocks validate interactions, not the native wire implementation.

## Checks and evidence

`just check` runs formatting, native all-target Clippy, workspace tests and
CSR/WASM Clippy. `cargo doc --workspace --no-deps --locked` checks Rustdoc.
An actual Trunk CSR build validates the desktop webview build; native tests
deliberately use Leptos SSR.

In-process peers establish transport/convergence properties, not successful
pairing between physical computers. Mocked-native browser review establishes
DOM interactions, not live Tauri file dialogs or Gatekeeper installation.
Use the [friend trial](PRIVATE_SHARING_PLAN.md#first-friend-trial) for that evidence.
