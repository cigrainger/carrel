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
  --color-text: #1a1a1a;
  --color-text-secondary: #5a5a5a;
  --color-text-tertiary: #8a8a8a;
  --color-bg: #fafaf7;          /* off-white, easier than pure white */
  --color-bg-elevated: #ffffff;
  --color-rule: #e5e5e0;
  --color-accent: #b85c38;       /* one accent, used sparingly */
  --color-highlight: #fff3a8;    /* highlight yellow, low saturation */
  --color-highlight-friend: #b8d4e8;  /* friend's highlights - distinguishable */
}
```

The off-white background (`#fafaf7`) is intentional. Pure white is harsh for sustained reading; a slight warm tint reduces eye strain. The accent (`#b85c38`) is a warm rust — distinctive without being shouty.

### Dark theme

```css
[data-theme="dark"] {
  --color-text: #e8e6e0;
  --color-text-secondary: #a8a6a0;
  --color-text-tertiary: #6a6864;
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
  --color-text-tertiary: #9a8e7e;
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
  --color-text-tertiary: #5a564e;
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

The user's choice is stored in `:self` Cozo state and applied via a `data-theme` attribute on `<html>`. Theme switches are instant — no animation, no fade. The reading view should look the same; only the colors change.

We do not auto-switch based on system theme by default. Some users want their reading app to be different from their system. The user can opt in to system-following in settings.

## Layout

### Application chrome

The desktop layout has three primary regions: a sidebar (subscriptions, lists, peers), a main content area (lists or reading view), and a status strip. The sidebar can collapse; in a reading view, the sidebar should be hidden by default to maximize reading focus.

```
┌─ Sidebar ─┬─────── Main content ─────────┐
│           │                                │
│ Today     │                                │
│ Friends   │     [list view or              │
│ Library   │      reading view]             │
│ Highlights│                                │
│           │                                │
│ ── Lists ─│                                │
│           │                                │
│ ── Feeds ─│                                │
│           │                                │
└───────────┴────────────────────────────────┘
   ↑ status strip (sync, errors, system status, optional)
```

The sidebar is the navigation root: the five primary "places" plus user-created lists and the feed list.

The status strip is small, at the bottom, showing minor system state: last sync time, fetch progress, error count. It's the system-tray-style surface we use *instead of* OS notifications. Users who want to ignore it can hide it; it never demands attention.

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

The footer is similarly minimal. A link to the original (so the user can verify the source or grab the URL), and a "send to ereader" action when the user wants to take it elsewhere. That's it.

When the user scrolls, the application chrome should fade — not literally animated, but the focus shifts to the text. The sidebar can remain visible but should not have any motion or attention-pulling elements.

### Lists

List views (Today, Friends, Library) display items in chronological order by default. Each item gets a row showing: title, source, time/length, an excerpt (one line truncated), and small affordances for tags or read state.

```
● Article Title                          Source · 5min   2h ago
  Brief excerpt of the first paragraph…  ⭑ #commons

○ Another article                        Site · 12min    yesterday
  Excerpt…
```

`●` and `○` indicate read/unread state. Star is a small affordance, tags are small text labels. No counts, no engagement signals.

The list cursor is a subtle background highlight or a thin left border, signalling which item is selected for keyboard actions.

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
p             previous item, mark current as read
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
2             Friends
3             Library
4             Highlights
5             Lists
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

Screen-reader users can drive the entire app. Every interactive element has appropriate ARIA. We do not use `<div>` for buttons; we use `<button>`. We do not implement custom dropdowns where `<select>` works.

## Component patterns

### Read state indicator

A small filled or empty circle: `●` for unread, `○` for read, partially-filled for in-progress. Subtle, no animation. A muted accent color signals "currently open."

### Star

A small star icon, filled when starred, outlined when not. Single click to toggle.

### Tags

Small lowercase labels with a subtle background tint, separated from titles. Clicking a tag filters the current view to items with that tag. No icons; just text.

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
