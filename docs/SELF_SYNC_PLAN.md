# From the reading loop to self-sync

The next goal is a working reading library on two devices belonging to one
person. We will complete the local workflows before building their replication.
The editorial layout, four destinations, and collapsible reader sidebar remain
the product baseline. Sharing with other people follows this goal.

## Sequence and acceptance checks

1. **Complete library.** Filters operate on the whole archive before limiting
   results. Lists use stable cursor pagination and an explicit **Load older**
   button. Opening an article and returning preserves the loaded archive,
   selection, and scroll position. Joined store queries replace per-row queries.
   OPML import/export preserves subscription titles and folders, avoids duplicate
   subscriptions, validates input before writing, and leaves saved articles alone.
2. **Local search.** Search the local article library, including cached text,
   with understandable matching and the same list/reader navigation. Search
   works offline and respects list filters. Empty, Unicode, and large archives
   have regression coverage. No remote search or recommendations.
3. **Annotation review.** Review highlights and their notes, edit notes, remove
   highlights deliberately, and export annotations in an open format. Keep
   original article references and quoted text. Test signature, deletion,
   persistence, and export invariants before replicating these facts.
4. **Two-device self-sync.** Pair two installations under one identity with
   distinct authorized device keys. Replicate subscriptions, saved items and
   cached reading content, reading positions/states, stars, and annotations.
   Test initial transfer, offline changes, reconnection, restart, concurrent
   edits, deletion, device revocation, forged facts, and blob integrity using
   independent in-process peers and a native smoke test. Both devices must
   converge without exposing private facts to a social peer.

Each milestone includes relevant documentation, meaningful tests, workspace
checks, and a reviewable commit. The goal is complete only when the final
two-device flow is implemented and verified; a transport demo is insufficient.

## Progress

The complete-library milestone now has whole-archive filtering, explicit
pagination, joined store reads scoped to each selected page, and atomic OPML
import/export. Archive and interchange regression coverage includes large
archives, tied timestamps, all filters, deletion, malformed/oversized input,
Unicode/XML escaping, and repeated imports. Native archive navigation was
checked with 250 isolated articles. The native import picker opens; automated
file selection requires macOS assistive access, which was unavailable. Parser,
filesystem, and store paths are covered independently. The milestone passes
`just check` with 188 tests, a full Trunk CSR build, and a native build without
the temporary inspection driver.

Local article search now covers titles and cached readable text within the
current list filter, with literal case-insensitive word prefixes and the same
ordering and explicit pagination. Query-scoped cursors and route URLs preserve
the query and loaded results on reader return. Migration 2 indexes existing
titles; the native worker rebuilds available older blobs with a quiet status
and retry path. Derived text stays local and is checked against its source
blob. A Cozo 0.7.6 index-update workaround has replacement regression coverage.
`just check` passes 199 tests, native and CSR lint, and formatting; both full
builds pass without the temporary inspection driver. Native checks cover
100-to-250 searched results, article return with selection and scroll intact,
Unicode cached-body matching in the oldest unread article, filter retention,
empty-result recovery, and the search keymap.

Annotation review now pages through the complete highlight archive, with every
current note and joined article context. Notes expand in place, can be edited
against a specific revision, and retain their drafts after failures. Source
return restores loaded pages, expanded notes, selection, focus, and scroll;
deep links wait for the annotation query before focusing. Highlight removal
uses signed permanent tombstones and hides attached notes. Failed removals
restore the row and its selection if the user has not moved away. Markdown
export reads a complete database snapshot, includes full notes and original
article references, escapes source text, and replaces files atomically.

Migration 3 preserves legacy signatures and resumes interrupted migration
stages. Note revisions preserve identity and immutable metadata; local edits
serialize across store clones, detect stale editors, and advance despite a
backward clock. Tests cover all note targets, unauthorized/tampered writes,
concurrent local edits, removal dominance, persistence, archive pagination
beyond 500 highlights, full exports, and failure recovery. Native checks used
250 isolated highlights and exercised full-note editing, real stale conflicts,
storage failure rollback, 100-to-250 browsing, exact source return, and partial
note creation failure/retry without a duplicate highlight. Native file-dialog
selection remains a manual check because macOS assistive access was unavailable;
export formatting and filesystem paths are covered independently.
`just check` passes 216 tests, formatting, native and CSR lint. Both full builds
pass with the temporary inspection dependency removed. The local milestones
are complete; the implemented self-sync flow is described below.

The self-sync identity foundation now binds one master and the installation's
existing independent device key in a single transaction. Verified certificates
are immutable; signed revocations deny a device permanently, even before its
certificate arrives or after certificate replay. Migration 4 retains legacy
rows, verifies their signed fields, and blocks old unproven revocations locally
without fabricating a signed proof. It never chooses an identity automatically.
Certified local devices can alter their identity's annotations, including work
created by a now-revoked device; revoked installations cannot make new writes.
Private key-file creation is atomic and refuses replacement, including concurrent
first use. Existing encrypted formats and signatures remain compatible.
`just check` passes 236 tests, formatting, and native/CSR lint. Both full app
builds pass. Coverage includes competing bindings across store clones, key-file
permissions and no-clobber races, all interrupted migration relation stages,
foreign/forged proofs, future timestamps, restart, permanent revocation under
arbitrary replay, and certified annotation ownership in native command results.

Schema 5 now stores content-addressed, signed note revisions with explicit
parents. Equal-time offline drafts remain separate heads until the user chooses
or combines them. An exact reviewed-head check prevents a newly arrived branch
from disappearing during resolution. Legacy signatures and complete history
survive every interrupted migration stage; permanent tombstones still dominate.
Verified private revisions wait durably for missing parents or article/highlight
targets, and promotion selects ready work before limiting the batch. Extreme
display clocks cannot block an edit. Timestamp comparisons no longer decide
which note the UI presents as current.

Highlights provides quiet conflict review with every complete version, device
label, and date. Stale recovery fetches the latest versions directly and retains
the editor draft; failed saves can be retried. Optimistic projections respect
new remote branches, and Markdown exports include every unresolved full body
and revision reference. Read-only devices can inspect versions without write
controls. Native QA exercised a third arriving draft, fresh review, failed-save
retry, explicit resolution, and read-only review on isolated fixture data.
`just check` passes 258 tests, formatting, and native/CSR lint. Full native and
CSR builds pass with temporary inspection dependencies and commands removed.

Schema 6 now journals signed read intent, position, star, and subscription actions
with explicit parents and a durable mirror queue. Equal-time or reversed-clock
branches follow the selected conflict policies; an action observing all heads
can reverse them. Reading and Starred query rows, including ordering timestamps,
converge independently of delivery order. Repeated scrolling never becomes an
explicit completion intent. Generic legacy cursors and negative states survive
local adoption, and rejected cursors retain the original data. Migration only
adds resumable relations/guards and never invents legacy signatures.

Native reading controls, feed actions, OPML import, and CLI subscription actions
use the journal. Imports remain atomic and preserve existing settings. HTTP
cache metadata stays local, chosen feed titles survive refresh, and late fetches
cannot resurrect removed membership. Verified deliveries wait durably for
parents/items, ready work cannot be starved by a blocked queue prefix, and mirror
acknowledgement removes only the exact revision. Separate-store tests cover all
delivery orders, duplicate replay, restart, clock boundaries, explicit reversal,
revocation/forwarding, forgery, raw immutability, and transactional failure.
`just check` passes 279 tests, formatting, and native/CSR lint. Full native and
CSR builds pass.

Legacy note portability now omits unsigned migration links from private wire
records and current-head reads. Partial copies recognize identical signed
versions without ancestry collisions or missing-parent waits. Signed legacy
times retain their original ordering within versions 1/2, preserving equal-time
drafts; neither those times nor unsigned links can hide a modern version 3 draft.
Migration 7 resumes normalization of older staging rows without changing stored
history or signatures. Regression coverage includes partial-copy edits, all
legacy delivery orders, extreme clocks, unsigned replacement claims, explicit
resolution, and interrupted/invalid migration.
`just check` passes 284 tests, formatting, and native/CSR lint; both full app
builds pass.

The private protocol host now uses persistent Iroh docs/blobs with the existing
certified operational key as its endpoint and document author. Shared endpoint
hooks cover incoming and outgoing docs, blobs, and gossip connections; typed
identity changes wake a recheck that closes unauthorized live connections.
Authorization query failures close access and surface an error. Cache setup
protects the dedicated directory and cleans up partially opened stores on failure.
Independent local Iroh endpoints verify actual bidirectional document/blob
transfer, reopened content and subsequent writes after reconnect, refusal of an
outsider's stolen write capability and known blob hash, all private ALPNs,
permanent revocation of live connections, and local revocation/startup recovery.
The host now underlies the private fact bridge and native pairing flow described below.
`just check` passes 293 tests, formatting, and native/CSR lint; full native and
CSR builds pass. Transport tests use independent endpoints on one machine;
physical two-machine checks remain a manual follow-up.

The two-device milestone now has a durable typed fact bridge, verified reading
cache transfer, and a Devices dialog with one-time copy/paste pairing. Master
seeds remain on their originating desktop; each installation keeps its original
independent operational key. Atomic recipient setup prevents partial binding to
an unrelated document after a crash. Device removal closes live private access.

Independent endpoint tests exercise article metadata, native cached HTML and
images, subscriptions, reading intent, stars, highlights and original notes.
Offline edits preserve both note drafts. Signed removals hide highlights and
attached notes and remove subscription membership. Invalid signatures, unknown
SDK authors, same-key shadowing, oversized headers, incorrect content lengths,
corrupt cached bytes and revocations are rejected. Cozo retains original fact
history, exact outbox acknowledgements and pending dependency/blob work. The
same document can be reconstructed from its local binding after mirror loss.

The native backend smoke test uses two isolated persistent app profiles and the
actual pairing and background-worker code. It verifies distinct retained device
keys, cache transfer, live invalidation, restart and further star changes without
a master file on the recipient. It caught a reconnect gap: the worker now retries
SDK reconciliation while gossip rebuilds. Store tests cover atomic setup rollback,
immutable document binding, pre-certificate revocation, resumed migration stages
and rejection of cross-article ancestry without starving valid work.

Workspace formatting, native and CSR lint, and all 319 tests pass. Full native and
Trunk CSR builds pass. The planned local milestones and two-instance self-sync
flow are implemented and verified. Physical two-machine networking and a manual
pass over the new Devices dialog remain useful release checks; the in-process
and native backend tests do not claim to verify those environments.

To try it, run `just dev` and open **Devices** in the sidebar. Choose **Set up
sync**, enter a desktop name and confirm a recovery passphrase. Then choose
**Pair another desktop**, unlock the recovery key and copy the invitation.
On the other installation, open **Devices → Use an invitation**, paste it and
name that desktop. Keep the invitation window open until pairing finishes.
Ordinary reading and subsequent sync do not require the passphrase. The desktop
holding the encrypted master can remove another desktop from this same dialog.

## Decisions already made

- Older articles extend the current list on demand with **Load older**. There
  is no automatic infinite scrolling and no article-count badge.
- OPML adds new subscriptions and skips duplicates, preserving existing titles,
  folders, and subscription settings.
- Search stays within the current article view and carries its query across
  Library filters. Titles and cached text are searched locally without a
  relevance cutoff or remote service.
- Annotation exports use Markdown, with complete quotes, full notes, dates,
  article links, and stable references. The export is independent of loaded pages.
- Cozo is authoritative. `carrel-sync` alone owns the Iroh mirror and incoming
  verification. Local writes remain useful while the other device is offline.
- Reading is explicit: opening, navigating, syncing, or reaching the end of
  text does not mark an article read.
- Keep scope to self-sync. Friends, social audiences, public publishing, and
  mobile shells are separate work.
- Concurrent offline note edits retain both versions, with a quiet **Review
  conflict** control. Resolution explicitly chooses or merges the retained drafts.
- Desktop pairing uses a one-time copy/paste invitation. QR pairing is deferred
  until it supports a useful mobile flow.
- Concurrent explicit read/unread actions keep the article **unread**. Concurrent
  star/unstar actions keep it **starred**. Concurrent subscription add/remove
  actions keep it **removed**. An explicit action after observing both branches
  supersedes the conflict; replaying an unchanged snapshot cannot undo it.

## Implementation boundaries

Private state uses signed causal history, so ordinary projection `:put` operations
never decide an offline conflict. Schema versions 4–8 retain original identity,
annotation, state and item proofs, mirror work and local transport configuration.
Portable subscriptions exclude device-local HTTP cache state. Incoming facts
have an explicit allow-list and pass typed verification; only an explicit local
setup action can choose the master. Friends, mobile pairing and master-key
backup/recovery UI are separate follow-up work.

The brief registry and current-documentation audit on 2026-10-10 found a
compatible family: [Iroh 1.3.0](https://crates.io/crates/iroh/1.3.0),
[iroh-docs 0.101.0](https://crates.io/crates/iroh-docs/0.101.0),
[iroh-blobs 0.103.1](https://crates.io/crates/iroh-blobs/0.103.1), and
[iroh-gossip 0.101.0](https://crates.io/crates/iroh-gossip/0.101.0).
The docs/gossip releases require Iroh `^1`; docs requires blobs `^0.103`.
All have Rust 1.91 as their minimum, within this workspace's requirement.
The later-dated blobs 0.97 maintenance release belongs to the older Iroh 0.95
family and is not the compatible choice. This family now powers the actual
private protocol host. Current APIs compose
Endpoint/Router with blobs, gossip, and docs; every private protocol needs the
bound-device authorization gate before accepting access.
