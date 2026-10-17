# Changelog

Carrel is pre-alpha. This file records app releases; detailed implementation
history remains in Git.

## 0.1.0-alpha.3

Private conversations and a more complete first-friend trial.

- Optional names and local profile pictures, named groups, and private replies
  attached to each deliberately shared article. Every conversation keeps its
  original audience; private reading notes stay separate.
- Select multiple friends and groups, review their combined audience, then choose
  one conversation together or independent one-person sends. Overlaps are
  deduplicated and retries preserve the original send.
- Participant cards show verified keys and introduction context. Connecting
  requires a private contact-code exchange and an explicit follow by each person.
- Reply ancestry and signed note-revision provenance, author removal, withdrawal,
  and retained conversation history across the reader's own paired desktops.
- Clearer pairing recovery, invitation copying, and a Share menu with Copy link
  that works before following anyone.
- Canonical Carrel links to saved articles and highlights, a minimal skippable
  welcome, and sidebar folders with creation and drag-and-drop organization.
- Linux deep-link dependency compatibility and bounded CI test parallelism.

Use alpha.3 on every desktop participating in profiles, groups or conversations;
alpha.2 does not support them. Database upgrades are additive; back up irreplaceable
material before upgrading and do not open an upgraded store with an older build.

Mac packages are Developer ID signed and notarized. Physical two-desktop delivery,
native file-picker selection and broader platform usability remain manual trial
checks. In-process peer tests and browser fixtures do not establish those results.
These builds have not had an independent security audit.

## 0.1.0-alpha.2

First packaged pre-alpha for careful local and friend trials.

- Local RSS/Atom/JSON feeds, website discovery, article URL capture, OPML and
  optional source folders, with offline title/body search and explicit pagination.
- Reading Room appearance, compact context bar, stable sidebar, keyboard controls,
  reading preferences, passage restoration and explicit completion.
- Durable highlights and notes, retained offline conflicts and Markdown export.
- Private self-device pairing and deliberate article sharing with one trusted
  person, with audience isolation and local recovery controls.
- Bounded HTTP ingestion, robots-cache expiry recovery, documented trust boundaries
  and automated native/CSR tests.
- Pairing invitation cleanup preserves invitations created by a reopened Settings
  form, even when an older request completes late.
- Native Apple Silicon/Intel Mac and Linux x86-64 release packages, matching source,
  release manifest and SHA-256 checksums.

Mac packages are Developer ID signed and notarized, with tickets attached to
both the app and disk image. Physical two-desktop sharing, native file-picker
selection and broad platform usability remain manual trial checks. These builds
have not had an independent security audit. Read the installation guide and keep
a private backup of irreplaceable material.
