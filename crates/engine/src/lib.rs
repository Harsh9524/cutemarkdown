//! cutemarkdown document engine: parse Markdown, lay it out and paint it with egui.
//!
//! This crate is the contract between the document engine and the app shell:
//!
//! * [`Document::parse`] turns Markdown source into a parsed document (pure; no egui needed).
//! * [`DocView`] owns a [`Document`] plus layout, scroll and find state, and paints it into an
//!   egui [`Ui`](egui::Ui). It handles reading-keyboard input (PgUp/PgDn/Space/Home/End/arrows)
//!   itself while no text field has focus.
//! * [`Style`] carries everything visual (palette, font family, text size, measure). The shell
//!   chooses it; the engine obeys it.
//! * [`fonts::install`] registers the bundled fonts with an egui context (call once at startup).
//!
//! Public items here are relied on by the shell. Additive changes are fine; renames and removals
//! are breaking.

pub mod fonts;
pub mod style;

use std::path::{Path, PathBuf};

pub use style::{FontChoice, Palette, Style, SyntaxPalette, ThemeKind};

/// A heading in document order, used for the outline sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    /// 1..=6
    pub level: u8,
    /// Plain text (inline markup stripped).
    pub text: String,
    /// Unique, GitHub-compatible slug (e.g. `rollback-plan`, `usage-1`).
    pub anchor: String,
}

/// Where a clicked link points. The shell decides what to do with it.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    /// `http(s)://`, `mailto:` and other external URLs: open in the system browser.
    External(String),
    /// A local file (resolved against the document's directory), e.g. `./architecture.md#data-model`.
    File { path: PathBuf, anchor: Option<String> },
    /// An in-document anchor (`#rollback-plan`), without the leading `#`.
    Anchor(String),
}

/// Parsed Markdown document.
pub struct Document {
    source: String,
    base_dir: Option<PathBuf>,
    title: Option<String>,
    headings: Vec<Heading>,
    word_count: usize,
}

impl Document {
    /// Parse Markdown. `base_dir` is used to resolve relative links and images.
    pub fn parse(source: &str, base_dir: Option<&Path>) -> Self {
        // STUB: minimal ATX-heading scan so the shell has something to work with.
        let mut headings = Vec::new();
        let mut in_fence = false;
        for line in source.lines() {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_fence = !in_fence;
            }
            if in_fence {
                continue;
            }
            let hashes = t.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
                let text = t[hashes..].trim().trim_end_matches('#').trim().to_owned();
                let anchor = text
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
                    .collect::<String>()
                    .replace(' ', "-");
                headings.push(Heading { level: hashes as u8, text, anchor });
            }
        }
        Self {
            title: headings.iter().find(|h| h.level == 1).map(|h| h.text.clone()),
            word_count: source.split_whitespace().count(),
            source: source.to_owned(),
            base_dir: base_dir.map(Path::to_path_buf),
            headings,
        }
    }

    /// Front-matter `title`, else the first H1.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn headings(&self) -> &[Heading] {
        &self.headings
    }

    /// Words of readable text (excluding front matter); used for read-time.
    pub fn word_count(&self) -> usize {
        self.word_count
    }

    pub fn base_dir(&self) -> Option<&Path> {
        self.base_dir.as_deref()
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// What happened in the document view during this frame.
#[derive(Clone, Debug, Default)]
pub struct DocOutput {
    /// A link was clicked this frame.
    pub clicked_link: Option<LinkTarget>,
    /// URL of the link under the pointer (for a status hint).
    pub hovered_link: Option<String>,
    /// Index into [`Document::headings`] of the section currently being read (scrollspy).
    pub active_heading: Option<usize>,
    /// Reading progress, 0.0 at the top to 1.0 at the bottom.
    pub progress: f32,
    /// Something (e.g. a code block) was copied to the clipboard this frame; the shell may toast.
    pub copied: bool,
}

/// Result of a find operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindStatus {
    pub total: usize,
    /// 0-based index of the current match, if any.
    pub current: Option<usize>,
}

/// Stateful document widget.
pub struct DocView {
    doc: Document,
    scroll_y: f32,
    pending_scroll: Option<f32>,
    find_query: String,
}

impl DocView {
    pub fn new(doc: Document) -> Self {
        Self { doc, scroll_y: 0.0, pending_scroll: None, find_query: String::new() }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Replace the document (e.g. live reload). With `keep_position`, the reader stays where
    /// they were (same section / same offset); otherwise the view starts at the top.
    pub fn set_document(&mut self, doc: Document, keep_position: bool) {
        self.doc = doc;
        if !keep_position {
            self.pending_scroll = Some(0.0);
        }
    }

    /// Paint the document filling the available space (it owns its vertical scroll area).
    pub fn show(&mut self, ui: &mut egui::Ui, style: &Style) -> DocOutput {
        // STUB: plain-text rendering so the shell can be developed against the real API.
        let mut out = DocOutput::default();
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(y) = self.pending_scroll.take() {
            area = area.vertical_scroll_offset(y);
        }
        let r = area.show(ui, |ui| {
            let w = ui.available_width().min(style.measure);
            ui.vertical_centered(|ui| {
                ui.set_max_width(w);
                for block in self.doc.source.split("\n\n") {
                    let t = block.trim();
                    if t.is_empty() {
                        continue;
                    }
                    let size = if t.starts_with("# ") { style.text_size * 2.0 } else { style.text_size };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(t).size(size).color(style.palette.text),
                        )
                        .wrap(),
                    );
                    ui.add_space(style.text_size * 0.8);
                }
            });
        });
        self.scroll_y = r.state.offset.y;
        let max = (r.content_size.y - r.inner_rect.height()).max(1.0);
        out.progress = (self.scroll_y / max).clamp(0.0, 1.0);
        let _ = &self.find_query;
        out
    }

    pub fn scroll_to_heading(&mut self, index: usize) {
        let _ = index;
    }

    /// Returns false if no heading/footnote has that anchor.
    pub fn scroll_to_anchor(&mut self, anchor: &str) -> bool {
        self.doc.headings.iter().position(|h| h.anchor == anchor).map(|i| self.scroll_to_heading(i)).is_some()
    }

    pub fn scroll_to_top(&mut self) {
        self.pending_scroll = Some(0.0);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.pending_scroll = Some(f32::MAX);
    }

    /// Current vertical scroll offset in points (for back/forward history).
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_y
    }

    pub fn set_scroll_offset(&mut self, y: f32) {
        self.pending_scroll = Some(y);
    }

    /// Set the find query (case-insensitive); highlights all matches and scrolls to the first one
    /// at or after the current position.
    pub fn set_find_query(&mut self, query: &str) -> FindStatus {
        self.find_query = query.to_owned();
        FindStatus::default()
    }

    pub fn find_next(&mut self) -> FindStatus {
        FindStatus::default()
    }

    pub fn find_prev(&mut self) -> FindStatus {
        FindStatus::default()
    }

    pub fn clear_find(&mut self) {
        self.find_query.clear();
    }
}
