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

## Make a private backup

Close Carrel normally and let pending saves finish. Copy the **whole archive
directory**, including `store`, `blobs`, `keys` and any sync storage, into your
private backup. Keeping the directory together avoids losing content referenced
by the database. The archive and operational keys are not encrypted at rest;
protect the backup accordingly.

Before restoring, keep a separate copy of the existing directory. Restore with
Carrel closed and retain filesystem ownership and permissions. This is a manual
same-installation recovery procedure, not an in-app recovery wizard. Webview
appearance preferences may need to be set again.

## Identity is not an installer

Sending the application does not send your archive or identity. A friend starts
their own identity; your second desktop uses Devices pairing. Do not copy your
key files to a friend or use a copied archive as a substitute for device pairing.

The master key is passphrase-encrypted. Its passphrase is needed for identity
authorization operations; Carrel cannot recover it through a central service.
Master-key backup/recovery UI is deferred. Keep the originating desktop and a
private backup safe while testing.

## Portable exports

Export subscriptions as OPML from Settings → Sources. Export highlights and
notes as Markdown from Highlights. These are useful portable copies, but they
do not contain the complete identity, archived article cache or sync history.
Use the whole-directory backup when you need that state.
