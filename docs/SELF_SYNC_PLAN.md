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
are complete; two-device self-sync remains active and unimplemented.

## Decisions already made

- Older articles extend the current list on demand with **Load older**. There
  is no automatic infinite scrolling and no article-count badge.
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

## Foundations to resolve before the sync milestone

The sync crate is currently a stub. The schema uses `Validity` values for
several mutable relations rather than historical validity keys; replication
must not assume that ordinary `:put` operations resolve concurrent writes.
Document and migrate the required history/merge semantics before implementing
them. Subscription settings must be distinguished from device-local HTTP
fetch state. Device authorization and revocation already have signed core
types; pairing must use those boundaries.

Verify the current compatible Iroh/docs/blobs release family when that
milestone begins. Keep the library audit brief and prefer existing small
dependencies for earlier milestones.
