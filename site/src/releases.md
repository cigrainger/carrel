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

Published Mac packages are Developer ID signed and notarized. CI prepares ad-hoc
draft packages; the maintainer signs and notarizes them locally before publishing.
Platform installation guidance is in [Getting started](getting-started.md).
Windows and Linux arm64 are not in the initial release matrix.

## Make a release

1. Choose a reviewed commit, run local checks and review the release limitations.
   Check the app's Cargo version and `tauri.conf.json` version agree.
2. Create and push an annotated version tag, for example `v0.1.0-alpha.2`, on
   the reviewed commit. Tagging starts native builds; the tag must match the
   app version. Do not move an existing published tag.
3. Inspect the workflow's draft prerelease and its packages, source archive,
   release notes and `SHA256SUMS`. Require branch CI to pass on the release commit
   before publishing; artifact preparation can run alongside those checks.
4. Complete [Mac signing and notarization](#mac-signing-and-notarization), then
   replace the draft's DMGs, manifest and checksums with the final versions.
5. Test installation/launch with the development server stopped. Complete the
   applicable native/file-picker and physical peer checks, recording actual
   evidence and remaining limitations. Publish the draft when ready.

The workflow can also be dispatched for an existing tag. Reruns keep the release
draft and replace its artifacts; they refuse to overwrite a published release.
The source commit is fixed before matrix builds start. Each build uses
`Cargo.lock` and pinned desktop tooling. This is repeatable release automation,
not a claim of byte-for-byte reproducible binaries.

Production builds generate Rust dependency notices using Cargo About 0.9.2 and
the checked-in `about.toml` policy/template. The native package includes those
notices, Carrel's license/grant, and the separate font/icon notices. Generated
notices and build output are not committed.

## Mac signing and notarization

Keep the Developer ID Application private key in the maintainer's macOS Keychain.
The current workflow builds drafts without that key; publication is a separate
maintainer action. A notarytool Keychain profile can be reused across apps in the
same developer team. It authenticates the account, not a particular bundle ID.
Create one if needed, entering the Apple ID and app-specific password in Terminal:

```sh
xcrun notarytool store-credentials Carrel --team-id YOUR_TEAM_ID
```

Use the profile name with `--keychain-profile`; never put its password in Git,
release notes or shell arguments. The certificate must be **Developer ID
Application**, rather than Apple Development or Apple Distribution. Carrel's
bundle ID is `net.carrel.reader`. No provisioning profile or App Store upload is
required for the current app's unrestricted entitlements.

For each architecture, work from a fresh copy of the CI draft, retaining the
original downloads separately:

1. Verify the draft's checksums and manifest source commit against its tag.
   Mount the DMG read-only and use `ditto` to copy out `Carrel.app`; detach it.
   Check its version, bundle ID and Mach-O architecture. Inventory embedded code
   before signing: current packages contain just the main executable. If that
   changes, sign nested code from the inside out; do not use `--deep` for signing.
2. Sign the copied app with the chosen Developer ID identity, secure timestamp
   and hardened runtime. Current packages have no explicit entitlements; never
   add `com.apple.security.get-task-allow` to a distributed app.
3. Zip the app with `ditto`, submit it, require an **Accepted** result, and inspect
   the notarization log for issues. Staple and validate the app ticket.

```sh
codesign --force --sign "$SIGNING_IDENTITY" --timestamp --options runtime Carrel.app
codesign --verify --deep --strict Carrel.app
ditto -c -k --keepParent Carrel.app Carrel.zip
xcrun notarytool submit Carrel.zip --keychain-profile Carrel --wait
xcrun notarytool log SUBMISSION_ID --keychain-profile Carrel app-notarization.json
xcrun stapler staple Carrel.app
xcrun stapler validate Carrel.app
spctl --assess --type execute --verbose=4 Carrel.app
```

4. Repackage the signed, stapled app in a read-only UDZO disk image. Preserve the
   original drag-to-Applications layout, volume icon and license agreement. One
   approach is to convert a copy of the draft to UDRW, mount that disposable copy,
   replace its app using `ditto`, detach, then convert back to UDZO. Conversion
   can drop the UDIF license agreement. Extract the original resources with
   `hdiutil udifderez -xml`, retain only `LPic`, `STR#`, `TEXT`, `TMPL` and `styl`,
   and merge them into the new image with `hdiutil udifrez -xml` before signing.
   Never copy the old `blkx` block map or replace all resources. Check the new
   image's Software License Agreement property with `hdiutil imageinfo -plist`
   and verify its block checksums with `hdiutil verify`. Do not modify the
   original draft or copy an unstapled app back into the image.
5. Sign the final DMG with the same Developer ID identity and a distinct signing
   identifier, such as `net.carrel.reader.dmg.macos-arm64`. Submit it, require
   **Accepted**, inspect its log, then staple and validate the DMG ticket. Verify
   the image signature and Gatekeeper acceptance:

```sh
codesign --sign "$SIGNING_IDENTITY" --timestamp \
  --identifier net.carrel.reader.dmg.macos-arm64 "$DMG"
xcrun notarytool submit "$DMG" --keychain-profile Carrel --wait
xcrun notarytool log SUBMISSION_ID --keychain-profile Carrel dmg-notarization.json
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
codesign --verify --strict "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG"
```

6. Mount each final DMG read-only and repeat signature, ticket, architecture,
   version and license-resource checks on the app inside it. Check the signer
   team and timestamp with `codesign --display --verbose=4`. Keep submission IDs
   and accepted logs as release evidence. An upload timeout does not cancel a
   submission; use `notarytool info SUBMISSION_ID` before submitting it again.
7. Put exactly the two final named DMGs in `signed-dmgs/`. With all seven original
   draft assets in `draft/`, finalize into a fresh directory:

```sh
python3 scripts/release.py finalize-macos vVERSION draft signed-dmgs final
```

This checks the complete original manifest/checksums and compares the source
archive to the release tag. It preserves the original source archive, source
commit and Linux bytes, then calculates new hashes and sizes for the final Mac
packages. It does **not** sign, notarize or replace the platform checks above.
Do not run `assemble` against a newer checkout to finalize an older draft: its
source archive would describe the wrong commit.

Upload the final DMGs, `release-manifest.json` and `SHA256SUMS` to the still-draft
release, download them again, and verify all seven assets together. Update release
notes to describe the actual signed packages, then publish as a prerelease. Never
replace packages on a published release; make a new version instead. A later
workflow rerun can replace a draft, so avoid rerunning CI assembly after signing.

See Apple's [distribution signing](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/)
and [notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)
references. Notarization is a distribution check, not a security audit or evidence
that physical peer trials have passed.

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
