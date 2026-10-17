# Identity and conversation around a share

Proposed next social milestone. Discussion is not implemented: the general
`reply` relation in SCHEMA is a design placeholder, not a working signed
transport, query API or UI. The current protocol sends a share to one recipient.
That first conversation would therefore be between the sharer and that person;
group conversations require an explicit group-audience design.

The vouch is the opening message: “this is worth your time, and here is why.”
Replies should continue that exchange, preserving the deliberate audience and
the article's context. This should move ahead of adding more content formats if
conversation is what makes Carrel useful to its first readers.

## Identity people can recognize

Cryptographic identities, certified desktop keys and local contact names already
exist. Add an optional name and avatar attached to that existing identity.
Names and pictures describe a person; the verified public key remains the trust
anchor. A locally chosen contact name takes precedence over a proposed name.
Desktop names stay separate from personal display names.

Use initials when no picture is chosen. An avatar should be a small imported
raster image, decoded and normalized locally, with bounded bytes and dimensions
and stripped metadata. Transfer its verified blob only through an authorized
contact relationship; never fetch avatar URLs or use a remote avatar service.
Profiles need signed, versioned records and explicit incoming allow-list rules.
They do not create a public profile or activity stream.

## A conversation has a specific audience

Attach the thread to an immutable share ID, not merely an article URL. Separate
shares of the same article remain separate conversations. State the participants
beside the composer; replying never expands that set. A new audience requires a
new deliberate share. Group threads must not be synthesized by joining separate
one-person shares.

Open a conversation from a received vouch or sent-share history. Keep the original
vouch, article access and chronological replies together. A reply can quote the
message it answers; indentation should stay shallow and readable. Sending is
explicit, works offline, and retains failed drafts. Reading stays available
behind the conversation. There are no reply counts, badges, notification pings,
typing indicators or popularity ordering.

Start with signed immutable replies and deliberate author removal. Editing can
follow the existing causal note-revision pattern if it earns its place; it must
retain concurrent drafts. A withdrawn share closes future participation locally
without promising to erase copies already received. Concurrent offline replies
and withdrawal need an explicit convergence rule before implementation.

## Engine and trust boundaries

Update VISION's milestone placement and SCHEMA's signed-record contract before
implementing the feature. A reply must bind its root share, author, parent and
body. The store owns queries and writes; `carrel-sync` owns verification,
audience checks and mirror translation. A database relation alone does not
authorize a message.

Reject forged authors, non-participants, cross-thread parents and cycles. Handle
out-of-order parents in a bounded pending state; never render a speculative
conversation or fetch arbitrary private blobs. Define withdrawal/removal and
self-device replication semantics alongside the protocol. Profile images must
not become a way to request unrelated private content.

Test the complete loop with real in-process peers: sender, recipient, another
authorized self-device and an unrelated friend. Include two conversations about
the same article, offline replies, restart/replay, parent arrival order,
withdrawal, removal and revoked devices. Audience separation and retention of
private notes/read state are acceptance criteria, not later hardening.

## Where Cozo helps

Carrel already uses native full-text indexes and temporal facts. Recursive
Datalog can assemble reply ancestry and revision provenance; joins can return
the article, original vouch, authors and permitted replies in one typed store
read. Authorization still comes from verified facts and explicit policy.

Later, reader-chosen cross-cutting views could combine sources, highlights,
notes and shares. MinHash-LSH is also worth a bounded experiment for identifying
syndicated near-duplicates. Any grouping should preserve item identities and
annotations and ask before merging. These are product proposals, not enabled
features or evidence that we need a database redesign.

References: [Cozo's relational and recursive model](https://github.com/cozodb/cozo),
[FTS and near-duplicate indexes](https://docs.cozodb.org/en/latest/releases/v0.7.html).
