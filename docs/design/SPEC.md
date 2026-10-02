# cutemarkdown v1 — Design Spec

Read-only Markdown viewer · native Rust · egui/eframe 0.36 · Windows 10/11.
All sizes are logical px at 100% scale (egui points). OS DPI scaling multiplies them. Content sizes marked **em** scale with the reader's text size (default 16 px). Chrome sizes never scale.

---

## 0. PO scope for this release (overrides "v1" labels below)

The spec describes the full v1 vision. This release ships it in priority order. **P0 must ship and be solid. P1 only once all of P0 is done and screenshot-verified. P2 is deferred** (don't build it; leave clean extension points). If P0 is at risk, cut polish, not robustness.

| Area | P0 (must) | P1 (if time) | P2 (deferred) |
|---|---|---|---|
| Rendering | All GFM blocks and inlines in §6: headings with H1/H2 rules and tick, lists with markers and indent guides, task lists, quotes, alerts, code (highlighting, header label, copy, horizontal scroll), tables (fit, then scroll), hr dots, images (local, remote, broken chip), footnotes section, collapsed front-matter row, Mermaid/math fallback labels, the safe HTML subset (incl. `<details>`), external-link ↗ | Code/table edge fades, wrap-code setting, footnote hover card, expandable front matter, semantic emoji tints, `align=center`, heading context menu (copy section, copy link) | Change marks, update pill, HTML `<table>` fallback, rich plain-text copy formatting, find diacritic-insensitivity |
| Engine behaviour | Virtualized layout (5,000-line doc scrolls smoothly), cross-block selection + Ctrl+C, find (all matches + current, scroll to match), scrollspy, scroll to heading or anchor, reading keys, content-anchored position on reload + follow-tail, link hit-testing and hover, bundled fonts + Windows system fallback fonts | Animated scrolls, scrollbar find ticks, `words_remaining`, `top_source_line` | opsz axis, font subsetting and cv05 freeze, zstd fonts |
| Chrome | Native title bar with file title, 44 px app bar (always visible), progress line, outline (docked/overlay, fixed 264 px), Aa popover (theme incl. Auto, font, size, width), find bar, ⋯ menu, toasts, empty state (mascot, open button, recent files), drag & drop and paste, Back/Forward, link safety rules (§7), live reload (polling is fine), settings JSON + window geometry, runtime window icon | App bar auto-hide, Zen, DWM caption color and dark title bar, link status pill, "min left", drag overlay, file-missing chip, shortcut overlay (Ctrl+/), mouse X1/X2, outline auto-collapse when >30 entries, Ctrl+N | Outline resize, resume-at-anchor for recents, open in editor *at a line*, custom editor string |
| Keys | Ctrl+O, Ctrl+W, Ctrl+F, Enter/F3 (+Shift), Esc, Ctrl ±/0 and Ctrl+wheel, Alt+←/→, Ctrl+R/F5, Ctrl+B, Ctrl+Shift+L, Ctrl+E, Ctrl+Shift+C, Ctrl+V, ↑↓ PgUp PgDn Space Home End | F11, Ctrl+↑/↓, Ctrl+, , Ctrl+/ | — |

---

## 1. Design principles

1. **The document is the hero.** Chrome is a single 44 px bar that hides while you read. Icons are muted, and borders appear only when they separate something.
2. **Structure comes from size, weight and space, not decoration.** AI docs are heading- and list-heavy, so hierarchy uses a type scale, generous space above headings (≥ 2.5× the space below), indent guides and the outline. Color is reserved for meaning: links, alerts, code, status.
3. **Warm, not sweet.** The rose→violet heritage appears in exactly four places: the accent (links, list markers, focus, active states), the H1 gradient tick, the progress line and the empty-state mascot. Neutrals are warm plum-greys. There are no tinted panels behind prose (blockquotes are bar-only).
4. **Calm under change.** Live reload never moves the reader. Scroll stays anchored to content, changes are marked quietly, and the view follows the end of the file only if the reader was already at the end.
5. **Honest fallbacks.** Anything we can't render (Mermaid, math, unsupported HTML, broken images) becomes a labelled, readable fallback. Never show raw garbage and never drop content silently.
6. **Fast is a feature.** First paint in under 200 ms for a 500 KB file. Scrolling runs at display refresh rate. Layout is cached per block and redone only when a block's source hash changes. Local content never shows a spinner.
7. **Safe by default.** No scripts. The only network traffic is image fetches. Links that would execute something are never launched.

## 2. v1 feature list

| Feature | Why it helps reading | Decision |
|---|---|---|
| GFM baseline: tables, task lists, strikethrough, autolinks, footnotes, GitHub alerts, emoji, `<details>` | This is what AI and GitHub docs actually contain | **v1** |
| Reading measure: Narrow / Medium / Wide / Full | Line length is the biggest legibility lever. Medium fits 80-column code | **v1** |
| "Aa" popover (theme, font, size, width, wrap code) | All reader comfort settings in one place; no settings window | **v1** |
| Themes Light / Sepia / Dark + Auto (follow Windows) | Ambient-light comfort. Sepia suits long reading sessions | **v1** |
| Outline sidebar with scrollspy, auto-hidden for docs with < 3 headings | Navigation and "where am I" in long, heading-heavy docs | **v1** |
| Outline auto-collapses H3s outside the active H2 when > 30 entries | Keeps 100-heading AI reports scannable | **v1** |
| Reading progress (2 px gradient line) | Shows position at a glance with almost no pixels | **v1** |
| Read time, shown as **"8 min left"** in the outline header (total in ⋯ menu) | Time remaining is actionable; a static total isn't. Not shown in the bar, to avoid clutter | **v1, modified** |
| Code: syntax highlighting, language label, copy button, horizontal scroll, global "wrap" toggle | Many languages per doc. Copying commands is the #1 action | **v1** |
| Indent guides for nested lists (level ≥ 2) | Deep AI bullet trees stay parseable | **v1** |
| Find (Ctrl+F) with highlighted matches and scrollbar ticks | Long docs. Ticks show where matches cluster | **v1** |
| Zoom = text size (Ctrl ±/0, Ctrl+wheel) | Text-only zoom keeps the chrome stable | **v1** |
| Live reload with content-anchored scroll, follow-tail, change marks, "Updated below" pill | Lets you watch an agent write without losing your place | **v1** |
| Relative `.md` links open in-app, with Back/Forward and per-entry scroll | Doc sets (README → docs/*.md) read like a site | **v1** |
| Front matter as a collapsed one-line summary | Metadata is available but not in the way | **v1** |
| Footnotes with hover preview | Read the note without jumping away | **v1** |
| Zen mode (F11): fullscreen, no chrome | Deep reading of long reports. Nearly free to build | **v1** |
| Empty state with recent files; resume position if the file is unchanged | Quick return to ongoing reading | **v1** |
| Drag & drop; Ctrl+V pastes Markdown (e.g. copied LLM output) | Common entry points; paste is a key AI use case | **v1** |
| Open in editor (Ctrl+E) at the top visible line | Viewer → fix → live reload loop | **v1** |
| Images: local and remote, sized, broken-image chip | READMEs depend on images and badges | **v1** |
| Wide tables: fit first, then horizontal scroll inside the frame | The page never scrolls sideways | **v1** |
| Heading context menu: *Copy section as Markdown*, *Copy link to section* | Paste a section into a chat or issue. Replaces hover anchors | **v1** (new) |
| Link hover status pill (shows the destination) | Trust: you know where a click goes | **v1** (new) |
| Semantic tint for status emoji (✅ ❌ ⚠️ 🟢 🔴 …) | Emoji are monochrome, and AI status tables rely on their color | **v1** (new) |
| Heading hover `#` anchors | Gutter clutter while mousing; the context menu covers the need | **Rejected** |
| Collapsible sections (heading folding) | Adds gutter affordances and state; the outline already covers navigation | Later |
| Mermaid / math rendering | Heavy dependencies. v1 shows labelled source (§6) | Later |
| Running section header in the bar, image lightbox, sticky table headers, line numbers, outline filter, HTML `<table>`, print/PDF, single-instance window reuse, `:shortcode:` emoji | Nice, but not core to reading | Later |
| Tabs, folder tree, editing, custom CSS/themes | Chrome and scope creep. Windows and relative links are enough | **Rejected** |

## 3. Layout & chrome

```
┌ native title bar (DWM caption color = theme bg) "design.md — cutemarkdown" ┐
│▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀ 2px progress line (y=0) ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀│
│ [▤] [←][→]                         app bar 44px          [⌕] [Aa] [⋯]      │  ← overlays, auto-hides
│ CONTENTS   8 min left │                                                 ┃  │
│  Overview             │        ┌──── column: 46em = 736px ────┐         ┃  │  ← overlay scrollbar
│ ▌Architecture         │        │ H1 / body / code / tables …  │         ┃  │
│    Data model         │        └──────────────────────────────┘         ┃  │
│   264px (200–400)     │  ← gutter ≥ 48 →                   ← gutter ≥ 48 →  │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Window.** Native title bar, kept for robust snap, resize and Snap Layouts. Set `DWMWA_CAPTION_COLOR` to `bg` (Win11) and `DWMWA_USE_IMMERSIVE_DARK_MODE` in Dark. Title: `{file name} — cutemarkdown`, with ` (missing)` appended if the file is gone, or `Pasted text — cutemarkdown`. Default size 1100×860, centered. Minimum 480×360. Each launch opens a new window; Ctrl+O replaces the document in the current window.

**App bar (44 px).** Full width. It overlays both the sidebar and the content, which reserve 44 px of top padding. Background is `bg` at 94% opacity. A 1 px `border` line at the bottom appears (120 ms fade) once scrollY > 0.
- Left, 8 px padding, 4 px gaps: Outline toggle (Lucide `panel-left`), Back, Forward. Back and Forward appear only when history exists; the unavailable direction is shown disabled (`faint`).
- Right: optional **File missing** chip (WARNING fg on WARNING tint, 22 px tall, 12 px text, fully rounded), Find (`search`), **Aa** (the text "Aa" in Inter 15/600, not an icon), More (`ellipsis`).
- Icon buttons are 32×32 with radius 8 and an 18 px icon at 1.75 stroke in `muted`. Hover: `bg-hover` fill, icon in `text`. Pressed: `border` fill. Toggled on (outline open, find open, popover open): `accent-soft` fill, icon in `accent`. Tooltip after 600 ms shows the label plus the shortcut in `muted`.
- **Auto-hide.** Always shown while scrollY < 120. Hides (slides up 44 px and fades, 160 ms) after ≥ 64 px of continuous user scroll down. Comes back on: ≥ 24 px of scroll up, pointer within 56 px of the top for 150 ms, Alt, keyboard focus in the bar, or any open find bar, popover or menu. Never hides while hovered. Programmatic scrolls (outline, links, find) don't change its state.
- **Progress line.** 2 px tall at y = 0, drawn above everything, full window width. Linear gradient from `grad-a` to `grad-b`. Width = scrollY / (contentH − viewportH). Hidden when the document fits in the viewport.

**Outline sidebar.** Background is `bg` with no divider. The 8 px resize hit zone shows a 1 px `border-strong` line on hover. Width 264 by default, resizable 200–400, persisted.
- Available only when the document has ≥ 3 headings at levels 1–3. Otherwise the toggle is disabled with the tooltip "No outline for short documents".
- **Docked** when window width ≥ sidebar width + measure + 96. Below that it becomes an **overlay**: `surface` background, 280 px wide, popover shadow, slides in from the left in 160 ms, with a scrim of `text` at 6%. It closes on Esc, outside click or item click. The overlay starts closed regardless of preference.
- Padding is 56 top, 12 sides, 24 bottom. Header row: `CONTENTS` (11 px / 600, +0.08em tracking, uppercase, `muted`) on the left; "8 min left" (11 px, `muted`) on the right. Minutes left = words below the viewport top ÷ 230 (code blocks excluded); shows "<1 min left" near the end.
- Entries: levels 1–3. If the document has exactly one H1 and it is the first block, it is left out and H2 becomes the top level. Each nested level indents 14 px. Rows are 13.5/20 Inter 400, padding 5×10, radius 6, single line with ellipsis; the full text shows in a tooltip after 500 ms. Inline formatting is flattened to plain text; emoji are kept.
- Hover: `bg-hover` fill, text in `text`; at rest, text is `muted`. **Active:** `accent-soft` fill, `link`-colored text and a 2 px `accent` bar (radius 1) flush left. Font weight does not change, to avoid jitter.
- With more than 30 entries, H3s are shown only under the active H2, expanding over 120 ms. The list auto-scrolls to keep the active row visible (centered if it was off-screen).
- **Scrollspy rule.** Active = the last heading whose top is ≤ viewport top + 30% of viewport height. When scrolled to the very bottom, active = the last heading whose top is inside the viewport, so short final sections can light up.

**Content column.**
- Column width = min(measure, viewport − 2×gutter). It is centered in the area to the right of a docked sidebar. Toggling the sidebar animates the column position over 160 ms.
- Measures (em × text size): **Narrow 38em** (608 px) · **Medium 46em** (736 px, the default; fits 80 columns of 14 px mono plus padding) · **Wide 56em** (896 px) · **Full** (viewport − gutters). Measures are based on the sans size, so switching to serif doesn't move the column.
- Gutter: 48 px at viewport ≥ 960, 32 px from 640 to 959, 20 px below 640.
- Top padding: 44 (bar) + 40. Bottom padding: 40% of viewport height, so the last heading can reach reading height. The first block has no top margin.
- Nothing in the page ever scrolls horizontally. Only code blocks and tables scroll, inside their own frames.

**Scrollbar.** An overlay that takes no layout width.
- Thumb: 6 px wide while scrolling and for 800 ms after, then fades out over 300 ms. It grows to 10 px when the 14 px hit zone at the right edge is hovered.
- Color: `text` at 22% (hover 38%, dragging 50%). Fully rounded, minimum length 32 px.
- Clicking the track pages. Find matches draw 2 px `find-current` ticks across the track.

**Floating layers (z-order low → high):** content · overlay sidebar · app bar · find bar · popovers/menus · update pill · toasts · drag overlay.
- **Find bar.** Card anchored top-right of the content viewport, 8 px below the bar (which is forced visible while find is open) and 16 px from the right edge. 340×40, `surface`, 1 px `border`, radius 10, popover shadow. Contents: `search` icon 16 in `muted` · input (14 px; placeholder "Find" in `muted`) · count "3 of 17" (12 px `muted`; "No results" in CAUTION fg) · ↑ ↓ · `Aa` match-case toggle · ×. Buttons are 28 px.
- **Popovers and menus.** `surface`, 1 px `border`, radius 12, padding 6 (menus) or 16 (Aa). Shadow: (0, 8) blur 24 at `shadow` plus (0, 1) blur 2 at half alpha. Menu items are 30 px tall, 13.5 px text; shortcuts are right-aligned in `muted`; separators are 1 px `border` with 6 px margin.
- **Toast.** Bottom center, 24 px above the edge. 32 px tall, padding 0×14, fully rounded. Background `text`, foreground `bg` (inverted), 13/500, optional 14 px leading icon. Appears with a fade and 8 px rise over 160 ms. Stays 1.6 s (errors 4 s). One slot; a new toast replaces the current one. Toasts are not used for automatic reloads.
- **Update pill.** Bottom center, 64 px above the edge. "↓ Updated below", 30 px tall, `surface`, 1 px `border-strong`, `link` text 13/500, shadow. Click → smooth-scroll to the first changed block. Disappears when that block is reached or after 6 s.
- **Link status pill.** Bottom-left, 8 px inset. 24 px tall, `surface`, 1 px `border`, radius 6, 12 px `muted` text, middle ellipsis, max 60% of window width. Shows 300 ms after hovering a link and hides immediately on leave.

**Empty state (first run, or no document).** The app bar shows only Aa and ⋯. A single 400 px column is centered with its center at 42% of the window height.
- **Mascot.** 64 px line drawing of the smiling page: 2 px `accent` stroke, `accent-soft` page fill, eyes and smile in `text`, blush dots in `accent` at 35%. This is the only place the "cute" illustration appears.
- 20 px below: "Open a Markdown file" (20/650, `text-strong`).
- 6 px below: "Drop a file here, press Ctrl O, or paste with Ctrl V" (14 px `muted`, with keycaps).
- 20 px below: primary button "Open file…": 36 px tall, padding 0×16, radius 8, `accent` fill, `on-accent` 14/600 text. Hover darkens the fill 6%; focus shows the ring.
- 32 px below: `RECENT` label (same style as `CONTENTS`), then up to 8 rows of 44 px. Each row: `file-text` 16 in `muted`, name 14/500 in `text`, folder 12 px `muted` with middle ellipsis. Hover: `bg-hover` with radius 8 and a × button to remove. Missing files show the name in `muted` and a "Missing" label at 11 px. The section is hidden when the list is empty.

**Drag overlay (any state).** Fills the window with `accent-soft` at 85%. Inset 16 px is a rounded rectangle (radius 16) with a 2 px dashed `accent` stroke. Centered: `file-text` 32 and "Drop to open" (16/600 `link`).
- Non-Markdown drops show the toast "Not a Markdown file". Accepted types: `.md .markdown .mdown .mkd .mdx .txt`.
- With several files, the first opens here and the rest open in new windows.

**Zen (F11).** Fullscreen. Sidebar and bar are hidden; the progress line stays. The bar reappears only when the pointer is within 8 px of the top. Esc exits when find isn't open.

## 4. Typography

**Fonts (all OFL; subset to Latin, Latin Ext, Greek, Cyrillic, Vietnamese, punctuation, arrows and math operators; zstd-compressed in the binary).** Sizes below were measured after subsetting.

| Role | Font | Instance | Size raw / compressed |
|---|---|---|---|
| UI, sans body, all headings | **Inter 4** (variable) | wght 400–700, opsz 14–32 kept (Display cut for headings). Freeze `cv05` (l with tail) at build time so Il1 are distinct | 502 / 242 KB |
| Sans italic | Inter Italic | wght 400–700, opsz pinned at 14 | 368 / 175 KB |
| Serif body (optional) | **Literata** (variable) | wght 400–700, opsz pinned at 12 | 370 / 160 KB |
| Serif italic | Literata Italic | same | 356 / 161 KB |
| Code | **JetBrains Mono** (variable) | wght 400–700, plus box drawing; **`calt` ligatures off** | 90 / 45 KB |
| Emoji | **Noto Emoji** (monochrome) | static wght 400 | 842 / 553 KB |

Total ≈ 2.5 MB raw, **≈ 1.34 MB in the binary**. Literata and Noto Emoji are decompressed on first use. If egui can't set the opsz axis per size, pin opsz at 14 (saves 137 KB) and rely on the tracking values below.

**Fallback.** For glyphs missing from all bundled fonts (CJK, Arabic, Hebrew, Indic and so on), lazily load system fonts from `C:\Windows\Fonts` in this order: Segoe UI, Microsoft YaHei UI, Yu Gothic UI, Malgun Gothic, Nirmala UI, Segoe UI Symbol. Never render tofu when a system font has the glyph.

**Type scale (text size T = 16 by default; sans mode).**

| Element | Size | Line height | Weight | Tracking | Color | Space above / below |
|---|---|---|---|---|---|---|
| Body, list item | 1em = 16 | 26 (1.625) | 400 | 0 | `text` | block gap 16 |
| H1 | 1.875em = 30 | 38 | 700 | −0.022em | `text-strong` | 56 / 20 (below = after the rule, §6) |
| H2 | 1.4375em = 23 | 30 | 650 | −0.017em | `text-strong` | 40 / 14 (after the rule) |
| H3 | 1.1875em = 19 | 26 | 650 | −0.012em | `text-strong` | 32 / 8 |
| H4 | 1em = 16 | 24 | 700 | −0.006em | `text-strong` | 24 / 6 |
| H5 | 0.8125em = 13 | 20 | 700, UPPERCASE | +0.06em | `text-2` | 24 / 4 |
| H6 | 0.8125em = 13 | 20 | 600 | 0 | `muted` | 20 / 4 |
| Bold (`strong`) | inherits | — | 600 (serif 650) | — | `text-strong` | — |
| Code block | 0.875em = 14 | 22 | 400 | 0 | `text` | 20 / 20 |
| Inline code | 0.875em | — | 400 | 0 | `icode-fg` | — |
| Table | 0.9375em = 15 | 22 | 400; header 600 | 0 | `text` | 20 / 20 |
| Footnotes, front matter | 0.875em = 14 | 22 | 400 | 0 | `text-2` | — |
| UI text (not scaled) | 13 (menus 13.5, labels 12, overlines 11) | 1.4 | 400–600 | +0.005em; overlines +0.08em | `text` / `muted` | — |

- **Serif mode.** Paragraphs, list items, blockquotes, table cells and footnotes use Literata at T+1 with a 28 px line (1.65). Headings, UI and code don't change, so sans headings over serif text keep a strong hierarchy.
- **Rhythm.** The gap between two blocks is max(previous.below, next.above); margins collapse, never add. A heading that directly follows another heading gets 12 px above it. Boxed blocks (code, tables, alerts, quotes, images, details) use 20 px.
- **Text size steps:** 12, 13, 14, 15, **16**, 17, 18, 20, 22, 24, 26, 28. All em values follow T.
- **Keeping hierarchy clear in heading-heavy AI docs:**
  - Each heading level differs from its neighbors in at least two of size, weight, case and rule.
  - H1 and H2 carry rules; H5 is an uppercase overline, so it never looks like a bold sentence.
  - Space above a heading is at least 2.5× the space below, so each heading visibly belongs to the text after it.
  - A body bold lead-in (`**Goal:**`, weight 600) stays visually below H4 (700, on its own line, 24 px above it).

## 5. Color tokens

**Core.** All colors are opaque sRGB hex. "@n%" means alpha.

| Token | Light | Sepia | Dark |
|---|---|---|---|
| `bg` (page, sidebar, bar) | `#FCFAF9` | `#F6EFE4` | `#18151C` |
| `surface` (popovers, cards, overlay sidebar) | `#FFFFFF` | `#FBF7F0` | `#221E27` |
| `bg-hover` | `#F4EEF0` | `#EEE4D6` | `#2A2530` |
| `text` (body) | `#2A2430` | `#382D25` | `#E9E4EC` |
| `text-strong` (headings, bold) | `#1D1822` | `#2A211A` | `#F7F3F9` |
| `text-2` (quotes, footnotes) | `#4F4755` | `#54473C` | `#CFC7D4` |
| `muted` | `#6B6271` | `#6B5D50` | `#A59CAD` |
| `faint` (decor and disabled only, never body text) | `#A0979F` | `#A79784` | `#6E6575` |
| `border` | `#EDE5E8` | `#E6DCCD` | `#2E2834` |
| `border-strong` (indent guides, kbd, checkboxes) | `#DDD2D7` | `#D6C8B5` | `#3E3645` |
| `accent` (markers, fills, focus, active bar) | `#C2407A` | `#A84470` | `#F07AAB` |
| `on-accent` (text on an `accent` fill) | `#FFFFFF` | `#FFFFFF` | `#18151C` |
| `accent-soft` (active and hover tints) | `#F9E8EF` | `#F0DFD9` | `#3A2332` |
| `link` | `#B8336C` | `#A13A60` | `#F59AC0` |
| `grad-a → grad-b` (progress, H1 tick, icon) | `#E0558A → #7B5CE6` | `#B8507A → #6E58C9` | `#F07AAB → #A893FF` |
| `selection` (text color unchanged) | `#F6D2E1` | `#EACDC8` | `#5B2A45` |
| `icode-bg` / `icode-fg` | `#F5EDF0` / `#9A2E5E` | `#EDE2D3` / `#8A3354` | `#2A2230` / `#F6A9CA` |
| `code-bg` / `code-border` | `#F7F3F4` / `#EEE6E9` | `#F0E7DA` / `#E5D9C8` | `#1F1B24` / `#2C2631` |
| `table-head` / `table-zebra` | `#F5F0F2` / `#FAF7F8` | `#EEE4D6` / `#F3EBE0` | `#24202A` / `#1C1920` |
| `quote-bar` | `#E2C3D1` | `#D9BDB6` | `#4D3443` |
| `find-match` (text unchanged) | `#FFE7A1` | `#F9DC85` | `#5E4A12` |
| `find-current` (+1 px ring `#E0951A`; Dark text → `bg`) | `#FFC266` | `#F2A94A` | `#E0A33A` |
| `shadow` | `#3C192D` @12% | `#50321A` @14% | `#000000` @50% |

**Alerts** (fg = icon, title and border base; tint = fill; border = 1 px):

| Alert | Light fg · tint · border | Sepia fg · tint · border | Dark fg · tint · border |
|---|---|---|---|
| NOTE | `#2864BE` · `#EEF3FB` · `#C1D0E8` | `#2A60AD` · `#E6E9EA` · `#BDC7D5` | `#7EB0FF` · `#242837` · `#394765` |
| TIP | `#1D7748` · `#EBF6EF` · `#BED5C7` | `#2F6E3A` · `#E5ECDB` · `#BECBB4` | `#6CD39E` · `#222C2C` · `#335246` |
| IMPORTANT | `#7A4ED6` · `#F3EEFC` · `#D8CAEF` | `#6A47BE` · `#EDE5EA` · `#CFC0D9` | `#B7A0FF` · `#2B2637` · `#4B4165` |
| WARNING | `#946000` · `#FCF3E3` · `#DFCFB3` | `#875700` · `#F4E5C8` · `#D7C4A4` | `#E9B45A` · `#312823` · `#5B4830` |
| CAUTION | `#C3304B` · `#FCECEF` · `#ECC1C8` | `#AE2D45` · `#F4DFD8` · `#E2B9B7` | `#FF8A9C` · `#34232B` · `#623A45` |

**Syntax** (on `code-bg`; never bold or italic):

| Role | TextMate scopes (syntect) | Light | Sepia | Dark |
|---|---|---|---|---|
| keyword | keyword, storage, markup tag names | `#B02D67` | `#9E2C5A` | `#F58DB9` |
| string | string, regexp, char | `#2D7A50` | `#3B6A2A` | `#8CD6A6` |
| number | constant.numeric | `#A2560A` | `#8F4F0B` | `#F2B46E` |
| constant | constant.language/other, support.constant, booleans, null | `#8C4A00` | `#7F3F14` | `#F59E7A` |
| function | entity.name.function, support.function, function calls | `#5D45C9` | `#5640A8` | `#B9A7FF` |
| type | entity.name.type/class, support.type, attributes, JSON/YAML/TOML keys | `#1C6B8A` | `#1E5D76` | `#7CC9E0` |
| comment | comment | `#6F6875` | `#73665A` | `#958C9E` |
| operator / punctuation | keyword.operator, punctuation | `#5E5664` | `#5A4D42` | `#BCB3C4` |
| everything else | variables, plain text | `text` | `text` | `text` |
| diff add / del line bg | markup.inserted / deleted | `#E2F2E7` / `#FBE3E7` | `#DCE8CC` / `#F1D6CF` | `#1F3328` / `#3A1E27` |

**Semantic emoji tints** (the glyph takes the color; all other emoji use the current text color): **TIP fg** ✅ ✔ ☑ 🟢 🟩 💚 · **CAUTION fg** ❌ ✖ ❎ ⛔ 🚫 🛑 🔴 🟥 ❗ ‼ · **WARNING fg** ⚠ 🟡 🟨 🚧 💡 · **orange** (Light `#C25A12`, Sepia `#A9500F`, Dark `#F4A261`) 🟠 🟧 🔥 · **NOTE fg** ℹ 🔵 🟦 · **IMPORTANT fg** 🟣 🟪 · **accent** ✨ ⭐ 🌟 ❤ 💖 · **faint** ⚪ ⬜ · **text-strong** ⚫ ⬛

**Computed contrast (WCAG 2.x):**

| Pair | Light | Sepia | Dark |
|---|---|---|---|
| Body text on `bg` | **14.5:1** | **11.7:1** | **14.4:1** |
| `muted` | 5.6 | 5.6 | 6.8 |
| `link` | 5.4 | 5.6 | 8.8 |
| `link` on `accent-soft` (active outline row) | 4.8 | 4.9 | 7.0 |
| `text` on `code-bg` | 13.7 | 10.9 | 13.5 |
| Lowest syntax role on `code-bg` | 4.8 (string) | 4.5 (comment) | 5.3 (comment) |

Also: alert titles on their tint are ≥ 4.7:1 in every theme; `on-accent` on `accent` is 4.9 / 5.6 / 7.0 (Light / Sepia / Dark); `accent` markers meet the 3:1 non-text minimum (Light 4.7); no readable text uses `faint` (disabled controls are exempt); Dark avoids pure white and pure black.

## 6. Component specs

- **H1.** Below the text (12 px gap) is a full-column 1 px `border` rule. A 48×3 px, radius 1.5 gradient tick (`grad-a → grad-b`) sits on the rule's left end.
- **H2.** 1 px `border` rule 8 px below the text.
- **H3–H6.** No rules.
- **Headings in general:**
  - Emoji in headings use the heading color unless they have a semantic tint.
  - Slugs follow GitHub: lowercase, drop punctuation except `-` and `_`, spaces become `-`, duplicates get `-1`, `-2`, so cross-file anchors from GitHub docs resolve.
  - Right-click a heading (in the document or the outline): **Copy section as Markdown** (source from the heading to the next heading of the same or higher level), **Copy link to section** (`#slug`), **Open in editor here**.
- **Paragraphs.** Soft line breaks render as spaces; hard breaks (two trailing spaces or `\`) render as breaks. Text is left-aligned and never justified. No hyphenation.
- **Emphasis.**
  - `em`: true italic.
  - `strong`: 600 in `text-strong`.
  - `~~del~~`: 1 px strike line at x-height ÷ 2, with the text in `muted`.
  - `<sup>` / `<sub>`: 0.75em, raised 0.35em or lowered 0.2em.
  - `<mark>`: WARNING tint background with radius 3.
- **Links.**
  - Color `link`. 1 px underline 2 px below the baseline at `link` @40%; on hover the underline goes to 100% at 1.5 px. Hand cursor. Keyboard focus shows the ring.
  - **External** (`http`, `https`, `mailto`) links get a trailing ↗ (Lucide `arrow-up-right`, 0.7em, `link` @70%, 2 px gap). Bare autolinked URLs and internal links (`#anchor`, relative paths) get no glyph.
  - Every link shows the status pill on hover.
- **Lists.**
  - Each level indents 26 px; at level ≥ 5, 18 px. The marker box is 26 px wide, with the marker centered at x = 10 and vertically on the first line's x-height center.
  - Bullets: L1 is a 6 px filled `accent` disc; L2 a 6 px `accent` ring with 1.5 px stroke; L3 a 6×1.5 px `muted` dash; L4 and deeper cycle disc, ring, dash in `muted`.
  - Ordered lists: numbers in `muted` 500, tabular figures, right-aligned in a box as wide as the widest number + 6 px (minimum 26). `start=` is honored.
  - Spacing: 4 px between items in tight lists, 12 px in loose lists. Lists have 16 px above and below; nested lists 4 px above.
  - **Indent guides:** for every nested list, a 1 px `border-strong` line at the parent marker's x. It runs from 4 px below the parent's first line to the bottom of its last child.
- **Task lists.**
  - The 15×15 checkbox (radius 4) replaces the bullet in the marker box.
  - Unchecked: 1.5 px `border-strong` stroke on a `surface` fill.
  - Checked: `accent` fill with a 2 px `on-accent` check mark, and the item text turns `muted`.
  - Checkboxes are read-only, with no hover state.
- **Blockquotes.** No fill. A 3 px `quote-bar` sits on the left, and content starts 16 px after it. Text is `text-2`, not italic. Each nesting level adds another bar 16 px further in.
- **Alerts** (`> [!NOTE|TIP|IMPORTANT|WARNING|CAUTION]`):
  - Container: radius 8, tint fill, 1 px border, padding 12×16.
  - Title row (22 px): a 16 px Lucide icon at 1.75 stroke (`info`, `lightbulb`, `message-square-warning`, `triangle-alert`, `octagon-alert`), an 8 px gap, then the title (Note / Tip / Important / Warning / Caution) in 14/650 alert fg. Content starts 6 px below.
  - Body text keeps `text` and the normal component styles. Text on the marker line is treated as body. An unknown `[!FOO]` renders as a normal blockquote.
- **Code blocks.**
  - Container: `code-bg`, 1 px `code-border`, radius 8.
  - When a language is given, a 32 px **header** shows the display name on the left at padding 16 (12/500 `muted`; for example `ts` becomes "TypeScript", `sh` and `bash` "Shell", `ps1` "PowerShell", `yml` "YAML"; unknown names are shown as written) and the copy button on the right. Code then has padding 2/16/14/16.
  - Without a language (including indented code), there is no header and padding is 14×16. The copy button floats 8 px from the top-right on hover, on a `code-bg` pill.
  - **Copy button:** 28×24, 14 px `copy` icon in `muted`. On hover a "Copy" label appears to the left of the icon. On click: a `check` icon and "Copied" in TIP fg for 1.5 s. It copies the exact source without a trailing newline.
  - **No wrap by default.** The block scrolls horizontally with Shift+wheel or the touchpad, showing a 6 px thumb on hover. A 16 px fade (`code-bg` → transparent) marks each clipped edge.
  - The "Wrap long code lines" setting soft-wraps instead, indenting continuation lines to the line's leading whitespace + 2ch.
  - Tabs equal 4 spaces. There are no line numbers. Files over 10 MB, or blocks over 5,000 lines, are rendered without highlighting.
  - **Diff:** each `+` or `-` line gets a full-width diff bg with the sign in string or CAUTION fg. `@@` lines use the function color.
- **Inline code.** Mono 0.875em in `icode-fg` on `icode-bg`, radius 4, 5 px horizontal padding. The background rect is the line box minus 4 px, vertically centered. Inside links the text uses `link` color; inside headings it scales with the heading.
- **`<kbd>`.** Inter 0.8em/500 `text` on `surface`, 1 px `border-strong` with a 2 px bottom edge, radius 4, padding 1×6.
- **Tables.**
  - Frame: radius 8, 1 px `border` (cell content is clipped to the radius).
  - Header row: `table-head` fill, 600 weight in `text-strong`, 1 px `border-strong` bottom. Body rows are separated by 1 px `border`, and even rows get `table-zebra`.
  - Cells: padding 8×12; the text wraps.
  - GFM alignment is honored. Columns without alignment where ≥ 80% of cells are numeric are right-aligned (tabular figures if available).
  - Sizing: each column's minimum is its longest word (capped at 240) and its maximum is its natural width (capped at 420). Columns shrink toward their minimum to fit the column; if the table still doesn't fit, it scrolls horizontally inside the frame, with edge fades as on code blocks.
- **Horizontal rule.** Three centered 4 px `faint` dots, 12 px apart, with 32 px above and below. It replaces the full-width line so it never stacks with an H2 rule.
- **Images.**
  - Relative paths resolve against the file's folder; `http(s)` loads in the background (10 s timeout, 15 MB cap, ≤ 5 redirects, no cookies, cached per session). Supported: PNG, JPG, GIF (animated), SVG (rasterized at device scale), WebP.
  - An image alone in a paragraph is a block: centered, max-width = column, aspect kept, radius 6. Inline images (badges) sit on the baseline at natural size.
  - `width`/`height` attributes reserve space; anything else may reflow, which is safe because of scroll anchoring.
  - **Broken image:** an inline chip with `image-off` 16 px, the alt text (or file name) in 14 px `muted`, a 1 px dashed `border-strong` border, radius 6, padding 4×8. Its tooltip gives the path and the reason (Not found / Unsupported / Too large / Network).
  - Right-click menu: Copy image, Copy image address, Open image.
- **Footnotes.**
  - References are a superscript number (0.75em, 600, `link`) with no brackets.
  - Hovering a reference shows a 250 ms-delayed card (`surface`, radius 8, shadow, max 360 wide, padding 10×12, 14 px) with the note.
  - Clicking a reference scrolls to the note, which flashes `accent-soft` for 1.2 s, and pushes a history entry.
  - The notes section at the end: 40 px above, a 1 px `border` rule, a `FOOTNOTES` overline, then a 14 px `text-2` ordered list. Each note ends with a ↩ back-link. The section is not in the outline.
- **Front matter** (YAML `---` or TOML `+++` on line 1 only):
  - Collapsed by default: a single row in a 1 px `border`, radius 8 container with padding 8×12. The row has a 12 px chevron, a `FRONT MATTER` overline and a preview of the first three scalar values (`title: Spec · status: draft · date: 2026-10-01`, 13 px `muted`, ellipsis).
  - Expanding (chevron rotates 90° over 120 ms) shows the raw source as a highlighted block inside the container.
  - Front matter is excluded from word counts.
- **Mermaid, math and diagrams.**
  - Fences labelled `mermaid`, `plantuml`, `dot`/`graphviz` or `d2` render as normal code blocks whose header reads "Mermaid diagram" (etc.) followed by "· not rendered" in `muted`.
  - `math`, `latex` and `tex` fences, and `$$…$$` blocks on their own lines, render as code blocks labelled "Math (TeX) · not rendered".
  - **Inline `$…$` is not parsed**, to avoid false positives on currency.
- **HTML (safe subset):**

  | HTML | Handling |
  |---|---|
  | `<details>`/`<summary>` | Bordered card styled like front matter. The summary is 600 weight with a chevron. Closed unless the `open` attribute is set. Markdown inside is rendered. |
  | `<br>`, `<kbd>`, `<sup>`, `<sub>`, `<img src alt width height>`, `<a href name id>`, `<b>`, `<strong>`, `<i>`, `<em>`, `<code>`, `<s>`, `<del>`, `<ins>` (underline), `<u>`, `<mark>`, `<hr>`, `<picture>` (renders its `<img>`) | Rendered as specified above. |
  | `<p\|div\|h1–h6 align="center">` and `<center>` | Centers the children (common in README headers). |
  | `<div>`, `<span>`, `<section>` and other unknown container tags | Tags are stripped and the children rendered. |
  | `<table>` HTML | Code block labelled "HTML · not rendered". |
  | `<script>`, `<style>`, `<iframe>`, `<object>`, `<embed>`, `<template>`, `<noscript>`, form controls, `<video>`, `<audio>` | Dropped along with their content. |
  | `<!-- comments -->` | Hidden. |
  | Attributes other than those listed | Ignored. |

  Raw tags are never shown as text.

## 7. Interactions & keyboard shortcuts

| Keys | Action |
|---|---|
| Ctrl+O | Open file dialog (`.md .markdown .mdown .mkd .mdx .txt`) |
| Ctrl+N | New empty window |
| Ctrl+W | Close the window |
| Ctrl+F | Open find. Pre-fills a single-line selection and selects the input text |
| Enter / F3 · Shift+Enter / Shift+F3 | Next / previous match. Wraps around, and the count briefly reads "Wrapped" |
| Esc | Closes, in priority order: popover or menu → find → overlay sidebar → Zen |
| Ctrl+= or Ctrl++ · Ctrl+− · Ctrl+0 · Ctrl+wheel | Text size up / down / reset to 16. One step per notch, throttled to 100 ms. Toast "Text 113%". Keeps the scroll anchor |
| Alt+← / Alt+→ · Mouse X1 / X2 | Back / Forward (file history plus in-document jumps) |
| Ctrl+↑ / Ctrl+↓ | Previous / next heading (H1–H3), placed 24 px below the bar |
| ↑ / ↓ | Scroll 48 px |
| PgUp / PgDn · Shift+Space / Space | Scroll one viewport minus 64 px |
| Home / End (also with Ctrl) | Top / bottom |
| Ctrl+R / F5 | Reload, with toast "Reloaded" |
| F11 | Zen fullscreen |
| Ctrl+B | Toggle outline |
| Ctrl+Shift+L | Cycle theme Auto → Light → Sepia → Dark, with toast |
| Ctrl+, | Open the Aa popover |
| Ctrl+E | Open in editor at the top visible line |
| Ctrl+Shift+E | Reveal in Explorer |
| Ctrl+C · Ctrl+A | Copy selection · select all |
| Ctrl+Shift+C | Copy the whole document's Markdown source |
| Ctrl+V (no input focused) | Render clipboard text as "Pasted text"; this pushes a history entry, so Back returns |
| Ctrl+/ | Shortcut overlay (a card listing this table) |
| Alt | Reveal the app bar |
| Tab / Shift+Tab | Move focus through links and buttons; Enter activates |

**Link clicks:**
- `#anchor` scrolls smoothly and pushes history. Outline clicks don't push history.
- A relative `.md` file opens in this window and pushes history (with `#anchor` honored). Ctrl+click or middle-click opens it in a new window.
- A relative folder opens its `README.md` or `index.md`, or is revealed in Explorer if it has neither.
- Other local files open with the system handler only if their extension is on the allowlist: images, `pdf txt log csv json yaml yml toml xml html htm`. Anything else (`exe bat cmd ps1 vbs js msi lnk scr hta reg url …`) is never launched; it is revealed in Explorer with the toast "Revealed in Explorer".
- `http`, `https` and `mailto` open in the default browser or mail app.
- Any other scheme is blocked, with the toast "Blocked link (scheme:)".
- A missing target shows the toast "File not found: path".

**Scrolling.**
- Wheel notches animate 120 ms ease-out (3 lines = 78 px). Precision touchpads pass through unanimated.
- Anchor jumps: 220 ms ease-out-cubic. For distances over 3 viewports, jump to 1 viewport short of the target first, then animate.
- If Windows "Show animations" is off, nothing animates.

**Find.**
- Search is incremental (80 ms debounce), case- and diacritic-insensitive by default. It runs on rendered text across inline formatting, and covers code, tables, alerts, footnotes and collapsed containers (the current match's container expands).
- The current match scrolls to 35% of the viewport height.
- On Esc, the current match becomes the selection.
- Find re-runs after a live reload and keeps the match closest to the previous one.

**Live reload.**
- Watch the parent folder (to survive atomic-rename saves), debounce 120 ms, and wait until the file size has been stable for 120 ms (at most 1 s). Poll mtime every 2 s as a fallback.
- Identical content (by hash) is ignored.
- **Anchor:** the top visible block's stable key (heading path + block ordinal + text-hash prefix) and the pixel offset within it are restored. Fallbacks: the nearest preceding heading, then the same scroll fraction.
- **Follow:** if the viewport bottom was within 48 px of the end, stay pinned to the end (200 ms scroll).
- **Change marks:** new or changed blocks (at most 20; skipped entirely on a larger rewrite) get a 3 px × block-height `accent` bar, radius 1.5, 16 px left of the column. It fades in over 120 ms, holds 1.5 s and fades out over 1.5 s.
- The update pill shows when changes are entirely below the viewport and Follow is off.
- If the file goes missing, keep the last render, show the chip and ` (missing)` in the title, and resume when it reappears.
- **Encoding:** UTF-8 (with or without BOM) and UTF-16 with BOM. Invalid UTF-8 falls back to Windows-1252.

**Selection and copy.** Selection runs across blocks (I-beam over text, arrow over gaps). Ctrl+C produces plain text: blocks separated by a blank line, list items prefixed `- ` or `1. ` with 2 spaces of indent per level, code verbatim, table rows tab-separated (they paste into Excel). The text context menu offers Copy, Select all, Copy link (on links) and the heading items (§6).

**Open in editor.** ShellExecute with the `edit` verb; if that fails, VS Code (`code -g file:line`) when on PATH, then `notepad.exe`. An optional `editor` string in the settings file (e.g. `"code -g {file}:{line}"`) overrides this. Disabled for pasted text.

**General states.** Hover transitions take 120 ms ease-out, panels 160 ms. The focus ring is 2 px `accent` with a 2 px offset and follows the element's radius. Disabled controls use `faint`. Tooltips: `surface`, radius 6, 12.5 px, after 600 ms.

## 8. Settings (persisted)

Settings are changed only in the **Aa popover**: 296 px wide, anchored below Aa, with changes applied live. It has five rows with 12/600 `muted` labels:
1. **Theme:** four 56×40 swatches (radius 8; each draws its own `bg`, "Aa" in its `text`, and a 6 px `accent` dot). Auto is drawn as a diagonal half light, half dark. Captions are 11 px. The selected swatch has a 2 px `accent` ring.
2. **Font:** a segmented control, "Sans" set in Inter and "Serif" set in Literata, 32 px tall.
3. **Text size:** a − / "16 px" / + stepper. A "Default" link appears when the size isn't 16.
4. **Width:** a segmented control: Narrow · Medium · Wide · Full.
5. **Wrap long code lines:** a 32×18 switch.

Storage: `%APPDATA%\cutemarkdown\settings.json`. Writes are debounced 500 ms and also happen on exit, atomically (write to a temp file, then rename). A corrupt file is backed up as `settings.bad.json` and defaults are used. Window geometry that falls off every monitor is re-centered on the primary monitor.

```json
{ "theme": "auto", "font": "sans", "text_size": 16, "width": "medium", "wrap_code": false,
  "outline_open": true, "outline_width": 264,
  "window": { "x": 120, "y": 80, "w": 1100, "h": 860, "maximized": false },
  "recent": [{ "path": "C:\\…\\design.md", "opened": "2026-10-02T09:12:00Z", "hash": "…", "anchor": "…" }],
  "editor": null }
```

- Up to 12 recent files are stored; 8 are shown.
- `anchor` restores the reading position on reopen only if `hash` still matches the file, so regenerated docs start at the top.
- The ⋯ menu holds the remaining commands: Open…, Open recent ▸, Open in editor, Reveal in Explorer, Copy Markdown source, Reload, Zen mode, Keyboard shortcuts, About. Its footer shows `2,340 words · 11 min read` in 12 px `muted`.

## 9. Acceptance checklist

1. At 1280×800, Light, Medium, text size 16: the column is 736 px wide and centered in the area right of a 264 px docked outline. Body text is Inter 16/26 `#2A2430` on `#FCFAF9`.
2. A document with fewer than 3 headings shows no outline and a disabled outline button. At 1000 px window width, the outline opens as an overlay with a shadow and doesn't push the content.
3. Exactly one outline row is active: the section containing the line at 30% of viewport height. It has an `accent-soft` pill, `link` text and a 2 px `accent` bar.
4. H1 shows a full-width hairline with a 48×3 gradient tick on its left; H2 has a hairline; H3–H6 have none. Except where a heading directly follows another heading, the space above a heading is ≥ 2.5× the space below.
5. After 64 px of scrolling down, the app bar is gone and the 2 px gradient progress line is still at y = 0. Scrolling up 24 px brings the bar back.
6. A `ts` code block shows a "TypeScript" header and a copy icon, doesn't wrap, and shows a right-edge fade on long lines (both edges once scrolled). Clicking copy shows "Copied" with a check for 1.5 s.
7. All five alerts render with icon, title, tint and border using the exact hex values in §5. A Mermaid fence shows the header "Mermaid diagram · not rendered".
8. A four-level nested list shows the markers disc, ring, dash, disc; 26 px per level; and 1 px guides under each parent marker.
9. Ctrl+F "the": every match is filled `find-match`, the current one `find-current` with a ring, the count reads "k of n", and ticks appear on the scrollbar.
10. Live reload: editing a paragraph above the viewport shifts the top visible line by ≤ 2 px, and an accent bar appears beside the changed block, then disappears within about 3.5 s. At the bottom of the file, appended text keeps the view pinned to the end.
11. A 12-column table scrolls sideways inside its rounded frame. The page itself never has a horizontal scrollbar at any window width ≥ 480.
12. A broken image renders as a dashed chip with an icon and alt text. Remote badges in a README render inline on the baseline.
13. ✅ ❌ ⚠️ 🟢 render as monochrome glyphs in TIP, CAUTION, WARNING and TIP colors. Other emoji use the current text color.
14. External links end in ↗. Hovering any link shows its destination pill at the bottom-left within 300 ms.
15. Dark screenshots contain no pure `#FFFFFF` or `#000000` pixels in the chrome or text. The empty state shows the mascot, title, keycap hint, "Open file…" button and up to 8 recent rows, and the bar shows only Aa and ⋯.
