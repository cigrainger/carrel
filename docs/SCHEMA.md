# Schema

## Purpose of this document

This document is the source of truth for Carrel's data model. It describes the relations that live in Cozo, the invariants they enforce, the trust boundaries they cross, and the rules for evolving them over time.

Code and schema must agree. When they disagree, this document wins, and the code gets fixed in the same change. Schema changes start here, in writing, before they appear in the `:create` statements.

If you are about to make a change that touches the data model, read this document end-to-end first. The schema is small enough to hold in your head; doing so will make the change cleaner.

## Core concepts

### Entities and facts

Every piece of state in Carrel is an *entity* identified by a content-addressed ID, with *facts* attached to it. An entity could be an item (an article, a book, a paper), a highlight, a note, a peer, a share — anything we want to talk about and reason over.

IDs are 32-byte BLAKE3 hashes, hex-encoded. They are derived deterministically:

For external content (articles, books, papers): hash of the canonical identifier (URL with `?utm_*` and similar tracking parameters stripped, DOI, ISBN). This means two users encountering the same article via different feeds end up with the same item ID.

For user-created content (notes, highlights, shares): hash of (content + author public key + creation timestamp at second resolution). This makes IDs tamper-evident and unique without coordination.

Facts are tuples that say something about an entity. Most facts are append-only and superseded by later facts via Cozo's `Validity` type, which gives us "latest fact wins" semantics for free without rolling our own timestamp logic.

### Validity and time-travel

Cozo's `Validity` type is `[timestamp_microseconds, asserted_or_retracted]`. A fact written with `Validity::now()` becomes the current truth. A subsequent fact with a later `Validity` supersedes it. Querying without a time argument returns "as of now"; queries can time-travel by specifying a different validity.

This is the mechanism by which we get monotonic semantics for non-monotonic operations. Unstarring an item is not a delete; it's a new fact that says `[item, :starred, false, t]`. The starred-false fact supersedes the earlier starred-true fact when queried at the current time. The full history is preserved.

For the rare cases where we genuinely want to delete (a privacy concern, or a request to remove user-created content), we issue a *tombstone* fact and the application layer treats tombstoned entities as absent. We do not garbage-collect tombstoned data automatically; that's a deliberate compaction step, not implicit behavior.

```
:create entity_tombstone {
    id: String,
    tombstoned_at: Validity,
    =>
    reason: String?
}
```

Tombstones are entity-scoped. Once an entity ID is tombstoned, future writes for that entity ID are rejected where the store can enforce that cheaply, and read paths treat the entity as absent. The `reason` field is developer-facing context, not user-facing copy.

### Signatures and trust

Facts that may cross trust boundaries — that may be sent to or received from another peer — carry a cryptographic signature by the originating author. The bridge layer (`carrel-sync`) verifies every incoming signed fact against the claimed author's public key before writing it to Cozo.

Facts that are purely local (your read state, your private tags, your application configuration) do not carry signatures. The sync channel between your own devices is already authenticated; signing each fact would be wasted work.

The schema makes this explicit: signed-fact relations have a `signature` field; local-only relations don't. This is the load-bearing distinction in the schema and we treat it as inviolable.

### The peer allow-list

Not every fact type is acceptable from peers, even when properly signed. A peer cannot tell us our own read state; that fact only makes sense locally. The bridge layer maintains an explicit allow-list of fact types accepted from remote peers; everything not on the list is dropped at the bridge.

The allow-list is defined per-relation in this document, in the *Trust* annotation on each section.

## Identity and peers

```
:create peer {
    pubkey: Bytes,            ;; 32-byte ed25519 public key
    =>
    pet_name: String?,        ;; what *you* call them locally
    self_described_name: String?,  ;; what they call themselves
    is_self: Bool,            ;; true for your own keypairs
    added_at: Validity,
    last_seen: Validity?
}
```

The `peer` relation lists every public key your local instance knows about, including your own (with `is_self: true` for your master key and any device sub-keys you've authorized).

`pet_name` is the local label you assign — Petname-system style. `self_described_name` is what they put in their card. The two are separate because trust is local: a peer cannot rename themselves in your view of the world.

**Trust:** local-only. The peer table is per-instance state. New peers are added either via card import (out of band) or via self-sync from your other devices.

```
:create device_authorization {
    master_pubkey: Bytes,     ;; the user's master identity
    device_pubkey: Bytes,     ;; the device sub-key
    =>
    authorized_at: Validity,
    revoked_at: Validity?,
    device_name: String?,     ;; "Chris's phone", set by the user
    device_cert: Bytes,       ;; signed cert: master signs (device_pubkey + authorized_at + device_name)
}
```

When a peer presents a fact signed by a device sub-key, the bridge verifies (a) the signature on the fact against the device key, and (b) the device cert against the master key. If either fails, the fact is dropped.

`device_cert` is canonical CBOR for the `DeviceCert` structure in `carrel-core::identity`. The master signature covers exactly `(master_pubkey, device_pubkey, authorized_at_micros, device_name)` encoded as canonical CBOR. Revocation certificates use the same rule: the master signs `(master_pubkey, device_pubkey, revoked_at_micros)`.

Revocation: writing a `revoked_at` value invalidates the device. The bridge propagates revocations to peers via the social sync layer (your followers learn that your phone is no longer authorized).

**Trust:** signed. Cross-syncs to your own devices via `doc:self`. Visible to followers via `doc:public` so they can verify your facts.

## Items: the unified model

The fundamental insight in our data model is that articles, books, papers, podcast episodes, and videos are all *items* with shared structure and type-specific extensions. The base `item` relation captures the shared structure; extension relations capture per-type details.

```
:create item {
    id: String,               ;; entity.id — BLAKE3 hash of canonical identifier
    =>
    kind: String,             ;; "article" | "book" | "paper" | "podcast_episode" | "video" | "newsletter"
    title: String,
    creators: [String],       ;; authors, podcast hosts, video creators — generic across types
    primary_url: String?,     ;; canonical URL where applicable
    published_at: Validity?,
    language: String?,        ;; BCP 47 tag
    summary: String?,         ;; abstract, dek, episode notes — short
    discovered_at: Validity   ;; when this item entered our local store
}
```

The `kind` field is a discriminator for which extension relation(s) to consult. An item *may* appear in multiple extension relations — a book chapter might exist in both `item_book` and `item_paper`, for example — and this is handled by querying the relevant extensions for the kinds you care about.

`creators` is deliberately a generic list of strings, not a structured author/role model. We do not need full bibliographic precision for this product; we need "who made this." If a future use case demands more, we add a side relation rather than complicate `item`.

**Trust:** signed when shared (a share carries item facts to peers who don't have them yet); local otherwise.

### Identifiers

Items often have multiple stable identifiers. A paper has a DOI, possibly an arXiv ID, possibly a canonical URL. A book has an ISBN, possibly an OpenLibrary ID. An article might be reachable via several URLs (canonical, AMP, syndicated copies). We track these explicitly so we can dedupe when the same item arrives via different paths.

```
:create item_identifier {
    item_id: String,
    scheme: String,           ;; "url" | "doi" | "isbn" | "arxiv" | "openlibrary" | "feed_guid" | "content_hash"
    value: String,
    =>
    is_canonical: Bool,       ;; one identifier per (item, scheme) is canonical
    discovered_at: Validity
}
```

When ingesting a new item, the pipeline first checks `item_identifier` for matches against the new item's identifiers. If any match, the new fetch updates the existing item rather than creating a duplicate. The dedup logic lives in `carrel-feeds` and is exercised by integration tests.

**Trust:** local, derived from item facts.

### Type extensions

Each item kind has an optional extension relation with kind-specific metadata.

```
:create item_article {
    item_id: String,
    =>
    feed_url: String?,        ;; if discovered via RSS
    word_count: Int?,
    estimated_read_minutes: Int?,
    site_name: String?,
    byline: String?           ;; raw byline text from the article, useful when creators[] is generic
}

:create item_book {
    item_id: String,
    =>
    isbn: String?,
    publisher: String?,
    page_count: Int?,
    cover_blob_id: String?,   ;; iroh blob hash of cover image
    is_physical: Bool,        ;; true if user is tracking a paper book they own
    edition: String?,
    pub_year: Int?
}

:create item_paper {
    item_id: String,
    =>
    venue: String?,           ;; journal, conference, preprint server
    abstract: String?,
    citation_count: Int?,     ;; if known, from external lookup
    pdf_blob_id: String?,
    arxiv_id: String?,
    doi: String?
}

:create item_podcast_episode {
    item_id: String,
    =>
    podcast_title: String,
    episode_number: Int?,
    duration_seconds: Int?,
    audio_url: String,
    audio_blob_id: String?,   ;; cached audio if user enabled
    transcript_blob_id: String?
}

:create item_video {
    item_id: String,
    =>
    duration_seconds: Int?,
    thumbnail_url: String?,
    thumbnail_blob_id: String?,
    platform: String,         ;; "youtube" | "vimeo" | "peertube" | etc.
    embed_url: String?
}
```

Adding a new item kind is a focused change: a new value of `item.kind`, a new extension relation, an ingest pipeline that produces it, and queries that know how to render it. The base `item` relation does not change.

**Trust:** local, derived from item facts. Only the parent `item` is shared via signed facts; extensions are recomputed on the receiving side from the item's identifiers.

### Content-type detection

Beyond kind, items have *content-shape* properties that help readers triage. We compute these during ingest using deterministic heuristics — no external services, no LLMs, no opinions, just structural analysis of the content.

```
:create item_shape {
    item_id: String,
    =>
    has_video_embed: Bool,    ;; YouTube, Vimeo, etc. iframe in body
    has_audio_embed: Bool,
    is_link_roundup: Bool,    ;; high outbound link density relative to word count
    is_long_form: Bool,       ;; word count > threshold (default 2500)
    is_short: Bool,           ;; word count < threshold (default 300)
    has_code: Bool,           ;; presence of <pre>/<code> blocks
    has_math: Bool,           ;; presence of math markup or known math classes
    detected_at: Validity
}
```

These facts power triage queries: "unread items that aren't videos" when at the desk, "video posts" for couch-mode, "long-form" for Sunday morning, "link roundups" when you have time to follow rabbit holes. The thresholds are configurable.

The detection is rule-based and lives in `carrel-feeds`. Adding a new shape attribute means: pick a heuristic, write the detector, add a relation field, write tests with fixture inputs, document the threshold (if any) here. No AI involved.

**Trust:** local, derived from item content. Recomputable on demand.

## Content storage

Bulky content — readable HTML, images, EPUBs, PDFs, transcripts — is stored as iroh-blobs and referenced from Cozo facts by hash. Cozo holds metadata; iroh-blobs holds payloads.

```
:create item_content {
    item_id: String,
    format: String,           ;; "html_readable" | "html_original" | "epub" | "pdf" | "text" | "markdown" | "audio_mp3"
    =>
    blob_id: String,          ;; BLAKE3 hash, also the iroh-blob key
    fetched_at: Validity,
    extracted_with: String?,  ;; "readability" | "trafilatura" | "manual" | etc.
    byte_size: Int
}
```

A single item may have multiple content blobs in different formats: the readable HTML for in-app reading, an EPUB for sending to ereader, a PDF original. They share the item_id; queries pick the format they need.

Resilience: the moment a user highlights an item, we ensure the readable HTML blob exists. This is what makes highlights survive the original page disappearing. The content cache is not optional metadata; it's load-bearing for the second-brain promise.

**Trust:** the metadata fact is signed when shared (a peer sharing an item with its content reference includes the blob_id); the blob itself is fetched separately via iroh-blobs.

## Personal state

State that is yours alone — your read state, your tags, your stars, your lists — is not shared with peers. It syncs between your own devices via `doc:self` but does not cross trust boundaries.

```
:create read_state {
    item_id: String,
    =>
    state: String,            ;; "unread" | "reading" | "read" | "abandoned" | "skimmed"
    progress: Float?,         ;; 0.0 to 1.0, generic across types
    progress_label: String?,  ;; human-readable: "page 47 of 320", "12:34 of 45:00", "scroll 60%"
    last_position: Json?,     ;; type-specific cursor (CFI for epub, page+offset for pdf, scrollY for html)
    updated_at: Validity
}
```

`state` is explicit and user-controlled. The default behavior is: opening an item moves it to `reading`; closing prompts (once, with a "stop asking" option) whether to mark `read`. After that, no nagging. We do not silently auto-mark items read.

`progress` is generic 0.0-1.0 so queries can compare across types: "items I'm 80% through" works for articles, books, podcasts, videos uniformly. `progress_label` is the human string we show in the UI.

```
:create item_tag {
    item_id: String,
    tag: String,
    =>
    added_at: Validity,
    retracted_at: Validity?
}
```

Tags are user-applied labels. An item can have many tags; the same tag can apply to many items. Retracting a tag writes a `retracted_at` value rather than deleting the row, preserving history.

```
:create item_star {
    item_id: String,
    =>
    starred: Bool,
    updated_at: Validity
}
```

Star is a special boolean that gets its own table for query speed. Could in principle be a tag with name "starred"; we keep it separate because it's used everywhere and benefits from being a one-bit column rather than a string match.

```
:create reading_list {
    id: String,               ;; entity.id
    =>
    name: String,
    description: String?,
    is_ordered: Bool,         ;; true for queues; false for unordered collections
    created_at: Validity
}

:create reading_list_item {
    list_id: String,
    item_id: String,
    =>
    position: Int?,           ;; for ordered lists
    added_at: Validity,
    removed_at: Validity?
}
```

Lists are user-created collections: "currently reading," "to read about commons," "books for kid." Some are ordered queues; others are unordered piles. Membership is monotonic with explicit removal markers.

**Trust:** all of the above are local-only by default. Future feature: lists can be optionally shared (book club, reading group), at which point they'd grow signature fields and an audience scope. That's a v2 design decision.

## Annotations

Highlights, notes, and connections are first-class entities. They have their own IDs, can be queried independently, and are shareable.

```
:create highlight {
    id: String,               ;; entity.id
    =>
    item_id: String,          ;; what this highlight is on
    quoted_text: String,      ;; the exact selected text
    location: Json,           ;; type-specific: see below
    location_label: String?,  ;; human-readable: "p. 47", "ch. 3, ¶12", "00:14:23"
    color: String?,           ;; user-chosen color, optional
    created_by: Bytes,        ;; pubkey of the author (you, or a friend if synced)
    created_at: Validity,
    signature: Bytes
}
```

The `location` JSON is type-specific. We follow the W3C Web Annotation model where applicable (interop path with Hypothesis later):

For HTML: TextQuoteSelector (prefix + exact + suffix) + TextPositionSelector (char offsets in readable content). The prefix/suffix make the highlight resilient to the page changing — fuzzy match the quote in the new content using surrounding context.

For EPUB: an `epubcfi` string — the standard Canonical Fragment Identifier.

For PDF: `{page: int, char_start: int, char_end: int}` against the extracted text layer.

For audio/video: `{start_seconds: float, end_seconds: float, transcript_quote: string?}`.

For physical books (where the user typed the quote): `{page: int}` plus the quoted_text itself; no further structure.

Highlights are append-only. To "edit" a highlight, you make a new one. This matters for the social layer: if you shared a highlight and then edited it, you'd be silently changing what your friend saw. Editing a *note attached to* a highlight is fine and expected; editing the highlight itself is not.

**Trust:** signed. Highlights you share with peers carry your signature.

```
:create note {
    id: String,
    =>
    body: String,             ;; markdown
    target_kind: String?,     ;; "item" | "highlight" | null (standalone)
    target_id: String?,
    created_by: Bytes,
    created_at: Validity,
    updated_at: Validity,
    signature: Bytes
}
```

Notes are markdown text. They can attach to an item, attach to a highlight, or stand alone (a thought you want to capture without anchoring to anything). The same operation — make a note — covers all three use cases; the target_kind/target_id fields just say where it points.

Notes are mutable. We store both `created_at` and `updated_at`; the signature covers the current body and `updated_at`.

**Trust:** signed. Notes can be shared.

```
:create connection {
    id: String,
    =>
    source_id: String,        ;; any entity: item, highlight, or note
    target_id: String,
    relation: String,         ;; "cites" | "extends" | "contradicts" | "related_to" | "responds_to" | "exemplifies" | "see_also"
    note: String?,            ;; why these are connected, optional
    created_by: Bytes,
    created_at: Validity,
    signature: Bytes
}
```

Connections wire entities together. This is the Zettelkasten primitive. A highlight from a book connected to an article that argues against it; a paper extending another; a note responding to a highlight from a podcast.

The `relation` vocabulary is small and curated. We don't accept arbitrary relations because the value comes from being able to query "all things that contradict X" and so on; an explosion of relation types kills that.

Connections power some of the most interesting queries: "show me the network 2 hops out from this note," "what cites this paper," "what contradicts this argument."

**Trust:** signed. Connections you make can be shared as part of your reasoning.

## Social: follows, audiences, shares

```
:create follow {
    follower: Bytes,          ;; you, on some device
    followed: Bytes,          ;; the peer being followed
    =>
    started_at: Validity,
    stopped_at: Validity?,
    audience_tags: [String]?  ;; local-only categorization: ["climbing", "tech"]
}
```

Following is asymmetric. You can follow without being followed back. The `audience_tags` are *your local categorization* of this person, not anything they see — pet-name-style metadata for filtering ("show me only what my climbing friends are sharing").

```
:create audience {
    id: String,
    =>
    name: String,             ;; "tech friends", "climbing crew", "family", "public"
    kind: String,             ;; "private" | "public"
    created_at: Validity
}

:create audience_member {
    audience_id: String,
    peer_pubkey: Bytes,       ;; not used for kind="public"
    =>
    added_at: Validity
}
```

Audiences are *your* groups for scoped sharing. A share targets one or more audiences. Each audience has a `kind`:

`private` audiences flow through iroh-doc sync to authorized peers. The `audience_member` table is the authoritative list of who can read this audience's shares; this drives the capability tokens issued for the corresponding iroh-doc.

`public` audiences flow through the publisher worker as JSON Feed / Atom output. There are no specific members; the published feed is reachable by anyone with the URL. The `audience_member` table is unused for public audiences.

A share can target both: "I'm sharing this with my tech friends *and* publicly." The single share fact carries multiple audience IDs; the sync bridge and publisher each filter for the audiences they care about.

**Trust:** local. Your audience structure is your own; peers don't see your full audience graph.

```
:create share {
    id: String,
    =>
    item_id: String,          ;; what's being shared
    shared_by: Bytes,         ;; pubkey of sharer
    note: String?,            ;; the share-with-note text
    audiences: [String],      ;; audience IDs this share targets
    created_at: Validity,
    retracted_at: Validity?,
    signature: Bytes          ;; covers item_id + note + audiences + timestamp
}
```

A share is the act of vouching: "this is worth your time." The note is optional but encouraged in the UI (a share without a note is allowed, a share with a note is the default flow).

Retraction sets `retracted_at`. The application treats retracted shares as absent. Like all our delete-shaped operations, this is honor-system in a P2P world: peers who already received the share have local copies, and we cannot guarantee deletion. We are honest about this in the UI — sharing is a vouch, and once vouched you cannot guarantee unvouching.

```
:create share_highlight {
    share_id: String,
    highlight_id: String,
    =>
    added_at: Validity
}
```

A share can be associated with a highlight: "I'm sharing this article *because of this passage*." The highlight is the anchor for the share's argument.

```
:create share_reaction {
    share_id: String,
    reactor: Bytes,
    =>
    kind: String,             ;; "saved" | "read" | "discussed_offline"
    created_at: Validity,
    signature: Bytes
}
```

Reactions are deliberately minimal and deliberately *not* "likes." The available kinds are `saved` (I'm putting this in my queue), `read` (I read it), and `discussed_offline` (we already talked about this). No counts of any of these are visible in the UI — they're private signals from a friend back to a sharer that say "I received this."

```
:create share_reply {
    id: String,
    =>
    share_id: String,         ;; what we're replying to
    parent_reply_id: String?, ;; for threading within a share's discussion
    body: String,             ;; markdown
    author: Bytes,
    created_at: Validity,
    signature: Bytes
}
```

Replies enable discussion on shares. We model them explicitly rather than as notes-with-targets because share threads have specific query patterns ("show me the conversation around this share") that benefit from a dedicated relation.

Threading via `parent_reply_id`. A reply at the top level has `parent_reply_id: null`.

**Trust:** all share-related relations are signed. Shares flow to followers via `doc:public` (or audience docs); replies go to the participant set of the share's audiences.

## Feeds

```
:create feed {
    url: String,              ;; canonical feed URL
    =>
    title: String?,
    description: String?,
    last_fetched: Validity?,
    last_modified_header: String?,  ;; for conditional GET
    etag_header: String?,
    fetch_interval_seconds: Int,
    consecutive_failures: Int,
    folder: String?,
    auto_mark_read: Bool      ;; for high-volume feeds you don't want guilt over
}
```

The feed table is just the *subscription*. When a feed item arrives, it becomes an `item` (with `kind: "article"` typically) and gets a `feed_guid` identifier in `item_identifier` linking it back. The feed itself is not the item.

Adaptive intervals: `fetch_interval_seconds` is updated based on observed update frequency. Decay up when feeds prove stale; decay down when they prove fresh.

`auto_mark_read` is for the firehose subscriptions (Hacker News, link aggregators) where the user wants them present but doesn't want unread accumulation.

**Trust:** local. Your feed list is yours.

## The peer allow-list

This section is the canonical reference for which fact types the bridge accepts from remote peers.

| Relation | From peers? | Notes |
|----------|:-----------:|-------|
| `peer` | No | You manage your own peer list |
| `device_authorization` | Yes | Required for verifying multi-device peers |
| `item` | Yes | Carried as part of share payloads |
| `item_identifier` | Yes | Carried with items |
| `item_article`, `item_book`, etc. | Yes | Carried with items |
| `item_shape` | No | Recomputed locally on ingest |
| `item_content` (metadata) | Yes | Triggers blob fetch |
| `read_state` | No | Local only |
| `item_tag` | No | Local only |
| `item_star` | No | Local only |
| `reading_list`, `reading_list_item` | No | Local only (until shared lists are designed) |
| `highlight` | Yes | Shared as part of social activity |
| `note` | Yes | Only when target is a shared item or highlight |
| `connection` | Yes | Shared as part of someone's reasoning |
| `follow` | No | Your follow graph is yours |
| `audience` | No | Your audience structure is yours |
| `audience_member` | No | Your group memberships are yours |
| `share` | Yes | The primary social fact |
| `share_highlight` | Yes | Linkage between shares and highlights |
| `share_reaction` | Yes | Reactions to your shares come from peers |
| `share_reply` | Yes | Discussion |
| `feed` | No | Your subscriptions are yours |
| `entity_tombstone` | No | Local privacy/deletion marker; shared deletion semantics need an explicit design |

The bridge enforces this list. Adding a new relation requires deciding its trust class explicitly, in this document, before the code accepts it.

## Public syndication

Facts in audiences with `kind: "public"` flow through a different pipeline than peer sync. The publisher worker (in `carrel-app::background::publisher`) watches for shares to public audiences and writes a JSON Feed and Atom feed to a user-configured output path.

The published feed contains:

- `feed.title`: the audience name
- `feed.author`: the user's `self_described_name`, with their public key in an extension field
- `feed.items`: each public share, with title, content (the share note), URL (the shared item's URL), date, and an extension carrying the original item metadata

Subscribers — Carrel users or anyone with an RSS reader — pull the feed at their own cadence. Subscribers do not need to be authorized; the public feed is meant to be open.

The publisher does not push. It writes a file. Distribution is the user's existing infrastructure (a git repo + GitHub Pages, an rsync target, a Dropbox-synced folder). Carrel does not host.

## Migrations

Schema changes go through a version. The `schema_version` table tracks which migrations have been applied:

```
:create schema_version {
    version: Int,
    =>
    applied_at: Validity,
    description: String
}
```

Migration code lives in `carrel-store/src/migrations/` with one file per version. A migration:

1. Reads from old relations.
2. Writes to new relations.
3. Records the version in `schema_version`.
4. Optionally drops old relations once we're confident no read paths still need them.

We keep migration code in the codebase indefinitely. A user upgrading from a years-old version should be able to walk the full chain. Migrations should be idempotent — running the same migration twice is a no-op.

Breaking changes (a relation field removed, a meaning changed) require a migration. Additive changes (a new relation, a new optional field with a default) can ship without a migration version bump if existing queries don't break.

Update this document *before* writing the migration code. The migration's description in this document is what the maintainer six months from now will read to understand what changed and why.

## Invariants

The schema enforces certain invariants. These are documented here and tested in `carrel-core` (where pure logic lives) and `carrel-store` (where Cozo enforces them).

**Entity IDs are stable.** Once an item has an ID, that ID does not change. If an item's URL changes (the publisher moves it), we add a new identifier; we do not change the ID. Otherwise highlights and connections pointing at the item would break.

**Signatures verify or the fact is rejected.** No exceptions. A fact that fails signature verification is dropped at the bridge with a logged warning.

**Validity is monotonic per-fact.** A fact written with `validity = t` cannot be superseded by a fact with `validity < t`. The system clock could in principle violate this if it goes backwards; we use `validity = max(now, last_validity_for_this_key + 1µs)` to enforce monotonicity.

**Audience kind is consistent.** An audience created as `private` cannot become `public` later; that would change the trust semantics of past shares. To "publicize" things, the user creates a new public audience and re-shares.

**Tombstones are forever.** A fact that has been tombstoned does not come back. If a user wants the entity back, they create a new entity; the old ID stays dead.

These invariants are tested with property-based tests in `carrel-core`. New invariants get tests in the same change.
