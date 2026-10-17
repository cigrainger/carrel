# Troubleshooting

## An update failed

Saved articles remain available. Open the status strip's **Details**, Settings →
Sources, or **Source activity** through the command palette. Retry the affected
source; a failure on one source should not interrupt your current reader/list.
Technical details are available deliberately, separate from the quiet message.

Fetched documents are limited to 16 MiB of decoded bytes; robots policies to
512 KiB. An oversized or unavailable robots policy stops the fetch safely.
An article's extraction or image failure can leave feed metadata or saved text
available while the full cache needs another attempt.

## Old articles are missing from Today

Today means published within the preceding 24 hours. Fetching an old post does
not change its publication date. Look in Library or the source's list, newest
publication first. **Load older** extends that list. Search and filters remain
active until cleared.

## A share has not arrived

Keep both desktops open. In People, check the person's key/code and choose
**Try connection again**. Both people must follow each other. Contact-code
availability is not evidence of delivery; the received article is. If connection
information changed, review a new code against the expected public key.

The archive remains useful offline. A failed exchange should not discard your
saved vouch or private notes. Record which step of the friend trial failed,
along with versions, OS/architecture and the quiet People error.

## The app will not open

Confirm the package matches your CPU architecture. For macOS pre-alpha builds,
follow [first-launch approval](getting-started.md#macos). On Linux, include your
distribution, WebKitGTK version if known, and the terminal error in a report.
The current release matrix is not evidence of live GUI testing on every desktop.

## Report a problem

Use [GitHub Issues](https://github.com/cigrainger/carrel/issues) with version,
platform, reproduction steps, expected behavior and observed behavior. Share a
minimal public example rather than your archive. Leave article bodies, private
notes, passphrases, keys, invitation text and contact codes out of reports.
For a security issue, use
[private reporting](https://github.com/cigrainger/carrel/security/advisories/new).
