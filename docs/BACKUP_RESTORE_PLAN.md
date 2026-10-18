# Backup and restore

**Implemented in current source; not included in the published alpha.3 packages.**
A Carrel backup preserves a
reading life: archived text/images, annotations and their complete history,
subscriptions/folders, personal state, conversations, local contact/group settings,
unfinished local reply drafts and the identity material actually held on this
desktop. Private sharing and
introductions remain the other immediate priority, described in
[CONVERSATION_PLAN.md](CONVERSATION_PLAN.md#introductions-with-a-choice).

## Agreed first delivery

Create an explicitly chosen, encrypted backup file through a native save dialog.
Offer restore through Getting started and Settings, using a native file picker,
passphrase entry and a preview before activation. The first version supports
restoration into an empty installation, with no merging or replacement of a
populated archive. This scope was explicitly selected by the maintainer.

The preview identifies the backup date, app/schema compatibility, identity and
whether master-key recovery is available. Technical counts belong to validation
evidence, not a dashboard. Cancellation, a wrong passphrase or a failed validation
leaves the current installation intact. Passphrases stay in local form memory,
clear on close, never enter logs or webview preference storage, and cannot be reset
by Carrel. Backup encryption does not remove the existing master-key passphrase.

This is user-controlled local durability. There is no cloud account, scheduled
upload, recovery service or background notification. Native dialogs keep reading
mounted and return focus/passage on close. Long archive operations use plain
status and cancellation where safe, with no spinner around an ordinary local edit.

## A consistent archive

Cozo remains authoritative. The store owns creation and restoration of its
consistent snapshot; the app orchestrates the snapshot, immutable cached blobs,
key material, native file access and restart. The webview receives structured
preview/results and never accepts arbitrary filesystem paths or database scripts.

Use the database's supported full backup path, retaining schema, indexes,
triggers, validity values and original signed bytes. Cozo 0.7.6's `backup_db`
copies a transaction snapshot into a SQLite backup file. It requires enabling
Cozo's `storage-sqlite` feature, enabled for this purpose. This is a backup
container; the live store remains RocksDB. The added dependencies pass the
repository's license policy and appear in the generated notices.

Its `restore_backup` requires a truly fresh database, before migrations create
relations. The application's current startup calls `migrate` immediately after
opening the store, so an ordinary empty Library is insufficient. Restore must
use a separate staged store and an explicit startup activation path.
`import_from_backup` is not a substitute: it skips triggers and cannot import
into relations with indexes.

These behaviors were checked against the installed Cozo 0.7.6
`src/runtime/db.rs` and the [upstream API documentation](https://docs.cozodb.org/en/latest/nonscript.html).

Take the snapshot before selecting its referenced cached content. Copy only
validated content-addressed files, checking digest and length while reading.
An absent cache entry already recorded as unavailable must remain explicit in
the preview/result; a completed offline archive must never silently become a
metadata-only backup. Include historical content needed by retained highlights
and signed revisions, not just the currently displayed article body.

Flush queued local writes and coordinate identity/key changes with the snapshot.
Concurrent feed/network work must not introduce a reference to uncaptured data or
pair a snapshot with the wrong identity files. Keep the Iroh mirror and transport
caches out of the backup; reconstruct them through carrel-sync from retained Cozo
history after restore. Existing private capabilities require the same protection
as identity keys.

## Encryption and validation

The file uses Rust age 0.12.1's passphrase-encrypted streaming format, without
armor, plugins or external recipients. Its scrypt identity must bound the work
factor before deriving a key from an untrusted header. Large archives must not require loading all cached bodies into
memory. Version the outer format and authenticate its manifest, entry order,
lengths and final completion. Bound work and lengths before allocation or expensive
passphrase derivation. Keep all sensitive filenames, keys and metadata inside
the encrypted payload. Do not invent a new cryptographic primitive or reuse a
nonce/key pair. Format 1 accepts the known schema versions 13, 14 and 15. Its fixed entries are
`database.sqlite`, optional `master.enc`, and `blobs/<lowercase BLAKE3 hash>`.
The encrypted manifest is bounded to 16 MiB and 100,002 entries, total declared
content to 1 TiB, and the age header to 8 KiB. The scrypt work factor and accepted
maximum are both 18. Validate the manifest's exact source catalog, columns,
triggers, portable records and local draft bounds before executing known migrations;
validate the current catalog again before recovery preparation. Unknown schemas
remain refused. See [ADR 0005](ADR-0005-local-backup-recovery.md).

Write to an owner-only temporary destination, sync it, then install the completed
file atomically. Existing backups are replaced only through the native save
dialog's explicit choice. A failed write must preserve the previous backup.
Temporary decrypted material uses owner-only storage and is removed on cancellation
or failure. A failed validation never activates an incomplete archive.

Accept only the documented archive entry set. Reject duplicate or unexpected
entries, traversal, absolute paths, links, malformed lengths, oversized manifests,
unsupported versions, authentication failures and truncated/trailing data.
Validate the restored schema and compatibility, all referenced blobs, key headers,
certificate signatures and the local identity binding before activation. Preserve
original journal/signature bytes; restore does not fabricate provenance.

## Identity is not an installation clone

Restoring the archive must not start a second network endpoint with the old
desktop's operational private key. The recovery flow needs a fresh device key and
an explicit valid authorization before starting private or friend protocols.
Previously signed annotations and social history retain their original authors.

The originating desktop holds the encrypted master; a paired desktop ordinarily
does not. A backup of a paired desktop cannot claim to recover the master. When a
master is available and unlocked locally, it can authorize the recovered desktop.
Otherwise retain the restored archive and guide the user through authorization
from another own desktop. Do not silently create another master identity.
Friend connections must explicitly review any replacement certified contact code.

The recovery/rebinding exception and capability handling are documented in SCHEMA,
ARCHITECTURE and ADR 0005. All ordinary immutable-binding, certificate,
revocation and peer allow-list guarantees continue to apply. In particular, an old
backup cannot silently undo a device revocation learned by another desktop.

When the recovered desktop resumes with its master key, existing own desktops
must explicitly review its new contact code in Settings → Devices → Reconnect a
recovered desktop. The code must carry a valid certificate under their existing
master and bind the advertised endpoint to that device key. Inspection writes
nothing; confirmation atomically records its certificate, private publication
work and signed connection hint. A blocked/revoked key remains denied. This is
own-device authorization and never follows a social contact. Existing friends
review the replacement contact code separately in People.

## Activation and verification

Restore into a private staging location, validate and open it in isolation, then
offer an explicit restart to activate it. Drain local writes and stop background
workers before switching stores. Recheck destination eligibility at activation;
an article or identity created after preview must prevent accidental replacement.
An interruption at any stage leaves either the old installation usable or the
fully validated replacement, with a recoverable activation record. Startup must
handle that record before opening Cozo and launching workers.

Verification covers a tested restore, rather than just a successful export:

- Restore Unicode articles, offline images, historical highlight content,
  unresolved note branches, read/star positions, removed subscriptions and folders.
- Preserve sent/received conversations, withdrawals, reply removals, local
  contacts/groups, signed provenance and durable pending work, including private
  conversation consent/leave history and unfinished installation-local reply drafts.
- Before identity authorization, reach received and own sent conversations through
  reader article information; retain read-only history, draft copying and explicit
  discard without enabling replies, Leave, removals, follows or peer access.
- Cover unbound reading, originating-master and paired-device backups; no cloned
  operational identity, automatic friend follow or wider audience after restore.
- Exercise wrong passphrases, corrupt/truncated files, excessive input bounds,
  missing blobs, future schema, wrong keys, revoked devices, cancellation, disk
  failure and process interruption around every activation boundary.
- Run against real temporary RocksDB stores; reopen the final restored store,
  verify offline content and signatures, then verify permitted network behavior
  with distinct in-process device identities.
- Native activation and distinct in-process Iroh peers exercise recovery and
  explicit reconnection. Actual create/choose file-picker selection and physical
  two-desktop recovery remain manual platform checks; browser fixtures do not
  establish those results.

OPML and Markdown exports remain useful independent formats. Their existing
contracts stay intact; neither contains enough information to substitute for
this full recovery flow.
