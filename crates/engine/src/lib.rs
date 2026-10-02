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

mod find;
mod highlight;
mod html;
mod icons;
mod ir;
mod layout;
mod parse;
mod paths;
mod slug;
mod view;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use paths::{file_uri_to_path, is_remote_path};
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
    File {
        path: PathBuf,
        anchor: Option<String>,
    },
    /// An in-document anchor (`#rollback-plan`), without the leading `#`.
    Anchor(String),
}

/// Parsed Markdown document.
pub struct Document {
    source: String,
    base_dir: Option<PathBuf>,
    p: parse::Parsed,
    /// Characters no bundled font covers (system fallbacks are loaded for them).
    missing: Vec<char>,
    search: OnceLock<Vec<find::SearchText>>,
}

impl Document {
    /// Parse Markdown. `base_dir` is used to resolve relative links and images.
    pub fn parse(source: &str, base_dir: Option<&Path>) -> Self {
        let p = parse::parse(source, base_dir);
        let missing = fonts::missing_chars(p.non_ascii.iter().copied());
        Self {
            source: source.to_owned(),
            base_dir: base_dir.map(Path::to_path_buf),
            p,
            missing,
            search: OnceLock::new(),
        }
    }

    /// Front-matter `title`, else the first H1.
    pub fn title(&self) -> Option<&str> {
        self.p.title.as_deref()
    }

    pub fn headings(&self) -> &[Heading] {
        &self.p.headings
    }

    /// Words of readable text (excluding front matter); used for read-time.
    pub fn word_count(&self) -> usize {
        self.p.word_count
    }

    pub fn base_dir(&self) -> Option<&Path> {
        self.base_dir.as_deref()
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// 1-based source line of a heading (for "Open in editor here").
    pub fn heading_line(&self, index: usize) -> Option<usize> {
        self.p.heading_meta.get(index).map(|m| m.line as usize)
    }

    /// Markdown source of a heading's section: from the heading to the line before the next
    /// heading of the same or higher level ("Copy section as Markdown").
    pub fn section_markdown(&self, index: usize) -> Option<String> {
        let m = self.p.heading_meta.get(index)?;
        let src = self.source.replace("\r\n", "\n");
        let lines: Vec<&str> = src.split('\n').collect();
        let a = (m.line as usize).saturating_sub(1);
        let b = (m.section_end as usize).min(lines.len());
        (a < b).then(|| lines[a..b].join("\n").trim_end().to_owned())
    }

    /// Does this document have a heading, footnote or HTML anchor with this name?
    pub fn has_anchor(&self, anchor: &str) -> bool {
        self.p.anchors.contains_key(anchor)
    }

    fn search_texts(&self) -> &[find::SearchText] {
        self.search
            .get_or_init(|| self.p.texts.iter().map(find::SearchText::new).collect())
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
    /// Readable words below the top of the viewport (code excluded), for "N min left".
    pub words_remaining: usize,
    /// 1-based source line of the first visible block (for "Open in editor here").
    pub top_source_line: usize,
    /// The document is taller than the viewport (show the progress line only then).
    pub scrollable: bool,
    /// The link in `clicked_link` was Ctrl+clicked or middle-clicked: open it in a new window.
    pub link_new_window: bool,
    /// "Open in editor here" was chosen in a heading's context menu: 1-based source line.
    pub open_in_editor_line: Option<usize>,
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
    v: view::ViewState,
}

impl DocView {
    pub fn new(doc: Document) -> Self {
        let v = view::ViewState::new(&doc);
        Self { doc, v }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Replace the document (e.g. live reload). With `keep_position`, the reader stays where
    /// they were (same section / same offset); otherwise the view starts at the top.
    pub fn set_document(&mut self, doc: Document, keep_position: bool) {
        self.v.set_document(&self.doc, &doc, keep_position);
        self.doc = doc;
    }

    /// Paint the document filling the available space (it owns its vertical scroll area).
    pub fn show(&mut self, ui: &mut egui::Ui, style: &Style) -> DocOutput {
        self.v.show(&self.doc, ui, style)
    }

    pub fn scroll_to_heading(&mut self, index: usize) {
        if index < self.doc.p.headings.len() {
            self.v.request(view::ScrollReqPub::Heading(index));
        }
    }

    /// Returns false if no heading/footnote has that anchor.
    pub fn scroll_to_anchor(&mut self, anchor: &str) -> bool {
        let anchor = anchor.trim_start_matches('#');
        let name = if self.doc.p.anchors.contains_key(anchor) {
            anchor.to_owned()
        } else {
            let lower = anchor.to_lowercase();
            if self.doc.p.anchors.contains_key(&lower) {
                lower
            } else {
                return false;
            }
        };
        self.v.request(view::ScrollReqPub::Anchor(name));
        true
    }

    pub fn scroll_to_top(&mut self) {
        self.v.request(view::ScrollReqPub::Y(0.0));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.v.request(view::ScrollReqPub::Bottom);
    }

    /// Current vertical scroll offset in points (for back/forward history).
    pub fn scroll_offset(&self) -> f32 {
        self.v.scroll_offset()
    }

    pub fn set_scroll_offset(&mut self, y: f32) {
        self.v.request(view::ScrollReqPub::Y(y));
    }

    /// Set the find query (case-insensitive); highlights all matches and scrolls to the first one
    /// at or after the current position.
    pub fn set_find_query(&mut self, query: &str) -> FindStatus {
        let cs = self.v.find_case_sensitive();
        self.v.set_find(&self.doc, query, cs, true)
    }

    /// The find bar's `Aa` toggle: match case on/off, re-running the current query.
    pub fn set_find_case_sensitive(&mut self, on: bool) -> FindStatus {
        self.v.set_find_case(&self.doc, on)
    }

    pub fn find_next(&mut self) -> FindStatus {
        self.v.find_step(true)
    }

    pub fn find_prev(&mut self) -> FindStatus {
        self.v.find_step(false)
    }

    /// Current find state (e.g. after a live reload re-ran the query).
    pub fn find_status(&self) -> FindStatus {
        self.v.find_status()
    }

    /// Ends find: clears matches and highlights (the match-case setting is kept).
    pub fn clear_find(&mut self) {
        self.v.clear_find();
    }

    pub fn has_selection(&self) -> bool {
        self.v.has_selection()
    }

    /// The current text selection as plain text (what Ctrl+C copies), e.g. to pre-fill find.
    pub fn selected_text(&self) -> Option<String> {
        let t = self.v.selected_text(&self.doc);
        (!t.is_empty()).then_some(t)
    }

    /// Esc in the find bar: the current match becomes the text selection (best effort).
    pub fn select_current_match(&mut self) {
        self.v.select_current_match();
    }

    pub fn select_all(&mut self) {
        self.v.select_all(&self.doc);
    }

    pub fn clear_selection(&mut self) {
        self.v.clear_selection();
    }

    /// CPU time of the last [`Self::show`] call, in seconds (benchmarks).
    pub fn last_show_secs(&self) -> f64 {
        self.v.last_show_secs
    }

    /// Every block has been laid out at the current width (layout runs progressively).
    pub fn layout_complete(&self) -> bool {
        self.v.layout_complete
    }
}
