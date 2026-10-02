//! Headless DocView tests: drive an `egui::Context` directly (no window, no GPU).

use engine::{DocOutput, DocView, Document, FontChoice, Style, ThemeKind};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples");

struct Harness {
    ctx: egui::Context,
    style: Style,
    frame: u32,
    size: egui::Vec2,
}

impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        engine::fonts::install(&ctx);
        let mut style = Style::new(ThemeKind::Light, FontChoice::Sans, 16.0, 736.0);
        style.top_inset = 44.0;
        Self {
            ctx,
            style,
            frame: 0,
            size: egui::vec2(1100.0, 860.0),
        }
    }

    fn frame(&mut self, view: &mut DocView) -> DocOutput {
        self.frame_with(view, Vec::new())
    }

    fn frame_with(&mut self, view: &mut DocView, events: Vec<egui::Event>) -> DocOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.frame as f64 / 60.0),
            events,
            ..Default::default()
        };
        self.frame += 1;
        let mut out = DocOutput::default();
        let style = self.style.clone();
        self.ctx
            .run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    out = view.show(ui, &style);
                });
            })
            .drop_without_applying_deltas();
        out
    }

    /// Run frames until layout is complete and animations have settled.
    fn settle(&mut self, view: &mut DocView) -> DocOutput {
        let mut out = self.frame(view);
        for _ in 0..400 {
            let before = view.scroll_offset();
            out = self.frame(view);
            if view.layout_complete() && (view.scroll_offset() - before).abs() < 0.01 {
                break;
            }
        }
        out
    }
}

fn load(name: &str) -> Document {
    let path = std::path::Path::new(SAMPLES).join(name);
    let src = std::fs::read_to_string(&path).unwrap();
    Document::parse(&src, path.parent())
}

#[test]
fn hero_doc_anchor_jump_and_scrollspy() {
    let mut h = Harness::new();
    let mut view = DocView::new(load("ai-report.md"));
    let out = h.settle(&mut view);
    assert_eq!(out.active_heading, Some(0));
    assert!(out.scrollable);
    assert_eq!(out.progress, 0.0);

    let idx = view
        .document()
        .headings()
        .iter()
        .position(|x| x.anchor == "rollback-plan")
        .unwrap();
    assert!(view.scroll_to_anchor("rollback-plan"));
    assert!(
        view.scroll_to_anchor("#Rollback-Plan"),
        "case-insensitive fallback"
    );
    assert!(!view.scroll_to_anchor("no-such-anchor"));
    let out = h.settle(&mut view);
    assert_eq!(out.active_heading, Some(idx));
    assert!(out.progress > 0.5 && out.progress < 1.0, "{}", out.progress);
    assert!(out.words_remaining > 0 && out.words_remaining < view.document().word_count());
    // "Open in editor here" line: the heading line, give or take the block boundary.
    let line = view.document().heading_line(idx).unwrap();
    assert!(
        out.top_source_line >= line.saturating_sub(2) && out.top_source_line <= line + 2,
        "{} vs {line}",
        out.top_source_line
    );
}

#[test]
fn scrollspy_line_is_30_percent_down_the_window() {
    // commonmark-edge: the second H1 starts just below 30% of an 860 px window (SPEC §3,
    // §9 item 3), so the first section is the one being read at the top.
    let mut h = Harness::new();
    let mut view = DocView::new(load("commonmark-edge.md"));
    let out = h.settle(&mut view);
    assert_eq!(out.active_heading, Some(0));
}

#[test]
fn anchor_jump_on_open_lands_like_an_in_document_jump() {
    for (name, anchor) in [
        ("ai-report.md", "rollback-plan"),
        ("ai-report.md", "appendix-a-consumer-inventory"),
        ("long.md", "section-40"),
    ] {
        let mut h = Harness::new();
        // Right after opening, as the shell does for `file.md#anchor`: most heights are
        // still estimates.
        let mut view = DocView::new(load(name));
        if !view.scroll_to_anchor(anchor) {
            let a = view.document().headings()[view.document().headings().len() / 2]
                .anchor
                .clone();
            assert!(view.scroll_to_anchor(&a));
        }
        h.settle(&mut view);
        let on_open = view.scroll_offset();
        // The same jump once everything is laid out.
        view.scroll_to_top();
        h.settle(&mut view);
        let a = anchor.to_owned();
        if !view.scroll_to_anchor(&a) {
            let a = view.document().headings()[view.document().headings().len() / 2]
                .anchor
                .clone();
            view.scroll_to_anchor(&a);
        }
        h.settle(&mut view);
        assert!(
            (view.scroll_offset() - on_open).abs() < 1.0,
            "{name}#{anchor}: {on_open} on open vs {}",
            view.scroll_offset()
        );
    }
}

#[test]
fn any_window_size_renders() {
    for size in [
        (100.0, 50.0),
        (1.0, 1.0),
        (100000.0, 10.0),
        (60.0, 2000.0),
        (480.0, 30.0),
    ] {
        let mut h = Harness::new();
        h.size = egui::vec2(size.0, size.1);
        let mut view = DocView::new(load("ai-report.md"));
        h.settle(&mut view);
        view.scroll_to_anchor("rollback-plan");
        h.settle(&mut view);
        // Pointer over the scrollbar, a footnote and a drag-select past the edges.
        let events = vec![
            egui::Event::PointerMoved(egui::pos2(size.0 - 2.0, size.1 / 2.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(size.0 / 2.0, size.1 / 2.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(egui::pos2(size.0 + 50.0, size.1 + 50.0)),
        ];
        h.frame_with(&mut view, events);
        h.frame(&mut view);
    }
}

#[test]
fn huge_code_blocks_find_select_and_scroll() {
    let code: String = (0..20_000)
        .map(|i| format!("line {i:05} = foo({i}) # comment\n"))
        .collect();
    let src = format!("# Log\n\nBefore.\n\n```\n{code}```\n\nAfter the log.\n");
    let mut h = Harness::new();
    let mut view = DocView::new(Document::parse(&src, None));
    h.settle(&mut view);
    // Find scrolls a chunk far down into view.
    let st = view.set_find_query("line 19876 ");
    assert_eq!(st.total, 1);
    let out = h.settle(&mut view);
    let match_line = 6 + 19876;
    assert!(
        out.top_source_line <= match_line && out.top_source_line + 40 >= match_line,
        "top line {} vs match on line {match_line}",
        out.top_source_line
    );
    // Drag-select a few lines in the middle of the viewport (a lazily shaped chunk).
    let press = |pressed: bool, y: f32| egui::Event::PointerButton {
        pos: egui::pos2(400.0, y),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.frame_with(
        &mut view,
        vec![egui::Event::PointerMoved(egui::pos2(400.0, 300.0))],
    );
    h.frame_with(&mut view, vec![press(true, 300.0)]);
    h.frame_with(
        &mut view,
        vec![egui::Event::PointerMoved(egui::pos2(400.0, 420.0))],
    );
    h.frame_with(&mut view, vec![press(false, 420.0)]);
    let sel = view.selected_text().expect("a selection");
    assert!(
        sel.contains("= foo(") && sel.lines().count() >= 3,
        "{sel:?}"
    );
    // Select all still copies every line.
    view.select_all();
    let all = view.selected_text().unwrap();
    assert!(all.contains("line 00000 = foo(0)") && all.contains("line 19999 = foo(19999)"));
    view.scroll_to_bottom();
    let out = h.settle(&mut view);
    assert!((out.progress - 1.0).abs() < 1e-3);
}

#[test]
fn end_reaches_the_bottom_of_a_long_doc() {
    let mut h = Harness::new();
    let mut view = DocView::new(load("long.md"));
    h.frame(&mut view);
    view.scroll_to_bottom();
    let out = h.settle(&mut view);
    assert!((out.progress - 1.0).abs() < 1e-3, "{}", out.progress);
    assert_eq!(
        out.active_heading,
        Some(view.document().headings().len() - 1)
    );
}

#[test]
fn ctrl_down_steps_through_headings() {
    let mut h = Harness::new();
    let mut view = DocView::new(load("ai-report.md"));
    h.settle(&mut view);
    let ctrl_down = egui::Event::Key {
        key: egui::Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    let mut offsets = vec![view.scroll_offset()];
    for _ in 0..3 {
        h.frame_with(&mut view, vec![ctrl_down.clone()]);
        h.settle(&mut view);
        offsets.push(view.scroll_offset());
    }
    assert!(
        offsets.windows(2).all(|w| w[1] > w[0] + 1.0),
        "each Ctrl+Down moves to the next heading: {offsets:?}"
    );
}

#[test]
fn select_all_copies_structured_text() {
    let mut h = Harness::new();
    let mut view = DocView::new(load("ai-report.md"));
    h.settle(&mut view);
    assert_eq!(view.selected_text(), None);
    view.select_all();
    let text = view.selected_text().unwrap();
    assert!(text.contains("Migration Plan: Moving the Billing Service"));
    assert!(text.contains("\n- [x] Unit tests for every aggregate invariant"));
    assert!(
        text.contains("Option\tComplexity\tAudit trail"),
        "table rows are tab-separated"
    );
    assert!(
        !text.contains('\u{2060}'),
        "no layout markers in copied text"
    );
    view.clear_selection();
    assert!(!view.has_selection());
}

#[test]
fn reload_keeps_the_reader_in_place() {
    let mut h = Harness::new();
    // Normalise line endings: Windows checkouts may use CRLF, and the edit below matches "\n".
    let src = std::fs::read_to_string(std::path::Path::new(SAMPLES).join("ai-report.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let mut view = DocView::new(Document::parse(&src, None));
    h.settle(&mut view);
    view.scroll_to_anchor("rollback-plan");
    let before = h.settle(&mut view);
    let y0 = view.scroll_offset();
    // Insert two paragraphs near the top, above the reader.
    let edited = src.replacen(
        "## TL;DR\n",
        "## TL;DR\n\nA brand new paragraph.\n\nAnd another one that is a bit longer.\n",
        1,
    );
    view.set_document(Document::parse(&edited, None), true);
    let after = h.settle(&mut view);
    assert_eq!(after.active_heading, before.active_heading);
    assert!(
        view.scroll_offset() > y0 + 40.0,
        "{} vs {y0}",
        view.scroll_offset()
    );

    // At the end, appended content keeps the view pinned to the end.
    view.scroll_to_bottom();
    h.settle(&mut view);
    let appended = format!("{edited}\n\n## Appendix B\n\nMore text.\n");
    view.set_document(Document::parse(&appended, None), true);
    let out = h.settle(&mut view);
    assert!((out.progress - 1.0).abs() < 1e-3, "{}", out.progress);
}

#[test]
fn find_counts_steps_and_case() {
    let mut h = Harness::new();
    let mut view = DocView::new(load("ai-report.md"));
    h.settle(&mut view);
    let st = view.set_find_query("the");
    assert_eq!(st.total, 92);
    assert_eq!(st.current, Some(0));
    assert_eq!(view.find_next().current, Some(1));
    assert_eq!(view.find_prev().current, Some(0));
    assert_eq!(view.find_prev().current, Some(91), "wraps around");
    let st = view.set_find_case_sensitive(true);
    assert_eq!(st.total, 83);
    view.select_current_match();
    assert_eq!(view.selected_text().as_deref(), Some("the"));
    view.clear_find();
    assert_eq!(view.find_status().total, 0);
    h.settle(&mut view);
}

#[test]
fn every_sample_renders_in_every_theme() {
    for name in [
        "ai-report.md",
        "architecture.md",
        "commonmark-edge.md",
        "short.md",
    ] {
        for theme in [ThemeKind::Light, ThemeKind::Sepia, ThemeKind::Dark] {
            let mut h = Harness::new();
            h.style = Style::new(theme, FontChoice::Serif, 18.0, 600.0);
            h.style.wrap_code = true;
            let mut view = DocView::new(load(name));
            h.settle(&mut view);
            view.scroll_to_bottom();
            h.settle(&mut view);
        }
    }
}

#[test]
fn reload_inside_an_edited_block_keeps_the_reader_in_place() {
    let mut h = Harness::new();
    let src = std::fs::read_to_string(std::path::Path::new(SAMPLES).join("ai-report.md")).unwrap();
    let mut view = DocView::new(Document::parse(&src, None));
    h.settle(&mut view);
    let idx = view
        .document()
        .headings()
        .iter()
        .position(|x| x.text.contains("Implementation Steps"))
        .unwrap();
    view.scroll_to_heading(idx);
    h.settle(&mut view);
    // Deep inside the (single, ~1000 px tall) ordered list, its top well above the viewport.
    view.set_scroll_offset(view.scroll_offset() + 600.0);
    h.settle(&mut view);
    let y0 = view.scroll_offset();
    // Edit a later item of that list, below the reading line.
    let edited = src.replacen("a separate sign-off.", "a separate sign-off (edited).", 1);
    assert_ne!(edited, src);
    view.set_document(Document::parse(&edited, None), true);
    h.settle(&mut view);
    assert!(
        (view.scroll_offset() - y0).abs() < 1.0,
        "{} vs {y0}",
        view.scroll_offset()
    );
}

/// Pathologically deep documents (SPEC principle 7: no file may crash the reader), with text
/// from their deepest level that must still be shown.
fn deep_docs() -> Vec<(&'static str, String, &'static str)> {
    let n = 5000;
    // (Lists are indented per level, so their source grows quadratically: 1000 levels.)
    let list = |marker: &str, indent: usize| -> String {
        (0..1000)
            .map(|i| format!("{}{marker} item {i}\n", " ".repeat(indent * i)))
            .collect()
    };
    vec![
        ("quotes", format!("{} deep\n", ">".repeat(n)), "deep"),
        ("spaced quotes", format!("{}deep\n", "> ".repeat(n)), "deep"),
        ("list", list("-", 2), "item 999"),
        ("ordered list", list("1.", 3), "item 999"),
        (
            "alert",
            format!("> [!NOTE]\n{}deep\n", "> ".repeat(n)),
            "deep",
        ),
        (
            "footnote",
            format!("See[^a].\n\n[^a]: {}deep\n", "> ".repeat(n)),
            "deep",
        ),
        (
            "emphasis",
            format!("{}deep{}\n", "*".repeat(20000), "*".repeat(20000)),
            "deep",
        ),
        (
            "mixed emphasis",
            format!("{}deep{}\n", "*_".repeat(5000), "_*".repeat(5000)),
            "deep",
        ),
        (
            "links",
            format!("{}deep{}\n", "[".repeat(n), "](u)".repeat(n)),
            "deep",
        ),
        (
            "details",
            format!(
                "{}\ndeep\n\n{}\n",
                "<details open><summary>s</summary>\n".repeat(n),
                "</details>\n".repeat(n)
            ),
            "deep",
        ),
        (
            "html inline",
            format!("x {}deep{}\n", "<b><i>".repeat(n), "</i></b>".repeat(n)),
            "deep",
        ),
        (
            "table cell",
            format!(
                "| a |\n|---|\n| {}deep{} |\n",
                "*".repeat(20000),
                "*".repeat(20000)
            ),
            "deep",
        ),
    ]
}

#[test]
fn deep_nesting_never_overflows_a_1mb_stack() {
    // Windows gives the main thread 1 MB; the app parses and lays out documents on it.
    let t = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            for (name, src, needle) in deep_docs() {
                let mut h = Harness::new();
                let mut view = DocView::new(Document::parse(&src, None));
                h.settle(&mut view);
                view.select_all();
                let text = view.selected_text().unwrap_or_default();
                assert!(text.contains(needle), "{name}: {needle:?} kept");
                view.scroll_to_bottom();
                h.settle(&mut view);
            }
        })
        .unwrap();
    t.join().expect("no stack overflow or panic");
}
