# A reading life of your own

Carrel is a desktop reader for the open web. Subscribe to feeds, save an article,
read it offline, keep a passage, and send it to someone whose attention you value.
Your archive lives on your computer, under your own keys.

It is inspired by Google Reader's small circles of trusted curators and the
habit of keeping a commonplace book. Sharing is a deliberate vouch to a person
you chose. Personal highlights, notes and reading state stay private.

The reading room has four destinations: **Today**, **Reading**, **Library** and
**Highlights**. A stable sidebar keeps sources within reach; quiet controls and
keyboard shortcuts keep your place. Completion is explicit. There are no
engagement metrics, recommendations, notifications or telemetry.

## Try Carrel

**Carrel is pre-alpha.** The reader, local archive, annotation tools and first
peer-sharing flow are implemented. Physical two-desktop testing and broader
native/platform checks are still in progress. Keep a private backup of anything
you cannot replace.

Start with the [installation guide](getting-started.md). Published builds, when
available, are on [GitHub Releases](https://github.com/cigrainger/carrel/releases).
The source is available under
[AGPL-3.0-or-later](https://github.com/cigrainger/carrel/blob/main/LICENSE).

## Read, keep, share

Add a website or feed address. Organize sources into optional folders. Search
the cached archive offline. Save an arbitrary article without subscribing to
its site. Export your subscriptions as OPML and your highlights/notes as Markdown.

When something deserves another reader, share that article with an optional
note. Your computers exchange the archive directly when they can connect,
using peer-to-peer transport and relay connectivity where needed. There is no
Carrel account or central service we operate.

Books and papers metadata, Pocket import, send-to-ereader, mobile and public
sharing are future work. The
[vision](https://github.com/cigrainger/carrel/blob/main/docs/VISION.md) explains
the broader purpose; these guides describe the current app.

## Built carefully

The engine is written in Rust, with Tauri and Leptos at the desktop boundary,
Cozo as the source of truth and Iroh for verified peer exchange. Automated tests
cover signed facts, offline conflicts, durable storage and audience isolation.
That evidence is distinct from a completed physical friend trial or an
independent security audit.

[Read the source](https://github.com/cigrainger/carrel) or
[contribute](https://github.com/cigrainger/carrel/blob/main/CONTRIBUTING.md).
This site is static documentation from the same repository. It has no analytics,
external fonts, sign-up form or application backend.
