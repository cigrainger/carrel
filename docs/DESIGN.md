# Design

## Purpose of this document

This document is the source of truth for Carrel's visual and interaction design. It describes the typography, color, layout, keymap, and interaction principles that make Carrel feel like Carrel rather than like a generic web app.

Code and design must agree. When they disagree, this document wins, and the code gets fixed in the same change. Design changes start here, in writing, before they appear in CSS or component code.

The goal is precision. A different competent designer, given this document and the data model, should be able to implement a Carrel UI that looks and behaves consistently with what already exists. Where a decision is a *taste* call rather than a derivable one, this document records the call so we don't relitigate it every time.

## Design philosophy

Carrel is a tool, not a platform. The visual and interaction design must reinforce that distinction in every detail. The reading view is sacred: when the user is reading, the UI is the page, and chrome stays out of the way. The chrome that exists is for navigating, organizing, and acting — not for engaging the user with metrics, prompts, or recommendations.

We commit to eight working principles that this document operationalizes:

**Reading is the primary act.** Every design decision either improves reading or doesn't compete with it. If a feature would compromise the reading experience, the feature loses.

**Keyboard-first, mouse fallback.** Every action has a key. The mouse is for the things mice are uniquely good at: scrolling long content, clicking links in articles, occasional drag operations. Everything else is faster from the keyboard.

**No engagement metrics, anywhere.** No follower counts, no read counts, no like counts, no streaks, no popularity sorting. These mechanisms turn reading tools into attention economies. Their absence is load-bearing.

**No notifications outside the app.** The dock icon does not bounce. The system tray does not show counts. The app does not push. The user comes to Carrel when they want to read.

**Honest read state.** The user marks things read when they've read them. We do not silently auto-clear unread counts to manufacture productivity, nor accumulate them to induce guilt.

**Speed is a feature.** Local interactions are sub-frame. Reading-flow keystrokes (`j`, `k`, `o`) are imperceptible. Network operations happen in the background and never block the UI.

**Craft over ceremony.** We polish typography, keyboard responsiveness, and reading-flow latency. We do not polish onboarding flows, marketing surfaces, or growth loops. The product *is* the polish.

**Restraint in motion.** Animation conveys meaning, briefly. We don't use motion for delight; we use it for clarity. 150-200ms ease-out is the default; longer or flashier transitions are a sign of drift.

These commitments are non-negotiable for the v1 product. Subsequent sections operationalize them.

## Typography

### Editorial direction

The first complete reading loop uses an Editorial layout: warm paper, charcoal,
a restrained rust accent, asymmetric alignment, generous row spacing, and serif
article titles with short excerpts. Inter remains the navigation face and Source
Serif 4 the reading face. Long titles wrap. Structural rules and typography carry
the hierarchy; controls are square and lightly ruled. Dense mode is a later
addition, not a second interface in this pass.

The display heading is 3rem, the reader title 2.25rem, and list titles 1.5rem.
The fixed sidebar is 216px wide. Control radius is zero. Default light surfaces
are paper `#f5f2eb` and elevated paper `#faf8f3`, with ink `#252722`, secondary
text `#62645d`, tertiary text `#65675f`, rules `#d2d0c5` / `#bdbbb0`, and accent
`#9a462f`. Existing dark, sepia, and black themes remain available.

Navigation uses Lucide icons beside labels, a compact 36px row, and a small
square rust marker for the active destination. Active text remains ink rather
than turning the whole row into an accent-colored card. Subscriptions form the
next group; Devices and OPML actions share a ruled utility group at the bottom.
Icons use the maintained `lucide-leptos` integration, with only the required
categories enabled, 16–18px sizes and a 1.5px stroke. Icons inside named controls
are decorative. Read, star and compact reader controls are icon-only native
buttons with current action labels, tooltips and keyboard hints. Star fills when
saved; pressed read/star state remains available to assistive technology.

Publisher titles take precedence. Genuinely untitled posts use a short opening
excerpt, using up to 120 Unicode source characters at a word boundary when available,
instead of the URL. The remaining text can appear underneath without repeating
the heading. This is a display fallback, including for older saved entries;
the original article metadata remains intact.

The application mark is a page sheltered by an architectural frame, using
paper, charcoal and rust. Its editable source is
`crates/carrel-app/icons/app-icon.svg`; native PNG, ICNS and ICO assets are
derived with `cargo tauri icon`. The app tile follows native rounded icon
conventions while application controls retain square corners.

Typography is the most important visual decision in a reading app. The user spends the majority of their time looking at body text; the body text deserves real care.

### Typefaces

We ship three serif options for body text and let the user pick one. The default is Source Serif 4. We are deliberate about the alternatives — they are *meaningfully different* in feel rather than three slightly different garaldes.

**Source Serif 4** (Adobe, OFL). The default. Variable, has true small caps, optical size axis, designed for long-form reading on screens. Free.

**Literata** (Google Fonts, OFL). Designed by TypeTogether for Google Books. Slightly warmer and more book-like than Source Serif. The "feels like reading a real book" choice.

**EB Garamond** (OFL). For users who want a classic Garalde feel. Lower x-height than the others, so it reads visually smaller at the same nominal size — we tune the size up by ~1pt when this is selected.

The user picks one of these. We do not offer a free font picker; that's a rabbit hole, and the three choices already cover the distinct aesthetic preferences a serious reader is likely to have.

For users who really want sans-serif body, we offer **Inter** (Rasmus Andersson, OFL). One option. No second sans.

For chrome (sidebars, menus, settings labels) we use **Inter** universally. This creates a coherent visual hierarchy: the body is "the document," the chrome is "the application," and they are distinct.

For code blocks within articles we use **JetBrains Mono** (OFL), with programming ligatures off (this is a reading context, not editing).

### Type scale and sizing

```css
:root {
  /* Type scale - modular, base 16px */
  --text-xs: 0.75rem;      /* 12px */
  --text-sm: 0.875rem;     /* 14px */
  --text-base: 1rem;       /* 16px */
  --text-lg: 1.125rem;     /* 18px */
  --text-xl: 1.25rem;      /* 20px */
  --text-2xl: 1.5rem;      /* 24px */
  --text-3xl: 1.875rem;    /* 30px */
  --text-display: 3rem;   /* list page heading */
  --text-reader-title: 2.25rem;
  
  /* Reading view - separate scale, user-configurable */
  --reading-size: 1.125rem;       /* default 18px, user can adjust */
  --reading-line-height: 1.65;
  --reading-measure: 68ch;        /* max line length */
  --reading-paragraph-indent: 1.5em;
}
```

The reading size lives in its own variable, separate from the chrome scale, because it's user-configurable. The chrome scale is fixed; the reading size adapts to user preference.

68ch as max measure is deliberately on the longer side for body text. Most typography guides suggest 45-75 characters per line; we sit at the upper end because Carrel is a serious-reading tool and longer measures suit denser prose. Users who prefer shorter measures can adjust.

Line height of 1.65 is high. This is also deliberate — comfortable for sustained reading, even at the cost of less content visible per screen. We are optimizing for a reading session, not for information density.

### OpenType features

```css
.reading-view {
  font-family: var(--font-body);
  font-size: var(--reading-size);
  line-height: var(--reading-line-height);
  font-feature-settings: 
    "kern" 1,    /* kerning */
    "liga" 1,    /* standard ligatures (fi, fl) */
    "onum" 1,    /* old-style figures - more elegant in running text */
    "calt" 1;    /* contextual alternates */
  font-optical-sizing: auto;  /* use optical-size axis for variable fonts */
}
```

Old-style figures (`onum`) is the unsung detail. Numbers in running text should vary in height like lowercase letters do, not stand at uppercase-height. Most fonts have it; few designers use it. It makes prose with numbers feel professional in a way readers can't quite articulate.

Optical sizing automatically thickens strokes at small sizes and thins them at large sizes when the variable font has an `opsz` axis. Source Serif does. Set it once and forget it.

### Paragraph and heading style

Indented paragraphs (no inter-paragraph spacing), not space-between. This is the book convention and it reads better for sustained text. The first paragraph after a heading or block break is *not* indented.

```css
.reading-view p {
  margin: 0;
  text-indent: var(--reading-paragraph-indent);
}

.reading-view p:first-of-type,
.reading-view h1 + p,
.reading-view h2 + p,
.reading-view h3 + p,
.reading-view blockquote p:first-child {
  text-indent: 0;
}
```

Headings are significantly less aggressive than the web default. We're inside an article view; headings are for structure, not for grabbing attention.

```css
.reading-view h1 { font-size: 1.75em; line-height: 1.2; font-weight: 600; margin: 2em 0 0.5em; letter-spacing: -0.01em; }
.reading-view h2 { font-size: 1.3em;  line-height: 1.3; font-weight: 600; margin: 1.5em 0 0.5em; }
.reading-view h3 { font-size: 1.1em;  font-weight: 600; margin: 1.5em 0 0.25em; font-style: italic; }
```

Note: `h3` is italic rather than bold. This is a minor typographic preference that gives a third level of hierarchy without piling on weight, and it reads more book-like.

### Optional flourishes

Drop caps and first-line small caps are user-toggleable. They look great on essays and bad on technical articles. Default: first-line small caps on, drop cap off. Both can be turned off for users who find them distracting.

```css
/* User-toggleable, controlled by data attribute on body */
[data-flourishes="on"] .reading-view > p:first-of-type::first-line {
  font-variant-caps: small-caps;
  letter-spacing: 0.05em;
}
```

### Math, code, CJK, RTL

**Math**: KaTeX rendered in the webview. Articles with math markup get rendered on display. Server-side pre-rendering during ingest is a v2 optimization.

**Code**: syntax highlighting via Tree-sitter at ingest time. Themes match the active reading theme.

**CJK**: Source Han Serif as the CJK companion to Source Serif. The font stack handles per-character glyph selection automatically. Line height tightens to 1.5 for CJK content; size scales up slightly because Han characters need more pixels.

**RTL languages**: `dir="rtl"` set on the reading view based on detected article language. Logical CSS properties (`margin-inline-start`, etc.) used throughout to keep the layout correct in either direction.

## Color and theming

Color is functional, not decorative. The accent color appears sparingly and only where attention is genuinely warranted (a hover state, a small affordance). The base palette is restrained: text, two or three secondary text shades, background, elevated background, rule lines, and a single highlight color.

### Default theme tokens (light)

```css
:root {
  --color-text: #252722;
  --color-text-secondary: #62645d;
  --color-text-tertiary: #65675f;
  --color-bg: #f5f2eb;
  --color-bg-elevated: #faf8f3;
  --color-media-bg: #faf8f3;    /* paper backing for source images in every theme */
  --color-rule: #d2d0c5;
  --color-rule-strong: #bdbbb0;
  --color-accent: #9a462f;
  --color-highlight: #fff3a8;    /* highlight yellow, low saturation */
  --color-highlight-friend: #b8d4e8;  /* friend's highlights - distinguishable */
}
```

The paper background (`#f5f2eb`) and rust accent (`#9a462f`) establish the Editorial direction. Accent is reserved for active navigation, selection, and small action affordances.

### Dark theme

```css
[data-theme="dark"] {
  --color-text: #e8e6e0;
  --color-text-secondary: #a8a6a0;
  --color-text-tertiary: #929088;
  --color-bg: #1a1a18;
  --color-bg-elevated: #242422;
  --color-rule: #2a2a28;
  --color-accent: #d97a52;       /* warmer accent for dark mode */
  --color-highlight: #5a4d1a;
  --color-highlight-friend: #2d4659;
}
```

Same warm bias as light mode — `#1a1a18` rather than pure neutral. Text is `#e8e6e0` rather than `#ffffff` to reduce contrast slightly.

### Sepia theme

```css
[data-theme="sepia"] {
  --color-text: #3a2f24;
  --color-text-secondary: #6a5d4e;
  --color-text-tertiary: #685c4b;
  --color-bg: #f4ecd8;
  --color-bg-elevated: #ede4cd;
  --color-rule: #d8cdb3;
  --color-accent: #8a4a2a;
  --color-highlight: #d8c878;
  --color-highlight-friend: #b8c8a8;
}
```

For users who like the e-reader feel.

### OLED black theme

```css
[data-theme="black"] {
  --color-text: #d4d2cc;
  --color-text-secondary: #948e84;
  --color-text-tertiary: #878278;
  --color-bg: #000000;            /* true black for OLED */
  --color-bg-elevated: #0e0e0c;
  --color-rule: #1a1a18;
  --color-accent: #d97a52;
  --color-highlight: #4a3d10;
  --color-highlight-friend: #1d2e3d;
}
```

For night reading on OLED screens. Pure black saves power and reduces light bleed.

### Theme switching

The current implementation stores the theme in webview local storage and applies `data-theme` to the application shell. The Appearance control in the status strip opens the theme choices. Moving preferences into `:self` Cozo state is future work. Theme switches are instant; only the colors change.

We do not auto-switch based on system theme by default. Some users want their reading app to be different from their system. The user can opt in to system-following in settings.

## Layout

### Application chrome

The desktop layout has three primary regions: a sidebar (destinations and subscriptions), a main content area (lists or reading view), and a status strip. The sidebar can collapse; in a reading view, the sidebar should be hidden by default to maximize reading focus.

```
┌─ Sidebar ───────┬──── Main content ──────────┐
│ carrel.         │                             │
│                 │                             │
│ Today           │                             │
│ Reading         │     [list or article]       │
│ Library         │                             │
│ Highlights      │                             │
│                 │                             │
│ Subscriptions   │                             │
│ …               │                             │
└─────────────────┴─────────────────────────────┘
   ↑ status strip (sync, errors, system status, optional)
```

The sidebar has four primary destinations: Today, Reading, Library, and
Highlights. All, Unread, and Starred are filters inside Library, with their
existing routes retained. Friends and Lists appear when their first useful flows
work. Subscriptions sit below the primary destinations. There are no counts or
badges. Today currently means recent arrivals in the past 24 hours; its subtitle
states that window explicitly.

There is one reader layout with a collapsible navigation sidebar. It starts
hidden. Cmd+B and a visible reader control show or hide it without remounting
the article or losing its scroll position. The choice carries across articles
for the session. Returning to a list restores its filter, selected item, and
scroll position. When an item leaves a filter, select the nearest remaining
neighbor.

Selection follows article identity when a refresh inserts or reorders rows.
Returning from the reader restores keyboard focus to the selected article
without changing the restored list scroll offset.

### Local article search

Each article list has one quiet, ruled search field below its heading and
Library filters. Its label names the active view (including Unread, Starred,
or the selected feed). Search stays within that view and retains its existing
order. Changing Library filters carries the query along. `/` focuses the field,
`\` clears the query from the list, and Escape leaves the field and returns
focus to the selected article. Typing updates the query after a brief pause;
it does not add a history entry for every word or steal focus when results arrive.

The field searches titles and locally cached article text. Words are literal
and case-insensitive, and every word must match a word beginning in the title
or text. A short help line states this behavior. There is no relevance ranking,
result count, remote request, or suggested query. Empty results say that no
articles match in the current view and offer to clear the search. The query,
loaded pages, selection, and scroll survive opening an article and returning.

Older cached text is prepared in the background. While it is being prepared,
a quiet line says that titles are available. If some text cannot be prepared,
the line says so and offers a retry. No spinner or modal blocks local reading.

The status strip is small, at the bottom, showing quiet operation and error messages without counts. It's the system-tray-style surface we use *instead of* OS notifications. Users who want to ignore it can hide it; it never demands attention.

### The reading view

The reading view is the application's most important screen and the one that should feel most like *the page* rather than the application.

```
┌──────────────────────────────────────────┐
│                                          │
│           Title Of The Article           │
│           by Author Name                  │
│           Source · 12 min · 2026-04-30    │
│                                          │
│   The article body, set in our chosen    │
│   serif at the user's chosen size, with  │
│   generous line height, indented         │
│   paragraphs, and properly handled       │
│   typography…                            │
│                                          │
│                                          │
│           ─ end of article ─              │
│                                          │
│   [open original]  [send to ereader]      │
│                                          │
└──────────────────────────────────────────┘
```

The header is small and quiet: title, byline, source/length/date in muted secondary text. No related-articles strip, no engagement bar, no recommended-for-you. Just the article.

The reader has a slim persistent action rail above the scrolling article area.
It exposes a named return destination, plain Previous/Next navigation, sidebar
visibility, star/unstar, mark read/unread, open original, and item info. Previous
and Next only navigate. A separately labeled “Mark read & next” action and `n`
explicitly complete the current article. At the final item, completion returns
to the originating list. `p` browses backward without marking read. Existing
action identifiers remain valid for custom keymaps. The title and byline align
with the left edge of the body measure.

The item-info surface is user-initiated and appears in the article scroll area before the text, again without overlaying the article. It holds metadata that would clutter the article header: source/feed, canonical URL, cache state, discovery/publish dates, length facts, and content-shape tags. `Esc` closes item info before it navigates away from the reader.

The footer is similarly minimal. It marks the end of the article and may repeat a link to the original so the user can verify the source or grab the URL. Send-to-ereader waits until that flow is real. That's it.

Publication dates in the reading header use a short calendar date; item info
retains the precise source timestamp. The reading measure uses the body font
and body size. Long URLs wrap, while wide code, tables, and display math scroll
within their own region. A missing readable cache is stated beside the available
summary and original link, rather than presented as a complete article.
Source images retain their colors on a light paper backing in every theme.
This keeps transparent ink diagrams and image-based equations readable in dark
themes without inverting photographs or illustrations.

Article links and Open original open in the system browser, preserving the
reader. Fragment links to targets in the cached article stay in the reader.
Selecting article text deliberately reveals a small Highlight / Add note
popover. These controls use the same selection and saved selectors as `h` / `H`.
The popover is dismissed when the selection clears or the reader scrolls.

When the user scrolls, the application chrome should fade — not literally animated, but the focus shifts to the text. The sidebar can remain visible but should not have any motion or attention-pulling elements.

### Lists

List views (Today, Library, Unread, Reading, Starred, and individual feeds) display items in chronological order by default unless the route is explicitly ordered by local action time: Reading uses the latest read-state update, and Starred uses the latest star update. Each item gets a row showing: title, source, time/length, a short excerpt (up to two lines), read state, and quiet star/read controls.

```
● Article Title                          Source · 5min   2h ago
  Brief excerpt of the first paragraph…  ⭑ #commons

○ Another article                        Site · 12min    yesterday
  Excerpt…
```

`●` indicates unread; `○` indicates another local read state. The title is a native button that opens the article. Quiet, separately labeled buttons star/unstar and mark read/unread without opening it. Keyboard cursor navigation remains primary. No counts or engagement signals.

The list cursor is a subtle background highlight or a thin left border, signalling which item is selected for keyboard actions.

Keyboard list movement keeps the selected row visible and focuses its open
control. Focusing any row control also selects that row. Native buttons and
links retain their Enter/Space activation; the keymap does not consume those
keys while a control is focused. Rows and reader controls remain mounted during
state changes so changing a label does not discard keyboard focus.

Lists initially show a bounded page. **Load older** extends the same list on
demand, retaining its selection and scroll position. The button disappears at
the end of the archive. Filtering applies to all saved articles before any
limit; an old unread or starred article remains reachable. Returning from
reading and refreshing after a local write preserve the loaded older boundary.

### Private state across devices

Private state synchronization preserves explicit intent. Concurrent read/unread
keeps unread, star/unstar keeps starred, and feed add/remove keeps removed.
An explicit action after observing the conflict reverses the result normally;
these simple registers do not add conflict dialogs. Reading position has its
own history, so delayed scrolling cannot undo completion or mark-unread. A
causally later position can move backwards when rereading. Reading and Starred
ordering uses consistent display timestamps from the current history heads.

### Annotation review

Highlights uses the same editorial grid and explicit **Load older** interaction
as the article library. It shows the quote, original article context, and a
short excerpt of the latest note. A quiet **Notes** control expands all attached
notes in place; notes retain line breaks and each has an **Edit note** action.
Opening the source and returning restores the loaded pages, expanded notes,
selected highlight, focus, and scroll. Full notes use a readable line length.

Editing keeps the quote visible for context, preserves the note's identity, and
uses the existing focus-contained note editor. Cmd+Enter saves; Escape cancels.
A failed save restores the old note and retains the draft. A stale edit is
explained in the editor so the user can review the latest saved text while
retaining their draft. Local writes have no spinners.

Concurrent offline edits show **Review conflict** beside the note, without a
badge or notification. The control also appears on a collapsed highlight row
so conflicts remain discoverable without expanding every note. Review opens
the same focus-contained editor with each
retained version's full text, device label, and saved date. **Use this version**
copies that body into the editable draft; the user can also merge the text.
**Save resolution** creates a new version superseding the complete set being
reviewed. Cancel leaves all versions intact. If another version arrives during
review, saving retains the draft and explains that the versions changed; the
user can refresh the versions without replacing their draft. Ordinary **Edit
note** is available again after resolution. Markdown export labels every
unresolved version and includes its full body and stable revision reference.
Devices without editing permission can inspect every version but cannot save
a resolution.

Creating a highlight with a note also retains its draft on failure. If the
highlight succeeded first, retrying the note uses that confirmed highlight
instead of creating another one. The editor explains this partial save.

**Remove highlight** asks for a deliberate confirmation showing the quote and
stating that its attached notes will also disappear from this library. The
removed row disappears immediately; a failed write restores it and its selection
when the user has not moved elsewhere. Removing the
selected row selects its nearest remaining neighbor. There is no permanent
delete action hidden behind a keystroke without this confirmation.

**Export Markdown** uses a native save dialog and includes the entire surviving
annotation archive, with article titles and links, stable references, dates,
quotes, and full notes. It exports beyond the pages currently loaded. The
document remains readable without Carrel. Export and review contain no counts
or engagement signals. Friends and sharing remain separate work.

### The friends view

Shares from followed peers, in chronological order. Each share displays: the sharer's pet name, the share note (prominently — this is what they thought you should know), the item title, and small affordances to read, save, or react.

```
Sarah                                   3h ago
"This is the best framing of the commons 
problem I've read in years. Section 3 in 
particular."
  → On the Commons of the Internet
    Aaron Swartz · Aaron's Blog · 24min
  
Tom                                     yesterday
"For our climbing chat — really good piece on 
how guidebooks shape access politics."
  → Why guidebooks matter
    Climbing Mag · 8min
```

The note is the primary content; the item is the supporting context. This is the opposite of most "social" UI, where the source content is primary and the social wrapper is decoration. In Carrel, the note is the *act of curation*; the item is what's been curated.

## Keyboard

The sidebar's quiet **Devices** control opens desktop sync management. Local
reading requires no setup. The initial dialog offers **Set up sync** and **Use
an invitation**, with a desktop name and either a confirmed recovery passphrase
or pasted invitation. The desktop holding the encrypted master can create an
invitation or remove another desktop after unlocking it. Invitation text stays
selectable for ordinary copy/paste, expires after five minutes and is cancelled
when the dialog closes. Pairing and network work may show a disabled action;
local reading mutations retain their immediate optimistic behavior.

Incoming sync changes update lists, filters and annotations in place. An open
reader keeps its current scroll and unsaved note draft. Star actions capture
the clicked state, so a concurrent incoming update cannot invert the user's
request. Offline changes remain local until peers reconnect; there are no
badges, notifications, article counts or automatic completion.

The keymap is the primary input. Every action accessible via mouse must also be accessible via keyboard, ideally with a single keystroke or short sequence. Vim-style multi-key sequences where they make sense (`g g` for top, `g e` for end). Modifier keys reserved for global commands (`cmd-k` for command palette).

### Default keymap

**Navigation:**
```
j / k         next / prev item in list
J / K         next / prev unread item
o or Enter    open the cursored item
Esc or q      close current item / back
g g           top of list
g e           end of list
/             search current list
\             clear search
```

**Reading:**
```
Space         page down
S-Space       page up
n             next item, mark current as read (Reader's killer move)
p             previous item without changing read state
,             scroll to top of article
.             scroll to bottom of article
```

**Actions on cursored / open item:**
```
s             star / unstar
m             mark as read
M             mark as unread
t             tag (opens tag entry)
h             highlight selection (when text is selected)
H             highlight + add note
e             send to default ereader
E             send to ereader picker
c             share with last-used audience
C             share with audience picker
a             archive (mark read and hide from main lists)
```

**Modes / global:**
```
1             Today
2             Reading
3             Library
4             Highlights
cmd-B         show / hide sidebar while reading
?             keymap reference (overlay)
cmd-k         command palette
cmd-,         settings
```

**Within reading view, additional:**
```
←             previous unread (with prompt)
→             next unread (with prompt)
i             show item info (full metadata)
```

### Keymap stack

The keymap is implemented as a stack. The application root provides the base bindings (mode switches, command palette, help). Each route pushes its bindings on mount and pops them on unmount. The reading view layers further bindings on top of the current list view's bindings.

The dispatcher tries top-of-stack first; bindings cascade. This means the same key can mean different things in different contexts cleanly: `j` means "next item" in a list, "page down" inside an article. No giant match statements, no global state checks.

When a text input or contenteditable has focus, keymap dispatch is suspended except for explicitly marked global keys (`Esc` to dismiss, `cmd-Enter` to submit).

### Multi-key sequences

A sequence like `g g` is implemented with a small key buffer that times out after 500ms. Within the timeout, a second key extends the previous one. Outside the timeout, the second key starts fresh.

The help overlay shows multi-key sequences in their declared form (`g g`, `g e`).

### Customization

The keymap loads from `~/.config/carrel/keymap.toml`. The default keymap ships with the binary; user overrides merge on top. Action names are stable identifiers; keys are user-editable.

```toml
[bindings]
"j" = "next-item"
"k" = "prev-item"
"shift+j" = "next-unread-item"
"o" = "open-item"

[bindings.reading]
"h" = "highlight-selection"
"e" = "send-to-default-ereader"
```

Validation happens on load. Errors surface in the system status panel rather than blocking startup.

## Interaction principles

### Optimistic mutations

Actions that affect local state (star, mark read, tag, highlight) update the UI immediately, write to Cozo asynchronously, and reconcile silently. If a write fails — rare for local operations, more likely for sync — the UI reverts and a small toast surfaces in the status strip. Never a modal, never a blocking error.

This makes the app feel instant in the way Reader did: the UI is the user's intent, made manifest, with the database catching up behind the scenes.

### No spinners for local operations

Anything that touches only Cozo and the local filesystem must complete fast enough to not need a loading state. Cozo queries are microseconds. Renders are a frame. If a local operation is slow enough to warrant a spinner, that's a bug — fix the operation, don't add the spinner.

Spinners are acceptable only for genuinely network-bound operations (initial sync with a new peer, fetching a large blob), and even then they should be small and non-blocking.

### Errors are quiet

Errors live in the status strip, not in modals. A failed feed fetch, a broken sync session, a missing blob — these get logged and surfaced for users who want to look. The reading flow is not interrupted.

The exception: errors that *prevent* the user's just-attempted action (a permission failure, a malformed input) get a small toast near the action point, dismissable by clicking or pressing any key.

### Motion

Animations are short and meaningful. Defaults: 150ms ease-out for opens; 200ms for transitions between views; 100ms for hover affordances. Anything longer is suspect.

We use motion to convey state change (a thing arriving from a direction, a panel sliding away). We do not use motion for delight, polish, or pacing. Reading is the activity; the UI shouldn't be making little performances during it.

Reduced-motion preference: if the user has `prefers-reduced-motion: reduce` set, we replace motion with crossfades and skip the directional movement.

### Focus management

Tab order follows visual order. Focus rings are visible but restrained — a 2px outline in the accent color, on focusable elements only when focus arrives via keyboard. (We use `:focus-visible`, not `:focus`, so mouse clicks don't show focus rings.)

Modal-like elements (the share dialog, the tag input) trap focus and restore it on close. Esc closes them. Enter confirms them.

Feed and note dialogs use the native modal dialog behavior: background content
is inert, Escape cancels, and closing restores the initiating control. Feed
titles update in place when fetching discovers the publisher's name.

Screen-reader users can drive the entire app. Every interactive element has appropriate ARIA. We do not use `<div>` for buttons; we use `<button>`. We do not implement custom dropdowns where `<select>` works.

## Component patterns

### Read state indicator

A small filled or empty circle: `●` for unread, `○` for read, partially-filled for in-progress. Subtle, no animation. A muted accent color signals "currently open."

### Star

A small star icon, filled when starred, outlined when not. Single click to toggle.

### Tags

Small lowercase labels with a subtle background tint, separated from titles. Clicking a tag filters the current view to items with that tag. No icons; just text.

### Feed management

The sidebar Feeds section lists subscribed feed titles, falling back to host or URL when the feed has not been fetched yet. Feed rows are plain navigation rows with a small explicit remove affordance. Selecting a feed opens a normal item list filtered to that feed. No unread counts, badges, or freshness counters appear beside feeds.

Adding a feed uses a compact user-initiated overlay with one URL input. Saving
the subscription closes the dialog and exposes the subscribed feed immediately.
Its first fetch runs independently; a fetch failure does not undo or disguise a
successful subscription. Removing a feed makes clear that saved items remain in
Carrel; removal only stops future fetches.

Subscription import and export use native file dialogs, reachable from the
sidebar and the empty library. OPML import adds new subscriptions atomically
and preserves existing settings when a URL appears again. Nested folders are
retained as readable paths in the existing folder-label field and exported as
named groups. Imported subscriptions become visible immediately; fetching runs
separately. Export contains URLs, titles, and folders, with no personal reading
state or HTTP cache metadata. Cancellation leaves the current view untouched.

Progress writes are coalesced and serialized, with the latest position flushed
when leaving an article. Progress updates preserve explicit read/unread choices.
Async list/article/highlight results apply only to the request and item that
started them. A failed optimistic action restores the affected state and reports
the failure quietly, preserving the working list or article.

Feed fetch progress and non-fatal errors surface in the status strip. The sidebar may expose a small explicit "Fetch" command, but it never animates or prompts for attention.

An explicit **Fetch** obtains the feed body again so failed article caching can
recover even when the publisher's feed has not changed. Scheduled polling keeps
conditional requests. Partial content failures distinguish a successfully saved
feed from an incomplete article cache; diagnostic logs include a sample reason.

### Share affordance

In a list, a subtle indicator on items you've shared (a small dot next to the title). In the reading view, the share button appears in the footer along with other actions. Hovering or focusing it reveals the audience(s).

### Highlight rendering

User's own highlights: a flat yellow background (`--color-highlight`), text color unchanged.

Friend's highlights: a thin underline in `--color-highlight-friend`, no background. Distinct enough to recognize, subtle enough not to compete with the user's own.

When multiple friends have highlighted the same passage, we show the underline once with a small marker (a number or stack icon) that on hover lists which friends.

### Share-with-note

A modal-like overlay (centered, focus-trapped) with two fields: the note (autofocused), and the audience selector (defaults to last-used). Submitting (`cmd-Enter` or click) closes and shares; `Esc` cancels.

The audience selector is a small set of chips for the user's audiences; clicking toggles inclusion. The selector remembers the last-used set — for most users sharing happens to the same audience repeatedly, and we should make that one keystroke.

## The command palette

`cmd-k` opens the command palette. It is the power-user surface and the universal escape hatch.

The palette is a single text input plus a results list. Typing fuzzy-matches against:

- Commands: every action in the app, by name and keyboard shortcut
- Items: titles of saved items
- Feeds: subscribed feed titles
- Peers: pet names and self-described names
- Tags: every tag you've used

Matches from different categories are interleaved by relevance. Results have a small category badge to distinguish them.

Sublime-quality fuzzy matching: characters in the query must appear in order in the target, but not contiguously; matches earlier in the string and at word boundaries score higher.

`Enter` activates the selected result. `Esc` closes the palette without action. Up/down arrows navigate.

The palette is *always* available — even inside a modal, even while typing in a note. It's the user's ripcord.

## Anti-patterns

These are design moves that conflict with the principles and that we explicitly reject:

**Unread badges in app icons or system trays.** The whole point is no notifications outside the app. Adding a number to the dock icon is the camel's nose under the tent.

**Engagement-driven sorting.** Items appear in the order they arrived (or the order the user chose). No "trending," no "you might like," no "popular among your friends."

**Auto-clearing read counts.** Items become "read" because the user marked them so, not because they scrolled past. The data is honest about what the user has done.

**Modal interruptions.** Modals are reserved for actions the user explicitly initiated (share dialog, tag entry, audience picker). They are never used to surface status, errors, or prompts.

**Hover chrome.** UI that appears on hover and disappears when the cursor leaves is hostile to keyboard users and creates visual jitter. Affordances should be persistently visible (subtly) or only appear in response to deliberate action.

**Animation as decoration.** Every animation must convey something. "It looks nice" is not enough.

**Variable-width chrome.** The sidebar is a fixed width; the reading measure is a fixed width. We do not try to fluidly resize chrome to "match the user's window." Users with extremely narrow or wide monitors get different defaults; we don't reflow continuously.

**Onboarding tutorials.** The first run shows OPML import (or skip) and that's it. Discovery happens through use. The keymap reference (`?`) is the user's reference, not a forced walkthrough.

**Marketing or promotional surfaces in-app.** No "what's new" splash screens. No "upgrade" prompts (there's nothing to upgrade to). No surveys. The app does its job.

## What this document does not specify

A few things are deliberately left to designer judgment within these constraints:

**Exact icon design.** Icons follow the Lucide convention (24px, 1.5px stroke, simple geometric forms). Specific icon choices are made per-component without ceremony.

**Microcopy details.** Button labels, empty states, tooltips. We aim for short and direct; the exact wording isn't pinned in this document.

**Spacing details.** A 4px-based spacing scale is used throughout; specific values are chosen per-component.

**Subtle interaction details.** Hover state colors, focus ring exact shades, animation easing curves — these have defaults documented above but adjustments at the component level are fine without amending this doc.

When in doubt, the principles above govern. If a design choice would feel like Substack, Twitter, Notion, or LinkedIn, it's probably wrong for Carrel. If it would feel like Sublime Text, mutt, iA Writer, or a well-designed library, it's probably right.
