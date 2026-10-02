//! Layout pass: IR blocks → positioned draw items (galleys, decorations, rects) for one
//! top-level block at a given column width. Results are cached by the view and redone only
//! when the width, style, fonts or a block's own state (images, highlighting, toggles) change.
//!
//! Coordinates are block-local: x from the column's left edge, y from the block's top.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use egui::epaint::text::{
    ByteIndex, LayoutJob, LayoutSection, TextFormat, TextWrapping, VariationCoords,
};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Galley, Pos2, Rect, Stroke, Vec2, pos2, vec2,
};

use crate::fonts::{self, Face};
use crate::highlight::{self, LineBg, Role};
use crate::icons::Icon;
use crate::ir::*;
use crate::style::{AlertColors, Palette};

// ---------------------------------------------------------------------------------------------
// Output types

#[derive(Clone, Debug)]
pub enum ImgState {
    /// Loading; size if already known.
    Pending(Option<Vec2>),
    /// Loaded; size in points.
    Ready(Vec2),
    /// Failed with a short reason (Not found / Unsupported / Too large / Network).
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecoKind {
    CodeBg,
    Kbd,
    Mark,
    /// Line (rect is the 1 px line) in this color.
    Line(Color32),
    /// A status circle or square (🟡, 🟥, ⚪ …) painted as a solid shape over its transparent
    /// glyph, with an optional ring for the white ones.
    Dot {
        color: Color32,
        square: bool,
        ring: Option<Color32>,
    },
}

#[derive(Clone, Debug)]
pub struct Deco {
    pub rect: Rect,
    pub kind: DecoKind,
}

#[derive(Clone, Debug)]
pub struct LinkRange {
    pub link: u32,
    /// Underline segments (1 px tall rects at baseline + 2).
    pub underline: Vec<Rect>,
    /// Hit areas (line boxes of the link text).
    pub hit: Vec<Rect>,
}

#[derive(Clone, Debug)]
pub enum ObjKind {
    Image {
        uri: String,
    },
    /// Broken image chip: label galley (alt text or file name) and tooltip.
    Chip {
        label: Arc<Galley>,
        tooltip: String,
    },
    ExternalIcon,
}

#[derive(Clone, Debug)]
pub struct ObjPlace {
    pub rect: Rect,
    pub kind: ObjKind,
    pub link: Option<u32>,
}

/// A selectable text run.
#[derive(Clone, Debug)]
pub struct TextItem {
    /// Galley origin (block coords).
    pub pos: Pos2,
    pub galley: Arc<Galley>,
    /// Glyphs are painted this much lower than the line boxes (CSS half-leading).
    pub shift: f32,
    /// Base font ascent and line height (for baselines of rows).
    pub asc: f32,
    pub line_h: f32,
    pub run: RunId,
    /// Char index of the galley's first char within the run (code split per line when
    /// wrapping); 0 otherwise.
    pub char_base: usize,
    /// [`row_starts`] of the galley (computed once; painting selections and find matches
    /// needs it every frame).
    pub starts: Arc<[usize]>,
    /// A chunk of a huge code block, shaped only while it is on screen: `galley` is then an
    /// empty placeholder and the view lays out `lazy.job` when it needs the glyphs.
    pub lazy: Option<Arc<LazyGalley>>,
    /// Decorations, galley-relative.
    pub decos: Vec<Deco>,
    pub links: Vec<LinkRange>,
    pub objects: Vec<ObjPlace>,
}

/// What a lazily shaped text item needs: its layout job and its galley's (precomputed) rect.
#[derive(Debug)]
pub struct LazyGalley {
    pub job: LayoutJob,
    pub rect: Rect,
}

/// Code blocks with more lines than this (and no wrapping) are split into chunks that are
/// shaped only while visible, so memory follows the viewport, not the block (a 10 MB log in a
/// fence would otherwise keep every glyph alive: SPEC principle 6).
const LAZY_CODE_LINES: usize = 1000;
const CODE_CHUNK_LINES: usize = 256;

impl TextItem {
    pub fn rect(&self) -> Rect {
        let r = match &self.lazy {
            Some(l) => l.rect,
            None => self.galley.rect,
        };
        r.translate(self.pos.to_vec2())
    }

    /// The galley with glyphs: lazily shaped chunks are laid out now (egui's galley cache keeps
    /// them while they are used every frame and drops them once they are not).
    pub fn shaped(&self, ctx: &egui::Context) -> Arc<Galley> {
        match &self.lazy {
            Some(l) => ctx.fonts_mut(|f| f.layout_job(l.job.clone())),
            None => self.galley.clone(),
        }
    }

    /// Galley-relative baseline of a row (where glyphs visually sit).
    pub fn baseline(&self, row_y: f32, row_h: f32) -> f32 {
        row_y + self.asc + (row_h - self.line_h) + self.shift
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// Copy a code block's source. `floating`: no header (button shows on hover).
    CopyCode {
        run: RunId,
        floating: bool,
    },
    Toggle {
        id: u64,
    },
}

#[derive(Clone, Debug)]
pub struct ButtonItem {
    pub rect: Rect,
    pub kind: ButtonKind,
    /// Hover area that reveals a floating button (the code frame).
    pub reveal: Rect,
}

#[derive(Clone, Debug)]
pub struct ScrollItem {
    /// UI-state key for the horizontal offset.
    pub id: u64,
    /// Visible frame (block coords).
    pub frame: Rect,
    /// Content width (≥ frame width when scrollable).
    pub content_w: f32,
    /// Content items; x is relative to `frame.left()` before scrolling, y is block coords.
    pub items: Vec<Item>,
    /// Background used for the edge fades.
    pub fade: Color32,
    /// Per-band fade colors (y0, y1, color), e.g. table rows; empty = `fade` everywhere.
    pub fade_bands: Vec<(f32, f32, Color32)>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Text(TextItem),
    /// Non-selectable label (headers, list numbers, overlines).
    Label {
        pos: Pos2,
        galley: Arc<Galley>,
    },
    Fill {
        rect: Rect,
        color: Color32,
        radius: CornerRadius,
    },
    Frame {
        rect: Rect,
        stroke: Stroke,
        radius: CornerRadius,
    },
    Gradient {
        rect: Rect,
        a: Color32,
        b: Color32,
    },
    Circle {
        center: Pos2,
        radius: f32,
        fill: Color32,
        stroke: Stroke,
    },
    Icon {
        icon: Icon,
        rect: Rect,
        color: Color32,
    },
    Checkbox {
        rect: Rect,
        checked: bool,
    },
    Image {
        rect: Rect,
        uri: String,
        link: Option<Link>,
        src: String,
    },
    /// Block-level broken image chip.
    Chip {
        rect: Rect,
        label: Arc<Galley>,
        tooltip: String,
    },
    Scroll(ScrollItem),
    Button(ButtonItem),
    /// Chevron for a toggle (rotated when open).
    Chevron {
        rect: Rect,
        open: bool,
        color: Color32,
    },
}

impl Item {
    /// Vertical extent (block coords), for culling.
    pub fn y_range(&self) -> (f32, f32) {
        let r = match self {
            Item::Text(t) => t.rect().expand2(vec2(0.0, 4.0)),
            Item::Label { pos, galley } => galley.rect.translate(pos.to_vec2()),
            Item::Fill { rect, .. }
            | Item::Frame { rect, .. }
            | Item::Gradient { rect, .. }
            | Item::Icon { rect, .. }
            | Item::Checkbox { rect, .. }
            | Item::Image { rect, .. }
            | Item::Chip { rect, .. }
            | Item::Chevron { rect, .. } => *rect,
            Item::Circle { center, radius, .. } => {
                Rect::from_center_size(*center, Vec2::splat(radius * 2.0))
            }
            Item::Scroll(s) => s.frame,
            Item::Button(b) => b.rect.union(b.reveal),
        };
        (r.top(), r.bottom())
    }
}

/// A laid-out top-level block.
#[derive(Clone, Debug, Default)]
pub struct LBlock {
    pub height: f32,
    pub items: Vec<Item>,
    /// Anchor name → (y, height) within the block.
    pub anchors: Vec<(String, f32, f32)>,
    /// Image URIs referenced (polled by the view).
    pub uris: Vec<String>,
    /// Some code block is waiting for highlighting.
    pub needs_highlight: bool,
}

// ---------------------------------------------------------------------------------------------
// Environment

pub struct Env<'a> {
    pub ctx: &'a egui::Context,
    pub pal: &'a Palette,
    /// Text size T.
    pub t: f32,
    pub serif: bool,
    pub wrap_code: bool,
    pub ppp: f32,
    pub texts: &'a [RichText],
    /// Heading slugs by heading index.
    pub headings: &'a [crate::Heading],
    pub images: &'a HashMap<String, ImgState>,
    /// Containers whose open state is toggled from the default.
    pub toggled: &'a HashSet<u64>,
    /// Ask the highlighter to prioritize (visible blocks).
    pub urgent: Cell<bool>,
    metrics: RefCell<HashMap<(FontFamily, u32), (f32, f32)>>,
    needs_highlight: Cell<bool>,
    /// Laying out a footnote hover card: the note's "↩" back-link is left out.
    note_card: Cell<bool>,
}

impl<'a> Env<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: &'a egui::Context,
        pal: &'a Palette,
        t: f32,
        serif: bool,
        wrap_code: bool,
        texts: &'a [RichText],
        headings: &'a [crate::Heading],
        images: &'a HashMap<String, ImgState>,
        toggled: &'a HashSet<u64>,
    ) -> Self {
        Self {
            ctx,
            pal,
            t,
            serif,
            wrap_code,
            ppp: ctx.pixels_per_point(),
            texts,
            headings,
            images,
            toggled,
            urgent: Cell::new(false),
            metrics: RefCell::new(HashMap::new()),
            needs_highlight: Cell::new(false),
            note_card: Cell::new(false),
        }
    }

    /// Spec px at T = 16 → content px.
    pub fn em(&self, px: f32) -> f32 {
        px * self.t / 16.0
    }

    /// Can a container indent its content by `indent` within width `w`? Deeply nested quotes,
    /// lists and boxes stop indenting (and drawing bars and markers) once the text would get
    /// narrower than 12 em, so their content stays readable at any depth.
    fn can_indent(&self, w: f32, indent: f32) -> bool {
        w - indent >= self.em(192.0).max(160.0)
    }

    /// (ascent, row height) of a family at a size.
    fn metrics(&self, family: &FontFamily, size: f32) -> (f32, f32) {
        let key = (family.clone(), size.to_bits());
        if let Some(m) = self.metrics.borrow().get(&key) {
            return *m;
        }
        let m = self.ctx.fonts_mut(|f| {
            let font = f.fonts.font(family);
            let m = font.styled_metrics(self.ppp, size, &VariationCoords::default());
            (m.ascent, m.row_height)
        });
        self.metrics.borrow_mut().insert(key, m);
        m
    }

    fn galley(&self, job: LayoutJob) -> Arc<Galley> {
        self.ctx.fonts_mut(|f| f.layout_job(job))
    }

    fn is_open(&self, id: u64, default: bool) -> bool {
        default != self.toggled.contains(&id)
    }

    /// Simple single-format label.
    fn label(
        &self,
        text: &str,
        size: f32,
        weight: u16,
        color: Color32,
        tracking: f32,
        max_w: f32,
    ) -> Arc<Galley> {
        let mut job = LayoutJob::single_section(
            text.to_owned(),
            TextFormat {
                font_id: FontId::new(size, fonts::family(Face::Sans, weight)),
                color,
                extra_letter_spacing: tracking,
                ..Default::default()
            },
        );
        job.wrap = TextWrapping {
            max_width: max_w,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        self.galley(job)
    }
}

// ---------------------------------------------------------------------------------------------
// Text styles

#[derive(Clone, Debug)]
pub struct TextBase {
    pub serif: bool,
    pub weight: u16,
    pub size: f32,
    pub line_h: f32,
    pub color: Color32,
    pub strong_color: Color32,
    pub strong_weight: u16,
    pub tracking: f32,
    pub uppercase: bool,
}

#[derive(Clone, Debug)]
struct Ctx {
    /// Body text color (text, text-2 in quotes, muted for checked tasks).
    color: Color32,
    list_depth: u8,
    align: HAlign,
    in_item: bool,
    tight: bool,
    small: bool,
}

impl Ctx {
    fn root(pal: &Palette) -> Self {
        Self {
            color: pal.text,
            list_depth: 0,
            align: HAlign::Left,
            in_item: false,
            tight: false,
            small: false,
        }
    }
}

impl Env<'_> {
    fn body(&self, ctx: &Ctx) -> TextBase {
        let (serif, mut size, mut line_h) = if self.serif {
            let s = self.t + 1.0;
            (true, s, s * 28.0 / 17.0)
        } else {
            (false, self.t, self.t * 26.0 / 16.0)
        };
        let mut color = ctx.color;
        if ctx.small {
            size *= 0.875;
            line_h = size * 22.0 / 14.0;
            if color == self.pal.text {
                color = self.pal.text_2;
            }
        }
        TextBase {
            serif,
            weight: 400,
            size,
            line_h,
            color,
            strong_color: if color == self.pal.text {
                self.pal.text_strong
            } else {
                color
            },
            strong_weight: if serif { 650 } else { 600 },
            tracking: 0.0,
            uppercase: false,
        }
    }

    fn heading_base(&self, level: u8) -> TextBase {
        let t = self.t;
        let (em, lh, w, tr, color, upper) = match level {
            1 => (1.875, 38.0 / 30.0, 700, -0.022, self.pal.text_strong, false),
            2 => (
                1.4375,
                30.0 / 23.0,
                650,
                -0.017,
                self.pal.text_strong,
                false,
            ),
            3 => (
                1.1875,
                26.0 / 19.0,
                650,
                -0.012,
                self.pal.text_strong,
                false,
            ),
            4 => (1.0, 24.0 / 16.0, 700, -0.006, self.pal.text_strong, false),
            5 => (0.8125, 20.0 / 13.0, 700, 0.06, self.pal.text_2, true),
            _ => (0.8125, 20.0 / 13.0, 600, 0.0, self.pal.muted, false),
        };
        let size = t * em;
        TextBase {
            serif: false,
            weight: w,
            size,
            line_h: size * lh,
            color,
            strong_color: color,
            strong_weight: 700,
            tracking: tr * size,
            uppercase: upper,
        }
    }

    fn cell_base(&self, header: bool) -> TextBase {
        let (serif, size) = if self.serif {
            (true, (self.t + 1.0) * 0.9375)
        } else {
            (false, self.t * 0.9375)
        };
        TextBase {
            serif,
            weight: if header { 600 } else { 400 },
            size,
            line_h: size * 22.0 / 15.0,
            color: if header {
                self.pal.text_strong
            } else {
                self.pal.text
            },
            strong_color: self.pal.text_strong,
            strong_weight: if header {
                700
            } else if serif {
                650
            } else {
                600
            },
            tracking: 0.0,
            uppercase: false,
        }
    }

    fn code_base(&self) -> (f32, f32) {
        let size = self.t * 0.875;
        (size, size * 22.0 / 14.0)
    }

    fn family_for(&self, base: &TextBase, weight: u16, italic: bool, sans: bool) -> FontFamily {
        let face = match (base.serif && !sans, italic) {
            (true, false) => Face::Serif,
            (true, true) => Face::SerifItalic,
            (false, false) => Face::Sans,
            (false, true) => Face::SansItalic,
        };
        fonts::family(face, weight)
    }

    /// Text format for a span; also returns (family, size) of its font.
    fn format(&self, flags: u16, base: &TextBase) -> TextFormat {
        use crate::ir::flags::*;
        let pal = self.pal;
        let mut size = base.size;
        let mut weight = base.weight;
        let mut color = base.color;
        let mut tracking = base.tracking;
        let mut valign = Align::BOTTOM;
        let italic = flags & EM != 0;
        let mut mono = false;
        let mut sans = false;
        if flags & STRONG != 0 {
            weight = base.strong_weight.max(weight);
            color = base.strong_color;
        }
        if flags & CODE != 0 {
            mono = true;
            size *= 0.875;
            color = pal.icode_fg;
            tracking = 0.0;
        }
        if flags & KBD != 0 {
            sans = true;
            mono = false;
            size = base.size * 0.8;
            weight = 500;
            color = pal.text;
            tracking = 0.0;
        }
        if flags & (SUP | FOOTREF) != 0 {
            size *= 0.75;
            valign = Align::TOP;
        }
        if flags & FOOTREF != 0 {
            weight = 600;
            sans = true;
        }
        if flags & SUB != 0 {
            size *= 0.75;
        }
        if flags & STRIKE != 0 {
            color = pal.muted;
        }
        if flags & LINK != 0 {
            color = pal.link;
        }
        let family = if mono {
            FontFamily::Monospace
        } else {
            self.family_for(base, weight, italic, sans)
        };
        let base_family = self.family_for(base, base.weight, false, false);
        let (asc_b, _) = self.metrics(&base_family, base.size);
        let (asc_s, h_s) = self.metrics(&family, size);
        let line_height = if valign == Align::TOP {
            h_s.min(base.line_h)
        } else if flags & SUB != 0 {
            base.line_h - asc_b + asc_s - 0.2 * base.size
        } else {
            base.line_h - asc_b + asc_s
        };
        TextFormat {
            font_id: FontId::new(size, family),
            extra_letter_spacing: tracking,
            line_height: Some(line_height),
            color,
            valign,
            ..Default::default()
        }
    }

    fn base_format(&self, base: &TextBase) -> TextFormat {
        self.format(0, base)
    }
}

/// Colored circles and squares used as status markers (🟡, 🟥, ⚪ …): `Some(square)`.
/// Monochrome emoji fonts draw them as faint hatched outlines, which reads as "empty" next to
/// ✅ and ❌, so the engine paints them as solid shapes in their tint instead (SPEC §5).
pub fn status_shape(c: char) -> Option<bool> {
    match c {
        '🟠' | '🟡' | '🟢' | '🟣' | '🟤' | '🔴' | '🔵' | '⚪' | '⚫' => Some(false),
        '🟥' | '🟦' | '🟧' | '🟨' | '🟩' | '🟪' | '🟫' | '⬜' | '⬛' => Some(true),
        _ => None,
    }
}

/// Semantic tint for status emoji (SPEC §5).
pub fn emoji_tint(c: char, pal: &Palette) -> Option<Color32> {
    let a = &pal.alert;
    Some(match c {
        '✅' | '✔' | '☑' | '🟢' | '🟩' | '💚' => a.tip.fg,
        '❌' | '✖' | '❎' | '⛔' | '🚫' | '🛑' | '🔴' | '🟥' | '❗' | '‼' => {
            a.caution.fg
        }
        '⚠' | '🟡' | '🟨' | '🚧' | '💡' => a.warning.fg,
        '🟠' | '🟧' | '🔥' => pal.orange,
        'ℹ' | '🔵' | '🟦' => a.note.fg,
        '🟣' | '🟪' => a.important.fg,
        '✨' | '⭐' | '🌟' | '❤' | '💖' => pal.accent,
        '⚪' | '⬜' => pal.faint,
        '⚫' | '⬛' => pal.text_strong,
        '🟤' | '🟫' => pal.orange.lerp_to_gamma(Color32::BLACK, 0.35),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// Galley geometry helpers

/// Char index where each row starts (plus the total at the end).
pub fn row_starts(galley: &Galley) -> Vec<usize> {
    let mut out = Vec::with_capacity(galley.rows.len() + 1);
    let mut i = 0;
    for r in &galley.rows {
        out.push(i);
        i += r.char_count_including_newline().0;
    }
    out.push(i);
    out
}

/// Index of the row containing char `c` (the last row for `c` past the end).
fn row_of(galley: &Galley, starts: &[usize], c: usize) -> usize {
    let n = galley.rows.len().min(starts.len());
    starts[..n].partition_point(|&s| s <= c).saturating_sub(1)
}

/// Per-row horizontal segments covering chars `range`: (row index, x0, x1), galley coords.
/// Starts at the row containing `range.start`, so the cost is the rows covered (plus a binary
/// search), not the galley's size.
pub fn segments(galley: &Galley, starts: &[usize], range: Range<usize>) -> Vec<(usize, f32, f32)> {
    let mut out = Vec::new();
    if range.is_empty() {
        return out;
    }
    let first = row_of(galley, starts, range.start);
    for (ri, row) in galley.rows.iter().enumerate().skip(first) {
        let r0 = starts[ri];
        let n = row.glyphs.len();
        let r1 = r0 + n;
        let s = range.start.max(r0);
        let e = range.end.min(r1);
        let includes_newline = row.ends_with_newline && range.start <= r1 && range.end > r1;
        if s >= e && !includes_newline {
            if r0 >= range.end {
                break;
            }
            continue;
        }
        let x_at = |c: usize| -> f32 {
            let k = c - r0;
            if k < n {
                row.glyphs[k].pos.x
            } else {
                row.glyphs.last().map_or(0.0, |g| g.max_x())
            }
        };
        let x0 = row.pos.x + if s < e { x_at(s) } else { x_at(r1) };
        let mut x1 = row.pos.x + if s < e { x_at(e) } else { x_at(r1) };
        if includes_newline {
            x1 += 4.0; // show the selected newline
        }
        out.push((ri, x0, x1.max(x0)));
    }
    out
}

/// Character → galley-relative rect of its glyph (for find/scroll targets).
pub fn char_rect(galley: &Galley, starts: &[usize], c: usize) -> Rect {
    let first = row_of(galley, starts, c);
    for (ri, row) in galley.rows.iter().enumerate().skip(first) {
        let r0 = starts[ri];
        let r1 = starts[ri + 1];
        if c < r1 || ri + 1 == galley.rows.len() {
            let k = c.saturating_sub(r0);
            let x = row.pos.x + row.glyphs.get(k).map_or(row.size.x, |g| g.pos.x);
            return Rect::from_min_max(pos2(x, row.pos.y), pos2(x, row.pos.y + row.size.y));
        }
    }
    Rect::NOTHING
}

// ---------------------------------------------------------------------------------------------
// Rich text → TextItem

struct ObjSlot {
    obj: u32,
    char_b: usize,
    size: Vec2,
    kind: ObjKind,
    link: Option<u32>,
}

impl Env<'_> {
    fn object_size(&self, obj: &InlineObject, base: &TextBase, max_w: f32) -> (Vec2, ObjKind) {
        match obj {
            InlineObject::ExternalIcon => (
                vec2(2.0 + 0.7 * base.size, 0.7 * base.size),
                ObjKind::ExternalIcon,
            ),
            InlineObject::Image(img) => {
                let Some(uri) = &img.uri else {
                    let (sz, kind) = self.chip(img, "Unsupported");
                    return (sz, kind);
                };
                match self.images.get(uri) {
                    Some(ImgState::Failed(reason)) => self.chip(img, reason),
                    state => {
                        let natural = match state {
                            Some(ImgState::Ready(s)) => Some(*s),
                            Some(ImgState::Pending(s)) => *s,
                            _ => None,
                        };
                        let size = image_size(img, natural, max_w);
                        (size, ObjKind::Image { uri: uri.clone() })
                    }
                }
            }
        }
    }

    fn chip(&self, img: &ImageRef, reason: &str) -> (Vec2, ObjKind) {
        let name = if img.alt.trim().is_empty() {
            img.src
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&img.src)
                .to_owned()
        } else {
            img.alt.clone()
        };
        let label = self.label(
            &name,
            self.em(14.0),
            400,
            self.pal.muted,
            0.0,
            self.em(420.0),
        );
        let w = 8.0 + 16.0 + 6.0 + label.size().x + 8.0;
        let h = (label.size().y + 8.0).max(24.0);
        (
            vec2(w, h),
            ObjKind::Chip {
                label,
                tooltip: format!("{}\n{reason}", img.src),
            },
        )
    }

    /// Lay out a run. `x`, `y`: top-left of the box; `w`: wrap width.
    fn rich(&self, run: RunId, base: &TextBase, x: f32, y: f32, w: f32, align: HAlign) -> TextItem {
        use crate::ir::flags::*;
        let rt = &self.texts[run as usize];
        let base_fmt = self.base_format(base);
        let base_family = self.family_for(base, base.weight, false, false);
        let (asc_b, h_b) = self.metrics(&base_family, base.size);
        let shift = ((base.line_h - h_b) / 2.0).max(0.0);
        let mut job = LayoutJob {
            break_on_newline: true,
            ..Default::default()
        };
        job.wrap.max_width = w.max(1.0);
        job.halign = match align {
            HAlign::Left => Align::LEFT,
            HAlign::Center => Align::Center,
            HAlign::Right => Align::RIGHT,
        };
        let mut objs: Vec<ObjSlot> = Vec::new();
        // Status shapes: (char index, char, font size).
        let mut shapes: Vec<(usize, char, f32)> = Vec::new();
        // Char offsets of spans (galley chars == text chars).
        let mut span_chars: Vec<Range<usize>> = Vec::with_capacity(rt.spans.len());
        let mut chars = 0usize;
        let note_card = self.note_card.get();
        for span in &rt.spans {
            if note_card && span.flags & FOOTBACK != 0 {
                span_chars.push(chars..chars);
                continue;
            }
            let text = &rt.text[span.range.clone()];
            let n = text.chars().count();
            span_chars.push(chars..chars + n);
            match span.kind {
                SpanKind::Text => {
                    let fmt = self.format(span.flags, base);
                    let owned;
                    let text = if base.uppercase {
                        owned = text
                            .chars()
                            .map(|c| {
                                let mut u = c.to_uppercase();
                                match (u.next(), u.next()) {
                                    (Some(a), None) => a,
                                    _ => c,
                                }
                            })
                            .collect::<String>();
                        owned.as_str()
                    } else {
                        text
                    };
                    let size = fmt.font_id.size;
                    for (ci, c) in self.append_tinted(&mut job, text, fmt) {
                        shapes.push((chars + ci, c, size));
                    }
                }
                SpanKind::Pad(em) => {
                    let mut fmt = base_fmt.clone();
                    fmt.color = Color32::TRANSPARENT;
                    job.append(MARKER_STR, em * base.size, fmt);
                }
                SpanKind::Object(i) => {
                    let obj = &rt.objects[i as usize];
                    let (size, kind) = self.object_size(obj, base, w);
                    let mut fa = base_fmt.clone();
                    fa.color = Color32::TRANSPARENT;
                    let mut fb = fa.clone();
                    let need = size.y + base.line_h - asc_b - shift;
                    if need > base.line_h {
                        fb.line_height = Some(need);
                    }
                    job.append(MARKER_STR, 0.0, fa);
                    job.append(MARKER_STR, size.x, fb);
                    let link = match obj {
                        InlineObject::Image(img) => img.link.or(span.link),
                        InlineObject::ExternalIcon => span.link,
                    };
                    objs.push(ObjSlot {
                        obj: i,
                        char_b: chars + 1,
                        size,
                        kind,
                        link,
                    });
                }
            }
            chars += n;
        }
        if job.text.is_empty() {
            // Keep an empty row of the right height.
            job.append("", 0.0, base_fmt.clone());
            if job.sections.is_empty() {
                job.sections.push(LayoutSection {
                    leading_space: 0.0,
                    byte_range: ByteIndex(0)..ByteIndex(0),
                    format: base_fmt.clone(),
                });
            }
        }
        let galley = self.galley(job);
        let starts: Arc<[usize]> = row_starts(&galley).into();
        let anchor_x = match align {
            HAlign::Left => x,
            HAlign::Center => x + w / 2.0,
            HAlign::Right => x + w,
        };
        let mut item = TextItem {
            pos: pos2(anchor_x, y),
            galley,
            shift,
            asc: asc_b,
            line_h: base.line_h,
            run,
            char_base: 0,
            starts: starts.clone(),
            lazy: None,
            decos: Vec::new(),
            links: Vec::new(),
            objects: Vec::new(),
        };
        let g = item.galley.clone();
        let line_h = base.line_h;
        let row_line_box = |ri: usize| -> (f32, f32, f32) {
            let row = &g.rows[ri];
            let bl = row.pos.y + asc_b + (row.size.y - line_h) + shift;
            let top = bl - asc_b - shift;
            (top, top + line_h, bl)
        };
        let xh = base.size * if base.serif { 0.507 } else { 0.546 };

        // Inline code chips and keycaps: one box per element (its pads included), so the text
        // sits centered with the same padding on both sides (SPEC §6).
        let chip_kind = |f: u16| match () {
            _ if f & KBD != 0 => Some(DecoKind::Kbd),
            _ if f & CODE != 0 => Some(DecoKind::CodeBg),
            _ => None,
        };
        let mut si = 0;
        while si < rt.spans.len() {
            let Some(kind) = chip_kind(rt.spans[si].flags) else {
                si += 1;
                continue;
            };
            // The element runs to its closing pad (`<kbd>a</kbd><kbd>b</kbd>` is two keys).
            let mut sj = si + 1;
            while sj < rt.spans.len() && chip_kind(rt.spans[sj].flags) == Some(kind) {
                let closing = matches!(rt.spans[sj].kind, SpanKind::Pad(_))
                    && matches!(rt.spans.get(sj + 1).map(|s| s.kind), Some(SpanKind::Pad(_)));
                sj += 1;
                if closing {
                    break;
                }
            }
            let range = span_chars[si].start..span_chars[sj - 1].end;
            // egui places a pad marker after its leading space, so the opening pad lies left of
            // the first segment, except at the start of a wrapped row, where egui drops it.
            let lead = match rt.spans[si].kind {
                SpanKind::Pad(em) => em * base.size,
                _ => 0.0,
            };
            for (k, (ri, mut x0, x1)) in
                segments(&g, &starts, range.clone()).into_iter().enumerate()
            {
                let wrapped_start =
                    ri > 0 && starts[ri] == range.start && !g.rows[ri - 1].ends_with_newline;
                if k == 0 && !wrapped_start {
                    x0 -= lead;
                }
                let (top, bottom, bl) = row_line_box(ri);
                let rect = if kind == DecoKind::Kbd {
                    let h = base.size * 0.8 * 1.21 + 4.0;
                    let cy = bl - base.size * 0.29;
                    Rect::from_min_max(pos2(x0, cy - h / 2.0), pos2(x1, cy + h / 2.0))
                } else {
                    Rect::from_min_max(pos2(x0, top + 2.0), pos2(x1, bottom - 2.0))
                };
                item.decos.push(Deco { rect, kind });
            }
            si = sj;
        }
        // Other decorations by span flags.
        for (si, span) in rt.spans.iter().enumerate() {
            let range = span_chars[si].clone();
            let f = span.flags;
            if f & (MARK | STRIKE | UNDERLINE) == 0 {
                continue;
            }
            for (ri, x0, x1) in segments(&g, &starts, range.clone()) {
                let (top, bottom, bl) = row_line_box(ri);
                if f & (KBD | CODE) != 0 {
                    // Boxed above.
                } else if f & MARK != 0 {
                    item.decos.push(Deco {
                        rect: Rect::from_min_max(
                            pos2(x0 - 1.0, top + 2.0),
                            pos2(x1 + 1.0, bottom - 2.0),
                        ),
                        kind: DecoKind::Mark,
                    });
                }
                if f & STRIKE != 0 && span.kind == SpanKind::Text {
                    let yy = (bl - xh / 2.0).round();
                    item.decos.push(Deco {
                        rect: Rect::from_min_max(pos2(x0, yy - 0.5), pos2(x1, yy + 0.5)),
                        kind: DecoKind::Line(self.pal.muted),
                    });
                }
                if f & UNDERLINE != 0 && f & LINK == 0 {
                    let yy = (bl + 2.0).round();
                    item.decos.push(Deco {
                        rect: Rect::from_min_max(pos2(x0, yy - 0.5), pos2(x1, yy + 0.5)),
                        kind: DecoKind::Line(base.color),
                    });
                }
            }
        }
        // Status shapes: sitting on the baseline, cap-height tall, centered in the glyph's box.
        for (ci, c, size) in shapes {
            let Some(&(ri, x0, x1)) = segments(&g, &starts, ci..ci + 1).first() else {
                continue;
            };
            let (_, _, bl) = row_line_box(ri);
            let d = (0.72 * size).round();
            let square = status_shape(c).unwrap_or(false);
            let rect = Rect::from_center_size(
                pos2((x0 + x1) / 2.0, bl - d / 2.0),
                Vec2::splat(if square { d * 0.92 } else { d }),
            );
            let white = matches!(c, '⚪' | '⬜');
            item.decos.push(Deco {
                rect,
                kind: DecoKind::Dot {
                    color: emoji_tint(c, self.pal).unwrap_or(self.pal.muted),
                    square,
                    ring: white.then_some(self.pal.muted),
                },
            });
        }
        // Links (merge adjacent spans of the same link).
        let mut li = 0;
        while li < rt.spans.len() {
            let Some(link) = rt.spans[li].link else {
                li += 1;
                continue;
            };
            let mut lj = li + 1;
            while lj < rt.spans.len() && rt.spans[lj].link == Some(link) {
                lj += 1;
            }
            // Underline only under text (not the ↗ icon or image).
            let mut underline = Vec::new();
            let mut hit = Vec::new();
            for (sp, chars) in rt.spans[li..lj].iter().zip(&span_chars[li..lj]) {
                let segs = segments(&g, &starts, chars.clone());
                for (ri, x0, x1) in segs {
                    let (top, bottom, bl) = row_line_box(ri);
                    hit.push(Rect::from_min_max(pos2(x0, top), pos2(x1, bottom)));
                    if sp.kind == SpanKind::Text && sp.flags & flags::FOOTREF == 0 {
                        let yy = (bl + 2.0).round();
                        underline.push(Rect::from_min_max(pos2(x0, yy - 0.5), pos2(x1, yy + 0.5)));
                    }
                }
            }
            merge_rects(&mut underline);
            item.links.push(LinkRange {
                link,
                underline,
                hit,
            });
            li = lj;
        }
        // Inline objects.
        for slot in objs {
            let (ri, xb) = {
                let segs = segments(&g, &starts, slot.char_b..slot.char_b + 1);
                match segs.first() {
                    Some((ri, x0, _)) => (*ri, *x0),
                    None => continue,
                }
            };
            let (_, _, bl) = row_line_box(ri);
            let x0 = xb - slot.size.x;
            let rect = match slot.kind {
                ObjKind::ExternalIcon => {
                    let s = slot.size.y;
                    Rect::from_min_size(pos2(x0 + 2.0, bl - s * 0.92), Vec2::splat(s))
                }
                _ => Rect::from_min_size(pos2(x0, bl - slot.size.y), slot.size),
            };
            let _ = slot.obj;
            item.objects.push(ObjPlace {
                rect,
                kind: slot.kind,
                link: slot.link,
            });
        }
        item
    }

    /// Append text, giving status emoji their semantic tint. Status circles and squares are
    /// appended transparent; returns their (char index in `text`, char) to paint as shapes.
    fn append_tinted(
        &self,
        job: &mut LayoutJob,
        text: &str,
        fmt: TextFormat,
    ) -> Vec<(usize, char)> {
        let mut shapes = Vec::new();
        let mut last = 0;
        for (ci, (i, c)) in text.char_indices().enumerate() {
            if (c as u32) < 0x2000 {
                continue;
            }
            let tint = if status_shape(c).is_some() {
                shapes.push((ci, c));
                Some(Color32::TRANSPARENT)
            } else {
                emoji_tint(c, self.pal)
            };
            if let Some(tint) = tint {
                if i > last {
                    job.append(&text[last..i], 0.0, fmt.clone());
                }
                let mut end = i + c.len_utf8();
                // Keep a following variation selector with the emoji.
                if text[end..].starts_with('\u{FE0F}') {
                    end += 3;
                }
                let mut f = fmt.clone();
                f.color = tint;
                job.append(&text[i..end], 0.0, f);
                last = end;
            }
        }
        if last < text.len() {
            job.append(&text[last..], 0.0, fmt);
        }
        shapes
    }
}

fn merge_rects(v: &mut Vec<Rect>) {
    v.sort_by(|a, b| {
        a.top()
            .total_cmp(&b.top())
            .then(a.left().total_cmp(&b.left()))
    });
    let mut out: Vec<Rect> = Vec::with_capacity(v.len());
    for r in v.drain(..) {
        if let Some(last) = out.last_mut()
            && (last.top() - r.top()).abs() < 0.5
            && r.left() <= last.right() + 0.5
        {
            *last = last.union(r);
            continue;
        }
        out.push(r);
    }
    *v = out;
}

/// Display size of an image: natural size, `width`/`height` attributes, capped at `max_w`.
pub fn image_size(img: &ImageRef, natural: Option<Vec2>, max_w: f32) -> Vec2 {
    let nat = natural.unwrap_or(Vec2::ZERO);
    let aspect = if nat.x > 0.0 && nat.y > 0.0 {
        nat.y / nat.x
    } else {
        0.0
    };
    let (mut w, mut h) = match (img.width, img.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, if aspect > 0.0 { w * aspect } else { 0.0 }),
        (None, Some(h)) => (if aspect > 0.0 { h / aspect } else { 0.0 }, h),
        (None, None) => (nat.x, nat.y),
    };
    if w > max_w && w > 0.0 {
        h *= max_w / w;
        w = max_w;
    }
    vec2(w.max(0.0), h.max(0.0))
}

// ---------------------------------------------------------------------------------------------
// Blocks

#[derive(Default)]
struct Out {
    items: Vec<Item>,
    anchors: Vec<(String, f32, f32)>,
    uris: Vec<String>,
}

/// (space above, space below) of a block, in px at the current text size.
pub fn margins(kind: &BlockKind, t: f32) -> (f32, f32) {
    let e = |v: f32| v * t / 16.0;
    match kind {
        BlockKind::Heading { level, .. } => match level {
            1 => (e(56.0), e(20.0)),
            2 => (e(40.0), e(14.0)),
            3 => (e(32.0), e(8.0)),
            4 => (e(24.0), e(6.0)),
            5 => (e(24.0), e(4.0)),
            _ => (e(20.0), e(4.0)),
        },
        BlockKind::Paragraph { .. } | BlockKind::List(_) | BlockKind::Center(_) => {
            (e(16.0), e(16.0))
        }
        BlockKind::Code(_)
        | BlockKind::Table(_)
        | BlockKind::Alert { .. }
        | BlockKind::Quote(_)
        | BlockKind::Image { .. }
        | BlockKind::Details { .. }
        | BlockKind::FrontMatter(_) => (e(20.0), e(20.0)),
        BlockKind::Rule => (e(32.0), e(32.0)),
        BlockKind::Footnotes(_) => (e(40.0), 0.0),
        BlockKind::Anchor(_) => (0.0, 0.0),
    }
}

pub fn is_heading(kind: &BlockKind) -> bool {
    matches!(kind, BlockKind::Heading { .. })
}

/// Gap between two consecutive blocks (SPEC §4 Rhythm): margins collapse, except that a
/// heading after a heading gets 12, and whatever follows a heading gets the heading's own space
/// below, so the heading sits ≥ 2.5× closer to its content than to what precedes it.
pub fn gap(prev: &BlockKind, next: &BlockKind, t: f32) -> f32 {
    if matches!(next, BlockKind::Anchor(_)) || matches!(prev, BlockKind::Anchor(_)) {
        return 0.0;
    }
    if is_heading(prev) && is_heading(next) {
        return 12.0 * t / 16.0;
    }
    if is_heading(prev) {
        return margins(prev, t).1;
    }
    margins(prev, t).1.max(margins(next, t).0)
}

/// Lay out one top-level block at column width `w`.
pub fn layout_top(env: &Env, block: &Block, w: f32) -> LBlock {
    let mut out = Out::default();
    let mut y = 0.0;
    let ctx = Ctx::root(env.pal);
    env.needs_highlight.set(false);
    env.block(block, 0.0, w, &ctx, &mut out, &mut y);
    LBlock {
        height: y.max(0.0),
        items: out.items,
        anchors: out.anchors,
        uris: out.uris,
        needs_highlight: env.needs_highlight.get(),
    }
}

/// Lay out a footnote's blocks for the hover card (14 px, `text-2`).
pub fn layout_note(env: &Env, blocks: &[Block], w: f32) -> LBlock {
    let mut out = Out::default();
    let mut y = 0.0;
    let mut ctx = Ctx::root(env.pal);
    ctx.small = true;
    ctx.in_item = true;
    ctx.tight = true;
    // The "↩" back-link points at the reference under the pointer: not part of the note.
    let back_only = |b: &Block| match &b.kind {
        BlockKind::Paragraph { run, .. } => env.texts[*run as usize]
            .spans
            .iter()
            .all(|s| s.flags & flags::FOOTBACK != 0),
        _ => false,
    };
    let blocks = match blocks.split_last() {
        Some((last, rest)) if back_only(last) => rest,
        _ => blocks,
    };
    env.note_card.set(true);
    env.children(blocks, 0.0, w, &ctx, &mut out, &mut y);
    env.note_card.set(false);
    LBlock {
        height: y.max(0.0),
        items: out.items,
        anchors: out.anchors,
        uris: out.uris,
        needs_highlight: false,
    }
}

impl Env<'_> {
    fn children(&self, blocks: &[Block], x: f32, w: f32, ctx: &Ctx, out: &mut Out, y: &mut f32) {
        let mut prev: Option<&BlockKind> = None;
        for b in blocks {
            if let Some(p) = prev {
                let mut g = gap(p, &b.kind, self.t);
                if ctx.in_item {
                    g = g.min(self.em(12.0));
                    if ctx.tight && matches!(b.kind, BlockKind::List(_)) {
                        g = self.em(4.0);
                    }
                }
                *y += g;
            }
            self.block(b, x, w, ctx, out, y);
            if !matches!(b.kind, BlockKind::Anchor(_)) {
                prev = Some(&b.kind);
            }
        }
    }

    fn block(&self, b: &Block, x: f32, w: f32, ctx: &Ctx, out: &mut Out, y: &mut f32) {
        match &b.kind {
            BlockKind::Paragraph { run, align } => {
                let base = self.body(ctx);
                let align = if *align == HAlign::Left {
                    ctx.align
                } else {
                    *align
                };
                let item = self.rich(*run, &base, x, *y, w, align);
                *y += item.galley.rect.height();
                self.collect_uris(&item, out);
                out.items.push(Item::Text(item));
            }
            BlockKind::Heading {
                level,
                run,
                index,
                align,
            } => {
                let base = self.heading_base(*level);
                let align = if *align == HAlign::Left {
                    ctx.align
                } else {
                    *align
                };
                let top = *y;
                let item = self.rich(*run, &base, x, *y, w, align);
                *y += item.galley.rect.height();
                let anchor = self.headings.get(*index).map(|h| h.anchor.clone());
                self.collect_uris(&item, out);
                out.items.push(Item::Text(item));
                if *level <= 2 {
                    *y += if *level == 1 {
                        self.em(12.0)
                    } else {
                        self.em(8.0)
                    };
                    let rule = Rect::from_min_size(pos2(x, (*y).round()), vec2(w, 1.0));
                    out.items.push(Item::Fill {
                        rect: rule,
                        color: self.pal.border,
                        radius: CornerRadius::ZERO,
                    });
                    if *level == 1 {
                        let tick =
                            Rect::from_min_size(pos2(x, rule.center().y - 1.5), vec2(48.0, 3.0));
                        out.items.push(Item::Gradient {
                            rect: tick,
                            a: self.pal.grad_a,
                            b: self.pal.grad_b,
                        });
                    }
                    *y += 1.0;
                }
                if let Some(a) = anchor {
                    out.anchors.push((a, top, *y - top));
                }
            }
            BlockKind::Image {
                image,
                links,
                align,
            } => {
                let align = if *align == HAlign::Left {
                    ctx.align
                } else {
                    *align
                };
                self.block_image(image, links, x, w, align, b, out, y);
            }
            BlockKind::Code(c) => self.code(c, b.id, x, w, out, y),
            BlockKind::List(l) => self.list(l, x, w, ctx, out, y),
            BlockKind::Quote(blocks) => {
                let top = *y;
                let mut inner = ctx.clone();
                inner.color = self.pal.text_2;
                inner.in_item = false;
                let bar_w = 3.0;
                let indent = bar_w + self.em(16.0);
                if self.can_indent(w, indent) {
                    self.children(blocks, x + indent, w - indent, &inner, out, y);
                    out.items.push(Item::Fill {
                        rect: Rect::from_min_max(pos2(x, top), pos2(x + bar_w, *y)),
                        color: self.pal.quote_bar,
                        radius: CornerRadius::same(1),
                    });
                } else {
                    self.children(blocks, x, w, &inner, out, y);
                }
            }
            BlockKind::Alert { kind, blocks } => self.alert(*kind, blocks, x, w, ctx, out, y),
            BlockKind::Table(t) => self.table(t, b.id, x, w, out, y),
            BlockKind::Rule => {
                let cy = *y + 2.0;
                let cx = x + w / 2.0;
                for dx in [-12.0, 0.0, 12.0] {
                    out.items.push(Item::Circle {
                        center: pos2(cx + dx, cy),
                        radius: 2.0,
                        fill: self.pal.faint,
                        stroke: Stroke::NONE,
                    });
                }
                *y += 4.0;
            }
            BlockKind::Details {
                summary,
                open,
                blocks,
            } => self.details(*summary, *open, blocks, b.id, x, w, ctx, out, y),
            BlockKind::FrontMatter(fm) => self.front_matter(fm, b.id, x, w, out, y),
            BlockKind::Footnotes(notes) => self.footnotes(notes, x, w, ctx, out, y),
            BlockKind::Center(blocks) => {
                let mut inner = ctx.clone();
                inner.align = HAlign::Center;
                self.children(blocks, x, w, &inner, out, y);
            }
            BlockKind::Anchor(id) => out.anchors.push((id.clone(), *y, 0.0)),
        }
    }

    fn collect_uris(&self, item: &TextItem, out: &mut Out) {
        for o in &item.objects {
            if let ObjKind::Image { uri } = &o.kind {
                out.uris.push(uri.clone());
            }
        }
        let rt = &self.texts[item.run as usize];
        for o in &rt.objects {
            if let InlineObject::Image(ImageRef { uri: Some(u), .. }) = o
                && !out.uris.contains(u)
            {
                out.uris.push(u.clone());
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn block_image(
        &self,
        img: &ImageRef,
        links: &[Link],
        x: f32,
        w: f32,
        align: HAlign,
        b: &Block,
        out: &mut Out,
        y: &mut f32,
    ) {
        let _ = b;
        let link = img.link.and_then(|i| links.get(i as usize).cloned());
        let state = img.uri.as_ref().map(|u| self.images.get(u));
        if let Some(u) = &img.uri {
            out.uris.push(u.clone());
        }
        let failed = match state {
            None => Some("Unsupported".to_owned()),
            Some(Some(ImgState::Failed(r))) => Some(r.clone()),
            _ => None,
        };
        if let Some(reason) = failed {
            let (size, kind) = self.chip(img, &reason);
            if let ObjKind::Chip { label, tooltip } = kind {
                let left = match align {
                    HAlign::Center => x + (w - size.x) / 2.0,
                    _ => x,
                };
                out.items.push(Item::Chip {
                    rect: Rect::from_min_size(pos2(left, *y), size),
                    label,
                    tooltip,
                });
                *y += size.y;
            }
            return;
        }
        let natural = match state {
            Some(Some(ImgState::Ready(s))) => Some(*s),
            Some(Some(ImgState::Pending(s))) => *s,
            _ => None,
        };
        let size = image_size(img, natural, w);
        let left = x + (w - size.x) / 2.0; // block images are centered
        let rect = Rect::from_min_size(pos2(left, *y), size);
        if let Some(uri) = &img.uri {
            out.items.push(Item::Image {
                rect,
                uri: uri.clone(),
                link,
                src: img.src.clone(),
            });
        }
        *y += size.y;
    }

    fn code(&self, c: &CodeBlock, id: u64, x: f32, w: f32, out: &mut Out, y: &mut f32) {
        let pal = self.pal;
        let top = *y;
        let has_header = c.label.is_some();
        let header_h = if has_header { 32.0 } else { 0.0 };
        let (pad_t, pad_b) = if has_header {
            (2.0, 14.0)
        } else {
            (14.0, 14.0)
        };
        let pad_x = 16.0;
        let code = &self.texts[c.run as usize];
        let (size, line_h) = self.code_base();
        let family = FontFamily::Monospace;
        let (asc, h) = self.metrics(&family, size);
        let shift = ((line_h - h) / 2.0).max(0.0);

        // Highlighting (cached; plain until the worker delivers).
        let lang = c.lang.as_deref().unwrap_or("");
        let hl = if !lang.is_empty() && highlight::supported(lang) {
            let r = highlight::get_or_request(self.ctx, lang, &code.text, self.urgent.get());
            if r.is_none() {
                self.needs_highlight.set(true);
            }
            r
        } else {
            None
        };
        let fmt = |color: Color32| TextFormat {
            font_id: FontId::new(size, family.clone()),
            line_height: Some(line_h),
            color,
            ..Default::default()
        };
        let role_color = |r: Role| match r {
            Role::Plain => pal.text,
            Role::Keyword => pal.syntax.keyword,
            Role::String => pal.syntax.string,
            Role::Number => pal.syntax.number,
            Role::Constant => pal.syntax.constant,
            Role::Function => pal.syntax.function,
            Role::Type => pal.syntax.type_,
            Role::Comment => pal.syntax.comment,
            Role::Operator => pal.syntax.operator,
            Role::DelSign => pal.alert.caution.fg,
        };
        let text = &code.text;
        let no_spans = Vec::new();
        let spans = hl.as_ref().map_or(&no_spans, |h| &h.spans);
        // Galley for a byte range of the code, highlight spans clipped to it. `lead` is the
        // first section's leading space (negative for hanging indents).
        let job_for = |range: Range<usize>, lead: f32, wrap: f32| -> LayoutJob {
            let mut job = LayoutJob {
                break_on_newline: true,
                ..Default::default()
            };
            job.wrap.max_width = wrap;
            let mut pos = range.start;
            let push = |job: &mut LayoutJob, s: usize, e: usize, color: Color32| {
                let first = job.sections.is_empty();
                job.append(&text[s..e], if first { lead } else { 0.0 }, fmt(color));
            };
            let from = spans.partition_point(|(r, _)| r.end <= range.start);
            for (r, role) in &spans[from..] {
                if r.start >= range.end {
                    break;
                }
                let (s, e) = (r.start.max(range.start), r.end.min(range.end));
                if s > pos {
                    push(&mut job, pos, s, pal.text);
                }
                push(&mut job, s, e, role_color(*role));
                pos = e;
            }
            if pos < range.end {
                push(&mut job, pos, range.end, pal.text);
            }
            if job.sections.is_empty() {
                job.sections.push(LayoutSection {
                    leading_space: lead,
                    byte_range: ByteIndex(0)..ByteIndex(0),
                    format: fmt(pal.text),
                });
            }
            job
        };
        let build = |range: Range<usize>, lead: f32, wrap: f32| -> Arc<Galley> {
            self.galley(job_for(range, lead, wrap))
        };
        let inner_w = (w - 2.0 * pad_x).max(10.0);
        let code_top = top + header_h + pad_t;
        // (galley, x offset, y, char base, row starts, lazy chunk)
        type Piece = (
            Arc<Galley>,
            f32,
            f32,
            usize,
            Option<Arc<[usize]>>,
            Option<Arc<LazyGalley>>,
        );
        let mut pieces: Vec<Piece> = Vec::new();
        let n_lines = text.bytes().filter(|&b| b == b'\n').count() + 1;
        // Huge blocks are shaped lazily (see LAZY_CODE_LINES): their geometry comes from glyph
        // advances (monospace), so nothing is shaped for lines off screen.
        let lazy = n_lines > LAZY_CODE_LINES;
        let placeholder = lazy.then(|| build(0..0, 0.0, f32::INFINITY));
        let font_id = FontId::new(size, family.clone());
        let ascii_w: Vec<f32> = if lazy {
            self.ctx.fonts_mut(|f| {
                (0u8..128)
                    .map(|b| f.glyph_width(&font_id, b as char))
                    .collect()
            })
        } else {
            Vec::new()
        };
        let mut other_w: HashMap<char, f32> = HashMap::new();
        let mut line_w = |l: &str| -> f32 {
            if l.is_ascii() {
                return l.bytes().map(|b| ascii_w[b as usize]).sum();
            }
            self.ctx.fonts_mut(|f| {
                l.chars()
                    .map(|c| match c.is_ascii() {
                        true => ascii_w[c as usize],
                        false => *other_w
                            .entry(c)
                            .or_insert_with(|| f.glyph_width(&font_id, c)),
                    })
                    .sum()
            })
        };
        if self.wrap_code {
            // One galley per source line; continuation rows are indented to the line's
            // leading whitespace + 2ch (the first row starts that far to the left).
            let char_w = self.ctx.fonts_mut(|f| f.glyph_width(&font_id, ' '));
            let (mut yy, mut base, mut byte) = (code_top, 0usize, 0usize);
            for line in text.split('\n') {
                let cols: usize = line
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .map(|c| if c == '\t' { 4 } else { 1 })
                    .sum();
                let indent = ((cols + 2) as f32 * char_w).min(inner_w * 0.5);
                let wrap = (inner_w - indent).max(40.0);
                let range = byte..byte + line.len();
                let n_chars = line.chars().count();
                let h = if let Some(ph) = &placeholder {
                    let job = job_for(range, -indent, wrap);
                    let est = line_w(line);
                    let (rect, starts) = if est - indent <= wrap - 1.0 {
                        // (The first row starts `indent` to the left; egui's galley rect also
                        // spans the origin.)
                        let r =
                            Rect::from_min_max(Pos2::ZERO, pos2((est - indent).max(0.0), line_h));
                        (r, vec![0, n_chars])
                    } else {
                        // It wraps: shape it once to count its rows, without keeping the glyphs
                        // (egui's cache would hold every line until the end of the frame).
                        let job = Arc::new(job.clone());
                        let g = self
                            .ctx
                            .fonts_mut(|f| egui::epaint::text::layout(f.fonts, self.ppp, job));
                        (g.rect, row_starts(&g))
                    };
                    let lz = LazyGalley { job, rect };
                    pieces.push((
                        ph.clone(),
                        indent,
                        yy,
                        base,
                        Some(starts.into()),
                        Some(Arc::new(lz)),
                    ));
                    rect.height().max(line_h)
                } else {
                    let g = build(range, -indent, wrap);
                    let h = g.rect.height().max(line_h);
                    pieces.push((g, indent, yy, base, None, None));
                    h
                };
                yy += h;
                base += n_chars + 1;
                byte += line.len() + 1;
            }
        } else if let Some(ph) = &placeholder {
            // One row per line, each `line_h` tall: chunk geometry is known without shaping.
            let lines: Vec<&str> = text.split('\n').collect();
            let (mut yy, mut base, mut byte) = (code_top, 0usize, 0usize);
            for chunk in lines.chunks(CODE_CHUNK_LINES) {
                let bytes = chunk.iter().map(|l| l.len() + 1).sum::<usize>() - 1;
                let mut starts = Vec::with_capacity(chunk.len() + 1);
                let (mut chars, mut width) = (0usize, 0.0f32);
                for (k, l) in chunk.iter().enumerate() {
                    starts.push(chars);
                    chars += l.chars().count() + usize::from(k + 1 < chunk.len());
                    width = width.max(line_w(l));
                }
                starts.push(chars);
                let h = chunk.len() as f32 * line_h;
                let lz = LazyGalley {
                    job: job_for(byte..byte + bytes, 0.0, f32::INFINITY),
                    // A little slack: shaping can round differently from summed advances.
                    rect: Rect::from_min_size(Pos2::ZERO, vec2(width.ceil() + 2.0, h)),
                };
                pieces.push((
                    ph.clone(),
                    0.0,
                    yy,
                    base,
                    Some(starts.into()),
                    Some(Arc::new(lz)),
                ));
                yy += h;
                base += chars + 1;
                byte += bytes + 1;
            }
        } else {
            pieces.push((
                build(0..text.len(), 0.0, f32::INFINITY),
                0.0,
                code_top,
                0,
                None,
                None,
            ));
        }
        let piece_size = |(g, .., lazy): &Piece| lazy.as_ref().map_or(g.rect, |l| l.rect).size();
        let code_h = pieces
            .last()
            .map_or(line_h, |p| p.2 + piece_size(p).y.max(line_h) - code_top);
        let total_h = header_h + pad_t + code_h + pad_b;
        let frame = Rect::from_min_size(pos2(x, top), vec2(w, total_h));
        out.items.push(Item::Fill {
            rect: frame,
            color: pal.code_bg,
            radius: CornerRadius::same(8),
        });

        if let Some(label) = &c.label {
            let g = self.label(label, self.em(12.0), 500, pal.muted, 0.0, w - 80.0);
            let ly = top + (header_h - g.size().y) / 2.0;
            let lw = g.size().x;
            out.items.push(Item::Label {
                pos: pos2(x + 16.0, ly),
                galley: g,
            });
            if let Some(note) = &c.label_note {
                let g2 = self.label(note, self.em(12.0), 400, pal.muted, 0.0, w - 80.0 - lw);
                out.items.push(Item::Label {
                    pos: pos2(x + 16.0 + lw + 5.0, ly),
                    galley: g2,
                });
            }
        }
        // Scrollable code area.
        let area = Rect::from_min_max(
            pos2(x + 1.0, top + header_h),
            pos2(x + w - 1.0, top + total_h - 1.0),
        );
        let widest = pieces.iter().map(|p| piece_size(p).x).fold(0.0, f32::max);
        let content_w = if self.wrap_code {
            area.width()
        } else {
            (widest + 2.0 * pad_x).max(area.width())
        };
        let mut items = Vec::new();
        if let Some(h) = &hl {
            for (line, kind) in &h.line_bg {
                let line = *line as usize;
                // (y top, y bottom) of the source line: unwrapped, every line is one row.
                let span = if self.wrap_code {
                    pieces
                        .get(line)
                        .map(|(g, _, yy, ..)| (*yy, yy + g.rect.height().max(line_h)))
                } else {
                    let y0 = code_top + line as f32 * line_h;
                    Some((y0, y0 + line_h))
                };
                let Some((y0, y1)) = span else { continue };
                let color = match kind {
                    LineBg::Add => pal.syntax.diff_add_bg,
                    LineBg::Del => pal.syntax.diff_del_bg,
                };
                items.push(Item::Fill {
                    rect: Rect::from_min_max(pos2(-1.0, y0), pos2(content_w + 1.0, y1)),
                    color,
                    radius: CornerRadius::ZERO,
                });
            }
        }
        for (galley, indent, yy, base, starts, lazy) in pieces {
            let starts = starts.unwrap_or_else(|| row_starts(&galley).into());
            items.push(Item::Text(TextItem {
                pos: pos2(pad_x - 1.0 + indent, yy),
                galley,
                shift,
                asc,
                line_h,
                run: c.run,
                char_base: base,
                starts,
                lazy,
                decos: Vec::new(),
                links: Vec::new(),
                objects: Vec::new(),
            }));
        }
        out.items.push(Item::Scroll(ScrollItem {
            id,
            frame: area,
            content_w,
            items,
            fade: pal.code_bg,
            fade_bands: Vec::new(),
        }));
        out.items.push(Item::Frame {
            rect: frame,
            stroke: Stroke::new(1.0, pal.code_border),
            radius: CornerRadius::same(8),
        });

        // Copy button: in the header, or floating top-right on hover.
        let btn = if has_header {
            Rect::from_center_size(
                pos2(x + w - 8.0 - 14.0, top + header_h / 2.0),
                vec2(28.0, 24.0),
            )
        } else {
            Rect::from_min_size(pos2(x + w - 8.0 - 28.0, top + 8.0), vec2(28.0, 24.0))
        };
        out.items.push(Item::Button(ButtonItem {
            rect: btn,
            kind: ButtonKind::CopyCode {
                run: c.run,
                floating: !has_header,
            },
            reveal: frame,
        }));
        *y = top + total_h;
    }

    fn list(&self, l: &List, x: f32, w: f32, ctx: &Ctx, out: &mut Out, y: &mut f32) {
        let pal = self.pal;
        let level = ctx.list_depth.saturating_add(1);
        let base = self.body(ctx);
        let num_size = base.size;
        let num_family = self.family_for(&base, 500, false, true);
        let numbers: Vec<String> = (0..l.items.len())
            .map(|i| format!("{}.", l.start + i as u64))
            .collect();
        let num_galleys: Vec<Arc<Galley>> = if l.ordered {
            numbers
                .iter()
                .map(|n| {
                    self.galley(LayoutJob::simple_singleline(
                        n.clone(),
                        FontId::new(num_size, num_family.clone()),
                        pal.muted,
                    ))
                })
                .collect()
        } else {
            Vec::new()
        };
        let box_w = if l.ordered {
            let widest = num_galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
            (widest + 6.0).max(self.em(26.0))
        } else if level >= 5 {
            self.em(18.0)
        } else {
            self.em(26.0)
        };
        // Too deep to indent further: the items' text continues at this level's left edge,
        // without markers (they would overlap the text).
        let markers = self.can_indent(w, box_w);
        let box_w = if markers { box_w } else { 0.0 };
        // Bullets and checkboxes sit 16 px before the text (centered at x = 10 in a 26 px box,
        // and still 16 px before the text when deep levels indent only 18 px).
        let marker_x = if l.ordered || !markers {
            x + self.em(10.0)
        } else {
            x + box_w - self.em(16.0)
        };
        let item_gap = if l.tight { self.em(4.0) } else { self.em(12.0) };
        for (i, it) in l.items.iter().enumerate() {
            if i > 0 {
                *y += item_gap;
            }
            let item_top = *y;
            let mut inner = ctx.clone();
            inner.list_depth = level;
            inner.in_item = true;
            inner.tight = l.tight;
            if it.task == Some(true) {
                inner.color = pal.muted;
            }
            let first_item_index = out.items.len();
            let cx = x + box_w;
            let cw = (w - box_w).max(20.0);
            // Lay out children one by one so we can draw indent guides for nested lists.
            let mut prev: Option<&BlockKind> = None;
            let mut first_line: Option<(f32, f32, f32)> = None; // (baseline, row top, row bottom)
            for b in &it.blocks {
                if let Some(p) = prev {
                    let mut g = gap(p, &b.kind, self.t).min(self.em(12.0));
                    if l.tight && matches!(b.kind, BlockKind::List(_)) {
                        g = self.em(4.0);
                    }
                    *y += g;
                }
                let before = out.items.len();
                let start_y = *y;
                self.block(b, cx, cw, &inner, out, y);
                if first_line.is_none() {
                    first_line = first_text_line(&out.items[before..]).or(Some((
                        start_y + base.line_h * 0.72,
                        start_y,
                        start_y + base.line_h,
                    )));
                }
                if markers
                    && matches!(b.kind, BlockKind::List(_))
                    && let Some((_, _, fl_bottom)) = first_line
                {
                    out.items.push(Item::Fill {
                        rect: Rect::from_min_max(
                            pos2(marker_x - 0.5, fl_bottom + 4.0),
                            pos2(marker_x + 0.5, *y),
                        ),
                        color: pal.border_strong,
                        radius: CornerRadius::ZERO,
                    });
                }
                prev = Some(&b.kind);
            }
            if it.blocks.is_empty() {
                *y += base.line_h;
            }
            let (baseline, row_top, row_bottom) = first_line.unwrap_or((
                item_top + base.line_h * 0.72,
                item_top,
                item_top + base.line_h,
            ));
            let _ = first_item_index;
            let xh = base.size * if base.serif { 0.507 } else { 0.546 };
            let mid = baseline - xh / 2.0;
            if !markers {
                continue;
            }
            if let Some(checked) = it.task {
                // Centered on the x-height, kept within the first row, which can be shorter
                // than the box (a superscript-only line): then it hangs from the row's top.
                let s = self.em(15.0);
                let lo = row_top + s / 2.0;
                let hi = (row_bottom - s / 2.0).max(lo);
                let rect =
                    Rect::from_center_size(pos2(marker_x, mid.clamp(lo, hi)), Vec2::splat(s));
                out.items.push(Item::Checkbox { rect, checked });
            } else if l.ordered {
                let g = num_galleys[i].clone();
                // Right-aligned, sharing the first line's baseline.
                let (asc, _) = self.metrics(&num_family, num_size);
                let pos = pos2(x + box_w - 6.0 - g.size().x, baseline - asc);
                out.items.push(Item::Label { pos, galley: g });
            } else {
                let center = pos2(marker_x, mid);
                let style = (level - 1) % 3;
                let color = if level <= 2 { pal.accent } else { pal.muted };
                match style {
                    0 => out.items.push(Item::Circle {
                        center,
                        radius: 3.0,
                        fill: color,
                        stroke: Stroke::NONE,
                    }),
                    1 => out.items.push(Item::Circle {
                        center,
                        radius: 2.25,
                        fill: Color32::TRANSPARENT,
                        stroke: Stroke::new(1.5, color),
                    }),
                    _ => out.items.push(Item::Fill {
                        rect: Rect::from_center_size(center, vec2(6.0, 1.5)),
                        color,
                        radius: CornerRadius::ZERO,
                    }),
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn alert(
        &self,
        kind: AlertKind,
        blocks: &[Block],
        x: f32,
        w: f32,
        ctx: &Ctx,
        out: &mut Out,
        y: &mut f32,
    ) {
        let pal = self.pal;
        let colors: AlertColors = match kind {
            AlertKind::Note => pal.alert.note,
            AlertKind::Tip => pal.alert.tip,
            AlertKind::Important => pal.alert.important,
            AlertKind::Warning => pal.alert.warning,
            AlertKind::Caution => pal.alert.caution,
        };
        let top = *y;
        let fill_index = out.items.len();
        out.items.push(Item::Fill {
            rect: Rect::NOTHING,
            color: colors.tint,
            radius: CornerRadius::same(8),
        });
        let pad_x = 16.0;
        let pad_y = 12.0;
        let title_h = 22.0_f32.max(self.em(22.0));
        let icon = match kind {
            AlertKind::Note => Icon::Info,
            AlertKind::Tip => Icon::Lightbulb,
            AlertKind::Important => Icon::MessageSquareWarning,
            AlertKind::Warning => Icon::TriangleAlert,
            AlertKind::Caution => Icon::OctagonAlert,
        };
        let row_top = top + pad_y;
        let icon_s = self.em(16.0);
        out.items.push(Item::Icon {
            icon,
            rect: Rect::from_min_size(
                pos2(x + pad_x, row_top + (title_h - icon_s) / 2.0),
                Vec2::splat(icon_s),
            ),
            color: colors.fg,
        });
        let g = self.label(kind.title(), self.em(14.0), 650, colors.fg, 0.0, w);
        out.items.push(Item::Label {
            pos: pos2(
                x + pad_x + icon_s + 8.0,
                row_top + (title_h - g.size().y) / 2.0,
            ),
            galley: g,
        });
        *y = row_top + title_h + 6.0;
        let mut inner = ctx.clone();
        inner.color = pal.text;
        inner.in_item = false;
        let pad = if self.can_indent(w, 2.0 * pad_x + 2.0) {
            pad_x + 1.0
        } else {
            0.0
        };
        self.children(blocks, x + pad, w - 2.0 * pad, &inner, out, y);
        *y += pad_y;
        let rect = Rect::from_min_max(pos2(x, top), pos2(x + w, *y));
        out.items[fill_index] = Item::Fill {
            rect,
            color: colors.tint,
            radius: CornerRadius::same(8),
        };
        out.items.push(Item::Frame {
            rect,
            stroke: Stroke::new(1.0, colors.border),
            radius: CornerRadius::same(8),
        });
    }

    fn table(&self, t: &Table, id: u64, x: f32, w: f32, out: &mut Out, y: &mut f32) {
        let pal = self.pal;
        let ncols = t.aligns.len().max(t.header.len()).max(1);
        let pad_x = self.em(12.0);
        let pad_y = self.em(8.0);
        let hb = self.cell_base(true);
        let bb = self.cell_base(false);
        // Measure natural and longest-word widths. A word counts with the space after it: egui
        // only breaks after a space that fits on the row, so a column exactly as wide as its
        // longest word would push that space onto a row of its own (a blank line, then the
        // next word indented by a space).
        let mut min_w = vec![0.0f32; ncols];
        let mut max_w = vec![0.0f32; ncols];
        let measure =
            |run: RunId, base: &TextBase, col: usize, min_w: &mut [f32], max_w: &mut [f32]| {
                let item = self.rich(run, base, 0.0, 0.0, f32::INFINITY, HAlign::Left);
                let g = &item.galley;
                let natural = g.rect.width();
                let mut longest = 0.0f32;
                for row in &g.rows {
                    let mut start: Option<f32> = None;
                    for gl in &row.glyphs {
                        if gl.chr.is_whitespace() {
                            if let Some(s) = start.take() {
                                longest = longest.max(gl.max_x() - s);
                            }
                        } else if start.is_none() {
                            start = Some(gl.pos.x);
                        }
                    }
                    if let (Some(s), Some(last)) = (start, row.glyphs.last()) {
                        longest = longest.max(last.max_x() - s);
                    }
                }
                max_w[col] = max_w[col].max(natural.min(self.em(420.0)));
                // Rounded up so float noise in the wrapped layout can't break the word early.
                min_w[col] = min_w[col].max((longest.ceil() + 1.0).min(self.em(240.0)));
            };
        for (c, &run) in t.header.iter().enumerate().take(ncols) {
            measure(run, &hb, c, &mut min_w, &mut max_w);
        }
        for row in &t.rows {
            for (c, &run) in row.iter().enumerate().take(ncols) {
                measure(run, &bb, c, &mut min_w, &mut max_w);
            }
        }
        for c in 0..ncols {
            min_w[c] += 2.0 * pad_x;
            max_w[c] = max_w[c].max(min_w[c] - 2.0 * pad_x) + 2.0 * pad_x;
        }
        let avail = w - 2.0;
        let sum_min: f32 = min_w.iter().sum();
        let sum_max: f32 = max_w.iter().sum();
        let widths: Vec<f32> = if sum_max <= avail {
            max_w.clone()
        } else if sum_min <= avail && sum_max > sum_min {
            let k = (avail - sum_min) / (sum_max - sum_min);
            (0..ncols)
                .map(|c| min_w[c] + (max_w[c] - min_w[c]) * k)
                .collect()
        } else {
            min_w.clone()
        };
        let table_w: f32 = widths.iter().sum();
        let top = *y;
        let mut items = Vec::new();
        let mut row_y = top + 1.0;
        let all_rows: Vec<(&Vec<RunId>, bool)> = std::iter::once((&t.header, true))
            .chain(t.rows.iter().map(|r| (r, false)))
            .collect();
        let nrows = all_rows.len();
        let mut row_rects = Vec::new();
        let mut cell_items = Vec::new();
        for (ri, (cells, header)) in all_rows.iter().enumerate() {
            let base = if *header { &hb } else { &bb };
            let mut cx = 0.0;
            let mut row_h = 0.0f32;
            let mut texts = Vec::new();
            for (c, &cw) in widths.iter().enumerate() {
                if let Some(&run) = cells.get(c) {
                    let align = if t.numeric.get(c).copied().unwrap_or(false) {
                        HAlign::Right
                    } else {
                        t.aligns.get(c).copied().unwrap_or_default()
                    };
                    let item = self.rich(
                        run,
                        base,
                        cx + pad_x,
                        row_y + pad_y,
                        cw - 2.0 * pad_x,
                        align,
                    );
                    row_h = row_h.max(item.galley.rect.height());
                    texts.push(item);
                }
                cx += cw;
            }
            let h = row_h.max(base.line_h) + 2.0 * pad_y;
            row_rects.push((
                Rect::from_min_size(pos2(0.0, row_y), vec2(table_w, h)),
                *header,
                ri,
            ));
            cell_items.extend(texts.into_iter().map(Item::Text));
            row_y += h;
        }
        let total_h = row_y + 1.0 - top;
        let content_w = table_w.max(w - 2.0);
        let r8 = 7u8;
        let mut fade_bands = Vec::new();
        for (rect, header, ri) in &row_rects {
            let is_last = *ri + 1 == nrows;
            let fill = if *header {
                Some(pal.table_head)
            } else if ri % 2 == 0 {
                Some(pal.table_zebra)
            } else {
                None
            };
            fade_bands.push((rect.top(), rect.bottom() - 1.0, fill.unwrap_or(pal.bg)));
            let rect =
                Rect::from_min_max(rect.min, pos2(content_w.max(rect.right()), rect.bottom()));
            if let Some(color) = fill {
                let radius = CornerRadius {
                    nw: if *header { r8 } else { 0 },
                    ne: if *header { r8 } else { 0 },
                    sw: if is_last { r8 } else { 0 },
                    se: if is_last { r8 } else { 0 },
                };
                items.push(Item::Fill {
                    rect,
                    color,
                    radius,
                });
            }
            if !is_last {
                let line_color = if *header {
                    pal.border_strong
                } else {
                    pal.border
                };
                items.push(Item::Fill {
                    rect: Rect::from_min_max(
                        pos2(rect.left(), rect.bottom() - 1.0),
                        pos2(rect.right(), rect.bottom()),
                    ),
                    color: line_color,
                    radius: CornerRadius::ZERO,
                });
            }
        }
        items.extend(cell_items);
        let frame = Rect::from_min_size(pos2(x, top), vec2(w.min(table_w + 2.0), total_h));
        let frame = if table_w + 2.0 > w {
            Rect::from_min_size(pos2(x, top), vec2(w, total_h))
        } else {
            frame
        };
        let area = frame.shrink(1.0);
        out.items.push(Item::Scroll(ScrollItem {
            id,
            frame: area,
            content_w: table_w,
            items,
            fade: pal.bg,
            fade_bands,
        }));
        out.items.push(Item::Frame {
            rect: frame,
            stroke: Stroke::new(1.0, pal.border),
            radius: CornerRadius::same(8),
        });
        *y = top + total_h;
    }

    #[allow(clippy::too_many_arguments)]
    fn details(
        &self,
        summary: RunId,
        open_default: bool,
        blocks: &[Block],
        id: u64,
        x: f32,
        w: f32,
        ctx: &Ctx,
        out: &mut Out,
        y: &mut f32,
    ) {
        let pal = self.pal;
        let open = self.is_open(id, open_default);
        let top = *y;
        let pad_x = 12.0;
        let pad_y = 8.0;
        let mut base = self.body(ctx);
        base.weight = 600;
        base.strong_weight = 700;
        let chev = 12.0;
        let text_x = x + pad_x + chev + 8.0;
        let item = self.rich(
            summary,
            &base,
            text_x,
            top + pad_y,
            w - (text_x - x) - pad_x,
            HAlign::Left,
        );
        let row_h = item.galley.rect.height();
        let (bl, _) = {
            let row = &item.galley.rows[0];
            (item.pos.y + item.baseline(row.pos.y, row.size.y), 0)
        };
        let xh = base.size * 0.546;
        out.items.push(Item::Chevron {
            rect: Rect::from_center_size(
                pos2(x + pad_x + chev / 2.0, bl - xh / 2.0),
                Vec2::splat(chev),
            ),
            open,
            color: pal.muted,
        });
        let summary_rect = Rect::from_min_max(pos2(x, top), pos2(x + w, top + pad_y * 2.0 + row_h));
        out.items.push(Item::Text(item));
        *y = top + pad_y + row_h;
        if open {
            *y += self.em(10.0);
            let mut inner = ctx.clone();
            inner.in_item = false;
            let pad = if self.can_indent(w, 2.0 * pad_x + 2.0) {
                pad_x + 1.0
            } else {
                0.0
            };
            self.children(blocks, x + pad, w - 2.0 * pad, &inner, out, y);
            *y += self.em(4.0);
        }
        *y += pad_y;
        let rect = Rect::from_min_max(pos2(x, top), pos2(x + w, *y));
        out.items.push(Item::Frame {
            rect,
            stroke: Stroke::new(1.0, pal.border),
            radius: CornerRadius::same(8),
        });
        out.items.push(Item::Button(ButtonItem {
            rect: summary_rect,
            kind: ButtonKind::Toggle { id },
            reveal: summary_rect,
        }));
    }

    fn front_matter(&self, fm: &FrontMatter, id: u64, x: f32, w: f32, out: &mut Out, y: &mut f32) {
        let pal = self.pal;
        let open = self.is_open(id, false);
        let top = *y;
        let pad_x = 12.0;
        let pad_y = 8.0;
        let row_h = self.em(20.0);
        let chev = 12.0;
        let cy = top + pad_y + row_h / 2.0;
        out.items.push(Item::Chevron {
            rect: Rect::from_center_size(pos2(x + pad_x + chev / 2.0, cy), Vec2::splat(chev)),
            open,
            color: pal.muted,
        });
        let over = self.label(
            "FRONT MATTER",
            self.em(11.0),
            600,
            pal.muted,
            self.em(11.0) * 0.08,
            w,
        );
        let ox = x + pad_x + chev + 8.0;
        let ow = over.size().x;
        out.items.push(Item::Label {
            pos: pos2(ox, cy - over.size().y / 2.0),
            galley: over,
        });
        let px = ox + ow + 12.0;
        let preview = self.label(
            &fm.preview,
            self.em(13.0),
            400,
            pal.muted,
            0.0,
            (x + w - pad_x - px).max(10.0),
        );
        out.items.push(Item::Label {
            pos: pos2(px, cy - preview.size().y / 2.0),
            galley: preview,
        });
        let row_rect = Rect::from_min_max(pos2(x, top), pos2(x + w, top + pad_y * 2.0 + row_h));
        *y = top + pad_y + row_h;
        if open {
            *y += self.em(8.0);
            let lang = if fm.toml { "toml" } else { "yaml" };
            let c = CodeBlock {
                lang: Some(lang.into()),
                label: None,
                label_note: None,
                kind: CodeKind::Normal,
                run: fm.run,
            };
            let mut tmp = Out::default();
            let mut yy = *y;
            self.code(
                &c,
                id ^ 0x5eed,
                x + pad_x,
                w - 2.0 * pad_x,
                &mut tmp,
                &mut yy,
            );
            // No copy button inside front matter.
            out.items.extend(
                tmp.items
                    .into_iter()
                    .filter(|i| !matches!(i, Item::Button(_))),
            );
            *y = yy + self.em(4.0);
        }
        *y += pad_y;
        let rect = Rect::from_min_max(pos2(x, top), pos2(x + w, *y));
        out.items.push(Item::Frame {
            rect,
            stroke: Stroke::new(1.0, pal.border),
            radius: CornerRadius::same(8),
        });
        out.items.push(Item::Button(ButtonItem {
            rect: row_rect,
            kind: ButtonKind::Toggle { id },
            reveal: row_rect,
        }));
    }

    fn footnotes(&self, notes: &[Footnote], x: f32, w: f32, ctx: &Ctx, out: &mut Out, y: &mut f32) {
        let pal = self.pal;
        out.items.push(Item::Fill {
            rect: Rect::from_min_size(pos2(x, *y), vec2(w, 1.0)),
            color: pal.border,
            radius: CornerRadius::ZERO,
        });
        *y += 1.0 + self.em(16.0);
        let over = self.label(
            "FOOTNOTES",
            self.em(11.0),
            600,
            pal.muted,
            self.em(11.0) * 0.08,
            w,
        );
        let oh = over.size().y;
        out.items.push(Item::Label {
            pos: pos2(x, *y),
            galley: over,
        });
        *y += oh + self.em(10.0);
        let mut inner = ctx.clone();
        inner.small = true;
        inner.in_item = true;
        inner.tight = true;
        let base = self.body(&inner);
        let num_family = self.family_for(&base, 500, false, true);
        let nums: Vec<Arc<Galley>> = notes
            .iter()
            .map(|n| {
                self.galley(LayoutJob::simple_singleline(
                    format!("{}.", n.number),
                    FontId::new(base.size, num_family.clone()),
                    pal.muted,
                ))
            })
            .collect();
        let box_w = (nums.iter().map(|g| g.size().x).fold(0.0, f32::max) + 6.0).max(self.em(26.0));
        for (i, n) in notes.iter().enumerate() {
            if i > 0 {
                *y += self.em(6.0);
            }
            let top = *y;
            let before = out.items.len();
            self.children(&n.blocks, x + box_w, w - box_w, &inner, out, y);
            let (asc, _) = self.metrics(&num_family, base.size);
            let baseline = first_text_line(&out.items[before..])
                .map(|f| f.0)
                .unwrap_or(top + base.line_h * 0.72);
            let g = nums[i].clone();
            out.items.push(Item::Label {
                pos: pos2(x + box_w - 6.0 - g.size().x, baseline - asc),
                galley: g,
            });
            out.anchors.push((format!("fn-{}", n.name), top, *y - top));
        }
    }
}

/// (baseline, row top, row bottom) of the first text line among `items` (block coords).
fn first_text_line(items: &[Item]) -> Option<(f32, f32, f32)> {
    for it in items {
        match it {
            Item::Text(t) => {
                let row = t.galley.rows.first()?;
                let bl = t.pos.y + t.baseline(row.pos.y, row.size.y);
                let top = t.pos.y + row.pos.y;
                return Some((bl, top, top + row.size.y));
            }
            Item::Scroll(s) => {
                // Code block or table: center on its first line.
                for inner in &s.items {
                    if let Item::Text(t) = inner {
                        let row = t.galley.rows.first()?;
                        let bl = t.pos.y + t.baseline(row.pos.y, row.size.y);
                        return Some((bl, t.pos.y + row.pos.y, t.pos.y + row.pos.y + row.size.y));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{self, Parsed};

    /// Run `f` with the parsed `src` and a layout environment at text size `t`.
    fn with_env<R>(src: &str, t: f32, f: impl FnOnce(&Parsed, &Env) -> R) -> (Parsed, R) {
        with_env_wrap(src, t, false, f)
    }

    fn with_env_wrap<R>(
        src: &str,
        t: f32,
        wrap_code: bool,
        f: impl FnOnce(&Parsed, &Env) -> R,
    ) -> (Parsed, R) {
        let p = parse::parse(src, None);
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        let pal = Palette::light();
        let (images, toggled) = (HashMap::new(), HashSet::new());
        let env = Env::new(
            &ctx,
            &pal,
            t,
            false,
            wrap_code,
            &p.texts,
            &p.headings,
            &images,
            &toggled,
        );
        let r = f(&p, &env);
        drop(env);
        (p, r)
    }

    /// Lay out every top-level block of `src` at text size `t` and column width `w`.
    fn layout_doc(src: &str, t: f32, w: f32) -> (Parsed, Vec<LBlock>) {
        with_env(src, t, |p, env| {
            p.blocks.iter().map(|b| layout_top(env, b, w)).collect()
        })
    }

    fn galley_text(b: &LBlock) -> String {
        let mut s = String::new();
        for it in &b.items {
            if let Item::Text(t) = it {
                s.push_str(&t.galley.job.text);
            }
        }
        s
    }

    #[test]
    fn footnote_card_leaves_out_the_back_link() {
        for src in [
            "See[^a].\n\n[^a]: The note text.\n",
            "See[^a].\n\n[^a]:\n    ```\n    code\n    ```\n",
        ] {
            let (_, (card, section)) = with_env(src, 16.0, |p, env| {
                let Some(BlockKind::Footnotes(notes)) = p
                    .blocks
                    .iter()
                    .map(|b| &b.kind)
                    .find(|k| matches!(k, BlockKind::Footnotes(_)))
                else {
                    panic!("no notes")
                };
                let card = layout_note(env, &notes[0].blocks, 300.0);
                let section = layout_top(env, p.blocks.last().unwrap(), 600.0);
                (card, section)
            });
            assert!(galley_text(&section).contains('↩'), "{src}");
            assert!(!galley_text(&card).contains('↩'), "{src}");
            assert!(card.height > 0.0);
        }
    }

    fn text_items(b: &LBlock) -> Vec<&TextItem> {
        b.items
            .iter()
            .filter_map(|it| match it {
                Item::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    /// Galley x range of char `c` (row 0).
    fn glyph_x(t: &TextItem, c: usize) -> (f32, f32) {
        let row = &t.galley.rows[0];
        let g = &row.glyphs[c];
        (row.pos.x + g.pos.x, row.pos.x + g.max_x())
    }

    #[test]
    fn code_chips_and_keycaps_are_centered_on_their_text() {
        for (src, kind, pad) in [
            ("a `xy` b", DecoKind::CodeBg, 5.0),
            ("`xy` b", DecoKind::CodeBg, 5.0),
            ("a <kbd>xy</kbd> b", DecoKind::Kbd, 7.0),
        ] {
            for t in [14.0, 16.0, 20.0] {
                let (p, blocks) = layout_doc(src, t, 600.0);
                let item = text_items(&blocks[0])[0];
                let decos: Vec<_> = item.decos.iter().filter(|d| d.kind == kind).collect();
                assert_eq!(decos.len(), 1, "{src}: one box per element");
                let r = decos[0].rect;
                let plain: Vec<char> = p.texts[0].text.chars().collect();
                let first = plain.iter().position(|&c| c == 'x').unwrap();
                let (x0, _) = glyph_x(item, first);
                let (_, x1) = glyph_x(item, first + 1);
                let (left, right) = (x0 - r.left(), r.right() - x1);
                let want = pad * t / 16.0;
                assert!(
                    (left - want).abs() < 1.0 && (right - want).abs() < 1.0,
                    "{src} at {t}: padding {left} left, {right} right, want {want}"
                );
            }
        }
        // Adjacent keys stay separate keycaps.
        let (_, blocks) = layout_doc("<kbd>a</kbd><kbd>b</kbd>", 16.0, 600.0);
        let n = text_items(&blocks[0])[0]
            .decos
            .iter()
            .filter(|d| d.kind == DecoKind::Kbd)
            .count();
        assert_eq!(n, 2);
    }

    #[test]
    fn narrow_table_cells_wrap_between_words() {
        let words = [
            "alpha value long",
            "beta value",
            "gamma value",
            "delta",
            "epsilon value long",
            "zeta x",
            "eta value",
            "theta",
            "iota value",
            "kappa value long",
            "lambda v",
            "mu value",
        ];
        let header: Vec<String> = (1..=12).map(|i| format!("Column {i}")).collect();
        let src = format!(
            "| {} |\n|{}\n| {} |\n",
            header.join(" | "),
            "---|".repeat(12),
            words.join(" | ")
        );
        for (t, w) in [(16.0, 480.0), (16.0, 736.0), (14.0, 520.0), (20.0, 600.0)] {
            let (_, blocks) = layout_doc(&src, t, w);
            let Some(Item::Scroll(table)) = blocks[0]
                .items
                .iter()
                .find(|i| matches!(i, Item::Scroll(_)))
            else {
                panic!("no table")
            };
            let mut cells = 0;
            for it in &table.items {
                let Item::Text(item) = it else { continue };
                cells += 1;
                for row in &item.galley.rows {
                    let text: String = row.glyphs.iter().map(|g| g.chr).collect();
                    assert!(
                        !text.trim().is_empty() && !text.starts_with(char::is_whitespace),
                        "{t}/{w}: row {text:?} of {:?}",
                        item.galley.job.text
                    );
                }
            }
            assert_eq!(cells, 24);
        }
    }

    #[test]
    fn task_items_with_short_first_rows_lay_out_at_any_size() {
        for src in [
            "- [ ] <sup>1</sup>",
            "1. [x] <sup>1</sup>",
            "- [ ] <sup>1</sup><sup>2</sup>",
            "- [ ] **<sup>1</sup>**",
            "- [ ] <sub>1</sub>",
        ] {
            for t in 11..=28 {
                let (_, blocks) = layout_doc(src, t as f32, 600.0);
                let checkbox = blocks[0]
                    .items
                    .iter()
                    .find_map(|i| match i {
                        Item::Checkbox { rect, .. } => Some(*rect),
                        _ => None,
                    })
                    .unwrap();
                assert!(
                    checkbox.is_finite() && checkbox.width() > 0.0,
                    "{src} at {t}"
                );
            }
        }
    }

    #[test]
    fn deep_bullets_keep_their_distance_from_the_text() {
        let src: String = (0..7)
            .map(|i| format!("{}- level {}\n", "  ".repeat(i), i + 1))
            .collect();
        let (_, blocks) = layout_doc(&src, 16.0, 600.0);
        let mut texts = text_items(&blocks[0]);
        texts.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
        let mut markers: Vec<Pos2> = blocks[0]
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Circle { center, .. } => Some(*center),
                Item::Fill { rect, .. } if rect.height() < 2.0 => Some(rect.center()),
                _ => None,
            })
            .collect();
        markers.sort_by(|a, b| a.y.total_cmp(&b.y));
        assert_eq!(markers.len(), 7);
        for (m, t) in markers.iter().zip(&texts) {
            let gap = t.pos.x - m.x;
            assert!(
                (gap - 16.0).abs() < 0.5,
                "marker {m:?}, text at {}",
                t.pos.x
            );
        }
    }

    #[test]
    fn segments_start_at_the_right_row() {
        let src = "Lorem ipsum dolor sit amet, `code` consectetur adipiscing elit.  \nSed do \
                   eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim \
                   veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea.";
        let (_, blocks) = layout_doc(src, 16.0, 200.0);
        let t = text_items(&blocks[0])[0];
        let (g, starts) = (&t.galley, &t.starts[..]);
        assert!(g.rows.len() > 5);
        assert_eq!(starts, &row_starts(g)[..]);
        let n = *starts.last().unwrap();
        for a in (0..n).step_by(7) {
            for b in (a + 1..=n + 1).step_by(11) {
                // Exactly the rows whose chars (newline included) overlap the range.
                let want: Vec<usize> = (0..g.rows.len())
                    .filter(|&ri| starts[ri] < b && a < starts[ri + 1])
                    .collect();
                let rows: Vec<usize> = segments(g, starts, a..b).iter().map(|s| s.0).collect();
                assert_eq!(rows, want, "{a}..{b}");
            }
            let r = char_rect(g, starts, a);
            let row = (0..g.rows.len()).find(|&i| a < starts[i + 1]).unwrap();
            assert_eq!(r.top(), g.rows[row].pos.y);
        }
    }

    #[test]
    fn status_circles_are_solid_shapes() {
        let (p, blocks) = layout_doc("🟡 Fallback, ✅ done, 🟥\u{FE0F} no", 16.0, 600.0);
        assert!(
            p.texts[0].plain().starts_with("🟡 Fallback"),
            "copy text unchanged"
        );
        let item = text_items(&blocks[0])[0];
        let dots: Vec<_> = item
            .decos
            .iter()
            .filter_map(|d| match d.kind {
                DecoKind::Dot { color, square, .. } => Some((d.rect, color, square)),
                _ => None,
            })
            .collect();
        assert_eq!(dots.len(), 2, "✅ stays a glyph");
        let pal = Palette::light();
        assert_eq!(dots[0].1, pal.alert.warning.fg);
        assert!(!dots[0].2 && dots[1].2);
        // Inside the emoji's advance, cap-height tall.
        let (x0, x1) = glyph_x(item, 0);
        assert!(dots[0].0.left() >= x0 && dots[0].0.right() <= x1 + 0.5);
        assert!((dots[0].0.height() - 11.5).abs() < 1.0);
    }

    #[test]
    fn deep_nesting_stays_readable() {
        let quotes: String = (0..100)
            .map(|i| format!("{} q {i}\n{}\n", "> ".repeat(i + 1), ">".repeat(i + 1)))
            .collect();
        let list: String = (0..100)
            .map(|i| format!("{}- item {i}\n", "  ".repeat(i)))
            .collect();
        let alerts: String = (0..40)
            .map(|i| {
                format!(
                    "{}[!NOTE]\n{} note {i}\n",
                    "> ".repeat(i + 1),
                    "> ".repeat(i + 1)
                )
            })
            .collect();
        for src in [quotes, list, alerts] {
            for w in [400.0, 736.0] {
                let (_, blocks) = layout_doc(&src, 16.0, w);
                for t in text_items(&blocks[0]) {
                    let words = t.galley.job.text.split_whitespace().count();
                    assert!(
                        t.galley.rows.len() <= words.max(1),
                        "{w}: {:?} broken into {} rows",
                        t.galley.job.text,
                        t.galley.rows.len()
                    );
                    assert!(t.pos.x <= w - 160.0, "{w}: text at x = {}", t.pos.x);
                }
            }
        }
    }

    #[test]
    fn huge_code_blocks_are_shaped_lazily_in_chunks() {
        let code: String = (0..3000)
            .map(|i| format!("x{i} = foo({i}, \"bär\") # comment\n"))
            .collect();
        let src = format!("```\n{code}```\n");
        let (_, chunks) = with_env(&src, 16.0, |p, env| {
            let lb = layout_top(env, &p.blocks[0], 736.0);
            let Some(Item::Scroll(s)) = lb.items.iter().find(|i| matches!(i, Item::Scroll(_)))
            else {
                panic!("no code area")
            };
            let chunks: Vec<TextItem> = s
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            // Geometry without shaping: one line_h per line, the frame fits them all.
            let (_, line_h) = env.code_base();
            let h: f32 = chunks.iter().map(|t| t.rect().height()).sum();
            assert!((h - 3000.0 * line_h).abs() < 0.5, "{h}");
            assert!(lb.height > h);
            // Shaped on demand, the chunks match their precomputed rows and widths.
            for t in &chunks {
                let g = t.shaped(env.ctx);
                assert_eq!(&t.starts[..], &row_starts(&g)[..]);
                assert!(g.rect.width() <= t.rect().width() + 0.5);
                assert!(g.rect.width() >= t.rect().width() - 4.0);
                assert!((g.rect.height() - t.rect().height()).abs() < 0.5);
            }
            chunks
        });
        assert_eq!(chunks.len(), 3000_usize.div_ceil(CODE_CHUNK_LINES));
        assert!(
            chunks
                .iter()
                .all(|t| t.lazy.is_some() && t.galley.rows.len() <= 1)
        );
        // Char bases continue across chunks (the newline between them included).
        let mut base = 0;
        for t in &chunks {
            assert_eq!(t.char_base, base);
            base += t.starts.last().unwrap() + 1;
        }
        assert_eq!(base - 1, code.trim_end().chars().count());
    }

    #[test]
    fn huge_wrapped_code_blocks_are_shaped_lazily_per_line() {
        let code: String = (0..1500)
            .map(|i| {
                let tail = if i % 50 == 0 {
                    " lots of words".repeat(20)
                } else {
                    String::new()
                };
                format!("    x{i} = foo({i}, \"bär\"){tail}\n")
            })
            .collect();
        let src = format!("```\n{code}```\n");
        // The same block laid out eagerly (as a smaller block would be) must match line by line.
        with_env_wrap(&src, 16.0, true, |p, env| {
            let lb = layout_top(env, &p.blocks[0], 600.0);
            let Some(Item::Scroll(s)) = lb.items.iter().find(|i| matches!(i, Item::Scroll(_)))
            else {
                panic!("no code area")
            };
            let pieces: Vec<&TextItem> = s
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Text(t) => Some(t),
                    _ => None,
                })
                .collect();
            assert_eq!(pieces.len(), 1500);
            let mut y = pieces[0].pos.y;
            for t in pieces {
                assert!(t.lazy.is_some());
                let g = t.shaped(env.ctx);
                assert_eq!(&t.starts[..], &row_starts(&g)[..], "{:?}", g.job.text);
                assert!((t.rect().height() - g.rect.height()).abs() < 0.5);
                let (est, real) = (t.rect(), g.rect.translate(t.pos.to_vec2()));
                assert!(
                    (est.left() - real.left()).abs() < 0.5
                        && (est.right() - real.right()).abs() < 2.5,
                    "{est:?} vs {real:?} for {:?}",
                    g.job.text
                );
                assert!((t.pos.y - y).abs() < 0.5);
                y += t.rect().height();
            }
        });
    }

    #[test]
    fn headings_belong_to_the_text_after_them() {
        let t = 16.0;
        let para = BlockKind::Paragraph {
            run: 0,
            align: HAlign::Left,
        };
        let followers = [
            para.clone(),
            BlockKind::Rule,
            BlockKind::Quote(Vec::new()),
            BlockKind::Footnotes(Vec::new()),
        ];
        for level in 1..=6 {
            let h = BlockKind::Heading {
                level,
                run: 0,
                index: 0,
                align: HAlign::Left,
            };
            let above = gap(&para, &h, t);
            assert_eq!(above, margins(&h, t).0);
            for next in &followers {
                let below = gap(&h, next, t);
                assert!(
                    above >= 2.5 * below,
                    "H{level} then {next:?}: {above} above vs {below} below"
                );
            }
        }
    }
}
