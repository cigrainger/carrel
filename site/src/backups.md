# Encrypted backups

This flow is available in current source and will ship in the next release.
The published **alpha.3 installers do not include it**. For those builds, use the
[closed-app directory backup](data-and-recovery.md#alpha3-directory-backup).

A backup keeps the complete database, available cached text and images,
highlights and note revisions, subscriptions and folders, private reading state,
conversation history, unsent reply drafts, contacts and groups. It includes the encrypted master key
only when this desktop holds it. Appearance and keyboard preferences stay on the
installation. The backup does not copy the desktop's operational private key.

## Create a backup

1. Open **Settings → Backups**.
2. Enter and repeat a new backup passphrase of at least twelve characters.
3. Choose **Create encrypted backup**, then choose a destination in the native
   save dialog. Keep the `.carrel-backup` file somewhere private and separate from
   your working archive.

Keep the passphrase separately and safely. Carrel cannot reset it. Your identity's
master key retains its own original recovery passphrase; the backup passphrase
does not replace it. Creation leaves the archive available and does not publish
anything. A failed write preserves an existing destination file.
Pending draft saves finish first. If a save fails, creation stops and keeps the
text available for Retry, copying or explicit discard in its conversation.

## Preview and restore

The first restore flow requires an **empty installation**, including its identity,
contacts and hidden history. An empty-looking Library is not enough. It does not
merge or replace an existing archive.

1. On the empty installation, choose **Getting started → Restore backup**, or
   **Settings → Backups**.
2. Enter the backup passphrase and choose **Choose backup and preview**.
3. Select the file. Carrel decrypts and validates the complete archive before
   showing its date, source build, identity and master-key availability. If cached
   content was already missing, the preview says so.
4. Review it, then choose **Restart and restore**. Pending local saves finish before
   Carrel restarts. **Cancel review** discards the preview without importing.

Wrong passphrases, damaged files and unsupported formats leave the current archive
intact. Current source accepts backup schemas 13, 14 and 15: it validates the exact
source schema before applying the known upgrade. Unknown future schemas are
refused. Keep a copy of the source app and backup when moving between versions.
Keep the backup file until you have reopened the restored archive and checked important offline
articles, passages and notes. If restart was interrupted, reopen Carrel to resume
the prepared activation. If it has not begun, Backups also offers cancellation.

## Resume your identity

The restored archive works locally straight away. Carrel creates a **fresh device
key** and waits for authorization before syncing or sharing. Original notes and
conversation records retain their authors and signatures.
Unsent drafts keep their text and reply context, with fresh retry identities
for the recovered desktop. Open the article's **Info → Conversations** to read
retained received or sent threads and copy or deliberately discard a draft.
Sharing changes wait for identity authorization. Restoration never sends drafts
automatically or reopens a conversation you left or declined.

- **The backup holds your encrypted master:** open **Settings → Devices → Resume
  identity** and unlock it with the original recovery passphrase. On each other
  own desktop, open **Devices → Reconnect a recovered desktop**. Paste the recovered
  desktop's contact code from **Settings → People**, review the identity and fresh
  device key, then authorize it. Keep the apps open while they reconnect.
- **It is a paired-desktop backup without the master:** pair the recovered desktop
  using an invitation from another own desktop belonging to the same identity.
  The restored archive cannot be attached to a different master identity.
- **The archive never had an identity:** keep reading locally, or explicitly create
  an identity in Devices. This also authorizes the retained local note authors.
  Pairing those previously uncertified authors into a separate existing identity
  is outside this first recovery flow.

Existing friends need to review your replacement contact code separately in
People. Restoring does not follow anyone automatically, widen a conversation's
audience or undo a revocation learned by another desktop. A paired backup cannot
recover a master key it never held, and there is no central account recovery.

Use a current build on both own desktops for recovery pairing; alpha.3 cannot
authorize this recovery request. Recovered keys use the stable name **Recovered
desktop**. If you make another backup before resuming a recovery, it can contain
local note authors that were never certified. An invitation can resume it only
when the retained history or grant already certifies those authors. Otherwise
Carrel keeps the archive unbound and editable locally; resuming with the matching
master recovery key is required. Prefer completing authorization before making
the next backup intended for paired-desktop recovery.

Automated checks cover real temporary archives, interruption at activation
boundaries and reconnection between in-process peers. Native file-picker selection
and physical two-computer recovery remain manual trial checks.
