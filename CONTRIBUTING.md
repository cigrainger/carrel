# Contributing to Carrel

Start with [VISION](docs/VISION.md), then [ARCHITECTURE](docs/ARCHITECTURE.md).
Read [DESIGN](docs/DESIGN.md) for interaction work and [SCHEMA](docs/SCHEMA.md)
before changing stored facts. [AGENTS.md](AGENTS.md) applies to human and AI
contributors; the [code map](docs/CODE_MAP.md) locates the important boundaries.

## Working loop

1. Describe the concrete behavior you want to change. Discuss changes to the
   trust model, schema, dependencies or product scope before implementing them.
2. Make a focused change with regression tests for a bug, or tests covering a
   new invariant. Prefer fixtures and isolated stores to live web services.
3. Run `just check`. For UI changes, also build the actual CSR frontend and
   inspect it in a desktop webview. Native SSR checks do not validate WASM code.
4. Update the relevant docs in the same change. Explain the behavior, why it
   changed, and what the tests establish in your pull request.

Use Conventional Commits (`fix:`, `feat:`, `docs:`, `refactor:`, `test:`, `chore:`).
Keep generated assets, databases, keys, caches and local handovers out of commits.
`cargo fmt --all` formats every workspace member.

## Debugging and test builds

Normal development and test builds retain line-level backtraces for workspace
code, omit dependency debug symbols and disable incremental caches. Only the
Ed25519 and Curve25519 dependencies are optimized, keeping signature-heavy
replay and convergence checks practical without changing their verification.

Use `cargo test --profile debugging -p carrel-core` when stepping through code or
inspecting dependency variables. This opt-in profile restores full debug symbols,
unoptimized cryptography and incremental compilation.

## Readable code

Use domain names and small functions with a single purpose. Comments explain
invariants, ownership, lock ordering and why a surprising choice exists. Public
APIs have Rustdoc. Avoid narrating obvious statements or leaving a conversation
transcript in the code. A reader should be able to follow a command from the
webview to its store write and verification boundary without reconstructing
session history. Keep current guarantees distinct from plans.

Most tests belong beside pure logic or in crate integration tests. Test replay,
conflicting offline branches, rejected signatures and malformed input at the
boundary that owns those rules. Do not add permanent UI integration tests in v1.
Temporary browser review is useful evidence, with its native mocks stated.

## Reporting problems

Include the app version, OS/architecture, steps and the observed behavior. For
peer problems, include whether both apps were open and which trial step failed.
Use a minimal public example. Do not attach an archive, private notes, keys,
passphrases, invitation text or contact codes to a public issue.

Report security concerns through the process in [SECURITY.md](SECURITY.md).
Contributions are offered under the project's AGPL-3.0-or-later license, while
third-party assets retain their own terms. No contributor agreement is required.
