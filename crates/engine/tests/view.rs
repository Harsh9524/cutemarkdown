//! Headless DocView tests: drive an `egui::Context` directly (no window, no GPU).

use engine::{DocOutput, DocView, Document, FontChoice, Style, ThemeKind};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples");

struct Harness {
    ctx: egui::Context,
    style: Style,
    frame: u32,
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
        }
    }

    fn frame(&mut self, view: &mut DocView) -> DocOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 860.0),
            )),
            time: Some(self.frame as f64 / 60.0),
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
    let src = std::fs::read_to_string(std::path::Path::new(SAMPLES).join("ai-report.md")).unwrap();
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
