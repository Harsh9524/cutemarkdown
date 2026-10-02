//! Intermediate representation: what the parser produces and the layout pass consumes.
//!
//! Inline content is flattened into [`RichText`]: one string per text container (paragraph,
//! heading, table cell, code block…) plus style spans. Each `RichText` is a *run*, numbered in
//! document order; selection, find and copy address text as `(run, char index)`.
//!
//! Inline objects (images, the external-link arrow) and padding (inline code, `<kbd>`) are
//! represented in the text by [`MARKER`] characters, which are invisible and zero-width in
//! egui. Layout gives them their width. They are skipped by find and copy.

use std::ops::Range;
use std::path::PathBuf;

/// Invisible placeholder character (WORD JOINER). egui renders it with zero width.
pub const MARKER: char = '\u{2060}';
pub const MARKER_STR: &str = "\u{2060}";

/// Index of a [`RichText`] run (in document order).
pub type RunId = u32;

/// Inline style flags.
pub mod flags {
    pub const STRONG: u16 = 1 << 0;
    pub const EM: u16 = 1 << 1;
    pub const CODE: u16 = 1 << 2;
    pub const STRIKE: u16 = 1 << 3;
    pub const SUP: u16 = 1 << 4;
    pub const SUB: u16 = 1 << 5;
    pub const MARK: u16 = 1 << 6;
    pub const KBD: u16 = 1 << 7;
    pub const LINK: u16 = 1 << 8;
    pub const UNDERLINE: u16 = 1 << 9;
    /// Footnote reference number.
    pub const FOOTREF: u16 = 1 << 10;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpanKind {
    Text,
    /// A single marker char reserving `em` × font size of horizontal space (padding).
    Pad(f32),
    /// Two marker chars delimiting an inline object (index into [`RichText::objects`]).
    Object(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// Byte range into [`RichText::text`].
    pub range: Range<usize>,
    pub flags: u16,
    /// Index into [`RichText::links`].
    pub link: Option<u32>,
    pub kind: SpanKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkDest {
    External(String),
    /// Resolved local path plus optional anchor.
    File {
        path: PathBuf,
        anchor: Option<String>,
    },
    Anchor(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The destination as written (for the hover pill).
    pub href: String,
    pub dest: LinkDest,
    /// External links written as `[text](url)` get a trailing ↗; bare autolinks don't.
    pub external_icon: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InlineObject {
    Image(ImageRef),
    /// The ↗ after an external link (same link index as the text before it).
    ExternalIcon,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageRef {
    /// URI for egui's loaders (`file://…`, `https://…`), or `None` if the source was unsafe.
    pub uri: Option<String>,
    /// The source as written (tooltip, "copy image address").
    pub src: String,
    pub alt: String,
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// Link wrapping the image, if any (index into the containing `RichText::links`).
    pub link: Option<u32>,
}

/// One run of inline content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichText {
    pub run: u32,
    pub text: String,
    /// Contiguous, non-overlapping, covering `text`.
    pub spans: Vec<Span>,
    pub links: Vec<Link>,
    pub objects: Vec<InlineObject>,
}

impl RichText {
    /// Text as shown, minus markers (used for headings/outline and plain copies).
    pub fn plain(&self) -> String {
        self.text.chars().filter(|&c| c != MARKER).collect()
    }

    pub fn is_blank(&self) -> bool {
        self.text.chars().all(|c| c.is_whitespace() || c == MARKER)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeKind {
    Normal,
    /// Mermaid, PlantUML, Graphviz, D2: shown as source.
    Diagram,
    /// TeX math: shown as source.
    Math,
    /// Raw HTML we don't render (e.g. `<table>`).
    Html,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodeBlock {
    /// The info string's first word, as written (`ts`, `rust`…).
    pub lang: Option<String>,
    /// Header label (`TypeScript`, `Mermaid diagram`…). `None` = no header.
    pub label: Option<String>,
    /// Shown after the label in `muted` (`· not rendered`).
    pub label_note: Option<String>,
    pub kind: CodeKind,
    /// The code (no trailing newline). Its run is `text.run`.
    pub run: RunId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    /// `Some(checked)` for task items.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub ordered: bool,
    pub start: u64,
    pub tight: bool,
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub aligns: Vec<HAlign>,
    /// Columns right-aligned because ≥ 80 % of their cells are numeric (no explicit alignment).
    pub numeric: Vec<bool>,
    pub header: Vec<RunId>,
    pub rows: Vec<Vec<RunId>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontMatter {
    pub raw: String,
    /// `title: Spec · status: draft · …`
    pub preview: String,
    pub toml: bool,
    /// The raw source as a run (shown when expanded).
    pub run: RunId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Footnote {
    pub name: String,
    pub number: u32,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Heading {
        level: u8,
        run: RunId,
        index: usize,
        align: HAlign,
    },
    Paragraph {
        run: RunId,
        align: HAlign,
    },
    /// An image alone in its paragraph.
    Image {
        image: ImageRef,
        links: Vec<Link>,
        align: HAlign,
    },
    Code(CodeBlock),
    List(List),
    Quote(Vec<Block>),
    Alert {
        kind: AlertKind,
        blocks: Vec<Block>,
    },
    Table(Table),
    Rule,
    Details {
        summary: RunId,
        open: bool,
        blocks: Vec<Block>,
    },
    FrontMatter(FrontMatter),
    Footnotes(Vec<Footnote>),
    /// `<p|div align="center">`, `<center>`: children centered.
    Center(Vec<Block>),
    /// An HTML `id`/`name` anchor target with no visible content.
    Anchor(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// 1-based source lines (inclusive).
    pub line: u32,
    pub end_line: u32,
    /// Content-derived id, stable across reloads (UI state such as `<details>` open).
    pub id: u64,
}

/// Per-run metadata used by copy formatting and word counts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunInfo {
    pub kind: RunKind,
    /// Top-level block index containing the run.
    pub top: u32,
    /// List nesting depth (0 = not in a list).
    pub list_depth: u8,
    /// Marker (`- `, `3. `) if this run is the first text of a list item.
    pub list_marker: Option<String>,
    /// For table cells: (table ordinal, row, column).
    pub cell: Option<(u32, u32, u32)>,
    /// 1-based source line of the block that holds this run.
    pub line: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunKind {
    #[default]
    Text,
    Heading,
    Code,
    Cell,
    FrontMatter,
}
