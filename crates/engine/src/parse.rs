//! Markdown → IR: comrak AST walk, inline flattening, safe-HTML subset, front matter,
//! footnotes, alerts, math/diagram fallbacks.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use comrak::arena_tree::NodeEdge;
use comrak::nodes::{AlertType, AstNode, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Options, parse_document};

use crate::Heading;
use crate::html::{self, Token};
use crate::ir::*;
use crate::paths;
use crate::slug::Slugger;

/// Everything the parser produces.
#[derive(Default)]
pub struct Parsed {
    pub blocks: Vec<Block>,
    pub texts: Vec<RichText>,
    pub runs: Vec<RunInfo>,
    pub headings: Vec<Heading>,
    pub heading_meta: Vec<HeadingMeta>,
    /// Anchor name → top-level block containing it.
    pub anchors: HashMap<String, u32>,
    pub title: Option<String>,
    pub word_count: usize,
    /// Words per top-level block (code and front matter excluded).
    pub top_words: Vec<u32>,
    /// Stable keys per top-level block (reload anchoring).
    pub top_keys: Vec<BlockKey>,
    /// Distinct non-ASCII characters in the document (for fallback fonts).
    pub non_ascii: Vec<char>,
}

#[derive(Clone, Debug, Default)]
pub struct HeadingMeta {
    /// The heading text run.
    pub run: RunId,
    pub top: u32,
    /// 1-based source lines: the heading line and the last line of its section.
    pub line: u32,
    pub section_end: u32,
}

/// Content-anchored identity of a top-level block (SPEC §7 live reload).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockKey {
    pub path: u64,
    pub ordinal: u32,
    pub content: u64,
}

/// Containers (quotes, lists, alerts, footnotes, `<details>`) nested deeper than this are
/// flattened into one plain paragraph. That bounds the recursion in parsing, layout and paint
/// (no stack overflow on Windows' 1 MB main-thread stack), and the text stays readable.
const MAX_BLOCK_DEPTH: u16 = 32;
/// Inline formatting (emphasis, links, …) nested deeper than this keeps its text, unformatted.
const MAX_INLINE_DEPTH: u16 = 32;

const CODE_PAD_EM: f32 = 5.0 / 16.0;
const KBD_PAD_EM: f32 = 7.0 / 16.0;

pub fn parse(source: &str, base_dir: Option<&Path>) -> Parsed {
    // Normalize newlines; comrak handles CRLF but our line slicing is simpler with LF.
    let source = if source.contains('\r') {
        source.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        source.to_owned()
    };
    let source = source
        .strip_prefix('\u{FEFF}')
        .unwrap_or(&source)
        .to_owned();
    let lines: Vec<&str> = source.split('\n').collect();

    let mut p = P {
        lines: &lines,
        line_offset: 0,
        base_dir: base_dir.map(Path::to_path_buf),
        texts: Vec::new(),
        runs: Vec::new(),
        slugger: Slugger::default(),
        headings: Vec::new(),
        heading_meta: Vec::new(),
        footnote_numbers: HashMap::new(),
        footnotes: Vec::new(),
        html_anchors: Vec::new(),
        footrefs: Vec::new(),
        pending_marker: None,
        list_depth: 0,
        depth: 0,
        inline_depth: 0,
        table_ord: 0,
        cell: None,
        cur_line: 1,
        id_counts: HashMap::new(),
    };

    let mut blocks = Vec::new();
    let mut title = None;
    let mut body = source.as_str();
    if let Some(fm) = split_front_matter(&source) {
        let run = p.new_run_plain(&fm.inner, RunKind::FrontMatter);
        let preview = front_matter_preview(&fm.inner, fm.toml);
        title = front_matter_title(&fm.inner, fm.toml);
        let id = p.block_id("frontmatter", 1, fm.lines);
        blocks.push(Block {
            kind: BlockKind::FrontMatter(FrontMatter {
                raw: fm.inner.clone(),
                preview,
                toml: fm.toml,
                run,
            }),
            line: 1,
            end_line: fm.lines,
            id,
        });
        body = &source[fm.bytes..];
        p.line_offset = fm.lines;
    }

    let arena = Arena::new();
    let options = comrak_options();
    let root = parse_document(&arena, body, &options);
    let mut sink = Sink::default();
    for child in root.children() {
        p.block(child, &mut sink);
    }
    blocks.extend(sink.finish(&mut p));

    if !p.footnotes.is_empty() {
        let mut notes = std::mem::take(&mut p.footnotes);
        notes.sort_by_key(|n| n.number);
        let (line, end_line) = notes
            .iter()
            .flat_map(|n| n.blocks.iter())
            .fold((u32::MAX, 0), |(a, b), bl| {
                (a.min(bl.line), b.max(bl.end_line))
            });
        let line = if line == u32::MAX {
            lines.len() as u32
        } else {
            line
        };
        let id = p.block_id("footnotes", line, end_line.max(line));
        blocks.push(Block {
            kind: BlockKind::Footnotes(notes),
            line,
            end_line: end_line.max(line),
            id,
        });
    }

    let mut out = Parsed {
        texts: std::mem::take(&mut p.texts),
        runs: std::mem::take(&mut p.runs),
        headings: std::mem::take(&mut p.headings),
        heading_meta: std::mem::take(&mut p.heading_meta),
        title,
        ..Default::default()
    };
    if out.title.is_none() {
        out.title = out
            .headings
            .iter()
            .find(|h| h.level == 1)
            .map(|h| h.text.clone());
    }

    // Post-pass: which top-level block each run/heading/anchor lives in, words, stable keys.
    let total_lines = lines.len() as u32;
    let mut path: Vec<(u8, u64)> = Vec::new();
    let mut ordinal = 0u32;
    for (i, block) in blocks.iter().enumerate() {
        let top = i as u32;
        let mut runs = Vec::new();
        let mut heads = Vec::new();
        let mut html_ids = Vec::new();
        collect_block(block, &mut runs, &mut heads, &mut html_ids);
        let mut words = 0u32;
        for &r in &runs {
            let info = &mut out.runs[r as usize];
            info.top = top;
            if matches!(info.kind, RunKind::Text | RunKind::Heading | RunKind::Cell) {
                words += count_words(&out.texts[r as usize].text) as u32;
            }
        }
        for h in heads {
            out.heading_meta[h].top = top;
            out.anchors
                .entry(out.headings[h].anchor.clone())
                .or_insert(top);
        }
        for id in html_ids {
            out.anchors.entry(id).or_insert(top);
        }
        if let BlockKind::Footnotes(notes) = &block.kind {
            for n in notes {
                out.anchors.entry(format!("fn-{}", n.name)).or_insert(top);
            }
        }
        out.top_words.push(words);
        out.word_count += words as usize;

        // Stable key: heading path + ordinal within the section + content hash.
        if let BlockKind::Heading { level, run, .. } = &block.kind {
            while path.last().is_some_and(|(l, _)| *l >= *level) {
                path.pop();
            }
            path.push((*level, hash_str(&out.texts[*run as usize].text)));
            ordinal = 0;
        } else {
            ordinal += 1;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        out.top_keys.push(BlockKey {
            path: h.finish(),
            ordinal,
            content: hash_str(&slice_lines(&lines, block.line, block.end_line)),
        });
    }
    for (name, run) in std::mem::take(&mut p.footrefs) {
        let top = out.runs[run as usize].top;
        out.anchors.entry(format!("fnref-{name}")).or_insert(top);
    }
    for (id, run) in std::mem::take(&mut p.html_anchors) {
        let top = out.runs.get(run as usize).map_or(0, |r| r.top);
        out.anchors.entry(id).or_insert(top);
    }

    // Heading sections (for "copy section as Markdown").
    let n = out.heading_meta.len();
    for i in 0..n {
        let level = out.headings[i].level;
        let end = (i + 1..n)
            .find(|&j| out.headings[j].level <= level)
            .map(|j| out.heading_meta[j].line.saturating_sub(1))
            .unwrap_or(total_lines);
        out.heading_meta[i].section_end = end.max(out.heading_meta[i].line);
    }

    let mut chars: Vec<char> = source.chars().filter(|&c| !c.is_ascii()).collect();
    chars.sort_unstable();
    chars.dedup();
    out.non_ascii = chars;
    out.blocks = blocks;
    out
}

fn comrak_options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.alerts = true;
    o.extension.superscript = false;
    o.extension.math_dollars = false;
    // Front matter is split off before comrak sees the text (we also support `+++`).
    o.extension.front_matter_delimiter = None;
    o
}

fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn slice_lines(lines: &[&str], from: u32, to: u32) -> String {
    let a = (from.max(1) - 1) as usize;
    let b = (to as usize).min(lines.len());
    if a >= b {
        return String::new();
    }
    lines[a..b].join("\n")
}

pub fn count_words(text: &str) -> usize {
    text.split(|c: char| c.is_whitespace() || c == MARKER)
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .count()
}

fn collect_block(b: &Block, runs: &mut Vec<RunId>, heads: &mut Vec<usize>, ids: &mut Vec<String>) {
    match &b.kind {
        BlockKind::Heading { run, index, .. } => {
            runs.push(*run);
            heads.push(*index);
        }
        BlockKind::Paragraph { run, .. } => runs.push(*run),
        BlockKind::Image { .. } | BlockKind::Rule => {}
        BlockKind::Code(c) => runs.push(c.run),
        BlockKind::List(l) => {
            for it in &l.items {
                for b in &it.blocks {
                    collect_block(b, runs, heads, ids);
                }
            }
        }
        BlockKind::Quote(bs) | BlockKind::Center(bs) | BlockKind::Alert { blocks: bs, .. } => {
            for b in bs {
                collect_block(b, runs, heads, ids);
            }
        }
        BlockKind::Details {
            summary, blocks, ..
        } => {
            runs.push(*summary);
            for b in blocks {
                collect_block(b, runs, heads, ids);
            }
        }
        BlockKind::Table(t) => {
            runs.extend(t.header.iter().copied());
            for r in &t.rows {
                runs.extend(r.iter().copied());
            }
        }
        BlockKind::FrontMatter(f) => runs.push(f.run),
        BlockKind::Footnotes(notes) => {
            for n in notes {
                for b in &n.blocks {
                    collect_block(b, runs, heads, ids);
                }
            }
        }
        BlockKind::Anchor(id) => ids.push(id.clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// Front matter

struct FrontMatterSplit {
    inner: String,
    toml: bool,
    /// Number of source lines consumed (including both delimiters).
    lines: u32,
    bytes: usize,
}

fn split_front_matter(src: &str) -> Option<FrontMatterSplit> {
    let first_end = src.find('\n')?;
    let first = src[..first_end].trim_end();
    let toml = match first {
        "---" => false,
        "+++" => true,
        _ => return None,
    };
    let mut offset = first_end + 1;
    let mut inner = String::new();
    for (k, line) in src[offset..].split_inclusive('\n').enumerate() {
        let n = k as u32 + 2; // lines consumed so far, both delimiters included
        let t = line.trim_end();
        let closes = if toml {
            t == "+++"
        } else {
            t == "---" || t == "..."
        };
        offset += line.len();
        if closes {
            let inner = inner.trim_end_matches('\n').to_owned();
            // Only real metadata: a document may just as well open with a thematic break and
            // have another one further down (SPEC §1.5: never hide content).
            let meta = if toml {
                looks_like_toml(&inner)
            } else {
                looks_like_yaml(&inner)
            };
            return meta.then_some(FrontMatterSplit {
                inner,
                toml,
                lines: n,
                bytes: offset,
            });
        }
        if k == 0 && line.trim().is_empty() {
            // `---` then a blank line is a rule, not front matter (as in Pandoc).
            return None;
        }
        inner.push_str(line);
    }
    None
}

/// YAML metadata: top-level `key: value` lines (at least one), list items, indented
/// continuations and comments only.
fn looks_like_yaml(inner: &str) -> bool {
    let mut keys = 0;
    for line in inner.lines() {
        let t = line.trim_end();
        if t.is_empty() || t.trim_start().starts_with('#') || t.starts_with([' ', '\t']) {
            continue;
        }
        if t == "-" || t.starts_with("- ") {
            continue;
        }
        // `key:` then a space or the end of the line; the key is a single word or quoted.
        let key_line = t.split_once(':').is_some_and(|(key, rest)| {
            let quoted = key.len() >= 2 && key.starts_with(['"', '\'']);
            let word = !key.is_empty()
                && !key.contains(char::is_whitespace)
                && key
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '$' | '@'));
            (quoted || word) && (rest.is_empty() || rest.starts_with([' ', '\t']))
        });
        if !key_line {
            return false;
        }
        keys += 1;
    }
    keys > 0
}

/// TOML metadata: `key = value` lines (at least one), `[table]` headers, comments, and the
/// continuation lines of multi-line arrays and strings.
fn looks_like_toml(inner: &str) -> bool {
    let mut keys = 0;
    let mut open_brackets = 0i32;
    let mut in_string: Option<&str> = None;
    let brackets = |s: &str| {
        s.chars().filter(|&c| c == '[').count() as i32
            - s.chars().filter(|&c| c == ']').count() as i32
    };
    for line in inner.lines() {
        let t = line.trim();
        if let Some(q) = in_string {
            if t.contains(q) {
                in_string = None;
            }
            continue;
        }
        if open_brackets > 0 {
            open_brackets += brackets(t);
            continue;
        }
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') {
            continue;
        }
        let Some((key, value)) = t.split_once('=') else {
            return false;
        };
        let key = key.trim();
        let bare = |k: &str| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
        };
        let quoted = key.len() >= 2 && (key.starts_with('"') || key.starts_with('\''));
        if !(bare(key) || quoted) {
            return false;
        }
        keys += 1;
        let value = value.trim();
        for q in ["\"\"\"", "\'\'\'"] {
            if value.matches(q).count() % 2 == 1 {
                in_string = Some(q);
            }
        }
        if in_string.is_none() {
            open_brackets = brackets(value).max(0);
        }
    }
    keys > 0
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v.split(" #").next().unwrap_or(v).trim();
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        v[1..v.len() - 1].to_owned()
    } else {
        v.to_owned()
    }
}

fn front_matter_pairs(inner: &str, toml: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in inner.lines() {
        if line.starts_with([' ', '\t', '#', '-']) || line.trim().is_empty() {
            continue;
        }
        if toml && line.trim_start().starts_with('[') {
            break;
        }
        let sep = if toml { '=' } else { ':' };
        let Some((k, v)) = line.split_once(sep) else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if k.is_empty() || v.is_empty() || v.starts_with(['[', '{', '|', '>', '&', '*']) {
            continue;
        }
        out.push((k.trim_matches('"').to_owned(), unquote(v)));
    }
    out
}

fn front_matter_preview(inner: &str, toml: bool) -> String {
    front_matter_pairs(inner, toml)
        .into_iter()
        .take(3)
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn front_matter_title(inner: &str, toml: bool) -> Option<String> {
    front_matter_pairs(inner, toml)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("title"))
        .map(|(_, v)| v)
}

// ---------------------------------------------------------------------------------------------
// Code labels

/// Display name for a fence language (SPEC §6). Unknown names are shown as written.
pub fn display_name(lang: &str) -> String {
    let l = lang.to_ascii_lowercase();
    let s = match l.as_str() {
        "ts" | "typescript" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "js" | "javascript" | "mjs" | "cjs" | "node" => "JavaScript",
        "jsx" => "JSX",
        "sh" | "bash" | "zsh" | "shell" | "console" | "shell-session" | "shellsession" | "ksh" => {
            "Shell"
        }
        "fish" => "Fish",
        "ps1" | "powershell" | "pwsh" | "ps" | "psm1" => "PowerShell",
        "bat" | "cmd" | "batch" => "Batch",
        "yml" | "yaml" => "YAML",
        "json" | "jsonc" | "json5" | "jsonl" => "JSON",
        "toml" => "TOML",
        "ini" | "cfg" | "conf" => "INI",
        "rs" | "rust" => "Rust",
        "py" | "python" | "python3" | "py3" => "Python",
        "go" | "golang" => "Go",
        "c" | "h" => "C",
        "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "cs" | "csharp" | "c#" => "C#",
        "fs" | "fsharp" | "f#" => "F#",
        "java" => "Java",
        "kt" | "kotlin" | "kts" => "Kotlin",
        "swift" => "Swift",
        "rb" | "ruby" => "Ruby",
        "php" => "PHP",
        "pl" | "perl" => "Perl",
        "lua" => "Lua",
        "r" => "R",
        "scala" => "Scala",
        "dart" => "Dart",
        "zig" => "Zig",
        "nix" => "Nix",
        "hs" | "haskell" => "Haskell",
        "ex" | "exs" | "elixir" => "Elixir",
        "erl" | "erlang" => "Erlang",
        "clj" | "clojure" => "Clojure",
        "ml" | "ocaml" => "OCaml",
        "jl" | "julia" => "Julia",
        "sql" | "psql" | "postgres" | "postgresql" | "mysql" | "sqlite" => "SQL",
        "html" | "htm" | "xhtml" => "HTML",
        "xml" | "svg" | "xsl" | "plist" => "XML",
        "css" => "CSS",
        "scss" => "SCSS",
        "sass" => "Sass",
        "less" => "Less",
        "md" | "markdown" | "mdx" => "Markdown",
        "diff" | "patch" => "Diff",
        "dockerfile" | "docker" | "containerfile" => "Dockerfile",
        "makefile" | "make" | "mk" => "Makefile",
        "cmake" => "CMake",
        "graphql" | "gql" => "GraphQL",
        "proto" | "protobuf" => "Protocol Buffers",
        "hcl" | "tf" | "terraform" => "HCL",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "groovy" | "gradle" => "Groovy",
        "objc" | "objective-c" | "objectivec" => "Objective-C",
        "asm" | "nasm" | "assembly" => "Assembly",
        "vim" | "viml" => "Vim script",
        "nginx" => "Nginx",
        "csv" => "CSV",
        "log" => "Log",
        "regex" | "regexp" => "Regex",
        "text" | "txt" | "plain" | "plaintext" => "Text",
        _ => return lang.to_owned(),
    };
    s.to_owned()
}

fn fallback_label(lang: &str) -> Option<(CodeKind, &'static str)> {
    Some(match lang.to_ascii_lowercase().as_str() {
        "mermaid" | "mmd" => (CodeKind::Diagram, "Mermaid diagram"),
        "plantuml" | "puml" | "uml" => (CodeKind::Diagram, "PlantUML diagram"),
        "dot" | "graphviz" | "gv" => (CodeKind::Diagram, "Graphviz diagram"),
        "d2" => (CodeKind::Diagram, "D2 diagram"),
        "math" | "latex" | "tex" | "katex" => (CodeKind::Math, "Math (TeX)"),
        _ => return None,
    })
}

const NOT_RENDERED: &str = "· not rendered";

// ---------------------------------------------------------------------------------------------
// Parser state

struct P<'s> {
    lines: &'s [&'s str],
    line_offset: u32,
    base_dir: Option<PathBuf>,
    texts: Vec<RichText>,
    runs: Vec<RunInfo>,
    slugger: Slugger,
    headings: Vec<Heading>,
    heading_meta: Vec<HeadingMeta>,
    footnote_numbers: HashMap<String, u32>,
    footnotes: Vec<Footnote>,
    /// (html id, run it appeared in) for inline `<a id>` anchors.
    html_anchors: Vec<(String, RunId)>,
    /// First reference of each footnote: (name, run).
    footrefs: Vec<(String, RunId)>,
    pending_marker: Option<String>,
    list_depth: u8,
    /// Container nesting (see [`MAX_BLOCK_DEPTH`]) and inline nesting.
    depth: u16,
    inline_depth: u16,
    table_ord: u32,
    cell: Option<(u32, u32, u32)>,
    /// Source line of the block being converted (for RunInfo::line).
    cur_line: u32,
    id_counts: HashMap<u64, u32>,
}

/// Open HTML containers that can span several Markdown blocks (`<details>`, `<div align>`).
#[derive(Default)]
struct Sink {
    stack: Vec<Frame>,
    out: Vec<Block>,
}

struct Frame {
    tag: String,
    kind: FrameKind,
    blocks: Vec<Block>,
    line: u32,
    end_line: u32,
}

enum FrameKind {
    Details {
        open: bool,
        summary: Option<RunId>,
    },
    Center,
    /// An element opened beyond [`MAX_BLOCK_DEPTH`]: its blocks go to the parent.
    Flat,
}

impl Sink {
    fn push(&mut self, b: Block) {
        if let Some(f) = self.stack.last_mut() {
            f.end_line = f.end_line.max(b.end_line);
            f.blocks.push(b);
        } else {
            self.out.push(b);
        }
    }

    fn open(&mut self, p: &mut P, tag: &str, kind: FrameKind, line: u32) {
        let kind = if p.depth >= MAX_BLOCK_DEPTH {
            FrameKind::Flat
        } else {
            p.depth += 1;
            kind
        };
        self.stack.push(Frame {
            tag: tag.to_owned(),
            kind,
            blocks: Vec::new(),
            line,
            end_line: line,
        });
    }

    fn top_details_summary(&mut self) -> Option<&mut Option<RunId>> {
        match self.stack.last_mut() {
            Some(Frame {
                kind: FrameKind::Details { summary, .. },
                ..
            }) => Some(summary),
            _ => None,
        }
    }

    fn has_open(&self, tag: &str) -> bool {
        self.stack.iter().any(|f| f.tag == tag)
    }

    fn close(&mut self, tag: &str, p: &mut P, end_line: u32) {
        if !self.has_open(tag) {
            return;
        }
        while let Some(f) = self.stack.pop() {
            let done = f.tag == tag;
            self.emit(f, p, end_line);
            if done {
                break;
            }
        }
    }

    fn emit(&mut self, f: Frame, p: &mut P, end_line: u32) {
        let end_line = end_line.max(f.end_line);
        if matches!(f.kind, FrameKind::Flat) {
            for b in f.blocks {
                self.push(b);
            }
            return;
        }
        p.depth = p.depth.saturating_sub(1);
        let kind = match f.kind {
            FrameKind::Flat => unreachable!(),
            FrameKind::Details { open, summary } => {
                let summary = summary.unwrap_or_else(|| p.new_run_plain("Details", RunKind::Text));
                BlockKind::Details {
                    summary,
                    open,
                    blocks: f.blocks,
                }
            }
            FrameKind::Center => BlockKind::Center(f.blocks),
        };
        let id = p.block_id(&f.tag, f.line, end_line);
        self.push(Block {
            kind,
            line: f.line,
            end_line,
            id,
        });
    }

    fn finish(mut self, p: &mut P) -> Vec<Block> {
        while let Some(f) = self.stack.pop() {
            let end = f.end_line;
            self.emit(f, p, end);
        }
        self.out
    }
}

/// Inline builder state for one run.
struct Inl {
    rt: RichText,
    flags: u16,
    link: Option<u32>,
    /// Inside a dangerous element (tag name, nesting depth): content is dropped.
    drop: Option<(String, u32)>,
    /// Open inline HTML elements: (tag, flags before, link before).
    frames: Vec<(String, u16, Option<u32>)>,
}

impl Inl {
    fn new() -> Self {
        Self {
            rt: RichText::default(),
            flags: 0,
            link: None,
            drop: None,
            frames: Vec::new(),
        }
    }

    fn push(&mut self, s: &str, flags: u16, link: Option<u32>, kind: SpanKind) {
        if s.is_empty() {
            return;
        }
        let start = self.rt.text.len();
        self.rt.text.push_str(s);
        let end = self.rt.text.len();
        if kind == SpanKind::Text
            && let Some(last) = self.rt.spans.last_mut()
            && last.kind == SpanKind::Text
            && last.flags == flags
            && last.link == link
            && last.range.end == start
        {
            last.range.end = end;
            return;
        }
        self.rt.spans.push(Span {
            range: start..end,
            flags,
            link,
            kind,
        });
    }

    fn text(&mut self, s: &str) {
        if self.drop.is_some() {
            return;
        }
        // Like a browser: collapse runs of spaces (outside code) and drop leading spaces of a
        // line, e.g. the gap left by a dropped `<style>` element. Our marker char is reserved.
        let code = self.flags & (flags::CODE | flags::KBD) != 0;
        let needs_work = s.contains(MARKER) || (!code && (s.contains("  ") || s.starts_with(' ')));
        if !needs_work {
            self.push(s, self.flags, self.link, SpanKind::Text);
            return;
        }
        let mut out = String::with_capacity(s.len());
        let mut prev = self.rt.text.chars().next_back();
        for c in s.chars() {
            if c == MARKER {
                continue;
            }
            if !code && c == ' ' && matches!(prev, None | Some(' ') | Some('\n')) {
                continue;
            }
            out.push(c);
            prev = Some(c);
        }
        self.push(&out, self.flags, self.link, SpanKind::Text);
    }

    fn pad(&mut self, em: f32) {
        self.push(MARKER_STR, self.flags, self.link, SpanKind::Pad(em));
    }

    fn object(&mut self, obj: InlineObject) {
        let idx = self.rt.objects.len() as u32;
        self.rt.objects.push(obj);
        let s = format!("{MARKER}{MARKER}");
        self.push(&s, self.flags, self.link, SpanKind::Object(idx));
    }

    fn add_link(&mut self, link: Link) -> u32 {
        self.rt.links.push(link);
        (self.rt.links.len() - 1) as u32
    }

    fn ends_with_newline_or_empty(&self) -> bool {
        self.rt.text.is_empty() || self.rt.text.ends_with('\n')
    }

    /// The image if this run is exactly one image (plus whitespace).
    fn sole_image(&self) -> Option<ImageRef> {
        if self.rt.objects.len() != 1 {
            return None;
        }
        let InlineObject::Image(img) = &self.rt.objects[0] else {
            return None;
        };
        let only_ws = self
            .rt
            .text
            .chars()
            .all(|c| c == MARKER || c.is_whitespace());
        only_ws.then(|| img.clone())
    }
}

impl P<'_> {
    fn line(&self, l: usize) -> u32 {
        l as u32 + self.line_offset
    }

    fn block_id(&mut self, kind: &str, line: u32, end_line: u32) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        kind.hash(&mut h);
        slice_lines(self.lines, line, end_line).hash(&mut h);
        let base = h.finish();
        let n = self.id_counts.entry(base).or_insert(0);
        *n += 1;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (base, *n).hash(&mut h);
        h.finish()
    }

    fn new_run_info(&mut self, kind: RunKind) -> RunId {
        let id = self.runs.len() as RunId;
        self.runs.push(RunInfo {
            kind,
            top: 0,
            list_depth: self.list_depth,
            list_marker: if matches!(kind, RunKind::Text | RunKind::Heading | RunKind::Code) {
                self.pending_marker.take()
            } else {
                None
            },
            cell: self.cell,
            line: self.cur_line,
        });
        id
    }

    fn finish_run(&mut self, inl: Inl, kind: RunKind) -> RunId {
        let id = self.new_run_info(kind);
        let mut rt = inl.rt;
        rt.run = id;
        self.texts.push(rt);
        id
    }

    fn new_run_plain(&mut self, text: &str, kind: RunKind) -> RunId {
        let mut inl = Inl::new();
        inl.push(text, 0, None, SpanKind::Text);
        self.finish_run(inl, kind)
    }

    // ---- links and images ----------------------------------------------------------------

    fn resolve_link(&self, url: &str) -> LinkDest {
        let url = url.trim();
        if let Some(frag) = url.strip_prefix('#') {
            return LinkDest::Anchor(percent_decode(frag));
        }
        // A link that would reach another machine (`file://host/…`, a UNC path) is handed to
        // the shell as an external URL, which blocks it without touching the file system.
        let blocked = || LinkDest::External(url.to_owned());
        if has_scheme(url) {
            if url
                .get(..5)
                .is_some_and(|s| s.eq_ignore_ascii_case("file:"))
            {
                let (uri, anchor) = split_anchor(url);
                return match paths::file_uri_local_path(uri).and_then(|p| self.resolve_local(&p)) {
                    Some(path) => LinkDest::File { path, anchor },
                    None => blocked(),
                };
            }
            return LinkDest::External(url.to_owned());
        }
        let (path, anchor) = split_anchor(url);
        let path = percent_decode(path.split('?').next().unwrap_or(path));
        if path.is_empty() {
            return LinkDest::Anchor(anchor.unwrap_or_default());
        }
        match self.resolve_local(&path) {
            Some(path) => LinkDest::File { path, anchor },
            None => blocked(),
        }
    }

    /// Resolve a document-supplied local path against the document's folder. `None` when reading
    /// it would make the OS contact another machine (SPEC §1.7): a UNC or device path, unless it
    /// is on the share the document itself was opened from.
    fn resolve_local(&self, path: &str) -> Option<PathBuf> {
        let base = self.base_dir.as_deref();
        let full = match base {
            Some(b) => b.join(path),
            None => PathBuf::from(path),
        };
        let remote = paths::is_remote_path(&full, base)
            || (paths::looks_like_unc(path) && !paths::on_share_of(&full, base));
        (!remote).then_some(full)
    }

    fn image_uri(&self, src: &str) -> Option<String> {
        let src = src.trim();
        if src.is_empty() || !html::is_safe_url(src) {
            return None;
        }
        let lower = src.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return Some(src.to_owned());
        }
        let path = if lower.starts_with("file:") {
            paths::file_uri_local_path(src.split(['?', '#']).next().unwrap_or(src))?
        } else if has_scheme(src) {
            return None;
        } else {
            percent_decode(src.split(['?', '#']).next().unwrap_or(src))
        };
        let full = self.resolve_local(&path)?;
        // Pasted text has no folder: a relative path would be read relative to the working
        // directory (and on Windows egui would take its first segment for a host name).
        if full.is_relative() {
            return None;
        }
        Some(paths::local_file_uri(&full))
    }

    fn make_link(&self, url: &str, bare: bool) -> Option<Link> {
        if !html::is_safe_url(url) {
            return None;
        }
        let dest = self.resolve_link(url);
        let external = matches!(&dest, LinkDest::External(u) if {
            let l = u.to_ascii_lowercase();
            l.starts_with("http:") || l.starts_with("https:") || l.starts_with("mailto:")
        });
        Some(Link {
            href: url.to_owned(),
            dest,
            external_icon: external && !bare,
        })
    }

    // ---- inlines -------------------------------------------------------------------------

    fn inlines<'a>(&mut self, node: &'a AstNode<'a>, b: &mut Inl) {
        if self.inline_depth >= MAX_INLINE_DEPTH {
            b.text(&plain_text(node));
            return;
        }
        self.inline_depth += 1;
        for child in node.children() {
            self.inline(child, b);
        }
        self.inline_depth -= 1;
    }

    fn with_flag<'a>(&mut self, node: &'a AstNode<'a>, b: &mut Inl, flag: u16) {
        let prev = b.flags;
        b.flags |= flag;
        self.inlines(node, b);
        b.flags = prev;
    }

    fn inline<'a>(&mut self, node: &'a AstNode<'a>, b: &mut Inl) {
        let value = node.data().value.clone();
        match value {
            NodeValue::Text(t) => b.text(&t),
            NodeValue::SoftBreak => b.text(" "),
            NodeValue::LineBreak => b.text("\n"),
            NodeValue::Code(c) => {
                if b.drop.is_none() {
                    let prev = b.flags;
                    b.flags |= flags::CODE;
                    b.pad(CODE_PAD_EM);
                    b.text(&c.literal);
                    b.pad(CODE_PAD_EM);
                    b.flags = prev;
                }
            }
            NodeValue::Emph => self.with_flag(node, b, flags::EM),
            NodeValue::Strong => self.with_flag(node, b, flags::STRONG),
            NodeValue::Strikethrough => self.with_flag(node, b, flags::STRIKE),
            NodeValue::Superscript => self.with_flag(node, b, flags::SUP),
            NodeValue::Subscript => self.with_flag(node, b, flags::SUB),
            NodeValue::Highlight => self.with_flag(node, b, flags::MARK),
            NodeValue::Underline | NodeValue::Insert => self.with_flag(node, b, flags::UNDERLINE),
            NodeValue::Link(l) => {
                let text = plain_text(node);
                let url = l.url.as_str();
                let bare = text == url
                    || url.strip_prefix("mailto:") == Some(text.as_str())
                    || url.strip_prefix("http://") == Some(text.as_str())
                    || url.strip_prefix("https://") == Some(text.as_str());
                // Image links (badges, logos) get no ↗.
                let wraps_image = node
                    .descendants()
                    .any(|d| matches!(d.data().value, NodeValue::Image(_)));
                match self.make_link(url, bare || wraps_image) {
                    Some(link) if b.drop.is_none() => {
                        let icon = link.external_icon;
                        let idx = b.add_link(link);
                        let (pf, pl) = (b.flags, b.link);
                        b.flags |= flags::LINK;
                        b.link = Some(idx);
                        self.inlines(node, b);
                        if icon {
                            b.object(InlineObject::ExternalIcon);
                        }
                        b.flags = pf;
                        b.link = pl;
                    }
                    _ => self.inlines(node, b),
                }
            }
            NodeValue::Image(l) => {
                if b.drop.is_none() {
                    let alt = plain_text(node);
                    let img = ImageRef {
                        uri: self.image_uri(&l.url),
                        src: l.url.clone(),
                        alt,
                        width: None,
                        height: None,
                        link: b.link,
                    };
                    b.object(InlineObject::Image(img));
                }
            }
            NodeValue::FootnoteReference(r) => {
                if b.drop.is_none() {
                    let number = r.ix.max(1);
                    self.footnote_numbers
                        .entry(r.name.clone())
                        .or_insert(number);
                    if r.ref_num <= 1 {
                        self.footrefs
                            .push((r.name.clone(), self.runs.len() as RunId));
                    }
                    let idx = b.add_link(Link {
                        href: format!("#fn-{}", r.name),
                        dest: LinkDest::Anchor(format!("fn-{}", r.name)),
                        external_icon: false,
                    });
                    let f = b.flags | flags::FOOTREF | flags::LINK;
                    b.push(&number.to_string(), f, Some(idx), SpanKind::Text);
                }
            }
            NodeValue::HtmlInline(raw) => self.html_inline(&raw, b),
            NodeValue::EscapedTag(s) => b.text(s),
            NodeValue::Raw(s) => b.text(&s),
            NodeValue::Math(m) => b.text(&m.literal),
            _ => self.inlines(node, b),
        }
    }

    /// Handle one start/end tag (or text) in inline context. Shared by inline HTML in
    /// Markdown paragraphs and by HTML blocks.
    fn inline_token(&mut self, tok: &Token, b: &mut Inl) {
        if let Some((name, depth)) = &mut b.drop {
            match tok {
                Token::Start {
                    name: n,
                    self_closing: false,
                    ..
                } if n == name && !html::is_void(n) => *depth += 1,
                Token::End { name: n } if n == name => {
                    *depth -= 1;
                    if *depth == 0 {
                        b.drop = None;
                    }
                }
                _ => {}
            }
            return;
        }
        match tok {
            Token::Text(t) => b.text(t),
            Token::Ignored => {}
            Token::Start {
                name,
                attrs,
                self_closing,
            } => {
                let name = name.as_str();
                if html::is_dangerous(name) {
                    if !self_closing && !html::is_void(name) {
                        b.drop = Some((name.to_owned(), 1));
                    }
                    return;
                }
                let (pf, pl) = (b.flags, b.link);
                match name {
                    "br" => b.text("\n"),
                    "img" => {
                        let src = Token::attr(attrs, "src").unwrap_or_default();
                        let num = |k| {
                            Token::attr(attrs, k)
                                .and_then(|v| v.trim_end_matches("px").trim().parse::<f32>().ok())
                        };
                        let img = ImageRef {
                            uri: self.image_uri(src),
                            src: src.to_owned(),
                            alt: Token::attr(attrs, "alt").unwrap_or_default().to_owned(),
                            width: num("width"),
                            height: num("height"),
                            link: b.link,
                        };
                        b.object(InlineObject::Image(img));
                    }
                    "a" => {
                        if let Some(id) =
                            Token::attr(attrs, "id").or_else(|| Token::attr(attrs, "name"))
                            && !id.is_empty()
                        {
                            self.slugger.reserve(id);
                            self.html_anchors
                                .push((id.to_owned(), self.runs.len() as RunId));
                        }
                        if let Some(href) = Token::attr(attrs, "href")
                            && let Some(link) = self.make_link(href, false)
                        {
                            let idx = b.add_link(link);
                            b.flags |= flags::LINK;
                            b.link = Some(idx);
                        }
                    }
                    "kbd" => {
                        b.flags |= flags::KBD;
                        b.pad(KBD_PAD_EM);
                    }
                    "b" | "strong" => b.flags |= flags::STRONG,
                    "i" | "em" | "cite" | "dfn" | "var" => b.flags |= flags::EM,
                    "code" | "tt" | "samp" => {
                        b.flags |= flags::CODE;
                        b.pad(CODE_PAD_EM);
                    }
                    "s" | "del" | "strike" => b.flags |= flags::STRIKE,
                    "ins" | "u" => b.flags |= flags::UNDERLINE,
                    "mark" => b.flags |= flags::MARK,
                    "sup" => b.flags |= flags::SUP,
                    "sub" => b.flags |= flags::SUB,
                    _ => {}
                }
                if !self_closing && !html::is_void(name) {
                    b.frames.push((name.to_owned(), pf, pl));
                }
            }
            Token::End { name } => {
                if let Some(pos) = b.frames.iter().rposition(|(n, _, _)| n == name) {
                    // Close inner unclosed elements too.
                    while b.frames.len() > pos {
                        let (n, pf, pl) = b.frames.pop().unwrap_or_default();
                        match n.as_str() {
                            "kbd" => b.pad(KBD_PAD_EM),
                            "code" | "tt" | "samp" => b.pad(CODE_PAD_EM),
                            _ => {}
                        }
                        b.flags = pf;
                        b.link = pl;
                    }
                }
            }
        }
    }

    fn html_inline(&mut self, raw: &str, b: &mut Inl) {
        for tok in html::tokenize(raw) {
            // Text inside inline raw HTML is HTML text: collapse whitespace.
            let tok = match tok {
                Token::Text(t) => Token::Text(html::collapse_ws(&t)),
                t => t,
            };
            self.inline_token(&tok, b);
        }
    }

    // ---- blocks --------------------------------------------------------------------------

    fn blocks<'a>(&mut self, parent: &'a AstNode<'a>) -> Vec<Block> {
        if self.depth >= MAX_BLOCK_DEPTH {
            return self.flat_blocks(parent);
        }
        self.depth += 1;
        let mut sink = Sink::default();
        for child in parent.children() {
            self.block(child, &mut sink);
        }
        let blocks = sink.finish(self);
        self.depth -= 1;
        blocks
    }

    /// A container's content beyond [`MAX_BLOCK_DEPTH`]: its text as one paragraph, one line
    /// per block, gathered without recursion.
    fn flat_blocks<'a>(&mut self, parent: &'a AstNode<'a>) -> Vec<Block> {
        let mut text = String::new();
        for edge in parent.traverse() {
            match edge {
                NodeEdge::Start(n) => match &n.data().value {
                    NodeValue::Text(t) => text.push_str(t),
                    NodeValue::Code(c) => text.push_str(&c.literal),
                    NodeValue::CodeBlock(c) => text.push_str(&c.literal),
                    NodeValue::SoftBreak => text.push(' '),
                    NodeValue::LineBreak => text.push('\n'),
                    NodeValue::HtmlBlock(h) => html_text(&h.literal, &mut text),
                    _ => {}
                },
                NodeEdge::End(n) => {
                    if n.data().value.block() && !text.is_empty() && !text.ends_with('\n') {
                        text.push('\n');
                    }
                }
            }
        }
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let sp = parent.data().sourcepos;
        let (line, end_line) = (self.line(sp.start.line), self.line(sp.end.line));
        let mut inl = Inl::new();
        inl.text(text);
        let run = self.finish_run(inl, RunKind::Text);
        let id = self.block_id("flat", line, end_line);
        vec![Block {
            kind: BlockKind::Paragraph {
                run,
                align: HAlign::Left,
            },
            line,
            end_line: end_line.max(line),
            id,
        }]
    }

    fn block<'a>(&mut self, node: &'a AstNode<'a>, sink: &mut Sink) {
        let (value, sp) = {
            let d = node.data();
            (d.value.clone(), d.sourcepos)
        };
        let line = self.line(sp.start.line);
        self.cur_line = line;
        let end_line = self.line(sp.end.line).max(line);
        let kind = match value {
            NodeValue::Paragraph => {
                let src = slice_lines(self.lines, line, end_line);
                let t = src.trim();
                if t.len() > 4 && t.starts_with("$$") && t.ends_with("$$") {
                    let tex = t[2..t.len() - 2].trim_matches('\n').trim().to_owned();
                    self.code_block(
                        Some("math".into()),
                        &tex,
                        CodeKind::Math,
                        Some("Math (TeX)"),
                        Some(NOT_RENDERED),
                    )
                } else {
                    let mut inl = Inl::new();
                    self.inlines(node, &mut inl);
                    if let Some(img) = inl.sole_image() {
                        let links = inl.rt.links.clone();
                        BlockKind::Image {
                            image: img,
                            links,
                            align: HAlign::Left,
                        }
                    } else if inl.rt.is_blank() && inl.rt.objects.is_empty() {
                        return;
                    } else {
                        trim_trailing_newlines(&mut inl.rt);
                        let run = self.finish_run(inl, RunKind::Text);
                        BlockKind::Paragraph {
                            run,
                            align: HAlign::Left,
                        }
                    }
                }
            }
            NodeValue::Heading(h) => {
                let mut inl = Inl::new();
                self.inlines(node, &mut inl);
                trim_trailing_newlines(&mut inl.rt);
                self.heading(h.level, inl, line)
            }
            NodeValue::CodeBlock(cb) => {
                let literal = cb
                    .literal
                    .strip_suffix('\n')
                    .unwrap_or(&cb.literal)
                    .to_owned();
                let info = cb
                    .info
                    .trim()
                    .trim_start_matches('{')
                    .trim_start_matches('.');
                let lang = info
                    .split(|c: char| c.is_whitespace() || c == ',' || c == '}')
                    .next()
                    .unwrap_or("");
                if !cb.fenced || lang.is_empty() {
                    self.code_block(None, &literal, CodeKind::Normal, None, None)
                } else if let Some((kind, label)) = fallback_label(lang) {
                    self.code_block(
                        Some(lang.to_owned()),
                        &literal,
                        kind,
                        Some(label),
                        Some(NOT_RENDERED),
                    )
                } else {
                    let label = display_name(lang);
                    self.code_block(
                        Some(lang.to_owned()),
                        &literal,
                        CodeKind::Normal,
                        Some(&label),
                        None,
                    )
                }
            }
            NodeValue::List(l) => {
                let ordered = l.list_type == ListType::Ordered;
                let mut items = Vec::new();
                let mut n = l.start as u64;
                for item in node.children() {
                    let task = match &item.data().value {
                        NodeValue::TaskItem(t) => Some(t.symbol.is_some()),
                        NodeValue::Item(_) => None,
                        _ => continue,
                    };
                    let marker = if ordered {
                        format!("{n}. ")
                    } else {
                        "- ".to_owned()
                    };
                    let marker = match task {
                        Some(true) => format!("{marker}[x] "),
                        Some(false) => format!("{marker}[ ] "),
                        None => marker,
                    };
                    n += 1;
                    self.pending_marker = Some(marker);
                    self.list_depth = self.list_depth.saturating_add(1);
                    let blocks = self.blocks(item);
                    self.list_depth = self.list_depth.saturating_sub(1);
                    self.pending_marker = None;
                    items.push(ListItem { task, blocks });
                }
                BlockKind::List(List {
                    ordered,
                    start: l.start as u64,
                    tight: l.tight,
                    items,
                })
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                BlockKind::Quote(self.blocks(node))
            }
            NodeValue::Alert(a) => {
                let kind = match a.alert_type {
                    AlertType::Note => AlertKind::Note,
                    AlertType::Tip => AlertKind::Tip,
                    AlertType::Important => AlertKind::Important,
                    AlertType::Warning => AlertKind::Warning,
                    AlertType::Caution => AlertKind::Caution,
                };
                BlockKind::Alert {
                    kind,
                    blocks: self.blocks(node),
                }
            }
            NodeValue::Table(t) => self.table(node, &t.alignments),
            NodeValue::ThematicBreak => BlockKind::Rule,
            NodeValue::HtmlBlock(h) => {
                self.html_block(&h.literal, line, end_line, sink);
                return;
            }
            NodeValue::FootnoteDefinition(fd) => {
                let number = self.footnote_numbers.get(&fd.name).copied().unwrap_or(0);
                if number == 0 {
                    return; // unreferenced
                }
                let mut blocks = self.blocks(node);
                // Back-link "↩" at the end of the note's last paragraph.
                let back = Link {
                    href: format!("#fnref-{}", fd.name),
                    dest: LinkDest::Anchor(format!("fnref-{}", fd.name)),
                    external_icon: false,
                };
                if let Some(Block {
                    kind: BlockKind::Paragraph { run, .. },
                    ..
                }) = blocks.last_mut()
                {
                    let rt = &mut self.texts[*run as usize];
                    let idx = rt.links.len() as u32;
                    rt.links.push(back);
                    let start = rt.text.len();
                    rt.text.push_str(" ↩");
                    rt.spans.push(Span {
                        range: start..start + 1,
                        flags: flags::FOOTBACK,
                        link: None,
                        kind: SpanKind::Text,
                    });
                    rt.spans.push(Span {
                        range: start + 1..rt.text.len(),
                        flags: flags::LINK | flags::FOOTBACK,
                        link: Some(idx),
                        kind: SpanKind::Text,
                    });
                } else {
                    let mut inl = Inl::new();
                    let idx = inl.add_link(back);
                    inl.push(
                        "↩",
                        flags::LINK | flags::FOOTBACK,
                        Some(idx),
                        SpanKind::Text,
                    );
                    let run = self.finish_run(inl, RunKind::Text);
                    let id = self.block_id("fnback", line, end_line);
                    blocks.push(Block {
                        kind: BlockKind::Paragraph {
                            run,
                            align: HAlign::Left,
                        },
                        line,
                        end_line,
                        id,
                    });
                }
                self.footnotes.push(Footnote {
                    name: fd.name.clone(),
                    number,
                    blocks,
                });
                return;
            }
            _ => return,
        };
        let id = self.block_id(kind_name(&kind), line, end_line);
        sink.push(Block {
            kind,
            line,
            end_line,
            id,
        });
    }

    fn heading(&mut self, level: u8, inl: Inl, line: u32) -> BlockKind {
        let plain = inl.rt.plain();
        let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
        let anchor = self.slugger.slug(&plain);
        let index = self.headings.len();
        let run = self.finish_run(inl, RunKind::Heading);
        self.headings.push(Heading {
            level,
            text: plain,
            anchor,
        });

        self.heading_meta.push(HeadingMeta {
            run,
            top: 0,
            line,
            section_end: line,
        });
        BlockKind::Heading {
            level,
            run,
            index,
            align: HAlign::Left,
        }
    }

    fn code_block(
        &mut self,
        lang: Option<String>,
        text: &str,
        kind: CodeKind,
        label: Option<&str>,
        note: Option<&str>,
    ) -> BlockKind {
        let run = self.new_run_plain(text, RunKind::Code);
        BlockKind::Code(CodeBlock {
            lang,
            label: label.map(str::to_owned),
            label_note: note.map(str::to_owned),
            kind,
            run,
        })
    }

    fn table<'a>(&mut self, node: &'a AstNode<'a>, aligns: &[TableAlignment]) -> BlockKind {
        let table_ord = self.table_ord;
        self.table_ord += 1;
        let mut header = Vec::new();
        let mut rows = Vec::new();
        // GFM: rows are padded/truncated to the header's column count.
        let ncols = aligns.len().max(1);
        for (row_i, row) in node.children().enumerate() {
            let row_i = row_i as u32;
            let is_header = matches!(row.data().value, NodeValue::TableRow(true));
            let mut cells = Vec::new();
            for (col, cell) in row.children().enumerate().take(ncols) {
                self.cell = Some((table_ord, row_i, col as u32));
                let mut inl = Inl::new();
                self.inlines(cell, &mut inl);
                let run = self.finish_run(inl, RunKind::Cell);
                cells.push(run);
            }
            while cells.len() < ncols {
                self.cell = Some((table_ord, row_i, cells.len() as u32));
                cells.push(self.new_run_plain("", RunKind::Cell));
            }
            self.cell = None;
            if is_header {
                header = cells;
            } else {
                rows.push(cells);
            }
        }
        let explicit: Vec<bool> = aligns
            .iter()
            .map(|a| !matches!(a, TableAlignment::None))
            .collect();
        let aligns: Vec<HAlign> = aligns
            .iter()
            .map(|a| match a {
                TableAlignment::Center => HAlign::Center,
                TableAlignment::Right => HAlign::Right,
                _ => HAlign::Left,
            })
            .collect();
        let numeric = (0..aligns.len())
            .map(|c| {
                if explicit[c] {
                    return false;
                }
                let cells: Vec<String> = rows
                    .iter()
                    .filter_map(|r| r.get(c))
                    .map(|&run| self.texts[run as usize].plain().trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                !cells.is_empty()
                    && cells.iter().filter(|s| is_numeric(s)).count() * 5 >= cells.len() * 4
            })
            .collect();
        BlockKind::Table(Table {
            aligns,
            numeric,
            header,
            rows,
        })
    }

    // ---- HTML blocks ---------------------------------------------------------------------

    fn html_block(&mut self, raw: &str, line: u32, end_line: u32, sink: &mut Sink) {
        self.cur_line = line;
        let toks = tokenize_with_raw(raw);
        let mut inl: Option<(Inl, Option<u8>)> = None; // (builder, heading level)
        let mut summary: Option<Inl> = None;
        let mut pre: Option<String> = None;
        let mut skip_until: Option<(String, u32)> = None;
        let mut i = 0;

        // Emit the current inline paragraph/heading, if any.
        fn flush(
            p: &mut P,
            inl: &mut Option<(Inl, Option<u8>)>,
            sink: &mut Sink,
            line: u32,
            end_line: u32,
        ) {
            let Some((mut b, level)) = inl.take() else {
                return;
            };
            if b.rt.is_blank() && b.rt.objects.is_empty() {
                return;
            }
            trim_trailing_newlines(&mut b.rt);
            let kind = if let Some(level) = level {
                p.heading(level, b, line)
            } else if let Some(img) = b.sole_image() {
                BlockKind::Image {
                    image: img,
                    links: b.rt.links.clone(),
                    align: HAlign::Left,
                }
            } else {
                trim_leading_ws(&mut b.rt);
                let run = p.finish_run(b, RunKind::Text);
                BlockKind::Paragraph {
                    run,
                    align: HAlign::Left,
                }
            };
            let id = p.block_id(kind_name(&kind), line, end_line);
            sink.push(Block {
                kind,
                line,
                end_line,
                id,
            });
        }

        while i < toks.len() {
            let (tok, raw_tok) = &toks[i];
            i += 1;
            if let Some((name, depth)) = &mut skip_until {
                match tok {
                    Token::Start {
                        name: n,
                        self_closing: false,
                        ..
                    } if n == name => *depth += 1,
                    Token::End { name: n } if n == name => {
                        *depth -= 1;
                        if *depth == 0 {
                            skip_until = None;
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if let Some(buf) = &mut pre {
                match tok {
                    Token::End { name } if name == "pre" => {
                        let text = buf.trim_matches('\n').to_owned();
                        pre = None;
                        let kind = self.code_block(None, &text, CodeKind::Normal, None, None);
                        let id = self.block_id("pre", line, end_line);
                        sink.push(Block {
                            kind,
                            line,
                            end_line,
                            id,
                        });
                    }
                    Token::Text(t) => buf.push_str(t),
                    Token::Start { name, .. } if name == "br" => buf.push('\n'),
                    _ => {}
                }
                continue;
            }
            // Inside <summary>: inline content goes to the summary builder.
            if let Some(sb) = &mut summary {
                match tok {
                    Token::End { name } if name == "summary" => {
                        let mut sb = summary.take().unwrap_or_else(Inl::new);
                        trim_leading_ws(&mut sb.rt);
                        let text = sb.rt.text.trim_end().len();
                        sb.rt.text.truncate(text);
                        clamp_spans(&mut sb.rt);
                        let run = self.finish_run(sb, RunKind::Text);
                        if let Some(slot) = sink.top_details_summary() {
                            *slot = Some(run);
                        }
                    }
                    Token::Text(t) => sb.text(&html::collapse_ws(t)),
                    _ => self.inline_token(tok, sb),
                }
                continue;
            }
            match tok {
                Token::Ignored => {}
                Token::Text(t) => {
                    let t = html::collapse_ws(t);
                    if inl.is_none() && t.trim().is_empty() {
                        continue;
                    }
                    let (b, _) = inl.get_or_insert_with(|| (Inl::new(), None));
                    if b.ends_with_newline_or_empty() {
                        b.text(t.trim_start());
                    } else {
                        b.text(&t);
                    }
                }
                Token::Start {
                    name,
                    attrs,
                    self_closing,
                } => {
                    let center = Token::attr(attrs, "align")
                        .is_some_and(|a| a.eq_ignore_ascii_case("center"));
                    match name.as_str() {
                        n if html::is_dangerous(n) => {
                            if !self_closing && !html::is_void(n) {
                                skip_until = Some((n.to_owned(), 1));
                            }
                        }
                        "details" => {
                            flush(self, &mut inl, sink, line, end_line);
                            let open = attrs.iter().any(|(k, _)| k == "open");
                            sink.open(
                                self,
                                "details",
                                FrameKind::Details {
                                    open,
                                    summary: None,
                                },
                                line,
                            );
                        }
                        "summary" => {
                            flush(self, &mut inl, sink, line, end_line);
                            summary = Some(Inl::new());
                        }
                        "table" => {
                            flush(self, &mut inl, sink, line, end_line);
                            // Show the raw table source as a labelled, unrendered block.
                            let mut depth = 1;
                            let mut src = raw_tok.clone();
                            while i < toks.len() && depth > 0 {
                                let (t, r) = &toks[i];
                                match t {
                                    Token::Start {
                                        name,
                                        self_closing: false,
                                        ..
                                    } if name == "table" => depth += 1,
                                    Token::End { name } if name == "table" => depth -= 1,
                                    _ => {}
                                }
                                src.push_str(r);
                                i += 1;
                            }
                            let kind = self.code_block(
                                Some("html".into()),
                                src.trim(),
                                CodeKind::Html,
                                Some("HTML"),
                                Some(NOT_RENDERED),
                            );
                            let id = self.block_id("htmltable", line, end_line);
                            sink.push(Block {
                                kind,
                                line,
                                end_line,
                                id,
                            });
                        }
                        "pre" => {
                            flush(self, &mut inl, sink, line, end_line);
                            pre = Some(String::new());
                        }
                        "hr" => {
                            flush(self, &mut inl, sink, line, end_line);
                            let id = self.block_id("hr", line, end_line);
                            sink.push(Block {
                                kind: BlockKind::Rule,
                                line,
                                end_line,
                                id,
                            });
                        }
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                            flush(self, &mut inl, sink, line, end_line);
                            if center {
                                sink.open(self, name, FrameKind::Center, line);
                            }
                            let level = name[1..].parse().unwrap_or(1);
                            inl = Some((Inl::new(), Some(level)));
                        }
                        "p" | "div" | "center" | "section" | "article" | "header" | "footer"
                        | "main" | "nav" | "aside" | "figure" | "figcaption" | "blockquote"
                        | "ul" | "ol" | "li" | "dl" | "dt" | "dd" | "address" | "tr"
                        | "caption" => {
                            flush(self, &mut inl, sink, line, end_line);
                            if (center || name == "center") && !self_closing {
                                sink.open(self, name, FrameKind::Center, line);
                            }
                            if let Some(id) = Token::attr(attrs, "id").filter(|s| !s.is_empty()) {
                                self.anchor_block(id, line, end_line, sink);
                            }
                        }
                        "a" if Token::attr(attrs, "href").is_none() => {
                            // Pure anchor (<a name="x"></a>): a positioned, invisible target.
                            if let Some(id) =
                                Token::attr(attrs, "id").or_else(|| Token::attr(attrs, "name"))
                                && !id.is_empty()
                            {
                                if let Some((b, _)) = inl.as_mut() {
                                    self.inline_token(tok, b);
                                } else {
                                    self.anchor_block(id, line, end_line, sink);
                                    if !self_closing {
                                        let (b, _) = inl.get_or_insert_with(|| (Inl::new(), None));
                                        b.frames.push(("a".into(), 0, None));
                                    }
                                }
                            }
                        }
                        _ => {
                            let (b, _) = inl.get_or_insert_with(|| (Inl::new(), None));
                            self.inline_token(tok, b);
                        }
                    }
                }
                Token::End { name } => match name.as_str() {
                    "details" => {
                        flush(self, &mut inl, sink, line, end_line);
                        sink.close("details", self, end_line);
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        flush(self, &mut inl, sink, line, end_line);
                        sink.close(name, self, end_line);
                    }
                    "p" | "div" | "center" | "section" | "article" | "header" | "footer"
                    | "main" | "nav" | "aside" | "figure" | "figcaption" | "blockquote" | "ul"
                    | "ol" | "li" | "dl" | "dt" | "dd" | "address" | "tr" | "caption" => {
                        flush(self, &mut inl, sink, line, end_line);
                        sink.close(name, self, end_line);
                    }
                    _ => {
                        if let Some((b, _)) = inl.as_mut() {
                            self.inline_token(tok, b);
                        }
                    }
                },
            }
        }
        if let Some(sb) = summary.take() {
            let run = self.finish_run(sb, RunKind::Text);
            if let Some(slot) = sink.top_details_summary() {
                *slot = Some(run);
            }
        }
        flush(self, &mut inl, sink, line, end_line);
    }

    fn anchor_block(&mut self, id: &str, line: u32, end_line: u32, sink: &mut Sink) {
        self.slugger.reserve(id);
        let bid = self.block_id("anchor", line, end_line);
        sink.push(Block {
            kind: BlockKind::Anchor(id.to_owned()),
            line,
            end_line,
            id: bid,
        });
    }
}

fn kind_name(k: &BlockKind) -> &'static str {
    match k {
        BlockKind::Heading { .. } => "h",
        BlockKind::Paragraph { .. } => "p",
        BlockKind::Image { .. } => "img",
        BlockKind::Code(_) => "code",
        BlockKind::List(_) => "list",
        BlockKind::Quote(_) => "quote",
        BlockKind::Alert { .. } => "alert",
        BlockKind::Table(_) => "table",
        BlockKind::Rule => "hr",
        BlockKind::Details { .. } => "details",
        BlockKind::FrontMatter(_) => "fm",
        BlockKind::Footnotes(_) => "fn",
        BlockKind::Center(_) => "center",
        BlockKind::Anchor(_) => "anchor",
    }
}

/// Tokenize keeping each token's raw source (for `<table>` fallbacks).
fn tokenize_with_raw(raw: &str) -> Vec<(Token, String)> {
    html::tokenize_spans(raw)
        .into_iter()
        .map(|(t, r)| (t, raw[r].to_owned()))
        .collect()
}

fn trim_trailing_newlines(rt: &mut RichText) {
    let keep = rt.text.trim_end_matches(['\n', ' ']).len();
    if keep < rt.text.len() {
        rt.text.truncate(keep);
        clamp_spans(rt);
    }
}

fn trim_leading_ws(rt: &mut RichText) {
    let cut = rt.text.len() - rt.text.trim_start_matches([' ', '\n']).len();
    if cut == 0 {
        return;
    }
    rt.text.drain(..cut);
    for s in &mut rt.spans {
        s.range.start = s.range.start.saturating_sub(cut);
        s.range.end = s.range.end.saturating_sub(cut);
    }
    rt.spans.retain(|s| s.range.end > s.range.start);
}

fn clamp_spans(rt: &mut RichText) {
    let len = rt.text.len();
    for s in &mut rt.spans {
        s.range.end = s.range.end.min(len);
        s.range.start = s.range.start.min(len);
    }
    rt.spans.retain(|s| s.range.end > s.range.start);
}

/// The text of an HTML fragment (tags dropped, dangerous elements' content skipped).
fn html_text(raw: &str, out: &mut String) {
    let mut skip: Option<(String, u32)> = None;
    for tok in html::tokenize(raw) {
        match (&mut skip, tok) {
            (Some((name, depth)), Token::Start { name: n, .. }) if *name == n => *depth += 1,
            (Some((name, depth)), Token::End { name: n }) if *name == n => {
                *depth -= 1;
                if *depth == 0 {
                    skip = None;
                }
            }
            (Some(_), _) => {}
            (
                None,
                Token::Start {
                    name, self_closing, ..
                },
            ) if html::is_dangerous(&name) && !self_closing && !html::is_void(&name) => {
                skip = Some((name, 1));
            }
            (None, Token::Text(t)) => out.push_str(&t),
            _ => {}
        }
    }
}

fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut s = String::new();
    for d in node.descendants() {
        match &d.data().value {
            NodeValue::Text(t) => s.push_str(t),
            NodeValue::Code(c) => s.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => s.push(' '),
            _ => {}
        }
    }
    s
}

fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
    // A single letter is a Windows drive (`C:\x.md`), not a scheme.
    scheme.len() > 1
        && scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn split_anchor(url: &str) -> (&str, Option<String>) {
    match url.split_once('#') {
        Some((p, a)) => (p, Some(percent_decode(a))),
        None => (url, None),
    }
}

pub fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// "Numeric" table cell: digits with separators, signs, currency, percent and short units.
pub fn is_numeric(s: &str) -> bool {
    let s = s.trim();
    let s = s.trim_start_matches(['+', '-', '−', '~', '≈', '$', '€', '£', '¥', '±']);
    let mut t = s.trim_end_matches(['%', '×', 'x', '*']).trim_end();
    for unit in [
        "ms", "µs", "us", "ns", "min", "s", "h", "d", "GB", "MB", "KB", "kB", "TB", "B", "k", "M",
        "K", "px", "pt",
    ] {
        if let Some(rest) = t.strip_suffix(unit)
            && rest.ends_with(|c: char| c.is_ascii_digit() || c == ' ' || c == '\u{A0}')
        {
            t = rest.trim_end();
            break;
        }
    }
    !t.is_empty()
        && t.chars().any(|c| c.is_ascii_digit())
        && t.chars().all(|c| {
            c.is_ascii_digit() || matches!(c, ',' | '.' | ' ' | '\u{A0}' | '\u{202F}' | '_' | '\'')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(p: &Parsed) -> Vec<&'static str> {
        p.blocks.iter().map(|b| kind_name(&b.kind)).collect()
    }

    fn text(p: &Parsed, run: RunId) -> &str {
        &p.texts[run as usize].text
    }

    #[test]
    fn alerts() {
        let p = parse(
            "> [!NOTE]\n> Note body\n\n> [!CAUTION]\n> Careful\n\n> [!FOO]\n> plain quote\n",
            None,
        );
        assert_eq!(kinds(&p), vec!["alert", "alert", "quote"]);
        match &p.blocks[1].kind {
            BlockKind::Alert { kind, blocks } => {
                assert_eq!(*kind, AlertKind::Caution);
                assert!(matches!(blocks[0].kind, BlockKind::Paragraph { .. }));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn task_lists_and_nesting() {
        let src =
            "- [x] done\n- [ ] todo\n  - [x] child\n    - grandchild\n      - great\n- plain\n";
        let p = parse(src, None);
        let BlockKind::List(l) = &p.blocks[0].kind else {
            panic!()
        };
        assert_eq!(l.items.len(), 3);
        assert_eq!(l.items[0].task, Some(true));
        assert_eq!(l.items[1].task, Some(false));
        assert_eq!(l.items[2].task, None);
        let BlockKind::List(child) = &l.items[1].blocks[1].kind else {
            panic!("nested list")
        };
        assert_eq!(child.items[0].task, Some(true));
        let BlockKind::List(gc) = &child.items[0].blocks[1].kind else {
            panic!()
        };
        let BlockKind::List(ggc) = &gc.items[0].blocks[1].kind else {
            panic!()
        };
        assert_eq!(ggc.items.len(), 1);
        // Copy markers and depth are recorded for the first run of each item.
        let BlockKind::Paragraph { run, .. } = &l.items[0].blocks[0].kind else {
            panic!()
        };
        assert_eq!(p.runs[*run as usize].list_marker.as_deref(), Some("- [x] "));
        assert_eq!(p.runs[*run as usize].list_depth, 1);
    }

    #[test]
    fn ordered_start() {
        let p = parse("7. a\n8. b\n", None);
        let BlockKind::List(l) = &p.blocks[0].kind else {
            panic!()
        };
        assert!(l.ordered);
        assert_eq!(l.start, 7);
    }

    #[test]
    fn footnotes() {
        let src = "Text[^b] and[^a].\n\n[^a]: Note A.\n[^b]: Note B.\n[^unused]: never.\n";
        let p = parse(src, None);
        let BlockKind::Footnotes(notes) = &p.blocks.last().unwrap().kind else {
            panic!()
        };
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].name, "b");
        assert_eq!(notes[0].number, 1);
        assert_eq!(notes[1].name, "a");
        let BlockKind::Paragraph { run, .. } = &p.blocks[0].kind else {
            panic!()
        };
        assert_eq!(text(&p, *run), "Text1 and2.");
        assert!(p.anchors.contains_key("fn-a"));
        assert!(p.anchors.contains_key("fnref-b"));
        // Back-link appended to the note.
        let BlockKind::Paragraph { run, .. } = &notes[0].blocks[0].kind else {
            panic!()
        };
        assert!(text(&p, *run).ends_with('↩'));
    }

    #[test]
    fn slugs_with_duplicates() {
        let p = parse(
            "# Dup\n\n## Dup\n\n### Dup\n\n## The `render()` fn & [link](#)\n\n## 📋 Background\n",
            None,
        );
        let anchors: Vec<&str> = p.headings.iter().map(|h| h.anchor.as_str()).collect();
        assert_eq!(
            anchors,
            vec![
                "dup",
                "dup-1",
                "dup-2",
                "the-render-fn--link",
                "-background"
            ]
        );
        assert_eq!(p.headings[3].text, "The render() fn & link");
        assert_eq!(p.headings[4].text, "📋 Background");
    }

    #[test]
    fn safe_html_filtering() {
        let src = "<script>alert('x')</script>\n\nok <iframe src=x></iframe> <style>p{}</style> tail\n\n\
                   [js](javascript:alert(1))\n\n<a href=\"javascript:alert(1)\" onclick=\"x\">raw</a>\n\n\
                   <img src=\"x\" onerror=\"alert(1)\" alt=\"broken\">\n";
        let p = parse(src, None);
        for t in &p.texts {
            assert!(
                !t.text.contains("alert"),
                "dangerous content leaked: {:?}",
                t.text
            );
            assert!(!t.text.contains('<'), "raw tag shown: {:?}", t.text);
            for l in &t.links {
                assert!(!l.href.to_ascii_lowercase().contains("javascript"));
            }
        }
        let all: String = p
            .texts
            .iter()
            .map(|t| t.plain())
            .collect::<Vec<_>>()
            .join("|");
        assert!(all.contains("ok"));
        assert!(all.contains("tail"));
        assert!(all.contains("js"));
        assert!(all.contains("raw"));
        assert!(
            p.blocks.iter().any(
                |b| matches!(&b.kind, BlockKind::Image { image, .. } if image.alt == "broken")
            )
        );
    }

    #[test]
    fn details_and_inline_html() {
        let src = "<details>\n<summary>Click <b>me</b></summary>\n\nHidden **md**\n\n- one\n\n</details>\n\n\
                   Press <kbd>Ctrl</kbd>+<kbd>C</kbd>, H<sub>2</sub>O, <mark>hi</mark><br>next\n";
        let p = parse(src, None);
        let BlockKind::Details {
            summary,
            open,
            blocks,
        } = &p.blocks[0].kind
        else {
            panic!("{:?}", kinds(&p))
        };
        assert!(!open);
        assert_eq!(p.texts[*summary as usize].plain(), "Click me");
        assert_eq!(blocks.len(), 2);
        let BlockKind::Paragraph { run, .. } = &p.blocks[1].kind else {
            panic!()
        };
        let rt = &p.texts[*run as usize];
        assert_eq!(rt.plain(), "Press Ctrl+C, H2O, hi\nnext");
        assert!(rt.spans.iter().any(|s| s.flags & flags::KBD != 0));
        assert!(rt.spans.iter().any(|s| s.flags & flags::SUB != 0));
        assert!(rt.spans.iter().any(|s| s.flags & flags::MARK != 0));
    }

    #[test]
    fn html_table_and_center() {
        let src = "<p align=\"center\">\n  <img src=\"logo.png\" width=\"64\">\n</p>\n\n<table><tr><td>x</td></tr></table>\n";
        let p = parse(src, None);
        let BlockKind::Center(inner) = &p.blocks[0].kind else {
            panic!("{:?}", kinds(&p))
        };
        assert!(
            matches!(&inner[0].kind, BlockKind::Image { image, .. } if image.width == Some(64.0))
        );
        let BlockKind::Code(c) = &p.blocks[1].kind else {
            panic!()
        };
        assert_eq!(c.kind, CodeKind::Html);
        assert!(p.texts[c.run as usize].text.starts_with("<table>"));
    }

    #[test]
    fn front_matter_yaml_and_toml() {
        let p = parse(
            "---\ntitle: \"Spec\"\nstatus: draft\ntags: [a, b]\ndate: 2026-10-01\nmore: x\n---\n\n# H\n\nBody words here.\n",
            None,
        );
        let BlockKind::FrontMatter(fm) = &p.blocks[0].kind else {
            panic!()
        };
        assert_eq!(fm.preview, "title: Spec · status: draft · date: 2026-10-01");
        assert_eq!(p.title.as_deref(), Some("Spec"));
        assert_eq!(p.word_count, 4); // "H" + "Body words here." — front matter excluded
        assert_eq!(p.heading_meta[0].line, 9);
        let p = parse("+++\ntitle = \"T\"\n+++\nText\n", None);
        let BlockKind::FrontMatter(fm) = &p.blocks[0].kind else {
            panic!()
        };
        assert!(fm.toml);
        assert_eq!(fm.preview, "title: T");
        let p = parse("---\nnot front matter\n", None);
        assert!(!matches!(p.blocks[0].kind, BlockKind::FrontMatter(_)));
        // Nested YAML, lists, comments, block scalars; multi-line TOML values.
        let p = parse(
            "---\n# meta\ntitle: x\nauthors:\n  - a\n  - b\nsummary: |\n  Long text: with colons.\n---\nBody\n",
            None,
        );
        assert!(matches!(p.blocks[0].kind, BlockKind::FrontMatter(_)));
        let p = parse(
            "+++\ntitle = \"T\"\ntags = [\n  \"a\",\n  \"b\",\n]\n[params]\nx = 1\n+++\nBody\n",
            None,
        );
        assert!(matches!(p.blocks[0].kind, BlockKind::FrontMatter(_)));
    }

    #[test]
    fn a_leading_thematic_break_is_not_front_matter() {
        let src = "---\n\n# Release notes\n\nSome intro text.\n\n- one\n- two\n\n---\nAfter the second rule.\n";
        let p = parse(src, None);
        assert!(matches!(p.blocks[0].kind, BlockKind::Rule));
        assert!(matches!(p.blocks[1].kind, BlockKind::Heading { .. }));
        assert_eq!(p.headings.len(), 1);
        // Not metadata even without the blank line: prose between two rules.
        let p = parse("---\nJust a sentence here.\n---\nMore.\n", None);
        assert!(
            !p.blocks
                .iter()
                .any(|b| matches!(b.kind, BlockKind::FrontMatter(_)))
        );
        let p = parse("---\n# Title\n---\n", None);
        assert!(
            !p.blocks
                .iter()
                .any(|b| matches!(b.kind, BlockKind::FrontMatter(_)))
        );
        assert_eq!(p.headings.len(), 1);
    }

    #[test]
    fn mermaid_and_math_fallbacks() {
        let src = "```mermaid\ngraph LR\n```\n\n```ts\nlet x = 1\n```\n\n$$\nE = mc^2\n$$\n\nPrice is $5 and $10.\n";
        let p = parse(src, None);
        let BlockKind::Code(c) = &p.blocks[0].kind else {
            panic!()
        };
        assert_eq!(c.label.as_deref(), Some("Mermaid diagram"));
        assert_eq!(c.label_note.as_deref(), Some("· not rendered"));
        let BlockKind::Code(c) = &p.blocks[1].kind else {
            panic!()
        };
        assert_eq!(c.label.as_deref(), Some("TypeScript"));
        let BlockKind::Code(c) = &p.blocks[2].kind else {
            panic!("{:?}", kinds(&p))
        };
        assert_eq!(c.kind, CodeKind::Math);
        assert_eq!(c.label.as_deref(), Some("Math (TeX)"));
        assert_eq!(p.texts[c.run as usize].text, "E = mc^2");
        let BlockKind::Paragraph { run, .. } = &p.blocks[3].kind else {
            panic!()
        };
        assert_eq!(text(&p, *run), "Price is $5 and $10.");
    }

    #[test]
    fn links_and_images() {
        let base = if cfg!(windows) {
            Path::new(r"C:\docs")
        } else {
            Path::new("/docs")
        };
        let src =
            "[a](./x.md#sec) [b](https://e.com) <https://bare.com> [c](#top) ![i](img/p.png)\n";
        let p = parse(src, Some(base));
        let rt = &p.texts[0];
        assert_eq!(
            rt.links[0].dest,
            LinkDest::File {
                path: base.join("./x.md"),
                anchor: Some("sec".into())
            }
        );
        assert!(rt.links[1].external_icon);
        assert!(!rt.links[2].external_icon, "bare autolinks get no arrow");
        assert_eq!(rt.links[3].dest, LinkDest::Anchor("top".into()));
        let InlineObject::Image(img) = rt
            .objects
            .iter()
            .find(|o| matches!(o, InlineObject::Image(_)))
            .unwrap()
        else {
            panic!()
        };
        let want = if cfg!(windows) {
            r"file:///C:\docs\img\p.png"
        } else {
            "file:///docs/img/p.png"
        };
        assert_eq!(img.uri.as_deref(), Some(want));
    }

    fn image_uris(src: &str, base: Option<&Path>) -> Vec<Option<String>> {
        let p = parse(src, base);
        p.texts
            .iter()
            .flat_map(|t| &t.objects)
            .filter_map(|o| match o {
                InlineObject::Image(img) => Some(img.uri.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn images_never_reach_other_machines() {
        let base = if cfg!(windows) {
            Path::new(r"C:\docs")
        } else {
            Path::new("/docs")
        };
        // (Markdown unescapes `\\` in a destination to `\`; HTML attributes are taken as is.)
        let src = r#"x ![a](file://evil/x.png) ![b](//evil/s/x.png) ![c](\\\\evil\\s\\x.png)
<img src="\\evil\s\y.png"> <img src="//evil/s/y.png"> ![d](file:////evil/s/x.png)
![e](/\\evil\\s\\x.png) ![f](FILE://evil.example/s/x.png)
"#;
        let uris = image_uris(src, Some(base));
        assert_eq!(uris.len(), 8);
        assert!(uris.iter().all(Option::is_none), "{uris:?}");
        // Pasted text has no folder: relative images resolve to nothing.
        assert_eq!(image_uris("x ![](assets/logo.png)\n", None), vec![None]);
        // Local files still load, including `file:///` and `file://localhost/` sources.
        let ok = image_uris(
            "x ![](img/p.png) ![](file:///docs/q.png) ![](file://localhost/docs/r.png)\n",
            Some(base),
        );
        assert!(ok.iter().all(Option::is_some), "{ok:?}");
        if !cfg!(windows) {
            assert_eq!(ok[1].as_deref(), Some("file:///docs/q.png"));
            assert_eq!(ok[2].as_deref(), Some("file:///docs/r.png"));
        }
    }

    #[test]
    fn links_never_reach_other_machines() {
        let p = parse(
            r"[a](file://evil/s/a.md) [b](//evil/s/a.md) [c](\\\\evil\\s\\a.md) [d](file:///docs/a.md#x)",
            Some(Path::new("/docs")),
        );
        let dests: Vec<_> = p.texts[0].links.iter().map(|l| l.dest.clone()).collect();
        for (d, href) in dests[..3]
            .iter()
            .zip(["file://evil/s/a.md", "//evil/s/a.md"])
        {
            assert_eq!(d, &LinkDest::External(href.into()));
        }
        assert!(matches!(dests[2], LinkDest::External(_)));
        assert!(matches!(&dests[3], LinkDest::File { anchor: Some(a), .. } if a == "x"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_image_uris() {
        let uris = image_uris(
            "x ![](img/p.png) ![](C:/x.png) ![](file:///C:/y%20z.png)\n",
            Some(Path::new(r"C:\docs")),
        );
        assert_eq!(uris[0].as_deref(), Some(r"file:///C:\docs\img\p.png"));
        assert_eq!(uris[1].as_deref(), Some(r"file:///C:\x.png"));
        assert_eq!(uris[2].as_deref(), Some(r"file:///C:\y z.png"));
        let back = crate::paths::file_uri_to_path(uris[0].as_deref().unwrap());
        assert_eq!(back.as_deref(), Some(Path::new(r"C:\docs\img\p.png")));
        // A document on a share shows images from that share, and only that share.
        let share = Some(Path::new(r"\\nas\docs"));
        let uris = image_uris(
            "x ![](img/p.png) ![](//nas/docs/q.png) ![](//nas/other/q.png)\n",
            share,
        );
        assert_eq!(uris[0].as_deref(), Some(r"file:///\\nas\docs\img\p.png"));
        assert!(uris[1].is_some());
        assert_eq!(uris[2], None);
    }

    #[test]
    fn deep_nesting_is_flattened_not_dropped() {
        fn depth(blocks: &[Block]) -> usize {
            blocks
                .iter()
                .map(|b| {
                    1 + match &b.kind {
                        BlockKind::Quote(v) | BlockKind::Center(v) => depth(v),
                        BlockKind::Alert { blocks, .. } | BlockKind::Details { blocks, .. } => {
                            depth(blocks)
                        }
                        BlockKind::List(l) => {
                            l.items.iter().map(|i| depth(&i.blocks)).max().unwrap_or(0)
                        }
                        _ => 0,
                    }
                })
                .max()
                .unwrap_or(0)
        }
        // MAX_BLOCK_DEPTH + 1 containers, then the flattened paragraph.
        let max = MAX_BLOCK_DEPTH as usize + 2;
        for src in [
            format!("{}deep\n", "> ".repeat(100)),
            (0..100)
                .map(|i| format!("{}- item {i}\n", "  ".repeat(i)))
                .collect(),
            format!(
                "{}\ndeep\n\n{}",
                "<details>\n".repeat(100),
                "</details>\n".repeat(100)
            ),
        ] {
            let p = parse(&src, None);
            assert!(depth(&p.blocks) <= max, "{}", depth(&p.blocks));
            let all: String = p.texts.iter().map(|t| t.plain()).collect();
            assert!(all.contains("deep") || all.contains("item 99"), "{all}");
        }
        let p = parse(
            &format!("{}deep{}\n", "*".repeat(400), "*".repeat(400)),
            None,
        );
        assert!(p.texts[0].plain().contains("deep"));
    }

    #[test]
    fn block_image_and_numeric_columns() {
        let p = parse(
            "![alt](a.png)\n\n| n | name |\n|---|---|\n| 1,200 | a |\n| 45 ms | b |\n| +17% | c |\n",
            None,
        );
        assert!(matches!(p.blocks[0].kind, BlockKind::Image { .. }));
        let BlockKind::Table(t) = &p.blocks[1].kind else {
            panic!()
        };
        assert_eq!(t.numeric, vec![true, false]);
    }

    #[test]
    fn inline_code_has_padding_markers() {
        let p = parse("a `b` c", None);
        let rt = &p.texts[0];
        assert_eq!(rt.plain(), "a b c");
        assert_eq!(rt.text.chars().filter(|&c| c == MARKER).count(), 2);
    }
}
