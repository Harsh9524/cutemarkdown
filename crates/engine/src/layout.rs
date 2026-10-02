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
    /// Decorations, galley-relative.
    pub decos: Vec<Deco>,
    pub links: Vec<LinkRange>,
    pub objects: Vec<ObjPlace>,
}

impl TextItem {
    pub fn rect(&self) -> Rect {
        self.galley.rect.translate(self.pos.to_vec2())
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
        }
    }

    /// Spec px at T = 16 → content px.
    pub fn em(&self, px: f32) -> f32 {
        px * self.t / 16.0
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

/// Per-row horizontal segments covering chars `range`: (row index, x0, x1), galley coords.
pub fn segments(galley: &Galley, starts: &[usize], range: Range<usize>) -> Vec<(usize, f32, f32)> {
    let mut out = Vec::new();
    if range.is_empty() {
        return out;
    }
    for (ri, row) in galley.rows.iter().enumerate() {
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
    for (ri, row) in galley.rows.iter().enumerate() {
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
        // Char offsets of spans (galley chars == text chars).
        let mut span_chars: Vec<Range<usize>> = Vec::with_capacity(rt.spans.len());
        let mut chars = 0usize;
        for span in &rt.spans {
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
                    self.append_tinted(&mut job, text, fmt);
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
            decos: Vec::new(),
            links: Vec::new(),
            objects: Vec::new(),
        };
        let starts = row_starts(&item.galley);
        let g = item.galley.clone();
        let line_h = base.line_h;
        let row_line_box = |ri: usize| -> (f32, f32, f32) {
            let row = &g.rows[ri];
            let bl = row.pos.y + asc_b + (row.size.y - line_h) + shift;
            let top = bl - asc_b - shift;
            (top, top + line_h, bl)
        };
        let xh = base.size * if base.serif { 0.507 } else { 0.546 };

        // Decorations by span flags.
        for (si, span) in rt.spans.iter().enumerate() {
            let range = span_chars[si].clone();
            let f = span.flags;
            if f & (CODE | KBD | MARK | STRIKE | UNDERLINE) == 0 {
                continue;
            }
            for (ri, x0, x1) in segments(&g, &starts, range.clone()) {
                let (top, bottom, bl) = row_line_box(ri);
                if f & KBD != 0 {
                    let h = base.size * 0.8 * 1.21 + 4.0;
                    let cy = bl - base.size * 0.29;
                    item.decos.push(Deco {
                        rect: Rect::from_min_max(pos2(x0, cy - h / 2.0), pos2(x1, cy + h / 2.0)),
                        kind: DecoKind::Kbd,
                    });
                } else if f & CODE != 0 {
                    item.decos.push(Deco {
                        rect: Rect::from_min_max(pos2(x0, top + 2.0), pos2(x1, bottom - 2.0)),
                        kind: DecoKind::CodeBg,
                    });
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

    /// Append text, giving status emoji their semantic tint.
    fn append_tinted(&self, job: &mut LayoutJob, text: &str, fmt: TextFormat) {
        let mut last = 0;
        for (i, c) in text.char_indices() {
            if (c as u32) < 0x2000 {
                continue;
            }
            if let Some(tint) = emoji_tint(c, self.pal) {
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

/// Gap between two consecutive blocks (margins collapse; heading after heading gets 12).
pub fn gap(prev: &BlockKind, next: &BlockKind, t: f32) -> f32 {
    if matches!(next, BlockKind::Anchor(_)) || matches!(prev, BlockKind::Anchor(_)) {
        return 0.0;
    }
    if is_heading(prev) && is_heading(next) {
        return 12.0 * t / 16.0;
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
                let gap = self.em(16.0);
                self.children(blocks, x + bar_w + gap, w - bar_w - gap, &inner, out, y);
                out.items.push(Item::Fill {
                    rect: Rect::from_min_max(pos2(x, top), pos2(x + bar_w, *y)),
                    color: self.pal.quote_bar,
                    radius: CornerRadius::same(1),
                });
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
        let mut job = LayoutJob {
            break_on_newline: true,
            ..Default::default()
        };
        let mut pos = 0;
        if let Some(h) = &hl {
            for (r, role) in &h.spans {
                if r.start > pos {
                    job.append(&text[pos..r.start], 0.0, fmt(pal.text));
                }
                job.append(&text[r.clone()], 0.0, fmt(role_color(*role)));
                pos = r.end;
            }
        }
        if pos < text.len() {
            job.append(&text[pos..], 0.0, fmt(pal.text));
        }
        if job.text.is_empty() {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(0)..ByteIndex(0),
                format: fmt(pal.text),
            });
        }
        let inner_w = (w - 2.0 * pad_x).max(10.0);
        job.wrap.max_width = if self.wrap_code {
            inner_w
        } else {
            f32::INFINITY
        };
        let galley = self.galley(job);
        let code_h = galley.rect.height().max(line_h);
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
        let content_w = (galley.rect.width() + 2.0 * pad_x).max(area.width());
        let mut items = Vec::new();
        if let Some(h) = &hl {
            let starts = row_starts(&galley);
            let line_starts: Vec<usize> = std::iter::once(0)
                .chain(
                    text.char_indices()
                        .filter(|(_, c)| *c == '\n')
                        .map(|(i, _)| i + 1),
                )
                .collect();
            for (line, kind) in &h.line_bg {
                let Some(&byte) = line_starts.get(*line as usize) else {
                    continue;
                };
                let ch = text[..byte].chars().count();
                let rect = char_rect(&galley, &starts, ch);
                let color = match kind {
                    LineBg::Add => pal.syntax.diff_add_bg,
                    LineBg::Del => pal.syntax.diff_del_bg,
                };
                items.push(Item::Fill {
                    rect: Rect::from_min_max(
                        pos2(-1.0, top + header_h + pad_t + rect.top()),
                        pos2(content_w + 1.0, top + header_h + pad_t + rect.bottom()),
                    ),
                    color,
                    radius: CornerRadius::ZERO,
                });
            }
        }
        items.push(Item::Text(TextItem {
            pos: pos2(pad_x - 1.0, top + header_h + pad_t),
            galley,
            shift,
            asc,
            line_h,
            run: c.run,
            decos: Vec::new(),
            links: Vec::new(),
            objects: Vec::new(),
        }));
        out.items.push(Item::Scroll(ScrollItem {
            id,
            frame: area,
            content_w,
            items,
            fade: pal.code_bg,
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
        let level = ctx.list_depth + 1;
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
        let marker_x = x + self.em(10.0);
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
                if matches!(b.kind, BlockKind::List(_))
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
            if let Some(checked) = it.task {
                let s = self.em(15.0);
                let rect = Rect::from_center_size(
                    pos2(marker_x, mid.clamp(row_top + s / 2.0, row_bottom - s / 2.0)),
                    Vec2::splat(s),
                );
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
        self.children(
            blocks,
            x + pad_x + 1.0,
            w - 2.0 * pad_x - 2.0,
            &inner,
            out,
            y,
        );
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
        // Measure natural and longest-word widths.
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
                                longest = longest.max(gl.pos.x - s);
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
                min_w[col] = min_w[col].max(longest.min(self.em(240.0)));
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
        for (rect, header, ri) in &row_rects {
            let is_last = *ri + 1 == nrows;
            let fill = if *header {
                Some(pal.table_head)
            } else if ri % 2 == 0 {
                Some(pal.table_zebra)
            } else {
                None
            };
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
            self.children(
                blocks,
                x + pad_x + 1.0,
                w - 2.0 * pad_x - 2.0,
                &inner,
                out,
                y,
            );
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
