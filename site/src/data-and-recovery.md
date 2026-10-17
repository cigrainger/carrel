# Your data and recovery

Carrel keeps the database, content blobs and identity files on your computer.
The database is authoritative; peer transport maintains a verified mirror.

| Platform | Default archive directory |
| --- | --- |
| macOS | `~/Library/Application Support/Carrel` |
| Linux | `~/.local/share/carrel`, or under `XDG_DATA_HOME` |
| Windows development | `%APPDATA%/Carrel` |

`CARREL_DATA_DIR` selects a different archive. A development build and installed
app use the same default directory, so close one before opening the other.
The keymap lives in `~/.config/carrel/keymap.toml` on macOS/Linux, respecting
`XDG_CONFIG_HOME`; `CARREL_CONFIG_DIR` overrides its directory. Theme/reading
preferences are kept in the installation's webview storage.

## Encrypted backup in current source

Current source adds **Settings → Backups**, with encrypted files and a validated
preview before restoring into an empty installation. It is not included in the
published alpha.3 packages. See [Encrypted backups](backups.md) for the full flow
and the distinction between a backup passphrase and identity recovery.

## Alpha.3 directory backup

Close Carrel normally and let pending saves finish. Copy the **whole archive
directory**, including `store`, `blobs`, `keys` and any sync storage, into your
private backup. Keeping the directory together avoids losing content referenced
by the database. The archive and operational keys are not encrypted at rest;
protect the backup accordingly.

Before restoring, keep a separate copy of the existing directory. Restore with
Carrel closed and retain filesystem ownership and permissions. This is a manual
same-installation recovery procedure for alpha.3. Do not use a directory clone as
a second running endpoint. Webview
appearance preferences may need to be set again.

## Identity is not an installer

Sending the application does not send your archive or identity. A friend starts
their own identity; your second desktop uses Devices pairing. Do not copy your
key files to a friend or use a copied archive as a substitute for device pairing.

The master key is passphrase-encrypted. Its passphrase is needed for identity
authorization operations; Carrel cannot recover it through a central service.
Alpha.3 has no master-key backup/recovery UI. Current source's encrypted backup
includes the encrypted master only when held locally, and restores with a fresh
operational key. Keep the originating desktop and a private backup safe while
testing either flow.

## Portable exports

Export subscriptions as OPML from Settings → Sources. Export highlights and
notes as Markdown from Highlights. These are useful portable copies, but they
do not contain the complete identity, archived article cache or sync history.
Use a full backup when you need that state.
