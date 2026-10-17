# ADR 0005: encrypted local backup and fresh-device recovery

Status: implemented in current source, 18 October 2026; unreleased.

The maintainer selected an encrypted file and restoration into an empty
installation, with a preview. Replacing or merging a populated archive is outside
this version. The contract and tests are in [BACKUP_RESTORE_PLAN.md](BACKUP_RESTORE_PLAN.md).

Cozo's full transaction backup preserves schema, indexes, immutable signed bytes
and histories. Enable its existing SQLite storage feature solely as the portable
backup container; RocksDB remains the active database. Store creation/restoration
belongs to carrel-store. Native file selection, write flushing, atomic destination
replacement, staged activation and restart belong to carrel-app.

Use the Rust age implementation with password encryption, a fixed scrypt work
factor of 18 and a maximum accepted factor of 18. Native backup uses neither
plugins nor arbitrary recipients. A bounded header check precedes decryption.
Inside the standard encrypted stream, a versioned bounded manifest authenticates
the exact fixed entries, lengths, BLAKE3 hashes and completion marker. Large
payloads copy in bounded chunks. No filesystem names or key material appear
outside encryption. This adds a maintained encryption implementation instead of
designing another cipher/streaming nonce protocol. age is MIT/Apache-2.0; its
transitive notices remain subject to the repository's license checks.

Back up every available verified archive blob, including historical content no
longer referenced by the current article body. The source database snapshot is
taken under the existing compound-write lock order, together with the current
key headers. The native annotation/setup lock coordinates key-file creation.
The encrypted master is included when present; the operational private key is
never included. A paired desktop cannot claim to export a master it does not hold.
Appearance and keymap preferences stay installation-local in this first version.

Decrypt and restore into an owner-only temporary directory. The first format
requires the exact supported schema version; inspect tables, columns and trigger
definitions against a pristine schema, and verify retained portable signatures.
No backup-provided trigger runs before its definition is checked. Store-internal
staging clears the copied installation binding, reinstates the immutable-binding
guard, creates an independent device key and records local recovery ownership.
Preserve original certified authors, revocations and private document capabilities.
The Iroh disk mirror is reconstructed only after explicit identity authorization.

An unbound restored archive remains locally editable by its new operational key
through the explicit local ownership grant. A bound archive must resume under
its original master. A recovered master can authorize the fresh device; a paired
desktop backup can instead receive a matching invitation from another own desktop.
For an archive that had no master, explicit identity creation certifies the
original local authors as well as the fresh device. Pairing those uncertified
authors into an unrelated existing identity requires additional authorization
and is deferred. Restore never follows a new social contact or changes audiences.
Repeated recovery can retain a local author key that never reached identity
authorization. Explicit master resumption certifies these missing local authors,
preserving notes written between restores. It retains existing certificates and
never authorizes a blocked or revoked key.
The pure core recovery certificate fixes timestamp zero and label
`Recovered desktop`; recovery-aware invitations use that same rule. A distinct
signed request domain opts in without changing ordinary pairing bytes. Pairing
requires verified certificate coverage for every retained author; it cannot
silently strand intermediate notes or authorize extra historical keys.

Existing own desktops explicitly review the recovered device's signed contact
code. It must match the bound master and endpoint key; confirmation atomically
adds its certificate, private outbox record and connection hint. Previously denied
devices stay denied. This closes the fresh-endpoint authorization gap without
issuing another document capability or requiring every desktop to hold the master.
A real in-process Iroh test recovers a master, authorizes the new endpoint on an
existing paired desktop, delivers a new private fact, and refuses a revoked replay.
It also restores a paired archive without a master, authorizes its fresh key
through a recovery invitation, and delivers private state under the same identity.

Native preview retains only an opaque request token and owned staged directory;
late responses after cancellation are disposed. Confirmation rechecks that the
destination is empty, syncs staged content and atomically writes an activation
record. Preparation/cancellation are serialized through journal publication.
Published staging remains recoverable even when the journal's directory sync
fails. Cancellation records a resumable cleanup phase before removing decrypted
staging; startup finishes it before removing its journal. On restart, before
opening Cozo or starting workers, check emptiness
again, then record activation intent and rename store/keys/blobs from staging.
Move the unused original directories aside until the new archive is validated.
Each rename is recoverable from the record, so interruption rolls forward without
opening a mixture of old and restored directories. Only operation-owned staging
and unused previous directories are cleaned up. A populated destination cancels
the pending restore and opens its existing archive with a quiet explanation.

Native restart uses the existing progress/write flush and shutdown handshake.
Failure or cancellation before activation leaves current data intact. Physical
file-picker and platform checks remain separate from library and browser fixtures.
