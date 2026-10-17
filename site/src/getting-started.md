# Getting started

Choose the package for your computer from
[GitHub Releases](https://github.com/cigrainger/carrel/releases). Current targets
are Apple Silicon Mac, Intel Mac and Linux x86-64. Draft releases are maintainer
review builds; published prereleases remain early software.

## macOS

Open the `.dmg`, drag **Carrel** into **Applications**, eject the disk image and
open Carrel from Applications. Apple Silicon uses the `macos-arm64` package;
Intel uses `macos-x86_64`. You do not need Rust, Node or a development server.

Published Mac packages are Developer ID signed and notarized by Apple. Both the
app and disk image include their notarization tickets. macOS may still ask you
to confirm the first launch of an app downloaded from the internet. If it reports
an unverified developer or a damaged app, check that you have the current package
and verify its [checksum](releases.md#verify-a-download) before reporting the exact
message. See [Apple's explanation](https://support.apple.com/en-au/102445).

## Linux

The initial x86-64 packages are built on Ubuntu 24.04. A `.deb` can be installed
with the system package manager, which also resolves its runtime dependencies:

```sh
sudo apt install ./Carrel-VERSION-linux-x86_64.deb
```

An AppImage can be made executable and launched directly:

```sh
chmod +x Carrel-VERSION-linux-x86_64.AppImage
./Carrel-VERSION-linux-x86_64.AppImage
```

Replace `VERSION` with the version in the downloaded filename. Other
distributions may need additional WebKitGTK/FUSE support; report the distribution
and exact error rather than assuming the Ubuntu build is universal.

## Add something to read

An empty installation offers a short welcome sheet with **Add source**, **Save
article** and **Import OPML**. **Not now** skips it; the app menu's **Getting
started** reopens it whenever you want. Existing archives go straight to reading.
There is no account or sync setup required.

Use **Add source** beside Sources, in Settings → Sources, or in the context bar's
app menu. Paste a website address or RSS/Atom/JSON feed URL. Carrel discovers
feeds from websites; if several valid feeds exist, choose the intended one.

For a single page, use **Save article** in the context bar/app menu (`Cmd+N` on
Mac). It archives the article and its cached images without subscribing.

Today shows publications from the preceding 24 hours. Older posts belong in
Library, even if you just subscribed. Source and folder lists put the newest
publications first. Undated posts explicitly use their saved date.

## Bring subscriptions with you

Settings → Sources offers **Import OPML** and **Export OPML** using native file
dialogs. Import skips existing subscriptions and preserves their title/folder
settings. Optional source folders also round-trip through OPML.

Use **New folder** beside Sources to name a group and choose its first source.
Drag sources onto folder headings or **Unfiled** to move them. Each source's
**Organize** control offers the same folder choices from the keyboard, plus
**New folder** and removal. Folder names open combined chronological lists;
the separate arrow expands or collapses their sources.

The [reading guide](reading.md) covers navigation, appearance and annotations.
You can read locally without setting up peer sync or adding a person.
