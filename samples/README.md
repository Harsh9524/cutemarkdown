# cutemarkdown sample corpus

- `ai-report.md`: hero document, a polished agent-written migration plan (front matter, emoji headings, 4-level nested lists, tables incl. a wide one, all five alerts, task lists, footnotes, mermaid, LaTeX, links, code in list items).
- `architecture.md`: technical design doc with fenced code in 17 languages, a 200-character line, plain/no-language block, local, broken and remote (badge) images, and links back to `ai-report.md`.
- `commonmark-edge.md`: tricky CommonMark/GFM syntax (setext, emphasis, hard breaks, escapes, autolinks, reference links, safe vs dangerous HTML, nested quotes, list quirks, escaped-pipe tables, duplicate headings, emoji shortcodes, multilingual text).
- `long.md`: generated ~5,000-line, 40-section document (paragraphs, lists, code, tables) for scroll, outline and render performance testing.
- `gen_long.py`: deterministic generator that writes `long.md` (run `python3 samples/gen_long.py [sections]`).
- `short.md`: 6-line note with no headings, to verify the outline sidebar auto-hides.
- `img/diagram.png`: small real PNG (640x240) used by `architecture.md`; `img/missing.png` is deliberately absent.
