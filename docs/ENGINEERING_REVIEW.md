# Publication engineering review

17 October 2026. Scope: code boundaries and error handling, test ownership,
human/AI readability, repository hygiene, licensing, documentation and release
automation. This is an engineering review, not an independent security audit.

## Findings addressed

| Finding | Change and evidence |
| --- | --- |
| Successful HTTP and robots responses were read into unbounded buffers | Bound decoded chunks, with early Content-Length rejection. Tests cover absent lengths, empty/exact boundaries, streamed overflow, oversized documents and policy rejection before fetching the target. |
| Robots cache expiry could panic after a successful fetch | Apply the fetched policy directly, independent of cache reuse. The zero-TTL regression failed against the old code and covers both allowed and disallowed targets. |
| Extreme parsed crawl delays could panic during duration conversion | Use checked duration and clock conversion; fail closed on an unschedulable delay. Tests cover NaN, infinity, finite overflow, fractional values and absent/nonpositive values. |
| HTTP identity still used an example domain | Use the canonical public repository URL. |
| Archive queries and cached HTML preparation ran in synchronous native commands | Move item-list/detail reads to async commands with blocking workers, retaining their existing serialized responses. Worker failures do not serialize a panic payload; native projection tests retain behavior coverage. |
| A delayed Devices form cleanup could cancel a newly created pairing invitation | Scope native cancellation to the exact invitation credential. Closed forms cancel their own late response; pending/foreign codes do not cancel another form's invitation. A real-endpoint regression fails against the old behavior. |
| Package metadata did not declare the existing copyleft grant | Record AGPL-3.0-or-later throughout the workspace, retain the full license and add an explicit copyright/grant notice. |
| Bundled JetBrains Mono lacked its notice | Include the upstream OFL notice and inventory all bundled fonts/icons. Native packages carry license resources. |
| Native distribution needed dependency terms as well as Carrel's license | Generate the locked Rust graph's full notices with Cargo About 0.9.2, fail unresolved licensing in CI/packaging, and bundle the generated report without committing it. |
| Working notes and stale briefing language obscured current guarantees | Ignore local work, correct store API comments, add the code map, refresh contributor practice and distinguish implemented behavior from future plans. |
| The original desktop skeleton left an unused Library component and an unfinished Lists route | Remove both; unknown addresses offer a route back to the real Library. |
| README mixed development history with product use | Write a concise product entry point and a separate static guide for installation, reading, sharing, data/recovery, development and releases. |
| CI checked WASM types but did not build the actual frontend or packages | Add an actual CSR build, native platform checks, Rustdoc/workflow/docs checks, and native draft-release builds. Pin actions by commit and tooling by version. |
| Distribution lacked a source/artifact contract | Validate versions/tags, require the complete native matrix, archive the fixed source commit, generate a release manifest and SHA-256 checksums, and refuse published-release replacement. Boundary tests cover the tooling. |

## Architecture assessment

The crate layering is sound and worth preserving. Core owns canonical signatures
and causal conflict rules; store owns authoritative facts and transactional
projections; feeds owns open-web ingestion; sync owns all Iroh translation and
authorization; app composes the engine. The public code map makes those paths
discoverable without relying on conversation history.

The risk-focused review followed HTTP/robots/body handling, HTML/cache rendering,
key persistence, private endpoint gates, friend exchange, queued local writes,
archive projections and their regression coverage. Key creation uses private
temporary files and no-clobber installation; incoming facts have typed signature
and scope checks; persistent outbox/history records retain work across restarts.
Private-protocol authorization and friend audience selection remain separate.

Existing mutex-poison and impossible-state assertions were not mechanically
replaced with defaults. Those assertions document invariants; swallowing them
could permit partial state or weaken authorization. Network/parser failures
belong in typed errors and quiet recovery paths instead.

Native and frontend DTOs are still duplicated, and reader/store modules remain
large. These are specific maintenance costs. Field changes require checks at
both command boundaries; focused module extraction can follow real ownership
changes. A framework migration or a generic command abstraction would add risk
without resolving the current publication issues.

## Test and publication evidence

The automated core/store/sync tests include malformed inputs, signature failures,
audience isolation, conflicting offline branches, replay, interrupted storage
and real in-process peers. Native SSR tests and CSR/WASM checks cover different
code paths. Temporary browser review from the reader milestones is useful UI
evidence but uses mocked native commands.

The publication changes passed all 407 Rust tests, formatting, native and
CSR/WASM Clippy with warnings denied, and Rustdoc link/HTML checks locally.
Native all-target Clippy also passed on the declared minimum Rust 1.95 toolchain.
CI treats standard Clippy warnings as failures and records pedantic findings as
an advisory pass, for focused follow-up batches rather than mechanical rewrites.
Five release-tool tests cover version and artifact boundaries. The static guide
builds with local links checked; temporary WebKit review covers its chapters at
desktop and phone widths in light/dark themes, plus local search and asset use.
The invitation regression also passed an actual CSR/WebKit close/reopen sequence
with a deliberately delayed native response; those native commands were mocked.

The full Git history was scanned with Gitleaks 8.30.1 before publication;
the scan found no credential findings. This is a bounded tool result, not proof
that no secret could ever be present. Deterministic test seeds and the encrypted
compatibility fixture are intentionally public test material. Local handovers,
databases, keys, generated sites and build output are excluded from publication.

Release CI builds Apple Silicon and Intel Mac packages and Linux x86-64 packages.
That verifies compilation/packaging and automated regressions on those runners;
it does not establish successful native GUI use on every operating system.
Draft status preserves that distinction until a maintainer reviews the artifacts.

## Remaining manual evidence

Live native focus/window behavior, actual file-picker selection, downloaded-app
installation and physical two-computer pairing/sharing remain the release trial.
An independent cryptographic/security audit, master-key recovery UI, Windows,
automatic friend-device rotation and reproducible-binary research are not claimed
by this pass. The current app remains pre-alpha.
