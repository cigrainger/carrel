# UX review and restart proposal

9 October 2026 · Initial assessment and proposals; accepted decisions are recorded in `DESIGN.md`.

This is a historical assessment. Current visual and interaction decisions are in
[DESIGN](DESIGN.md); the [code map](CODE_MAP.md) describes the implemented engine.
Editorial was superseded by Field and the Reading Room study. The early
implementation followed Editorial, with dense mode deferred. Primary
navigation is Today, Reading, Library, and Highlights; Unread and Starred are
Library filters. The reader is one layout with a collapsible navigation sidebar,
controlled by Cmd+B or a visible button. The reading loop includes explicit
completion, return-context restoration, serialized optimistic actions,
coalesced progress writes, and stale-response guards. The findings below record
the app at the start of the review; the two-direction prototype is exploratory,
not the specification for the implemented reader.

## Recommendation

Restart with one complete reading loop: browse subscriptions, choose an article, read and annotate it, then return to the same place or deliberately finish and move on. Use that loop to establish both the visual language and the engineering standard for subsequent work.

The vision remains a useful constraint. The present implementation has enough substance to improve incrementally: a desktop shell, bundled fonts, feed ingestion and extraction, local library views, read state, stars, durable highlights and notes, and a stack-based keymap. It also presents unfinished capabilities and loses context in places that make an otherwise quiet tool feel unreliable.

This review is based on the project documents and source at `8a71dd7`, plus the checks recorded below. Browser and native-app inspection were unavailable in this session. Observed code behavior is distinguished from risks requiring runtime reproduction. The accompanying mockup uses invented articles and local presentation state; it is a design proposal, not a running Carrel client.

## Art direction

The requested direction is artistic, minimalist, and contemporary, with Swiss design, Bauhaus, and De Stijl influences. Translate those influences into composition: a deliberate grid, asymmetric alignment, tension between type scales, and a few strong horizontal and vertical rules.

Use the existing warm paper, charcoal, and rust palette as the first candidate. Let the accent identify the current destination and selected item. Give the interface character through placement and proportion. The reading page should feel typeset; the navigation should feel precise.

- Keep Inter for navigation and Source Serif 4 for sustained reading. Explore the serif in article-list titles as well. Bundled fonts remain a product requirement.
- Left-align the reader title and byline to the body measure. This replaces the current centered masthead and creates a consistent reading edge.
- Use square, lightly ruled controls and plain labels. Keep radii and filled surfaces scarce.
- Create hierarchy with spacing and type rather than adding icons to every destination.
- Keep dates, source names, and length subordinate to the title. Let long titles wrap instead of forcing a single truncated line.
- Give list selection a thin accent rule and a restrained background; keep keyboard selection distinct from focus.
- Preserve comfortable body sizing, line height, links, code, tables, math, and durable highlight rendering. Test difficult articles before judging a style from short prose alone.

Two candidates are shown in the accompanying proposal:

**Editorial** uses a broad heading, asymmetric section labels, serif article titles, and short excerpts. It is the recommended starting point for a reader whose main activity is choosing and reading essays.

**Index** uses a tighter list, sans-serif titles, a stable metadata column, and fewer excerpts. It is a useful alternative for a larger subscription collection. Both candidates use the same underlying actions and facts.

These are candidate layout decisions. Acceptance should be followed by an update to `DESIGN.md` before production changes.

## UX findings

| Finding | Evidence | Proposed change |
| --- | --- | --- |
| Navigation gives eight destinations similar weight, although several are filters over the same library. | [Sidebar](../crates/carrel-app/frontend/src/components/sidebar.rs) exposes Today, Unread, Reading, Starred, Friends, Library, Highlights, and Lists in one group. | Keep Today, Reading, Library, and Highlights as the initial primary destinations. Put All, Unread, and Starred filters inside Library. Keep existing route URLs and bindings where useful. |
| Friends and Lists lead to title-only screens. | The original [placeholders](https://github.com/cigrainger/carrel/blob/8a71dd7/crates/carrel-app/frontend/src/routes/placeholders.rs); the exported Library route was implemented separately. | Introduce these destinations when their first useful flow works. Private sharing is now implemented; the unused Lists route has been removed. |
| Returning from an article loses list selection. | [List route](../crates/carrel-app/frontend/src/routes/today.rs) initializes its cursor to zero; [navigation context](../crates/carrel-app/frontend/src/navigation.rs) stores item IDs and a return path but no list scroll position. | Restore selection by stable item ID, list scroll position, and active filter. If the item leaves the filter, select the nearest remaining neighbor. |
| Previous and Next have a hidden read-state side effect. | [Reader navigation](../crates/carrel-app/frontend/src/routes/reading.rs) uses `navigate_adjacent`, which calls `mark_current_read`, for both buttons. | Previous/Next only navigate. Offer an explicitly labeled “Mark read & next” action that preserves the existing `n` completion gesture. Keep opening, scrolling, and returning independent of completion. |
| Article rows truncate titles and provide no normal focusable open control. | [Item row](../crates/carrel-app/frontend/src/components/item_row.rs) attaches a click handler to a list item; [styles](../crates/carrel-app/frontend/style/app.css) make titles single-line. | Use semantic links or buttons for article opening; make read state and star actions discoverable with mouse, Tab, and the existing keymap. Preserve full titles. |
| Read and in-progress items look the same in a list. | The item row renders every state other than `unread` as an empty circle. | Distinguish unread, in-progress, and read with accessible labels and a quiet indicator. Preserve “Reading” as a useful resumption surface. |
| An empty first session gives little help starting. | Today says “No items today”; the sidebar says “No feeds yet.” | When there are no subscriptions, offer Add feed directly in the empty list. Keep an empty recent list distinct from an empty library or a failed query. OPML import should appear when implemented. |
| Feed management is more prominent than reading requires. | Every feed has a persistent remove button; Fetch and Add sit in the sidebar header. | Keep Add easy to find. Move removal and detailed fetch controls to the selected feed's deliberate management surface. This should retain full keyboard access. |
| Theme controls permanently occupy the status strip. | [Status strip](../crates/carrel-app/frontend/src/components/status_strip.rs) renders four theme buttons. | Put appearance choices in a user-initiated surface. Reserve the strip for quiet operational information and recoverable errors. |
| “Today” means a rolling 24-hour window, and the library has a hidden limit. | [Item commands](../crates/carrel-app/src/commands/items.rs) define a 24-hour lookback and a 100-item maximum. | Keep the label honest about the window until calendar-day semantics are chosen. Add deliberate access to older items before presenting Library as complete. Avoid infinite-scroll engagement patterns. |

## The proposed reading loop

1. Open Today to recent arrivals, in chronological order, with no counts. Use Reading to resume an unfinished article and Library to revisit older material.
2. Choose a row with mouse or `j`/`k`. Open with its semantic control, `o`, or Enter.
3. Read in a full-page view with the sidebar hidden. The top rail names the return destination and offers plain navigation, star, and read-state controls. Additional metadata is deliberately opened.
4. Select text and highlight with `h`; use `H` to add a note. Mouse access should expose the same actions after a deliberate selection. Notes and saved passages retain their connection to the archived source.
5. Return with Esc or the named back control and recover the prior list context. Finish with `n` or “Mark read & next.” At the final item, completion returns to the originating list rather than becoming a dead end.

No action should require learning the keymap before it becomes usable. Shortcuts are visible references and accelerators. The stack remains the single keyboard dispatcher.

## Engineering assessment

The repository is an implemented local reader foundation with unfinished surrounding product capabilities. The sync crate is currently a documented stub. There is no active private-sharing bridge, public publisher, or ereader flow in the desktop startup path. Search and command-palette bindings exist without corresponding frontend action handlers. Those are delivery gaps, and should be represented honestly in the UI and documentation.

The code's provenance is not a quality measure. The most valuable engineering work is the work that makes the reading loop trustworthy. The following items are grounded in the source; performance and race outcomes need targeted reproduction before fixes are claimed.

| Priority | Issue | Consequence and next step |
| --- | --- | --- |
| First | List mutations have incomplete failure reconciliation. | Mark-read/unread errors do not restore the previous rows. In Starred, an optimistic unstar removes the row before reconciliation, so a failed write cannot reinsert it. The route then displays the error in place of the entire list. Restore the affected state and report the failed action without removing working content. Add failure-path regression tests. |
| First | Async list and article loads lack a request-generation check. | A slower previous request can overwrite a newer request's result. Add route/item identity checks and test completion in reverse order. This is a source-level race risk, not a reproduced runtime incident. |
| First | Article display waits for a separate highlights request. | The reader sets the item only after highlights finish. Make cached article content available immediately and attach highlights independently, while rejecting stale responses. |
| First | Progress persistence runs on every scroll event. | Each event invokes a native command that writes read state. Coalesce updates, serialize them, and flush the latest position on navigation/close. Preserve explicit read-state decisions if an older progress update completes later. Measure writes and reader responsiveness. |
| First | Subscribing waits on an immediate network fetch. | The subscription is written first, but a failed fetch leaves the Add dialog reporting failure and does not take the normal refresh path. Show the accepted subscription immediately; report the first-fetch result separately. |
| Next | Query and invalidation logic is repeated across routes. | The documented `use_query` pattern is not implemented; the current feed controller polls every five seconds. Establish a small, shared request/invalidation mechanism after defining the actual flows. Avoid a broad framework refactor. |
| Next | Native list queries perform per-item lookups. | The list handler queries read state, star state, and article metadata separately for each candidate item. Filter Today/Unread in the store rather than scanning only 500 candidates; otherwise older unread items can be omitted. Measure representative libraries and move the typed query API into the store as appropriate. |
| Next | Transport types are duplicated. | Native and frontend item/filter types can drift. Consolidate genuinely shared contracts when touching these commands, while keeping I/O and UI dependencies out of core. |
| Next | Error translation exposes developer messages. | The native error serializes `self.to_string()` and routes render it directly. Map typed failure kinds to short actionable messages, with details available deliberately. |
| Next | Documentation overstates some implemented behavior. | Theme persistence uses webview local storage although DESIGN specifies Cozo; architecture describes more workers than startup launches. Mark implemented and planned behavior explicitly. |

Keep the existing layers, typed errors, pure parsing/extraction boundaries, local source of truth, and centralized keymap. There is no evidence from this pass that the project needs a wholesale rewrite or a framework change.

## Suggested sequence

**Choose the visual direction.** Review Editorial and Index against actual article titles and a realistic subscription collection. Decide navigation grouping, density, title alignment, and how much metadata belongs in the list. Record accepted decisions in `DESIGN.md`.

**Implement the reading loop.** Deliver the sidebar/list/reader as one coherent slice, including return-context restoration, explicit completion, usable mouse controls, empty states, and failure recovery. Fix the directly related request and persistence issues in the same work.

**Dogfood with real subscriptions.** Check long titles, short feeds, failed extraction, long essays, code, math, images, links, selections, and offline use. Verify keyboard, Tab, focus restoration, and narrow supported desktop windows. Measure local latency before adding loading treatments.

**Extend into curation.** Make highlight review and editing durable and useful, then implement the smallest complete private share-with-note flow. Treat sync as a separate engineering milestone with its required signature, audience, and convergence tests.

## Verification

- Repository started clean at `8a71dd7`.
- Baseline lint exposed two unused test imports. The dependency refresh removed them and updated one SSR expectation for Leptos's changed whitespace output.
- After the dependency refresh, `just check` passed: formatting, native lint, all 130 tests, and the new separate WASM/CSR lint check.
- After implementing the accepted reading loop, `just check` passed with 156 tests, native and CSR/WASM lint, and formatting. Regression coverage includes final-item completion, browse-only navigation, filtered-list restoration, failed and rapid optimistic actions, stale responses, late highlight results, progress persistence, reactive row metadata, and native button activation. A complete Trunk CSR build also passed. Metadata text contrast was checked numerically on base, elevated, and selected surfaces in all four themes.
- The WASM target is now installed. A real Trunk CSR build also passed, including wasm-bindgen generation and copied frontend assets. The default frontend workspace feature is SSR; native checks alone still do not establish that the real webview compiles.
- The interactive proposal's local navigation, plain next, explicit completion, filters, stars, highlights and notes, final-item return, saved-state restoration, and dialog controls were exercised with a DOM harness. This verifies those interactions, not rendered layout or the actual Tauri client.
- Hands-on application verification was unavailable during the initial review. The subsequent native pass below supplies rendered and interaction checks; neither pass claims a measured latency study or an exhaustive security audit.

The exploratory prototype introduces no backend changes; the reading-loop implementation updates native item commands without changing the schema.

## Native desktop pass

The follow-up pass used the actual macOS Tauri WKWebView with an isolated library
of 80 articles from Julia Evans, Simon Willison, and John D. Cook. It covered
long titles, prose, code, image-based equations, cached images, and all four
themes, at normal and 860 × 600 desktop window sizes. The maintainer's library
was not changed. The temporary inspection driver was removed before the final
application build; the downloaded corpus and screenshots stayed outside the
repository.

| Finding | Result |
| --- | --- |
| Clicking an article link navigated the app webview away from Carrel. | Article links and Open original use the system browser through the official Tauri opener. Native checks preserved the reader's URL, DOM, and scroll. Cached fragment targets stay in the reader. |
| Shutdown did not wait for queued local saves. | Normal close flushes the current scroll snapshot and waits for local writes. Reopening immediately after close restored 1450 px. A controlled write failure kept the window open; a second close succeeded, and reopening restored 1990 px. The close/quit state machine also has unit coverage. |
| A list refresh shifted selection, and moving keyed rows could lose focus. | Selection and row identity follow article IDs. A controlled 70-to-80-row refresh using the real corpus retained the Julia article, its keyboard focus, and the exact 9120 px list offset. |
| Feed titles stayed at their initial hostname after fetching. | Keyed sidebar rows read current feed metadata. The local one-article feed changed from its hostname to Julia Evans in place. |
| Feed dialogs did not close on Escape or make the background inert. | Native modal dialogs provide inert background, Escape, and focus restoration. Add Feed and note input focus were exercised in the webview. The note dialog keeps Cmd/Ctrl+Enter for saving. |
| Highlighting required learning a shortcut. | Deliberate selection exposes Highlight and Add note controls. Mouse-created highlights and a note survived native close/reopen; `h` remains available. |
| Leading whitespace could invalidate a selection; reattachment could split paragraphs. | Selector capture accounts for leading separators, and highlight spans wrap text within existing elements. Regression cases include Unicode and selections crossing element boundaries. Native rendering retained paragraph structure. |
| The header exposed raw timestamps and the reading measure used the chrome font. | The header uses a short date; item info retains the precise timestamp. The measure now uses Source Serif at the reading size. Narrow-window checks showed no reader overflow. |
| Transparent equation images disappeared in dark themes. | Source images retain their colors on a paper backing. Cached equation images loaded and remained legible in dark and black themes. |
| Escaped HTML code examples vanished from feed articles. | Feed parsing preserves HTML entities for the sanitizer. RSS, Atom, and JSON regression cases retain literal script examples while removing executable scripts. Re-ingesting a local copy of the real Julia entry produced ten escaped script examples and zero empty code blocks, verified in the native reader. |
| A summary could look like the complete cached article. | Missing-cache copy and the summary footer state what is available. A controlled missing-cache response was checked visually. |

An unreachable local feed was saved successfully, the Add dialog closed, and the
fetch failure appeared in the status strip while cached articles remained
available. This exercises fetch failure recovery, not a system-wide offline
network simulation. Keyboard shortcuts, dialog focus, and programmatic focus
attempts against an inert background were checked; a full assistive-technology
audit remains separate. Native inspection covered macOS, not Windows or Linux.

Existing library limits, search, OPML, highlight editing, and private sharing
remain later milestones. This pass makes the existing reading loop more
trustworthy without expanding those features.

Final verification passed: `just check` (formatting, native lint, all 171 tests,
and separate CSR/WASM lint), a full Trunk CSR build, and a native build without
the inspection driver. No schema migration is required.

The brief maintenance and successor audit is in [DEPENDENCY_REVIEW.md](DEPENDENCY_REVIEW.md). The two interactive layout proposals are in [carrel-reading-loop.html](../work/ux-review/carrel-reading-loop.html); these are inspectable design sketches, independent of the production frontend.

## Complete-library pass

Library lists now paginate with **Load older** rather than stopping at 100
articles. Filters query the whole archive before selecting a page; Unread no
longer scans only the newest 500 items. Store queries join metadata and personal
state for the selected page. Timestamp ties use stable article IDs, including
when feeds deliver many articles together.

Native macOS checks used an isolated 250-article archive. Loading the second
page retained the exact 15,271 px scroll offset and original selection; loading
the final page removed the continuation control. Opening article 171 and
returning restored all 250 rows, the selected article, keyboard focus, and the
26,545 px offset. Starring it refreshed the list without changing that context.
Unread found the oldest unread article. The maintainer's library was untouched.

OPML actions are quiet sidebar controls and are available from the empty
library. Import preserves existing subscriptions and stores nested folders as
readable paths. Export groups these labels in standard OPML. The native import
picker opened, but macOS denied assistive access for automated file selection;
picker completion remains a manual check. Parser, atomic store import,
filesystem import/export, and failure paths have regression tests, including
Unicode, invalid XML/entities/URLs, size limits, and repeated imports.

Verification passed `just check` (188 tests, formatting, native lint, and CSR/WASM
lint), a full Trunk CSR build, and a clean native build. The temporary inspection
driver and servers were removed/stopped. Smaller development profiles retain
useful backtraces without the dependency debug symbols and incremental caches
that exhausted disk space during the initial verification attempt.
