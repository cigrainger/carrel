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

### Public guide

The public site under `site/` is a static mdBook with two reading scales: an
editorial introduction and a practical reader's guide. Both share the app's
Paper, forest, moss and Dark roles below, its Ribbon mark, square controls and
restrained rules. The introduction uses a broad, asymmetrical composition and
large grotesk typography; the guide retains a 760px reading column, 18px serif
prose, a contents sidebar and local search. Narrow screens stack the introduction
and use the guide's existing collapsible contents. Print keeps the content and
omits navigation. The home page's reader specimen is explicitly illustrative,
not a screenshot or a claim about delivered features.

Inter and Source Serif 4 (regular and italic) are copied from the app's bundled
OFL assets, with their licenses, into `site/src/assets/`. No font build step or
remote font service is introduced. A small header partial supplies ordinary
home, guide, peer-to-peer explanation and source links. An optional script
connects its Light / Dark control to mdBook's existing theme preferences; all
reading and navigation remain available without that script. The default mdBook
template, search, chapter controls and theme machinery remain upstream-maintained.

Desktop chapter arrows are bounded 44px square controls in the reading gutters;
their hover and focus treatments stay within those controls and never cover the
masthead or article. Narrow screens retain mdBook's footer chapter navigation
with the same square controls. Their ordinary links and keyboard shortcuts remain.

The opening identifies Carrel as a peer-to-peer reader. A section immediately
below it illustrates two readers connected by explicit, verified contact details:
the selected article, cached images and share note travel to the friend, while
personal annotations and reading state stay private. It explains waiting while
offline, relay connectivity when needed, and separate private own-device sync.
This illustration shows one person's connection, not a public network or group
feed. The application also supports deliberate shares to fixed groups.
The introduction then covers reading and keeping passages and distinguishes
the current pre-alpha from the wider vision. Installation and recovery details
belong to the guide rather than the opening paragraph.
Focus uses the same rust/pale-coral roles as the app. There are no analytics,
signup forms, remote assets or animated promotional surfaces. Generated output
stays out of the repository. Markdown, the header partial, one stylesheet and
the small theme-control script are the maintained site sources.

### Reading Room revision of Field

The adopted 12 October 2026 study is `carrel-site/dist/system.html` and
`studio.html`, from the supplied Reading Room website/design output. Its
`assets/tokens.css`, `tokens.json` and `app.css` define the visual vocabulary:
neutral Paper, forest structure, moss secondary surfaces, restrained rules,
grotesk finding/navigation typography and serif reading. The local study was
inspected alongside its interactive Today, Reader and Highlights views.
The website's poster scale informs hierarchy rather than becoming app chrome.

Adoption translates those roles into the existing Leptos components. The
production app retains its 216px sidebar, equal 16px system labels, 56px context
bar, real native window-control reserve, Lucide actions, keymap stack and native
dialogs. Stored Light preferences become the Paper appearance without changing
their `light` value; Sepia and Black remain available. Existing font choices,
preference bounds, body size/measure/leading, loaded pages and passage anchors
remain the interaction contract. Demo content and temporary state are not used.
The study's future sharing view defines no new feature scope; implemented
People/sharing behavior continues under the same visual tokens.

Field replaces the previous Editorial implementation following a design reset.
The new references emphasize dramatic scale contrasts, asymmetrical grids,
confident grotesk typography, structural color fields, and geometric rhythm.
Those principles must organize the reading task rather than decorate it.
The exploration in `docs/prototypes/carrel-directions.html` presents three
alternatives using the same archive content and reader interactions:

- **Index:** horizontal destination navigation, a collapsible source index,
  and a chronological gutter. Cool white / cobalt and dark ink; a 64px section
  heading, 48px date, and 27px grotesk article titles.
- **Fold:** an architectural side panel holding destinations and sources,
  with the selected page meeting that panel at a clear seam. Clay, pale ivory,
  and ink; a 60px section heading and 27px grotesk article titles.
- **Field:** full-width destination navigation and article rows, with sources
  in a drawer rather than permanently consuming a column. Forest and pale mint;
  an expressive navigation band with 22px labels / a 32px active destination,
  and 29px grotesk article titles. The band contracts in the reader.

Field is the selected visual direction; Index and Fold remain comparison
proposals. The production layout now puts navigation and Sources in one left
sidebar, keeping Field's color fields and chronological composition. The original
prototypes document the exploration, not the current application chrome.
Their names are not user-selectable app themes.
The reader keeps the user's body font, a comfortable measure, and restrained
chrome. All alternatives
retain Today / Reading / Library / Highlights, explicit read and star actions,
publisher dates, square controls, Lucide icons, and the sidebar toggle. They
introduce no unread counts or rankings.

### Layout and type

The primary navigation and Sources share one left sidebar. Today, Reading,
Library and Highlights sit above the source index; Settings sits at the bottom.
Destination labels use equal-size 16px system type, with a narrow accent
rule and background for selection. Selection never changes font size or weight.
A single 56px forest context bar spans the window. Its leading sidebar toggle
stays in place; the remaining controls follow the current task. Lists show the
destination or source, refresh and Save article. The reader shows return,
previous/next, read/star, item info and Open original in this same bar, with Aa
for quick reading adjustments. App commands live in a quiet overflow menu.
The Ribbon wordmark belongs at the top of the sidebar. Cmd+B toggles the same
sidebar in lists and the reader.

Lists occupy the available content width. A chronological gutter shows a prominent
37px day at each new date and a quieter date on subsequent rows from that day.
The backend supplies calendar labels, explicitly distinguishing publication
from saved dates when the publisher provides none. Article titles use a 26px,
500-weight grotesk; metadata precedes them, serif excerpts follow, and read/star icons sit at the
right. Long titles wrap. Source Serif 4 remains the default reader face.
Controls remain square. Reading settings offers Editorial and Dense list density.
Dense keeps the same rows, dates, read/star controls and keyboard selection,
with smaller dates, 18px headings and no excerpt. Titles still wrap.
Switching density preserves the visible row and selection rather than remounting
the list. It applies to article lists; annotations keep their reading measure.

Section headings are 40px display type, with 36px at the minimum desktop pane.
The reader title is 40px grotesk, with 1.06 leading and −0.04em tracking.
Source, author, date and duration precede the title. A provided summary may
form a serif deck, followed by a rule; cached-summary fallbacks are not repeated.
The sidebar is 216px wide. Editorial rows use a 72px date gutter and stacked
read/star controls. When the content pane is 760px or narrower, they use a 56px
gutter, 32px day and 24px title.
Dense retains 18px titles and its smaller dates. This responds to the available
content width, so the open sidebar does not squeeze a large gutter and heading
into the minimum desktop window.
Paper uses `#f3f4ea` and elevated `#fafbf5`, with forest ink `#173d32`,
secondary ink `#536359`, rules `#c5cdbb` / `#819180`, and forest accent.
The navigation field uses `#173d32` with `#f3f4ea` text; the sidebar uses
`#e3e7d6`. Dark uses `#18251f`, pale `#eff2e4` ink and a `#102c22` bar.
Selection is a flat `#e1e8d4` / `#314a36` surface and 2px rule. Focus uses
`#a23423` on Paper and `#e7ad83` on Dark. Signal `#da402c` is an identity
graphic color, not application alert text. Sepia and black remain
available, retaining readable navigation pairs. These additions are production
tokens: `--font-display`, `--text-list-title`, `--text-nav-label`,
`--text-calendar`, `--navigation-height`,
`--color-nav-bg`, `--color-nav-text`, and `--color-source-bg`.

Highlights use a ruled, 760px reading column: source context before a 24px
serif quotation, a 16px serif note beside a structural rule, and persistent
quiet actions. Selection remains visible and keyboard-accessible. Native
dialogs use square borders and theme surfaces with a forest-tinted backdrop;
decorative shadows are removed. Focus is a distinct 2px rule, including search
inputs. Header controls retain a high-contrast paper outline on the forest bar.

Navigation uses Lucide icons with labels and a left rule for the active
destination. Sources stay in the left sidebar; subscription management, Devices
and OPML actions belong to Settings.
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

The application mark uses three printed lines turning into a C, with a small
coral cut at the end of the outer line. Forest and mint connect it to Field;
the repeated curves draw on the striped poster references. Ribbon is the
selected direction in `docs/prototypes/carrel-icon-directions.html`; Shelter
and Counterform remain comparisons. A monochrome version appears in the sidebar.
Its editable source is
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

Reading settings and Aa expose these four reading fonts with text size (14–26px),
line width (52–82ch) and line spacing (1.45–1.85). Changes preview directly in the
current article and retain the visible passage. Reset reading restores the
default typography without changing theme or density. Preferences are local to
this installation, using the existing webview preference storage; synchronization
of presentation settings remains future work. Every font is bundled for offline
use. Controls use the system font and native select/range elements.

For users who really want sans-serif body, we offer **Inter** (Rasmus Andersson, OFL). One option. No second sans.

For menus and settings labels we use the **platform system font**, with bundled
Inter as fallback. Field's wordmark and article headings use `--font-display`
(Helvetica Neue, with bundled Inter as fallback).
Article prose retains its independently chosen reading font.

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
  --text-display: 3rem;
  --text-reader-title: 2.25rem;
  --text-list-title: 1.8125rem; /* article heading */
  --text-nav-label: 1rem;
  --text-calendar: 3.5rem;
  
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

**Math**: recognized TeX spans are converted to MathML during extraction using
KaTeX. The webview renders native MathML, retaining display layout and structural
operators through cache sanitization. Display equations scroll within their own
region rather than widening the article.

**Code**: bundled JetBrains Mono, with long lines scrolling inside their own
region and a background matching the selected theme. Syntax highlighting at
ingest remains planned; plain code is retained without an extra frontend runtime.

**CJK**: native per-character font fallback currently supplies glyphs outside
the bundled Latin faces. A Source Han Serif companion and tuned CJK typography
remain planned.

**RTL languages**: the body uses `dir="auto"` and cached blocks retain the
publisher's `dir` and `lang` attributes. Lists and quotations use logical padding
and borders so indentation follows the text direction.

## Color and theming

Color is functional, not decorative. The accent color appears sparingly and only where attention is genuinely warranted (a hover state, a small affordance). The base palette is restrained: text, two or three secondary text shades, background, elevated background, rule lines, and a single highlight color.

### Paper theme tokens (stored as light)

```css
:root {
  --color-text: #173d32;
  --color-text-secondary: #536359;
  --color-text-tertiary: #536359;
  --color-bg: #f3f4ea;
  --color-bg-elevated: #fafbf5;
  --color-media-bg: #faf8f3;    /* paper backing for source images in every theme */
  --color-rule: #c5cdbb;
  --color-rule-strong: #819180;
  --color-accent: #173d32;
  --color-nav-bg: #173d32;
  --color-nav-text: #f3f4ea;
  --color-source-bg: #e3e7d6;
  --color-highlight: #ece7ac;
  --color-highlight-friend: #617e99;
  --color-selected: #e1e8d4;
  --color-focus: #a23423;
  --color-moss: #d5dfb9;
  --color-signal: #da402c;
  --color-backdrop: #102c2260;
}
```

The neutral paper field and forest toolbar establish the Reading Room composition.
Selection and action affordances use forest ink sparingly. Moss and signal are
identity graphic roles; signal is not alert text. Publisher
images keep a neutral paper backing across themes.

### Dark theme

```css
[data-theme="dark"] {
  --color-text: #eff2e4;
  --color-text-secondary: #b0bdab;
  --color-text-tertiary: #b0bdab;
  --color-bg: #18251f;
  --color-bg-elevated: #203128;
  --color-rule: #405344;
  --color-rule-strong: #6d816c;
  --color-accent: #d5dfb9;
  --color-nav-bg: #102c22;
  --color-nav-text: #f3f4ea;
  --color-source-bg: #21362b;
  --color-highlight: #555324;
  --color-highlight-friend: #9cb6cd;
  --color-selected: #314a36;
  --color-focus: #e7ad83;
}
```

Dark mode retains a forest band and pale type; it does not invert the band into
a light block. Text is pale `#eff2e4`.

### Sepia theme

```css
[data-theme="sepia"] {
  --color-text: #3a2f24;
  --color-text-secondary: #6a5d4e;
  --color-text-tertiary: #685c4b;
  --color-bg: #f4ecd8;
  --color-bg-elevated: #fbf4e5;
  --color-rule: #d8cdb3;
  --color-rule-strong: #9b8b6a;
  --color-accent: #8a4a2a;
  --color-nav-bg: #463923;
  --color-nav-text: #f4ecd8;
  --color-source-bg: #e6dcc2;
  --color-highlight: #d8c878;
  --color-highlight-friend: #b8c8a8;
  --color-selected: #e9dec5;
  --color-focus: #8d3327;
}
```

For users who like the e-reader feel.

### OLED black theme

```css
[data-theme="black"] {
  --color-text: #eff2e4;
  --color-text-secondary: #b0bdab;
  --color-text-tertiary: #b0bdab;
  --color-bg: #000000;            /* true black for OLED */
  --color-bg-elevated: #0e0e0c;
  --color-rule: #293a2e;
  --color-rule-strong: #536951;
  --color-accent: #d5dfb9;
  --color-nav-bg: #0b1e17;
  --color-nav-text: #f3f4ea;
  --color-source-bg: #101c15;
  --color-highlight: #4a3d10;
  --color-highlight-friend: #1d2e3d;
  --color-selected: #1d3023;
  --color-focus: #e7ad83;
}
```

For night reading on OLED screens. Pure black saves power and reduces light bleed.

### Theme switching

The current implementation stores the theme in webview local storage and applies
`data-theme` to the document and application shell. Settings → Reading and the
reader's Aa control share the theme choices. Moving preferences into `:self`
Cozo state is future work. Theme switches are instant; only the colors change.
Native window and dialog appearance follows the selected reading theme: Paper
and Sepia use light native controls, Dark and Black use dark controls. The native
window backing color matches the navigation field so exposed window edges remain coherent.

Aa opens downward from the context bar into a bounded, scrollable popover. A
quiet background marks the open state. Theme choices use 30px system-font
controls and identify the selected theme. Theme, font and range changes leave
the popover open for direct comparison. Escape closes it before any underlying
reader action and restores trigger focus; clicking outside or tabbing away
dismisses it without taking focus from the next control. Aa includes typography;
list density belongs to Settings → Reading. Cmd+, closes Aa and opens Settings.
Theme and reading settings are independent. Layout changes retain a visible text
character or article row at its viewport position, including delayed loading of a bundled font.
The bundled faces include real italic files for quoted and emphasized prose.

We do not auto-switch based on system theme. Some users want their reading app
to be different from their system. System-following remains future work.

## Layout

### Application chrome

The desktop layout has one compact context bar, a left sidebar, main content area,
and status strip. The sidebar starts visible and retains the user's visibility
choice across lists and articles. Sources scroll independently, keeping primary
navigation and utility controls reachable with a large subscription collection.
Below the supported desktop minimum, the sidebar overlays the left edge rather
than becoming a horizontal source strip; choosing a destination or source closes
that narrow overlay.

The sidebar ends with one Settings control. Settings opens a dedicated dialog
(Cmd+,), with Reading, Sources, People, Devices and Keyboard sections. Reading holds
theme, typography and list density; Sources holds subscriptions, OPML and source
recovery; People holds trusted contacts and sent vouches; Devices holds desktop
pairing; Keyboard shows the effective keymap.
Closing Settings returns to the same article or list, with its focus and visible
passage retained. Sections have equal-size labels and preserve drafts while the
dialog stays open. Closing it clears device passphrases and cancels an unused
pairing invitation.

A new installation with no saved articles or subscriptions offers one compact
Welcome to Carrel sheet. Add source, Save article and Import OPML use the existing
flows; Not now dismisses it. There are no tutorial steps, account requirements,
suggested subscriptions or progress indicators. Existing archives skip the sheet,
including archives whose Today list is empty. The choice is remembered per
installation. Getting started in the app menu reopens it over the current view.
Import cancellation and failure keep the sheet available; a failure explains how
to retry. Source and article capture keep their ordinary drafts and recovery.

Aa opens a small quick-reading popover in the context bar for theme and
typography. It shares the same preferences and controls as Settings. The status
strip contains operation messages and contextual recovery, with no preferences.
Commands and Keyboard reference remain reachable through the context menu and
their existing keyboard shortcuts. Aa and the app menu dismiss on Escape,
outside pointer actions or focus moving outside, preserving the next control's
focus. These controls share 14px system type,
30–34px square hit areas, a quiet hover fill and a visible keyboard focus ring.

On macOS, Field extends into the native titlebar. Native traffic lights and
window decorations remain in place; the separate title text is hidden. The
context bar is 56px high in lists and the reader. Its toggle sits at the trailing
edge of the sidebar's column, with the same 16px inset as the sidebar controls.
It stays at that position when the sidebar is hidden, keeping its relationship
to the panel clear. The macOS traffic-light area remains reserved. Below 720px,
the compact overlay layout keeps the toggle just after that reserve.
Blank header space can drag the window
and supports double-click zoom through Tauri. Navigation links and
toolbar buttons remain ordinary clickable controls. The toolbar uses quiet
borderless square buttons, with a hover or pressed fill. Chrome dims gently
when the window is inactive. Other desktop platforms keep their native titlebar.
The chrome uses the platform system font; article headings retain Field's
grotesk and article prose retains the chosen reading face.

```
┌───────────────────────────────────────────────────┐
│        ◧   current context / reader controls  ··· │
├───────────────────────────────────────────────────┤
│ carrel      │ date   source · length     actions │
│ Today       │                                    │
│ Reading     │        article heading             │
│ Library     │        excerpt                     │
│ Highlights  │                                    │
│ Sources     │                                    │
│ …           │                                    │
│ Settings    │                                    │
├───────────────────────────────────────────────────┤
│ status / contextual recovery                      │
└───────────────────────────────────────────────────┘
```

The sidebar has four primary destinations: Today, Reading, Library, and
Highlights. All, Unread, and Starred are filters inside Library, with their
existing routes retained. Friends and Lists appear when their first useful flows
work. Source settings holds subscription management. There are no counts or
badges. Today means posts published in the past 24 hours, with first arrival as
the fallback for undated posts. Its subtitle states the publication window
explicitly. Older posts remain in Library and individual feed views.
Empty Today and Reading views offer a quiet Browse Library link so their
suggested next step is directly available. Empty search results retain Clear
search instead of taking the user out of the current view.

There is one reader layout with the same collapsible navigation sidebar.
Cmd+B and a visible toolbar control show or hide it without remounting
the article. The visible text passage or list row stays in place when the
available width changes, including at the minimum desktop width. The choice carries across articles
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
│   ────────────────────────────────────   │
│   [open original]  [mark read & next]     │
│                                          │
└──────────────────────────────────────────┘
```

The header is small and quiet: title, byline, source/length/date in muted secondary text. No related-articles strip, no engagement bar, no recommended-for-you. Just the article.

The reader uses the window's single context bar above the scrolling article area.
It exposes a named return destination, plain Previous/Next navigation,
star/unstar, mark read/unread, open original, and item info. Source visibility
is controlled from the window toolbar or Cmd+B. Previous
and Next only navigate. A separately labeled “Mark read & next” action and `n`
explicitly complete the current article. At the final item, completion returns
to the originating list. `p` browses backward without marking read. Existing
action identifiers remain valid for custom keymaps. The title and byline align
with the left edge of the body measure.

The item-info surface is user-initiated and appears in the article scroll area before the text, again without overlaying the article. It holds metadata that would clutter the article header: source/feed, canonical URL, cache state, discovery/publish dates, length facts, and content-shape tags. `Esc` closes item info before it navigates away from the reader.

The article ends with one ruled footer aligned to its reading measure: Open
original on the left and Mark read & next on the right (Mark read & return for
the final item). Both use the same 30px square, icon-and-label system-font
controls as the context bar, with a quiet hover background and a visible
keyboard focus ring. There is no separate completion prompt or filled call to
action. Scrolling to this footer does not mark the article read. Saved summaries
retain their explicit cache notice above the body. Send-to-ereader waits until
that flow is real.

Publication dates in the reading header use a short calendar date; item info
retains the precise source timestamp. The reading measure uses the body font
and body size. Long URLs wrap, while wide code, tables, and display math scroll
within their own region. A missing readable cache is stated beside the available
summary and original link, rather than presented as a complete article.
Source images retain their colors on a light paper backing in every theme.
This keeps transparent ink diagrams and image-based equations readable in dark
themes without inverting photographs or illustrations.
If a responsive image fails, the reader tries its saved main image. If that also
fails, a compact “Image unavailable” placeholder retains any alternative text
and the publisher's caption. It never reserves a large empty paper rectangle;
Open original remains available in the footer.

Article links and Open original open in the system browser, preserving the
reader. Fragment links to targets in the cached article stay in the reader.
Selecting article text deliberately reveals a small Highlight / Add note
popover. These controls use the same selection and saved selectors as `h` / `H`.
The popover is dismissed when the selection clears or the reader scrolls.

When the user scrolls, the application chrome should fade — not literally animated, but the focus shifts to the text. The sidebar can remain visible but should not have any motion or attention-pulling elements.

### Lists

List views (Today, Library, Unread, Reading, Starred, and individual feeds) display items in chronological order by default unless the route is explicitly ordered by local action time: Reading uses the latest read-state update, and Starred uses the latest star update. Each item gets a row showing: title, source, time/length, a short excerpt (up to two lines), read state, and quiet star/read controls.

The visible date uses publication time when supplied, with first arrival as the
fallback for undated posts. Today uses that same date for membership and order,
within a rolling 24-hour window ending when the first page is requested; future
dates are excluded. Both window boundaries stay fixed while loading subsequent
pages. Library, Unread, and feed views use first arrival for chronological order.
Refreshing existing posts does not reset their arrival date. Publication date
corrections can change their position and membership in Today.

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

### Carrel links

Saved items have stable `carrel://item/<id>` addresses; highlights have
`carrel://highlight/<id>` addresses that open their exact passage. These use the
existing saved IDs, independent of titles, list positions or note revisions.
Copy Carrel link is available from the app menu, reader Info and highlight rows;
Cmd+Shift+C copies the current reader item or selected list item/highlight through
the keymap stack. Copying changes no reading or sharing state. Clipboard failure
offers the complete selectable link in a native dialog. Markdown annotation
exports retain publisher URLs and add Carrel links for returning to the archive.

An app link opens content already present on that desktop, including content
received through ordinary private sync. It carries no content or access grant.
Missing or removed targets leave the current view in place with a quiet
explanation. Incoming links wait while any dialog is open so note, URL, pairing
and palette drafts survive. The latest requested target opens after the dialog
closes. Opening the same highlight again refocuses its passage, including within
an already open article. The compact toolbar and ordinary reader-return behavior
remain intact.

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

The note editor offers Revision provenance on request. Cozo recursively follows
signed causal parents from every current head, showing revision and certified
device identities without selecting a conflict winner. Unsigned legacy migration
links are not presented as signed ancestry. Opening the disclosure leaves the
editable draft and reader context intact.

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

**Settings → Devices** opens identity setup and desktop sync management. Local
reading requires no setup. The first-desktop action creates an identity; a
separate action uses an invitation from another desktop you own. People and
Devices link to each other and explain their distinct access: a friend receives
deliberate shares; a paired desktop receives your private archive.
Setup, invitation creation and joining each show a short heading and instructions
for the desktop the user is currently on. Recovery copy explains that Carrel
cannot reset the passphrase. Generated invitations have an explicit **Copy
invitation** action with selectable-text fallback; they expire after five minutes
and are cancelled when Settings closes. Pairing success confirms authorization,
without claiming the archive has finished copying. Failed joins retain the
invitation/name and offer specific recovery for invalid or expired credentials.
Section changes retain pairing drafts. Cleanup is scoped
to the invitation that form owns: a delayed response from a
closed form must not cancel an invitation created after reopening Settings.
Pairing and network work may show a disabled action;
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
c             share with a person (explicit picker)
C             share with a person (explicit picker)
a             archive (mark read and hide from main lists)
```

**Modes / global:**
```
1             Today
2             Reading
3             Library
4             Highlights
cmd-B         show / hide navigation sidebar
?             keymap reference (overlay)
cmd-k         command palette
cmd-n         save an article URL
cmd-shift-n   add a source
cmd-r         refresh feeds
cmd-shift-r   source activity and recovery
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
Help uses a native modal with contained focus and restores the initiating
control. It lists only implemented actions and effective bindings in the current
stack, respecting custom overrides and shadowing. Unimplemented actions remain
known configuration names for compatibility but never consume keys or appear as
available commands. Text inputs permit only explicitly allowed bindings; native
dialog controls cannot dispatch reading actions behind them.
The context bar's app menu also offers named Commands and Keyboard reference
controls, so neither surface depends on learning its shortcut first. Cmd+B toggles
the sidebar from lists as well as the reader. Cached footnote/section jumps move keyboard focus to the
destination so subsequent Tab navigation continues there.

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

Errors that prevent the user's just-attempted action (a permission failure or
malformed input) appear inline with that action. Keep the form's draft and make
retry possible; an unrelated key must not dismiss an error or lose entered text.
An unknown route shows quiet explanatory text and an Open Library link, keeping
the current application shell available for recovery.

### Motion

Animations are short and meaningful. Defaults: 150ms ease-out for opens; 200ms for transitions between views; 100ms for hover affordances. Anything longer is suspect.

We use motion to convey state change (a thing arriving from a direction, a panel sliding away). We do not use motion for delight, polish, or pacing. Reading is the activity; the UI shouldn't be making little performances during it.

Reduced-motion preference: if the user has `prefers-reduced-motion: reduce` set, we replace motion with crossfades and skip the directional movement.

### Focus management

Tab order follows visual order. Keyboard focus uses a distinct 2px focus outline;
controls on the forest bar use the high-contrast navigation text color. Search
uses the same outline around its control group. Other controls use `:focus-visible`
so mouse clicks do not show focus rings.

Modal-like elements (the share dialog, the tag input) trap focus and restore it on close. Esc closes them. Enter confirms them.

Feed and note dialogs use the native modal dialog behavior: background content
is inert, Escape cancels, and closing restores the initiating control. Feed
titles update in place when fetching discovers the publisher's name.
Nested source dialogs return focus to Settings. If removing a source also removes
the initiating button, a surviving Settings control receives focus so Escape
and keyboard navigation continue within the dialog.

Screen-reader users can drive the entire app. Every interactive element has appropriate ARIA. We do not use `<div>` for buttons; we use `<button>`. We do not implement custom dropdowns where `<select>` works.

## Component patterns

### Read state indicator

A small filled or empty circle: `●` for unread, `○` for read, partially-filled for in-progress. Subtle, no animation. A muted accent color signals "currently open."

### Star

A small star icon, filled when starred, outlined when not. Single click to toggle.

### Tags

Small lowercase labels with a subtle background tint, separated from titles. Clicking a tag filters the current view to items with that tag. No icons; just text.

### Feed management

The toolbar's plus control opens **Save article**; Cmd+N opens the same
dialog through the keymap stack. The sidebar retains its separate **Add
source** control. Saving accepts an HTTP(S) article link or a bare website
address. It archives the page and its images, without subscribing to the site.
The input stays visible while saving and remains intact on failure for retry.
Cancel dismisses the dialog without navigating; an already submitted save may
finish in the background and report its result in the status strip. It cannot
close a newer dialog or navigate away from the user's current reading.

Successful saving leaves the current reading context in place and offers **Read
article** and **Done**, with an explicit **View in Library** link. A repeated URL
opens the existing article without resetting its read state or replacing its
archive. Partial image caching is stated beside the result. Esc closes the
dialog, focus returns to its initiating control, and no clipboard is read
automatically. A new article is unread; saving does not star or complete it.
Recent saved links also appear in Today under its ordinary publication-date
rule; the subtitle describes the library rather than only subscriptions.
When a saved page supplies no title or description, its opening excerpt supplies
the heading consistently in the confirmation, Library and reader.

The Sources section lists subscribed feed titles, falling back to host or URL
when the feed has not been fetched yet. Feed rows are plain navigation rows
with a quiet Organize control for folders and removal. Selecting a feed opens a normal item
list filtered to that feed. No unread counts, badges, or freshness counters
appear beside feeds.

Sources may have one optional folder, assigned in Settings → Sources. The folder
field accepts a new name or an existing one; blank means unfiled. Saving is
explicit and a failed save retains the draft. Folders appear alphabetically as
collapsible sidebar groups, with unfiled sources always visible first. Disclosure
and folder navigation are separate controls: selecting the name opens a combined
chronological list, with search, Load older and the usual reader-return context.
Folder membership is exact, including imported paths such as Research / Papers;
there is no hierarchy editor. A refresh never moves a source within its group.
Folders carry no counts and do not change saved articles or their reading state.
The sidebar also offers New folder: name it and choose its first source. Folders
are groups of subscriptions, so an empty group disappears when its last source
moves out. A source's quiet Organize control offers a native folder select,
New folder and the existing explicit removal confirmation. This provides the
keyboard alternative to dragging a source onto a folder heading or Unfiled.
Only a drag begun on a current source in this window can move it. A pending move
appears immediately, serializes further moves of that source and rolls back on
failure, retaining the current article or list. Folder disclosure and navigation
remain separate; dropping onto a collapsed group keeps it collapsed.
Source and folder article lists sort by publication date, newest first, with
first-arrival time used only for undated posts. Equal dates use a stable item-ID
tie breaker. A batch fetch or its feed-entry order cannot reverse the chronology.

Subscriptions use canonical URL order from their first appearance, matching the
store's list order. Fetching a title or updating fetch status does not reorder
the rows after the add dialog closes.

Adding a feed uses a compact user-initiated overlay accepting a website or feed
URL, including a bare website address. Discovery runs locally, first parsing a
direct feed, then inspecting standard alternate RSS, Atom, or JSON feed links.
Relative links resolve against the final page URL and its HTML base. Every
candidate is fetched politely and parsed before it is offered. One valid feed
is selected automatically; several valid feeds produce an explicit title-and-URL
chooser. An error leaves the input intact for correction or retry. Closing or
editing the form during discovery invalidates the pending result. Saving
the subscription closes the dialog and reveals Sources with the subscribed feed immediately.
Its first fetch runs independently; a fetch failure does not undo or disguise a
successful subscription. Removing a feed makes clear that saved items remain in
Carrel; removal only stops future fetches.

Subscription import and export use native file dialogs, reachable from
Settings → Sources and the empty Library. OPML import adds new subscriptions
atomically and preserves existing settings when a URL appears again. Nested folders are
retained as readable paths in the existing folder-label field and exported as
named groups. Imported subscriptions become visible immediately; fetching runs
separately. Export contains URLs, titles, and folders, with no personal reading
state or HTTP cache metadata. Cancellation leaves the current view untouched.

Progress writes are coalesced and serialized, with the latest position flushed
when leaving an article. Progress updates preserve explicit read/unread choices.
Async list/article/highlight results apply only to the request and item that
started them. A failed optimistic action restores the affected state and reports
the failure quietly, preserving the working list or article.

Feed fetch progress and non-fatal errors surface in the status strip. The context
bar exposes refresh in lists and its app menu; it never animates or prompts for
attention.
Source failures offer a quiet Details action opening Source activity. The same
recovery controls are in Settings → Sources, with Source activity also available
through the command palette and Cmd+Shift+R. Each source has its last check,
a plain explanation, deliberately opened technical details, and Refresh source
or Retry source. Retrying leaves the current reader and list context intact.
Problems remain until that source recovers or is removed; an unrelated fetch
does not erase them. A not-modified response clears a connection failure but
retains failed article caching, which requires a full retry. These diagnostics
are local session state, with stored consecutive failures supplying a quiet
last-update notice after restart. No status badges or problem counts are shown.

An explicit **Fetch** obtains the feed body again so failed article caching can
recover even when the publisher's feed has not changed. Scheduled polling keeps
conditional requests. Partial content failures distinguish a successfully saved
feed from an incomplete article cache; diagnostic logs include a sample reason.

### Share affordance

Share belongs in the reader context bar as a small popover. **Copy link** copies
the original webpage address; **Copy Carrel link** copies its private saved-item
address. These remain useful before connecting with anyone. With a followed
person available, **Share with people…** opens the native composer over the
mounted article; otherwise **Add a person…** opens Settings directly to People.
The `c` / `C` actions follow the same recipient guard and never open an empty
composer. A stale/failed contact read reports recovery without losing context.
Choose a person or named group explicitly and optionally enter a brief note. Saving queues a signed
vouch locally; failures retain the draft and retry identity. Sharing never
completes or stars an article.

People lives in Settings, alongside Reading, Sources, Devices and Keyboard.
Add a trusted public key or verified contact code; show the full key before
Follow. Contact information grants no private archive access. The four primary
destinations remain. Library gains Shared with you, ordered by latest vouch;
rows include the author's local name and note. Reader Info retains all active
received vouches for an article. People offers quiet retry and review/retraction
of sent shares. Retraction cannot erase a copy already received. No counts,
notifications or engagement signals are added.

People offers explicit Copy public key and Copy contact code controls. Copying
confirms quietly in that section; unavailable clipboard access selects the
complete text and explains how to copy it manually. It never reads the clipboard.
Contact fields remain stable during background refresh while being selected.
Both people explicitly follow the other's key before exchanging articles; a
bare key waits for connection information. Contact-code availability is not a
delivery receipt. Saved shares wait locally while a desktop is offline.

Sent articles pages newest first, with explicit Load older. Withdrawal or retry
keeps loaded pages and scroll; failures keep the current history available.
Background contact updates preserve a pending removal confirmation and keyboard
focus. The first friend trial checklist lives in PRIVATE_SHARING_PLAN.md.

### Highlight rendering

User's own highlights: a flat yellow background (`--color-highlight`), text color unchanged.

Friend's highlights: a thin underline in `--color-highlight-friend`, no background. Distinct enough to recognize, subtle enough not to compete with the user's own.

When multiple friends have highlighted the same passage, we show the underline once with a small marker (a number or stack icon) that on hover lists which friends.

### Share-with-note

Optional name and picture are edited under Settings → People, separately from
desktop names and the verified public key. Import a local image; show initials
when none is chosen. Pictures are normalized locally and never fetched from a
remote service. A person's local alias takes precedence over their proposed name.

People also holds named groups, with an explicit name and member checklist.
Groups are a convenience for selecting known people. Group names and contact
aliases remain on this desktop; a vouch carries the chosen participant keys.
The share composer has native checkboxes for people and groups. Overlaps resolve
to one recipient, and the complete reviewed audience appears before submission.
With several recipients, choose **Start a group conversation** or **Send separately**;
neither mode is preselected. The former explains that participants see one another
and share replies; the latter creates independent conversations and keeps each
recipient private. A sent share fixes its audience; later group edits never change it.
Removing a person stops direct exchange from this desktop without erasing received
history or changing an earlier group's fixed audience.

Conversation opens deliberately from a received vouch or sent history, in a
native dialog over the mounted reading/list view. The article title, opening
vouch and named participants precede chronological replies. Reply-to shows one
shallow context quotation rather than growing indentation. Each message offers
provenance/ancestry on request. Removed messages keep a quiet placeholder.
The composer states its fixed audience, retains failed drafts and retry tokens,
and sends explicitly (button or Cmd+Enter). Closing restores initiating focus
and passage. Unsent replies stay available when reopening a conversation during
the same app session; only explicit Send writes a durable queued message. A late
save acknowledges its exact submitted draft and preserves newer text. A withdrawal
leaves history readable, retains an unfinished draft for copying, and disables new replies.
There are no reply counts, badges, notifications or typing signals.

Participant names/avatars open a small native person card with their verified key
and introduction context ("Introduced through …'s share"). A missing private
profile uses the key fallback. **Connect** guides a deliberate two-way contact-code
exchange using the existing People flow, without leaving the thread. Copy your
own code, exchange it privately, review the other person's matching signed code,
and explicitly follow. Both people choose independently. A mismatched code cannot
change the card's selected identity. Joining a thread never follows someone or
distributes their connection address. Closing restores the thread and its draft.

A native dialog has an explicit audience chooser and a brief optional note.
Recipients start unselected; the dialog focuses the first choice. Submit
through the ordinary form or Cmd+Enter; Escape cancels. Settings, palette and
keyboard overlay guards preserve the draft. Archived content includes only the
chosen article and approved current images, with personal annotations excluded.
Public sharing remains future work.

## The command palette

`cmd-k` opens the command palette. It is the power-user surface and the universal escape hatch.

The implemented palette is a native modal with a single focused text input and
a bounded results list. Commands and sources match in-order abbreviations;
article search uses the same literal title/cached-text word prefixes as Library.
Commands come from the current keymap's implemented actions and display their
actual shortcut. Sources can be opened directly. Article results open the reader
with a Library search return context, retaining the query for continued browsing.
Up/down selects, Return activates, Escape dismisses and restores focus. A failed
article query retains working commands and sources. Request generations discard
late results after another query or dismissal. Searching never marks an item read.

The eventual wider palette can additionally match:

- Peers: pet names and self-described names
- Tags: every tag you've used

Current results use a quiet textual category and shortcut, without count badges.
Command/source matches are ordered by text match; article results retain Library
order. Peers and tags wait until their own useful flows exist.

Sublime-quality fuzzy matching: characters in the query must appear in order in the target, but not contiguously; matches earlier in the string and at word boundaries score higher.

`Enter` activates the selected result. `Esc` closes the palette without action. Up/down arrows navigate.

The palette is *always* available — even inside a modal, even while typing in a note. It's the user's ripcord.
When another dialog is open, the palette offers Return to current dialog and
keyboard reference. Navigation and mutation commands resume after that dialog
is finished or closed. Opening the palette never discards a note or URL draft.

## Anti-patterns

These are design moves that conflict with the principles and that we explicitly reject:

**Unread badges in app icons or system trays.** The whole point is no notifications outside the app. Adding a number to the dock icon is the camel's nose under the tent.

**Engagement-driven sorting.** Items appear in the order they arrived (or the order the user chose). No "trending," no "you might like," no "popular among your friends."

**Auto-clearing read counts.** Items become "read" because the user marked them so, not because they scrolled past. The data is honest about what the user has done.

**Modal interruptions.** Modals are reserved for actions the user explicitly initiated (share dialog, tag entry, audience picker). They are never used to surface status, errors, or prompts.

**Hover chrome.** UI that appears on hover and disappears when the cursor leaves is hostile to keyboard users and creates visual jitter. Affordances should be persistently visible (subtly) or only appear in response to deliberate action.

**Animation as decoration.** Every animation must convey something. "It looks nice" is not enough.

**Variable-width chrome.** The desktop navigation sidebar is a fixed width; the
reading measure has a fixed maximum. Below 720px the sidebar becomes a left
overlay and closes when a destination or source is chosen. Destination labels
retain the same size. Reading text does not continuously scale to fill the window.

**Onboarding tutorials.** The first run offers one skippable sheet for adding
reading, not a walkthrough. Discovery happens through use. The keymap reference
(`?`) is the user's reference, not a forced tutorial.

**Marketing or promotional surfaces in-app.** No "what's new" splash screens. No "upgrade" prompts (there's nothing to upgrade to). No surveys. The app does its job.

## What this document does not specify

A few things are deliberately left to designer judgment within these constraints:

**Exact icon design.** Icons follow the Lucide convention (24px, 1.5px stroke, simple geometric forms). Specific icon choices are made per-component without ceremony.

**Microcopy details.** Button labels, empty states, tooltips. We aim for short and direct; the exact wording isn't pinned in this document.

**Spacing details.** A 4px-based spacing scale is used throughout; specific values are chosen per-component.

**Subtle interaction details.** Hover state colors, focus ring exact shades, animation easing curves — these have defaults documented above but adjustments at the component level are fine without amending this doc.

When in doubt, the principles above govern. If a design choice would feel like Substack, Twitter, Notion, or LinkedIn, it's probably wrong for Carrel. If it would feel like Sublime Text, mutt, iA Writer, or a well-designed library, it's probably right.
