# Releases

Carrel's release workflow builds a **draft prerelease** from a version tag.
Draft status gives the maintainer a concrete set of artifacts to inspect before
publishing. It does not describe a stable or fully validated release.

## Release targets

| Target | Native build runner | Packages |
| --- | --- | --- |
| macOS arm64 | Apple Silicon macOS 15 | DMG |
| macOS x86-64 | Intel macOS 15 | DMG |
| Linux x86-64 | Ubuntu 24.04 | Debian package and AppImage |

Mac packages are ad-hoc signed, without Developer ID credentials or notarization.
Platform installation guidance is in [Getting started](getting-started.md).
Windows and Linux arm64 are not in the initial release matrix.

## Make a release

1. Confirm CI passes on the intended commit and review the release limitations.
   Check the app's Cargo version and `tauri.conf.json` version agree.
2. Create and push an annotated version tag, for example `v0.1.0-alpha.1`, on
   the reviewed commit. Tagging starts native builds; the tag must match the
   app version. Do not move an existing published tag.
3. Inspect the workflow's draft prerelease and its packages, source archive,
   release notes and `SHA256SUMS`.
4. Test installation/launch with the development server stopped. Complete the
   applicable native/file-picker and physical peer checks, recording actual
   evidence and remaining limitations. Publish the draft when ready.

The workflow can also be dispatched for an existing tag. Reruns keep the release
draft and replace its artifacts; they refuse to overwrite a published release.
The source commit is fixed before matrix builds start. Each build uses
`Cargo.lock` and pinned desktop tooling. This is repeatable release automation,
not a claim of byte-for-byte reproducible binaries.

## Verify a download

Download `SHA256SUMS` and the packages you intend to install from the same
release. On macOS:

```sh
shasum -a 256 -c SHA256SUMS
```

On Linux:

```sh
sha256sum -c SHA256SUMS
```

The manifest covers every packaged artifact and the source archive. Checking
the full manifest requires all listed files in the same directory; missing
files are reported as missing. Checksums establish that bytes match the release
manifest, not an independent publisher signature.

The GitHub Pages guide deploys separately from main. The repository is canonical;
generated website output, local archives and signing credentials are not committed.
