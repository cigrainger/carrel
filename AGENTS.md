# Working on Carrel

This document is the entry point for anyone — human or AI — about to work on Carrel. Read it first. It points at the deeper documents, sets expectations for how we work, and codifies the conventions that make the project consistent over time.

Carrel is a peer-to-peer reading and curation tool, designed as a successor to Google Reader's social layer but built local-first, on open protocols, and without engagement metrics. It is desktop-first in v1 (Tauri + Leptos, all-Rust). The full why and what is in `docs/VISION.md`; the full how is in `docs/ARCHITECTURE.md`. This document is about the *practice* of building it.

## Required reading

Before starting any non-trivial work, read in this order:

1. **`docs/VISION.md`** — what Carrel is, why it exists, and what it will never do. This is the document that prevents drift. If a proposed change conflicts with VISION, the change is wrong, not the document.
2. **`docs/ARCHITECTURE.md`** — how the system is put together. Crate layout, data flows, identity model, sync layer, anti-patterns.
3. **`docs/SCHEMA.md`** — if the work touches the data model.
4. **`docs/DESIGN.md`** — if the work touches the UI, typography, keymap, or interaction patterns.

If you are about to make a change and you have not read these, stop and read them. The cost of reading is small; the cost of building the wrong thing is large.

## Philosophy

Carrel is a tool, not a platform. A tool is reached for to do a job and then put down. A platform wants more of the user's time. Every architectural and design decision should push toward tool-ness. The user's attention is sacred; we do not compete for it.

We bias toward simpler solutions over clever ones, fewer features over more, owning a problem (writing the code) over depending on a heavy library that brings in fifteen transitive concerns we don't need. We resist generality until we need it. The data model is general; we do not materialize that generality into product features until they are real and used.

When in doubt, leave it out. The smaller v1 is, the more likely we ship it. The more constrained the surface area, the more carefully we can polish what's there. Reader at its peak was a small set of things done extraordinarily well. We aim for the same.

We treat ourselves seriously and lightly. This is real engineering on a real product, and it is also a small project we are building because we want to. Both can be true.

## Working with AI on this project

A non-trivial portion of this work is done with AI assistance. This is deliberate. The project is small, opinionated, and well-specified — exactly the conditions where AI coding assistance shines.

But AI tools have a strong gravitational pull toward the median of their training data, and for software the median is engagement-product code: dashboards with metrics, growth loops, "social" features that maximize time-in-app, generic React patterns, and a thousand small concessions to "what users expect." The values in `VISION.md` are deliberately *against* that median. So:

When working with AI on Carrel, the principles in `VISION.md` and `ARCHITECTURE.md` take precedence over anything an assistant suggests is "best practice" or "what users expect." If a suggestion proposes a feature that conflicts with the values, push back. If it generates UI that competes for attention, redirect. If it suggests instrumentation, refuse. If it wants to add a notification, an unread badge, a recommendation, a popularity signal, or a streak — refuse.

If you find yourself thinking "this is what users expect" or "every reading app does this," that is the signal to stop and check whether it conflicts with the project's commitments. Most of the time, it does. Carrel is interesting because it does not do those things; if it starts doing them, it stops being interesting.

Concrete heuristics for catching drift:

A change is suspicious if it adds counts, badges, follower numbers, or popularity signals.
A change is suspicious if it introduces a notification mechanism that pings the user from outside the app.
A change is suspicious if it adds an algorithmic recommendation, a "you might also like," or a "trending" surface.
A change is suspicious if it captures user data for the purpose of "improving the product" — we do not collect telemetry.
A change is suspicious if it adds a server-side dependency or anything that requires us to operate infrastructure.
A change is suspicious if it makes the UI more "engaging" rather than more functional.

Suspicion is not a veto. There may be a good reason. But the burden of proof is on the change, not on the principle.

## Workspace layout

Carrel is a Cargo workspace. Crate boundaries reflect real layering. The full description is in `docs/ARCHITECTURE.md`. Briefly:

```
carrel/
├── crates/
│   ├── carrel-core/        # pure logic, no I/O, no UI
│   ├── carrel-store/       # Cozo wrapper
│   ├── carrel-feeds/       # RSS/Atom/JSON, readability
│   ├── carrel-sync/        # Iroh bridge, peer management
│   ├── carrel-app/         # Tauri shell + Leptos frontend
│   └── carrel-cli/         # headless CLI for development
├── docs/                   # public-facing project docs
└── AGENTS.md               # this file
```

Layering rules are strict and documented in `docs/ARCHITECTURE.md`. The shorthand: dependencies only flow downward. `carrel-core` depends on nothing in the workspace. `carrel-app` depends on everything. If you find yourself needing a "core depends on store" relationship, the right answer is to extract an interface, not to invert the layering.

Maintainers may also keep a private workboard adjacent to this repo for in-flight design notes and task tracking. Public contributors don't need it; if you're collaborating closely with maintainers, ask.

## Development workflow

### Commands

```bash
just dev               # run carrel-app in dev mode with hot reload
just test              # run all tests across the workspace
just check             # cargo clippy + cargo fmt --check
just cli ...           # run carrel-cli with arguments
```

(These commands are aspirational at the time of writing; if `just` isn't set up yet, the first task that establishes it will document the equivalents.)

### Inner loop

When working on a feature, the loop is:

1. Read the relevant docs.
2. Make the change. Write the test alongside the change, not after.
3. Run `just check` and `just test`. Both must pass.
4. Update the docs that need updating in the same change as the code.
5. Commit. One logical change per commit.

### Commit conventions

We use [Conventional Commits](https://www.conventionalcommits.org/). Format: `type: imperative description`.

- `feat:` — new functionality (a new feature, a new fact type, a new view)
- `fix:` — bug fix
- `docs:` — documentation only
- `refactor:` — code change that neither fixes a bug nor adds a feature
- `test:` — adding or updating tests
- `chore:` — build, CI, tooling, dependencies

Never `git add -A`. Be intentional. Stage only files you changed. Never commit generated files, build artifacts, or editor configs. One logical change per commit.

If a commit message is going to be long, write it long. The commit message is the explanation that lives forever in the history. Future-us will thank you.

### Tool usage

Use built-in tools instead of shell commands for file operations. Read files with the dedicated read tool, not `cat` or `head`. Edit files with the edit tool, not `sed`. Search with the grep tool, not `grep` or `rg`. Find files with the glob tool, not `find` or `ls`.

The bash tool is for running builds, tests, and commands without dedicated tool equivalents (`cargo`, `just`, `git status`).

## Coding conventions

### Rust style

We use standard `rustfmt` and `clippy` configurations, with `clippy::pedantic` warnings enabled in CI but not enforced on every check. Address them in batches, not as a blocker.

Naming follows Rust conventions: `PascalCase` for types and traits, `snake_case` for functions and modules, `SCREAMING_SNAKE_CASE` for constants. Crate names are `kebab-case-with-prefix`: `carrel-core`, `carrel-store`, etc. Avoid abbreviations except where they're already terms of art (`db`, `id`, `url`, `http`).

Public API gets doc comments. Non-obvious internal logic gets inline comments. Obvious code does not get comments. If a comment explains *what* the code does rather than *why* it does it, the code probably needs to be clearer instead of commented.

### Errors

We use `thiserror` for typed errors at crate boundaries and `anyhow` only at the outermost application layer (Tauri command handlers, CLI entry points). Library crates do not depend on `anyhow` or use it internally. The cost of typed errors is real (more enum variants), but the benefit is that consumers can pattern-match on failure modes.

Error messages are written for the developer who will read them. They include enough context to be actionable. They do not include user-facing copy — that translation happens at the UI layer.

### Logging

We use `tracing` throughout. Spans are used for operations that span async boundaries (a feed fetch, a sync session, a command handler). Log levels:

- `error` for things the user might need to know about
- `warn` for things the developer should investigate eventually
- `info` for significant lifecycle events
- `debug` for enough detail to reconstruct what happened in a session
- `trace` for too much detail for normal operation

We never log fact contents. We log fact metadata (kind, id, length) but not bodies, because logs may be shared during debugging and we treat user data as confidential by default.

### Cozo and the data model

The schema is documented in `docs/SCHEMA.md`. When changing the schema:

1. Update `SCHEMA.md` first. Make the schema design decision in writing before implementation.
2. Update the `:create` statements in `carrel-store`.
3. Add a migration step if this is a breaking change to existing data.
4. Add property tests for any new invariants the schema implies.
5. Commit all of this together.

Anti-pattern: changing a relation in code without updating SCHEMA.md. The doc is the source of truth for the schema's intent; the code is the implementation. They must agree.

### Frontend (Leptos + Tauri)

The frontend never does business logic. It composes commands and renders results. If logic appears in a Leptos component that should run in Rust, move it to Rust and call it via a command.

Components have one job. A page-level component (a route) coordinates child components and orchestrates data fetching. Child components render structured data and dispatch user actions. They do not own application state — that lives in `carrel-store`, surfaced via `use_query`.

Optimistic mutations: actions that should feel instant (star, mark read, highlight) update the UI immediately and reconcile when the backend confirms. Spinners are forbidden for any operation that touches only local data.

The keymap stack documented in `docs/DESIGN.md` is how all keyboard input is handled. Page-level components register their bindings on mount and they pop off on unmount. Do not add window-level keydown listeners that bypass the stack.

### Sync

Code outside `carrel-sync` does not write directly to iroh-docs. The bridge in `carrel-sync` is the single point where Cozo facts become syncable entries, and where remote facts become Cozo facts. Bypassing this is an anti-pattern.

Every fact arriving from a peer is signature-verified before being written to Cozo. Every fact type has an explicit decision about whether it's accepted from peers (the allow-list in the bridge). When adding a new shareable fact type, update the allow-list and the bridge mapping in the same change.

The two-store model (Cozo as source of truth, iroh-docs as sync mirror) is intentional and load-bearing. If the bridge layer grows beyond a few hundred lines or accumulates special cases, that's a sign we've made it do too much; revisit the design rather than adding another special case.

### Feeds and politeness

Carrel is a citizen of the open web. We identify ourselves with a real User-Agent. We respect `robots.txt`. We send conditional requests (`If-None-Match`, `If-Modified-Since`). We rate-limit per host. We back off on errors.

When fetching content, we strip tracker pixels and sanitize HTML before storage. We never load resources from known-tracker hosts during extraction. We never log article content; only metadata.

Adaptive fetch intervals: feeds that update once a week should not be hit hourly. Track observed update frequency and decay the interval up when content is stale.

## Testing

Testing is part of the definition of done. A feature without complete tests is not done.

Three kinds of tests:

**Unit tests** for pure logic in `carrel-core` and parsing/extraction in `carrel-feeds`. These run in milliseconds. We aim for coverage of all logical branches and invariants.

**Integration tests** for `carrel-store` and `carrel-sync` against in-memory or temp-directory backends. These run in seconds. We test the boundaries: writing a fact and querying it back, signing and verifying, schema migrations.

**Multi-instance tests** for `carrel-sync` that spin up two or more in-process peers and verify sync semantics. These are slower but irreplaceable for verifying convergence properties — that two peers writing different facts converge to the same view, that signature verification rejects forgeries, that audience scoping is enforced.

Property-based tests (via `proptest`) are used heavily in `carrel-core` to verify monotonicity invariants of the fact model.

We do not have UI integration tests in v1. UI tests are expensive to write and fragile, and we get most of the value from a well-tested core.

What "complete tests" means for a feature:

Happy paths plural — primary use case plus meaningful variants. Sad paths plural — every failure mode. All variants — if a type has N variants, every variant gets a test. Adversarial inputs — empty collections, boundary values, Unicode, invalid combinations. Property tests where invariants exist.

Every bug fix gets a regression test. The regression test should fail without the fix and pass with it.

## Subsystem-specific guidance

### Working on `carrel-store`

`docs/SCHEMA.md` is authoritative. Read it before changing relations. The Cozo Validity model is how we get "latest fact wins" semantics for free; do not roll your own timestamp comparison logic. Queries should compose with existing patterns; if you find yourself fighting the query language, the schema design is probably the issue.

### Working on `carrel-sync`

The bridge model in `docs/ARCHITECTURE.md` is the mental model. Cozo is source of truth; iroh-docs is the sync mirror; the bridge owns translation. Do not split this responsibility across crates. Signatures are required on cross-trust facts; the cost of signing is small and the benefit is provability.

### Working on `carrel-feeds`

Politeness rules above are non-negotiable. The crate exposes pure functions where possible (parsing, extraction); side effects (HTTP, file I/O) are isolated and easy to mock. Test against fixture HTTP responses, not the live web.

### Working on `carrel-app` (frontend)

`docs/DESIGN.md` is the source of truth for visual and interaction design. The typography decisions, color tokens, and keymap are all there. Do not introduce new design tokens without updating DESIGN.md. The frontend is a thin shell over the engine; if you find yourself doing real work in Leptos, the work belongs in Rust.

## When you are unsure

The principles in `docs/VISION.md` and `docs/ARCHITECTURE.md` take priority over best practice. If something feels like the obvious move but conflicts with the documents, the documents win.

Ask rather than assume. A small clarification cost beats large rework cost. If a task or design is ambiguous, the right move is to surface the ambiguity and resolve it in writing before implementing.

Look for similar patterns in existing code before writing new ones. The first piece of work in a subsystem sets a tone; later work should echo that tone unless there's a deliberate reason not to.

When you're about to do something that feels heavy — a new dependency, a new layer, a new abstraction — pause and ask whether it can be smaller. Most of the time it can.

## A closing note

Carrel is being built carefully because we want to use it for years. Every small decision compounds. The discipline of doing things slowly and deliberately, of writing the doc updates with the code, of resisting the easy median solution — that's not bureaucracy. It's how a small project stays good as it grows. The alternative is a project that gradually becomes everything it was supposed to not be, which is the dominant failure mode of software like this.

We are not in a hurry. We are aiming for craft.
