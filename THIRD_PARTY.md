# Third-party notices

Production builds run Cargo About 0.9.2 against the locked workspace, including
native and WASM targets, and bundle `licenses/rust-dependencies.html` with the
full selected Rust dependency terms and copyright notices. `about.toml` records
the accepted license alternatives; unresolved licenses fail CI and packaging.
MPL-covered sources retain MPL terms and can be obtained through the exact crate
versions in Cargo.lock. This inventory covers Rust dependencies; native platform
libraries and distribution packages retain their own upstream terms.

Carrel's original code and documentation are AGPL-3.0-or-later. That grant does
not relicense third-party fonts, icons or dependencies.

The desktop frontend bundles the following assets, including their notices in
[`public/licenses`](crates/carrel-app/frontend/public/licenses/):

| Asset | Upstream | License | Notice |
| --- | --- | --- | --- |
| Source Serif 4 | [Adobe](https://github.com/adobe-fonts/source-serif) | OFL-1.1 | `source-serif-4.txt` |
| Literata | [Literata authors](https://github.com/googlefonts/literata) | OFL-1.1 | `literata.txt` |
| EB Garamond | [EB Garamond authors](https://github.com/octaviopardo/EBGaramond12) | OFL-1.1 | `eb-garamond.txt` |
| Inter | [Inter authors](https://github.com/rsms/inter) | OFL-1.1 | `inter.txt` |
| JetBrains Mono | [JetBrains Mono authors](https://github.com/JetBrains/JetBrainsMono) | OFL-1.1 | `jetbrains-mono.txt` |
| Lucide icon drawings | [Lucide](https://github.com/lucide-icons/lucide) | ISC | `lucide-icons.txt` |
| Lucide Leptos port | [Rust for Web](https://github.com/RustForWeb/lucide) | MIT | `lucide-port.txt` |

The Ribbon app mark is original repository artwork. Font software remains
under its own license, including any applicable reserved font names.

`Cargo.lock` records the exact Rust dependency graph. Package manifests in the
corresponding crates.io sources carry their license declarations and full terms;
use `cargo metadata --locked --format-version 1` to inspect that graph. The
release source archive retains these manifests and the bundled asset notices.

The documentation site is rendered by mdBook, whose bundled theme assets retain
their upstream notices. Generated site output is not checked into the repository.
