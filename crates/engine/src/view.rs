//! The document view: layout cache, virtualized painting, scrolling, selection, find, links.
//!
//! Scroll state is content-anchored: whenever block heights change (progressive layout, images
//! arriving, highlighting, reloads, text-size changes) the block at the reading position keeps
//! its on-screen position.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use egui::epaint::TextShape;
use egui::{
    Color32, CornerRadius, CursorIcon, Event, FontId, Galley, Id, Key, Modifiers, Painter,
    PointerButton, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, emath::GuiRounding as _,
    pos2, vec2,
};

use crate::find::{self, Match};
use crate::icons::{self, Icon};
use crate::ir::*;
use crate::layout::{self, ButtonKind, DecoKind, Env, ImgState, Item, LBlock, ObjKind, TextItem};
use crate::{DocOutput, Document, FindStatus, LinkTarget, Style};

/// Spec px (not scaled with text size).
const TOP_CONTENT_PAD: f32 = 40.0;
const SCROLLBAR_ZONE: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct LayoutKey {
    width: u32,
    t: u32,
    serif: bool,
    theme: crate::ThemeKind,
    wrap: bool,
    ppp: u32,
    fonts: u64,
}

#[derive(Clone, Debug, Default)]
struct Slot {
    lb: Option<Arc<LBlock>>,
    h: f32,
    valid: bool,
}

#[derive(Clone, Debug)]
enum ScrollReq {
    Y(f32),
    Bottom,
    Anchor(String),
    Heading(usize),
    Match(usize),
    /// Reload anchoring: block index and pixel offset of the reading line within it.
    Block(usize, f32),
}

#[derive(Clone, Copy, Debug)]
struct Anim {
    from: f32,
    to: f32,
    t0: f64,
    dur: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SelPos {
    run: u32,
    ch: u32,
}

#[derive(Clone, Copy, Debug)]
struct Selection {
    anchor: SelPos,
    head: SelPos,
}

impl Selection {
    fn ordered(&self) -> (SelPos, SelPos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

#[derive(Default)]
struct FindState {
    query: String,
    case_sensitive: bool,
    matches: Vec<Match>,
    current: Option<usize>,
}

/// A visible text item with its screen geometry (for hit-testing).
struct Hit {
    run: RunId,
    /// Screen position of the galley origin (line boxes, not glyph shift).
    origin: Pos2,
    /// Visible screen rect of the item (clipped for scrollers).
    rect: Rect,
    clip: Rect,
    galley: Arc<Galley>,
    /// Top-level block and path to the item (for link lookup).
    block: usize,
    item: ItemPath,
}

#[derive(Clone, Copy, Debug)]
struct ItemPath {
    index: usize,
    inner: Option<usize>,
}

#[derive(Default)]
struct Scrollbar {
    last_active: f64,
    drag_grab: Option<f32>,
}

enum Action {
    Toggle(u64),
    Copy(RunId),
}

pub(crate) struct ViewState {
    id: Id,
    key: Option<LayoutKey>,
    slots: Vec<Slot>,
    /// tops[i] = y of block i in document coordinates (gaps included); tops[n] = height.
    tops: Vec<f32>,
    tops_dirty: bool,
    col_w: f32,
    scroll_y: f32,
    pending: Option<ScrollReq>,
    anim: Option<Anim>,
    /// Keep the view pinned to the end (End key, scroll_to_bottom, follow-tail) until the
    /// reader scrolls away; survives heights changing during progressive layout.
    stick_bottom: bool,
    viewport_h: f32,
    top_inset: f32,
    hscroll: HashMap<u64, f32>,
    toggled: HashSet<u64>,
    copied: HashMap<RunId, f64>,
    images: HashMap<String, ImgState>,
    uri_blocks: HashMap<String, Vec<usize>>,
    hl_gen: u64,
    sel: Option<Selection>,
    sel_dragging: bool,
    find: FindState,
    flash: Option<(String, f64)>,
    sb: Scrollbar,
    fallbacks_requested: bool,
    hovered_link_since: Option<(String, f64)>,
    /// Measured cost of the last `show` (for benchmarks).
    pub last_show_secs: f64,
    pub layout_complete: bool,
}

impl ViewState {
    pub fn new(doc: &Document) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let mut s = Self {
            id: Id::new((
                "cutemarkdown-docview",
                COUNTER.fetch_add(1, Ordering::Relaxed),
            )),
            key: None,
            slots: Vec::new(),
            tops: Vec::new(),
            tops_dirty: true,
            col_w: 0.0,
            scroll_y: 0.0,
            pending: None,
            anim: None,
            stick_bottom: false,
            viewport_h: 600.0,
            top_inset: 0.0,
            hscroll: HashMap::new(),
            toggled: HashSet::new(),
            copied: HashMap::new(),
            images: HashMap::new(),
            uri_blocks: HashMap::new(),
            hl_gen: 0,
            sel: None,
            sel_dragging: false,
            find: FindState::default(),
            flash: None,
            sb: Scrollbar {
                last_active: -10.0,
                drag_grab: None,
            },
            fallbacks_requested: false,
            hovered_link_since: None,
            last_show_secs: 0.0,
            layout_complete: false,
        };
        s.reset_slots(doc, None);
        s
    }

    fn reset_slots(&mut self, doc: &Document, old: Option<(&Document, &[Slot])>) {
        let n = doc.p.blocks.len();
        // Height estimates: reuse heights of unchanged blocks, else guess from the source.
        let mut by_content: HashMap<u64, f32> = HashMap::new();
        if let Some((od, slots)) = old {
            for (k, s) in od.p.top_keys.iter().zip(slots) {
                by_content.entry(k.content).or_insert(s.h);
            }
        }
        self.slots = (0..n)
            .map(|i| {
                let b = &doc.p.blocks[i];
                let est = by_content
                    .get(&doc.p.top_keys[i].content)
                    .copied()
                    .unwrap_or_else(|| {
                        let lines = (b.end_line.saturating_sub(b.line) + 1) as f32;
                        lines * 26.0 + 8.0
                    });
                Slot {
                    lb: None,
                    h: est,
                    valid: false,
                }
            })
            .collect();
        self.tops_dirty = true;
        self.recompute_tops(doc);
        self.layout_complete = false;
    }

    fn recompute_tops(&mut self, doc: &Document) {
        let t = self.key.map(|k| f32::from_bits(k.t)).unwrap_or(16.0);
        let n = self.slots.len();
        self.tops.clear();
        self.tops.reserve(n + 1);
        let mut y = 0.0;
        for i in 0..n {
            if i > 0 {
                y += layout::gap(&doc.p.blocks[i - 1].kind, &doc.p.blocks[i].kind, t);
            }
            self.tops.push(y);
            y += self.slots[i].h;
        }
        self.tops.push(y);
        self.tops_dirty = false;
    }

    fn doc_height(&self) -> f32 {
        self.tops.last().copied().unwrap_or(0.0)
    }

    fn top_pad(&self) -> f32 {
        self.top_inset + TOP_CONTENT_PAD
    }

    fn max_scroll(&self) -> f32 {
        (self.top_pad() + self.doc_height() + 0.4 * self.viewport_h - self.viewport_h).max(0.0)
    }

    /// Document y of the reading line (just below the app bar).
    fn reading_y(&self, scroll: f32) -> f32 {
        scroll - TOP_CONTENT_PAD
    }

    fn block_at(&self, doc_y: f32) -> usize {
        let n = self.slots.len();
        if n == 0 {
            return 0;
        }
        let i = self.tops[..n].partition_point(|&t| t <= doc_y);
        i.saturating_sub(1).min(n - 1)
    }

    /// (block, offset of the reading line within it, block height).
    fn anchor(&self) -> Option<(usize, f32, f32)> {
        if self.slots.is_empty() || self.scroll_y <= 0.5 {
            return None;
        }
        let y = self.reading_y(self.scroll_y);
        let i = self.block_at(y);
        Some((i, y - self.tops[i], self.slots[i].h))
    }

    fn restore(&mut self, a: Option<(usize, f32, f32)>) {
        let Some((i, off, old_h)) = a else { return };
        if i >= self.slots.len() {
            return;
        }
        let new_h = self.slots[i].h;
        let off = if (new_h - old_h).abs() < 0.5 || off < 0.0 || old_h <= 0.0 {
            off
        } else {
            off * new_h / old_h
        };
        let y = self.tops[i] + off;
        let new_scroll = y + TOP_CONTENT_PAD;
        let delta = new_scroll - self.scroll_y;
        self.scroll_y = new_scroll;
        if let Some(anim) = &mut self.anim {
            // Keep animations pointed at the same content.
            anim.from += delta;
            anim.to += delta;
        }
    }

    // -----------------------------------------------------------------------------------------
    // Layout

    fn env<'a>(
        doc: &'a Document,
        ctx: &'a egui::Context,
        style: &'a Style,
        images: &'a HashMap<String, ImgState>,
        toggled: &'a HashSet<u64>,
    ) -> Env<'a> {
        Env::new(
            ctx,
            &style.palette,
            style.text_size,
            style.font == crate::FontChoice::Serif,
            style.wrap_code,
            &doc.p.texts,
            &doc.p.headings,
            images,
            toggled,
        )
    }

    /// Lay out invalid blocks among `want` (in order), then others until `budget` seconds.
    /// Keeps the reading position anchored. Returns true if anything changed.
    fn ensure_layout(
        &mut self,
        doc: &Document,
        ctx: &egui::Context,
        style: &Style,
        want: &[usize],
        budget: f64,
        urgent: bool,
    ) -> bool {
        let anchor = self.anchor();
        let start = std::time::Instant::now();
        let mut changed = false;
        let col_w = self.col_w;
        let mut new_uris: Vec<(String, usize)> = Vec::new();
        {
            let env = Self::env(doc, ctx, style, &self.images, &self.toggled);
            env.urgent.set(urgent);
            let mut lay = |i: usize, slots: &mut Vec<Slot>| {
                let lb = layout::layout_top(&env, &doc.p.blocks[i], col_w);
                for u in &lb.uris {
                    new_uris.push((u.clone(), i));
                }
                let s = &mut slots[i];
                if (s.h - lb.height).abs() > 0.01 {
                    changed = true;
                }
                s.h = lb.height;
                s.lb = Some(Arc::new(lb));
                s.valid = true;
            };
            for &i in want {
                if i < self.slots.len() && !self.slots[i].valid {
                    lay(i, &mut self.slots);
                }
            }
            if budget > 0.0 {
                env.urgent.set(false);
                let n = self.slots.len();
                for i in 0..n {
                    if start.elapsed().as_secs_f64() > budget {
                        break;
                    }
                    if !self.slots[i].valid {
                        lay(i, &mut self.slots);
                    }
                }
            }
        }
        for (u, i) in new_uris {
            let v = self.uri_blocks.entry(u).or_default();
            if !v.contains(&i) {
                v.push(i);
            }
        }
        if changed || self.tops_dirty {
            self.recompute_tops(doc);
            self.restore(anchor);
        }
        self.layout_complete = self.slots.iter().all(|s| s.valid);
        changed
    }

    fn invalidate(&mut self, i: usize) {
        if let Some(s) = self.slots.get_mut(i) {
            s.valid = false;
        }
    }

    fn visible_range(&self, extra: f32) -> (usize, usize) {
        let n = self.slots.len();
        if n == 0 {
            return (0, 0);
        }
        let top = self.scroll_y - self.top_pad() - extra;
        let bottom = self.scroll_y - self.top_pad() + self.viewport_h + extra;
        let a = self.block_at(top);
        let mut b = a;
        while b < n && self.tops[b] < bottom {
            b += 1;
        }
        (a, b.max(a + 1).min(n))
    }

    // -----------------------------------------------------------------------------------------
    // Public-ish API used by DocView

    pub fn request(&mut self, r: ScrollReqPub) {
        self.anim = None;
        self.pending = Some(match r {
            ScrollReqPub::Y(y) => ScrollReq::Y(y),
            ScrollReqPub::Bottom => ScrollReq::Bottom,
            ScrollReqPub::Anchor(a) => ScrollReq::Anchor(a),
            ScrollReqPub::Heading(i) => ScrollReq::Heading(i),
        });
    }

    pub fn scroll_offset(&self) -> f32 {
        match &self.pending {
            Some(ScrollReq::Y(y)) => *y,
            _ => self.scroll_y,
        }
    }

    pub fn set_document(&mut self, old: &Document, new: &Document, keep_position: bool) {
        let at_end = self.scroll_y >= self.max_scroll() - 48.0 && self.max_scroll() > 0.0;
        let anchor = self.anchor();
        let old_slots = std::mem::take(&mut self.slots);
        self.reset_slots(new, Some((old, &old_slots)));
        self.uri_blocks.clear();
        self.sel = None;
        self.sel_dragging = false;
        self.fallbacks_requested = false;
        if !keep_position {
            self.scroll_y = 0.0;
            self.pending = Some(ScrollReq::Y(0.0));
            self.hscroll.clear();
            self.find.matches.clear();
            self.find.current = None;
            if !self.find.query.is_empty() {
                let q = self.find.query.clone();
                self.set_find(new, &q, self.find.case_sensitive, false);
            }
            return;
        }
        if at_end {
            self.pending = Some(ScrollReq::Bottom);
        } else if let Some((i, off, _)) = anchor {
            let target = map_block(old, new, i);
            self.pending = Some(ScrollReq::Block(target.0, if target.1 { off } else { 0.0 }));
        }
        // Re-run find, keeping the match closest to the previous one.
        if !self.find.query.is_empty() {
            let prev = self
                .find
                .current
                .and_then(|c| self.find.matches.get(c).copied());
            let q = self.find.query.clone();
            self.set_find(new, &q, self.find.case_sensitive, false);
            if let Some(p) = prev {
                let best = self
                    .find
                    .matches
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, m)| {
                        (
                            (m.run as i64 - p.run as i64).abs(),
                            (m.start as i64 - p.start as i64).abs(),
                        )
                    })
                    .map(|(i, _)| i);
                self.find.current = best;
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Find

    pub fn set_find(
        &mut self,
        doc: &Document,
        query: &str,
        case_sensitive: bool,
        scroll: bool,
    ) -> FindStatus {
        self.find.query = query.to_owned();
        self.find.case_sensitive = case_sensitive;
        let texts = doc.search_texts();
        self.find.matches = find::find_all(texts, query, case_sensitive);
        self.find.current = None;
        if self.find.matches.is_empty() {
            return self.find_status();
        }
        // First match at or after the reading position.
        let y = self.reading_y(self.scroll_y);
        let block = self.block_at(y.max(0.0));
        let idx = self
            .find
            .matches
            .iter()
            .position(|m| doc.p.runs[m.run as usize].top as usize >= block)
            .unwrap_or(0);
        self.find.current = Some(idx);
        if scroll {
            self.pending = Some(ScrollReq::Match(idx));
        }
        self.find_status()
    }

    pub fn find_step(&mut self, forward: bool) -> FindStatus {
        let n = self.find.matches.len();
        if n == 0 {
            return self.find_status();
        }
        let next = match self.find.current {
            None => 0,
            Some(c) if forward => (c + 1) % n,
            Some(c) => (c + n - 1) % n,
        };
        self.find.current = Some(next);
        self.pending = Some(ScrollReq::Match(next));
        self.find_status()
    }

    pub fn find_status(&self) -> FindStatus {
        FindStatus {
            total: self.find.matches.len(),
            current: self.find.current,
        }
    }

    pub fn clear_find(&mut self) {
        let cs = self.find.case_sensitive;
        self.find = FindState {
            case_sensitive: cs,
            ..Default::default()
        };
    }

    pub fn find_case_sensitive(&self) -> bool {
        self.find.case_sensitive
    }

    /// Toggle match-case and re-run the current query, keeping the nearest match current.
    pub fn set_find_case(&mut self, doc: &Document, on: bool) -> FindStatus {
        let prev = self
            .find
            .current
            .and_then(|c| self.find.matches.get(c).copied());
        self.find.case_sensitive = on;
        if self.find.query.is_empty() {
            return self.find_status();
        }
        let q = self.find.query.clone();
        self.set_find(doc, &q, on, false);
        if let Some(p) = prev {
            // Prefer the first match at or after the previous current one.
            let idx = self
                .find
                .matches
                .iter()
                .position(|m| (m.run, m.start) >= (p.run, p.start))
                .or(if self.find.matches.is_empty() {
                    None
                } else {
                    Some(0)
                });
            self.find.current = idx;
        }
        if let Some(i) = self.find.current {
            self.pending = Some(ScrollReq::Match(i));
        }
        self.find_status()
    }

    /// The current match becomes the selection (SPEC §7: Esc in the find bar).
    pub fn select_current_match(&mut self) {
        if let Some(m) = self.find.current.and_then(|c| self.find.matches.get(c)) {
            self.sel = Some(Selection {
                anchor: SelPos {
                    run: m.run,
                    ch: m.start,
                },
                head: SelPos {
                    run: m.run,
                    ch: m.end,
                },
            });
        }
    }

    // -----------------------------------------------------------------------------------------
    // Selection

    pub fn has_selection(&self) -> bool {
        self.sel.is_some_and(|s| s.anchor != s.head)
    }

    pub fn select_all(&mut self, doc: &Document) {
        let n = doc.p.texts.len();
        if n == 0 {
            return;
        }
        let last = &doc.p.texts[n - 1];
        self.sel = Some(Selection {
            anchor: SelPos { run: 0, ch: 0 },
            head: SelPos {
                run: (n - 1) as u32,
                ch: last.text.chars().count() as u32,
            },
        });
    }

    pub fn clear_selection(&mut self) {
        self.sel = None;
    }

    /// Plain text of the selection: blocks separated by blank lines, list items prefixed,
    /// table cells tab-separated, code verbatim.
    pub fn selected_text(&self, doc: &Document) -> String {
        let Some(sel) = self.sel else {
            return String::new();
        };
        let (a, b) = sel.ordered();
        if a == b {
            return String::new();
        }
        let mut out = String::new();
        let mut prev: Option<&RunInfo> = None;
        for run in a.run..=b.run {
            let Some(rt) = doc.p.texts.get(run as usize) else {
                break;
            };
            let info = &doc.p.runs[run as usize];
            let n = rt.text.chars().count() as u32;
            let s = if run == a.run { a.ch.min(n) } else { 0 };
            let e = if run == b.run { b.ch.min(n) } else { n };
            let piece: String = rt
                .text
                .chars()
                .skip(s as usize)
                .take(e.saturating_sub(s) as usize)
                .filter(|&c| c != MARKER)
                .collect();
            if let Some(p) = prev {
                let sep = match (p.cell, info.cell) {
                    (Some((t1, r1, _)), Some((t2, r2, _))) if t1 == t2 => {
                        if r1 == r2 {
                            "\t"
                        } else {
                            "\n"
                        }
                    }
                    _ if info.list_marker.is_some() && p.list_depth > 0 => "\n",
                    _ => "\n\n",
                };
                out.push_str(sep);
            }
            if s == 0
                && let Some(m) = &info.list_marker
            {
                for _ in 1..info.list_depth {
                    out.push_str("  ");
                }
                out.push_str(m);
            }
            out.push_str(&piece);
            prev = Some(info);
        }
        out
    }

    fn sel_range(&self, run: RunId, len: usize) -> Option<std::ops::Range<usize>> {
        let sel = self.sel?;
        let (a, b) = sel.ordered();
        if a == b || run < a.run || run > b.run {
            return None;
        }
        let s = if run == a.run { a.ch as usize } else { 0 };
        let e = if run == b.run { b.ch as usize } else { len };
        (e > s).then_some(s..e.min(len))
    }

    // -----------------------------------------------------------------------------------------
    // Frame

    pub fn show(&mut self, doc: &Document, ui: &mut Ui, style: &Style) -> DocOutput {
        let t_start = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let now = ctx.input(|i| i.time);
        let pal = &style.palette;
        let mut out = DocOutput::default();

        if !self.fallbacks_requested {
            self.fallbacks_requested = true;
            if !doc.missing.is_empty() {
                crate::fonts::request_fallbacks(&ctx, &doc.missing);
            }
        }
        let font_gen = crate::fonts::generation();

        // Geometry.
        let inset = style.top_inset.max(0.0);
        if (inset - self.top_inset).abs() > 0.01 {
            // Keep the reading position when the bar appears/disappears (e.g. Zen).
            let a = self.anchor();
            self.top_inset = inset;
            self.restore(a);
        }
        self.viewport_h = rect.height().max(1.0);
        let vw = rect.width();
        let gutter = if vw >= 960.0 {
            48.0
        } else if vw >= 640.0 {
            32.0
        } else {
            20.0
        };
        let col_w = style.measure.min(vw - 2.0 * gutter).max(120.0).floor();
        let col_left = (rect.left() + (vw - col_w) / 2.0).round();
        self.col_w = col_w;
        let key = LayoutKey {
            width: col_w.to_bits(),
            t: style.text_size.to_bits(),
            serif: style.font == crate::FontChoice::Serif,
            theme: style.theme,
            wrap: style.wrap_code,
            ppp: ctx.pixels_per_point().to_bits(),
            fonts: font_gen,
        };
        if self.key != Some(key) {
            let t_changed = self.key.is_some_and(|k| k.t != key.t);
            self.key = Some(key);
            for s in &mut self.slots {
                s.valid = false;
            }
            if t_changed {
                self.tops_dirty = true;
            }
        }
        // New highlighting results → re-lay out blocks that were waiting.
        let hl = crate::highlight::generation();
        if hl != self.hl_gen {
            self.hl_gen = hl;
            for s in &mut self.slots {
                if s.lb.as_ref().is_some_and(|lb| lb.needs_highlight) {
                    s.valid = false;
                }
            }
        }
        self.poll_images(&ctx);

        // 1. Lay out what's visible now (anchored).
        let (a, b) = self.visible_range(self.viewport_h * 0.5);
        let want: Vec<usize> = (a..b).collect();
        self.ensure_layout(doc, &ctx, style, &want, 0.0, true);

        // 2. Input → scroll.
        self.handle_input(doc, ui, &resp, style, rect, now);

        // 3. Resolve programmatic scroll requests (may need specific blocks laid out).
        self.resolve_pending(doc, &ctx, style);
        self.step_anim(&ctx, now);
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());

        // 4. Lay out newly visible blocks, then spend a small budget on the rest.
        let (a, b) = self.visible_range(self.viewport_h * 0.5);
        let want: Vec<usize> = (a..b).collect();
        self.ensure_layout(doc, &ctx, style, &want, 0.0, true);
        if !self.layout_complete {
            let (a2, b2) = self.visible_range(self.viewport_h * 3.0);
            let near: Vec<usize> = (a2..b2).collect();
            self.ensure_layout(doc, &ctx, style, &near, 0.006, false);
            ctx.request_repaint();
        }
        if self.stick_bottom {
            let max = self.max_scroll();
            match &mut self.anim {
                Some(a) => a.to = max,
                None => self.scroll_y = max,
            }
        }
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());

        // 5. Paint and interact.
        let content_origin_y = rect.top() + self.top_pad() - self.scroll_y;
        let (a, b) = self.visible_range(0.0);
        let hits = self.collect_hits(doc, rect, col_left, content_origin_y, a, b);
        self.handle_pointer(doc, ui, &resp, rect, &hits, &mut out, now);
        let actions = self.paint(
            doc,
            ui,
            rect,
            col_left,
            content_origin_y,
            a,
            b,
            style,
            &mut out,
            now,
        );
        for act in actions {
            match act {
                Action::Toggle(id) => {
                    if !self.toggled.remove(&id) {
                        self.toggled.insert(id);
                    }
                    if let Some(i) = self.block_with_id(doc, id) {
                        self.invalidate(i);
                    }
                    ctx.request_repaint();
                }
                Action::Copy(run) => {
                    if let Some(rt) = doc.p.texts.get(run as usize) {
                        ctx.copy_text(rt.text.clone());
                        self.copied.insert(run, now);
                        out.copied = true;
                        ctx.request_repaint_after(std::time::Duration::from_millis(1600));
                    }
                }
            }
        }
        self.paint_scrollbar(ui, rect, pal, now);

        // 6. Outputs.
        self.outputs(doc, &mut out);
        crate::fonts::poll_fallbacks(&ctx);
        self.last_show_secs = t_start.elapsed().as_secs_f64();
        out
    }

    fn block_with_id(&self, doc: &Document, id: u64) -> Option<usize> {
        fn has(b: &Block, id: u64) -> bool {
            if b.id == id {
                return true;
            }
            match &b.kind {
                BlockKind::List(l) => l
                    .items
                    .iter()
                    .any(|it| it.blocks.iter().any(|b| has(b, id))),
                BlockKind::Quote(bs)
                | BlockKind::Center(bs)
                | BlockKind::Alert { blocks: bs, .. } => bs.iter().any(|b| has(b, id)),
                BlockKind::Details { blocks, .. } => blocks.iter().any(|b| has(b, id)),
                BlockKind::Footnotes(n) => n.iter().any(|n| n.blocks.iter().any(|b| has(b, id))),
                _ => false,
            }
        }
        doc.p.blocks.iter().position(|b| has(b, id))
    }

    fn poll_images(&mut self, ctx: &egui::Context) {
        if self.uri_blocks.is_empty() {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let mut invalid = Vec::new();
        let mut pending_any = false;
        for (uri, blocks) in &self.uri_blocks {
            let prev = self.images.get(uri);
            if matches!(prev, Some(ImgState::Ready(_)) | Some(ImgState::Failed(_))) {
                continue;
            }
            let state = match ctx.try_load_texture(
                uri,
                egui::TextureOptions::LINEAR,
                egui::load::SizeHint::Scale(ppp.into()),
            ) {
                Ok(egui::load::TexturePoll::Pending { size }) => {
                    pending_any = true;
                    ImgState::Pending(size)
                }
                Ok(egui::load::TexturePoll::Ready { texture }) => ImgState::Ready(texture.size),
                Err(e) => ImgState::Failed(load_reason(&e, uri)),
            };
            let changed = match (prev, &state) {
                (None, ImgState::Pending(None)) => false,
                (Some(ImgState::Pending(a)), ImgState::Pending(b)) => a != b,
                _ => true,
            };
            if changed {
                invalid.extend(blocks.iter().copied());
            }
            self.images.insert(uri.clone(), state);
        }
        for i in invalid {
            self.invalidate(i);
        }
        if pending_any {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Input

    fn handle_input(
        &mut self,
        doc: &Document,
        ui: &mut Ui,
        resp: &egui::Response,
        style: &Style,
        rect: Rect,
        now: f64,
    ) {
        let ctx = ui.ctx().clone();
        let page = (self.viewport_h - 64.0).max(48.0);
        // Wheel / touchpad.
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if delta != Vec2::ZERO {
                let mut used = Vec2::ZERO;
                if delta.y != 0.0 {
                    self.anim = None;
                    if delta.y > 0.0 {
                        self.stick_bottom = false;
                    }
                    self.scroll_y -= delta.y;
                    used.y = delta.y;
                    self.sb.last_active = now;
                }
                if delta.x != 0.0
                    && let Some((id, max)) = self.scroller_under_pointer(ui, rect)
                {
                    let off = self.hscroll.entry(id).or_insert(0.0);
                    *off = (*off - delta.x).clamp(0.0, max);
                    used.x = delta.x;
                }
                ui.input_mut(|i| i.smooth_scroll_delta -= used);
            }
        }
        // Reading keys (only when no text field wants the keyboard).
        if !ctx.egui_wants_keyboard_input() {
            let mut target: Option<f32> = None;
            let cur = self.anim.map_or(self.scroll_y, |a| a.to);
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    target = Some(cur + 48.0);
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    target = Some(cur - 48.0);
                }
                if i.consume_key(Modifiers::NONE, Key::PageDown)
                    || i.consume_key(Modifiers::NONE, Key::Space)
                {
                    target = Some(cur + page);
                }
                if i.consume_key(Modifiers::NONE, Key::PageUp)
                    || i.consume_key(Modifiers::SHIFT, Key::Space)
                {
                    target = Some(cur - page);
                }
                if i.consume_key(Modifiers::NONE, Key::Home)
                    || i.consume_key(Modifiers::COMMAND, Key::Home)
                {
                    target = Some(0.0);
                }
                if i.consume_key(Modifiers::NONE, Key::End)
                    || i.consume_key(Modifiers::COMMAND, Key::End)
                {
                    target = Some(f32::MAX);
                }
            });
            if let Some(t) = target {
                self.stick_bottom = t == f32::MAX;
                let t = t.clamp(0.0, self.max_scroll());
                self.animate_to(t, now, 0.12);
                self.sb.last_active = now;
            }
            let (prev_h, next_h) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::COMMAND, Key::ArrowUp),
                    i.consume_key(Modifiers::COMMAND, Key::ArrowDown),
                )
            });
            if prev_h || next_h {
                self.jump_heading(doc, next_h);
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A)) {
                self.select_all(doc);
            }
            let copy = ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Copy)));
            if copy && self.has_selection() {
                ctx.copy_text(self.selected_text(doc));
            }
        }
        let _ = (resp, style);
    }

    fn jump_heading(&mut self, doc: &Document, forward: bool) {
        let y = self.reading_y(self.scroll_y) + 24.0 - 16.0;
        let list: Vec<(usize, f32)> = doc
            .p
            .headings
            .iter()
            .enumerate()
            .filter(|(_, h)| h.level <= 3)
            .map(|(i, _)| (i, self.heading_y(doc, i)))
            .collect();
        let target = if forward {
            list.iter().find(|(_, hy)| *hy > y + 2.0)
        } else {
            list.iter().rev().find(|(_, hy)| *hy < y - 2.0)
        };
        if let Some((i, _)) = target {
            self.pending = Some(ScrollReq::Heading(*i));
        }
    }

    fn animate_to(&mut self, to: f32, now: f64, dur: f32) {
        if (to - self.scroll_y).abs() < 0.5 {
            self.anim = None;
            self.scroll_y = to;
            return;
        }
        self.anim = Some(Anim {
            from: self.scroll_y,
            to,
            t0: now,
            dur,
        });
    }

    fn step_anim(&mut self, ctx: &egui::Context, now: f64) {
        let Some(a) = self.anim else { return };
        let k = (((now - a.t0) as f32) / a.dur).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - k).powi(3); // ease-out cubic
        self.scroll_y = a.from + (a.to - a.from) * e;
        if k >= 1.0 {
            self.anim = None;
        } else {
            ctx.request_repaint();
        }
    }

    fn scroller_under_pointer(&self, ui: &Ui, rect: Rect) -> Option<(u64, f32)> {
        let p = ui.ctx().pointer_hover_pos()?;
        let col_left = (rect.left() + (rect.width() - self.col_w) / 2.0).round();
        let oy = rect.top() + self.top_pad() - self.scroll_y;
        let (a, b) = self.visible_range(0.0);
        for i in a..b {
            let Some(lb) = &self.slots[i].lb else {
                continue;
            };
            let origin = vec2(col_left, oy + self.tops[i]);
            for it in &lb.items {
                if let Item::Scroll(s) = it {
                    let f = s.frame.translate(origin);
                    let max = (s.content_w - s.frame.width()).max(0.0);
                    if f.contains(p) && max > 0.0 {
                        return Some((s.id, max));
                    }
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------------------------
    // Programmatic scrolling

    fn resolve_pending(&mut self, doc: &Document, ctx: &egui::Context, style: &Style) {
        let Some(req) = self.pending.take() else {
            return;
        };
        self.stick_bottom = matches!(req, ScrollReq::Bottom);
        let n = self.slots.len();
        match req {
            ScrollReq::Y(y) => {
                // Make sure blocks around the target exist so the clamp is meaningful.
                self.scroll_y = y;
                let (a, b) = self.visible_range(self.viewport_h);
                let want: Vec<usize> = (a..b).collect();
                self.ensure_layout(doc, ctx, style, &want, 0.0, true);
                self.scroll_y = y.clamp(0.0, self.max_scroll());
            }
            ScrollReq::Bottom => {
                // Lay out the tail so the end is exact.
                let want: Vec<usize> = (n.saturating_sub(8)..n).collect();
                self.ensure_layout(doc, ctx, style, &want, 0.0, true);
                self.scroll_y = self.max_scroll();
            }
            ScrollReq::Block(i, off) => {
                if i < n {
                    self.ensure_layout(doc, ctx, style, &[i], 0.0, true);
                    self.scroll_y = self.tops[i] + off.min(self.slots[i].h) + TOP_CONTENT_PAD;
                }
            }
            ScrollReq::Heading(h) => {
                if let Some(head) = doc.p.headings.get(h) {
                    let name = head.anchor.clone();
                    self.scroll_to_named(doc, ctx, style, &name, false);
                }
            }
            ScrollReq::Anchor(a) => self.scroll_to_named(doc, ctx, style, &a, true),
            ScrollReq::Match(m) => self.scroll_to_match(doc, ctx, style, m),
        }
    }

    fn scroll_to_named(
        &mut self,
        doc: &Document,
        ctx: &egui::Context,
        style: &Style,
        name: &str,
        flash: bool,
    ) {
        let Some(&top) = doc.p.anchors.get(name) else {
            return;
        };
        let i = top as usize;
        self.ensure_layout(doc, ctx, style, &[i], 0.0, true);
        let (off, _) = self.slots[i]
            .lb
            .as_ref()
            .and_then(|lb| {
                lb.anchors
                    .iter()
                    .find(|(n, _, _)| n == name)
                    .map(|(_, y, h)| (*y, *h))
            })
            .unwrap_or((0.0, 0.0));
        let y = self.tops[i] + off;
        let target = (y + 16.0).clamp(0.0, self.max_scroll());
        let now = ctx.input(|i| i.time);
        self.animate_to(target, now, 0.22);
        if flash && name.starts_with("fn-") {
            self.flash = Some((name.to_owned(), now));
        }
    }

    fn scroll_to_match(&mut self, doc: &Document, ctx: &egui::Context, style: &Style, m: usize) {
        let Some(mt) = self.find.matches.get(m).copied() else {
            return;
        };
        let top = doc.p.runs[mt.run as usize].top as usize;
        // Expand collapsed containers holding the match.
        let mut opened = false;
        if let Some(b) = doc.p.blocks.get(top) {
            for id in collapsed_containers(b, mt.run, &self.toggled) {
                self.toggled.insert(id);
                opened = true;
            }
        }
        if opened {
            self.invalidate(top);
        }
        self.ensure_layout(doc, ctx, style, &[top], 0.0, true);
        let Some(lb) = self.slots[top].lb.clone() else {
            return;
        };
        // Find the text item for the run: (y in block, scroller (id, x, frame width) if any).
        type Scroller = (u64, f32, f32);
        let mut found: Option<(f32, Option<Scroller>)> = None;
        for it in &lb.items {
            match it {
                Item::Text(t) if t.run == mt.run => {
                    let starts = layout::row_starts(&t.galley);
                    let r = layout::char_rect(&t.galley, &starts, mt.start as usize);
                    found = Some((t.pos.y + r.top(), None));
                }
                Item::Scroll(s) => {
                    for inner in &s.items {
                        if let Item::Text(t) = inner
                            && t.run == mt.run
                        {
                            let starts = layout::row_starts(&t.galley);
                            let r = layout::char_rect(&t.galley, &starts, mt.start as usize);
                            let x = t.pos.x + r.left();
                            found = Some((t.pos.y + r.top(), Some((s.id, x, s.frame.width()))));
                        }
                    }
                }
                _ => {}
            }
            if found.is_some() {
                break;
            }
        }
        let Some((y_in, scroller)) = found else {
            return;
        };
        if let Some((id, x, fw)) = scroller {
            let off = self.hscroll.entry(id).or_insert(0.0);
            if x < *off + 24.0 || x > *off + fw - 48.0 {
                *off = (x - fw * 0.3).max(0.0);
            }
        }
        let y = self.tops[top] + y_in;
        let target = (self.top_pad() + y - 0.35 * self.viewport_h).clamp(0.0, self.max_scroll());
        let now = ctx.input(|i| i.time);
        self.animate_to(target, now, 0.22);
    }

    fn heading_y(&self, doc: &Document, h: usize) -> f32 {
        let meta = &doc.p.heading_meta[h];
        let i = meta.top as usize;
        let base = self.tops.get(i).copied().unwrap_or(0.0);
        if matches!(
            doc.p.blocks.get(i).map(|b| &b.kind),
            Some(BlockKind::Heading { .. })
        ) {
            return base;
        }
        let name = &doc.p.headings[h].anchor;
        self.slots
            .get(i)
            .and_then(|s| s.lb.as_ref())
            .and_then(|lb| lb.anchors.iter().find(|(n, _, _)| n == name))
            .map_or(base, |(_, y, _)| base + y)
    }

    fn outputs(&self, doc: &Document, out: &mut DocOutput) {
        let max = self.max_scroll();
        out.progress = if max > 0.0 {
            (self.scroll_y / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.scrollable = max > 0.0;
        // Scrollspy.
        let vis_top = self.scroll_y - self.top_pad() + self.top_inset;
        let reference = vis_top + 0.3 * (self.viewport_h - self.top_inset);
        let at_bottom = max > 0.0 && self.scroll_y >= max - 1.0;
        let vis_bottom = self.scroll_y - self.top_pad() + self.viewport_h;
        let mut active = None;
        for (i, _) in doc.p.headings.iter().enumerate() {
            let y = self.heading_y(doc, i);
            if y <= reference || (at_bottom && y < vis_bottom) {
                active = Some(i);
            } else if !at_bottom {
                break;
            }
        }
        out.active_heading = active;
        // Words below the reading line, and the source line at the top.
        let n = self.slots.len();
        if n > 0 {
            let i = self.block_at(vis_top.max(0.0));
            let h = self.slots[i].h.max(1.0);
            let frac = ((self.tops[i] + h - vis_top.max(0.0)) / h).clamp(0.0, 1.0);
            let mut words = (doc.p.top_words[i] as f32 * frac) as usize;
            for w in &doc.p.top_words[i + 1..] {
                words += *w as usize;
            }
            out.words_remaining = words;
            out.top_source_line = self.source_line_at(doc, i, vis_top - self.tops[i]);
        }
    }

    /// Best-effort source line at an offset inside a top-level block.
    fn source_line_at(&self, doc: &Document, i: usize, off: f32) -> usize {
        let b = &doc.p.blocks[i];
        let mut line = b.line as usize;
        if off <= 0.0 {
            return line.max(1);
        }
        // Nested blocks (list items, quotes…) have their own lines: pick by proportion.
        let h = self.slots[i].h.max(1.0);
        let span = b.end_line.saturating_sub(b.line) as f32;
        line += ((off / h).clamp(0.0, 1.0) * span) as usize;
        line.max(1)
    }

    // -----------------------------------------------------------------------------------------
    // Hit-testing and pointer

    fn collect_hits(
        &self,
        _doc: &Document,
        rect: Rect,
        col_left: f32,
        oy: f32,
        a: usize,
        b: usize,
    ) -> Vec<Hit> {
        let mut hits = Vec::new();
        for i in a..b {
            let Some(lb) = &self.slots[i].lb else {
                continue;
            };
            let origin = vec2(col_left, oy + self.tops[i]);
            for (idx, it) in lb.items.iter().enumerate() {
                match it {
                    Item::Text(t) => {
                        let o = t.pos + origin;
                        hits.push(Hit {
                            run: t.run,
                            origin: o,
                            rect: t.galley.rect.translate(o.to_vec2()),
                            clip: rect,
                            galley: t.galley.clone(),
                            block: i,
                            item: ItemPath {
                                index: idx,
                                inner: None,
                            },
                        });
                    }
                    Item::Scroll(s) => {
                        let off = self.hscroll.get(&s.id).copied().unwrap_or(0.0);
                        let frame = s.frame.translate(origin);
                        let inner_origin = origin + vec2(s.frame.left() - off, 0.0);
                        for (j, inner) in s.items.iter().enumerate() {
                            if let Item::Text(t) = inner {
                                let o = t.pos + inner_origin;
                                hits.push(Hit {
                                    run: t.run,
                                    origin: o,
                                    rect: t.galley.rect.translate(o.to_vec2()).intersect(frame),
                                    clip: frame.intersect(rect),
                                    galley: t.galley.clone(),
                                    block: i,
                                    item: ItemPath {
                                        index: idx,
                                        inner: Some(j),
                                    },
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        // Document order (runs are numbered in reading order).
        hits.sort_by_key(|h| h.run);
        hits
    }

    /// Selection position under `p` (nearest text if between blocks).
    fn hit_pos(&self, hits: &[Hit], p: Pos2) -> Option<SelPos> {
        if hits.is_empty() {
            return None;
        }
        // Inside an item (with a little vertical slack).
        for h in hits {
            if h.rect.expand2(vec2(4.0, 2.0)).contains(p) {
                let c = h.galley.cursor_from_pos(p - h.origin);
                return Some(SelPos {
                    run: h.run,
                    ch: c.index.0 as u32,
                });
            }
        }
        // Same vertical band: nearest horizontally.
        let band: Vec<&Hit> = hits
            .iter()
            .filter(|h| h.rect.y_range().contains(p.y))
            .collect();
        if let Some(h) = band.iter().min_by(|a, b| {
            let da = (a.rect.center().x - p.x).abs();
            let db = (b.rect.center().x - p.x).abs();
            da.total_cmp(&db)
        }) {
            let c = h.galley.cursor_from_pos(p - h.origin);
            return Some(SelPos {
                run: h.run,
                ch: c.index.0 as u32,
            });
        }
        // Between blocks: end of the last item above, else start of the first below.
        let above = hits
            .iter()
            .filter(|h| h.rect.bottom() <= p.y)
            .max_by(|a, b| a.rect.bottom().total_cmp(&b.rect.bottom()));
        if let Some(h) = above {
            return Some(SelPos {
                run: h.run,
                ch: h.galley.end().index.0 as u32,
            });
        }
        hits.iter()
            .min_by(|a, b| a.rect.top().total_cmp(&b.rect.top()))
            .map(|h| SelPos { run: h.run, ch: 0 })
    }

    fn link_at(&self, doc: &Document, hits: &[Hit], p: Pos2) -> Option<(RunId, u32)> {
        for h in hits {
            if !h.clip.contains(p) || !h.rect.expand(4.0).contains(p) {
                continue;
            }
            let Some(t) = self.text_item(h) else { continue };
            for l in &t.links {
                if l.hit
                    .iter()
                    .any(|r| r.translate(h.origin.to_vec2()).contains(p))
                {
                    return Some((h.run, l.link));
                }
            }
            for o in &t.objects {
                if let Some(link) = o.link
                    && o.rect.translate(h.origin.to_vec2()).contains(p)
                {
                    return Some((h.run, link));
                }
            }
        }
        let _ = doc;
        None
    }

    /// Link of a block-level image under `p` (e.g. `[![logo](a.png)](https://…)`).
    fn image_link_at(&self, rect: Rect, p: Pos2) -> Option<Link> {
        let col_left = (rect.left() + (rect.width() - self.col_w) / 2.0).round();
        let oy = rect.top() + self.top_pad() - self.scroll_y;
        let (a, b) = self.visible_range(0.0);
        for i in a..b {
            let Some(lb) = &self.slots[i].lb else {
                continue;
            };
            let origin = vec2(col_left, oy + self.tops[i]);
            for it in &lb.items {
                if let Item::Image {
                    rect: r,
                    link: Some(link),
                    ..
                } = it
                    && r.translate(origin).contains(p)
                {
                    return Some(link.clone());
                }
            }
        }
        None
    }

    fn text_item<'a>(&'a self, h: &Hit) -> Option<&'a TextItem> {
        let lb = self.slots.get(h.block)?.lb.as_ref()?;
        match (lb.items.get(h.item.index)?, h.item.inner) {
            (Item::Text(t), None) => Some(t),
            (Item::Scroll(s), Some(j)) => match s.items.get(j)? {
                Item::Text(t) => Some(t),
                _ => None,
            },
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_pointer(
        &mut self,
        doc: &Document,
        ui: &Ui,
        resp: &egui::Response,
        rect: Rect,
        hits: &[Hit],
        out: &mut DocOutput,
        now: f64,
    ) {
        let ctx = ui.ctx();
        let pointer = ctx.pointer_interact_pos();
        let over_scrollbar = pointer.is_some_and(|p| p.x >= rect.right() - SCROLLBAR_ZONE)
            && self.max_scroll() > 0.0;
        let (pressed, down, released, shift, double, triple) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.modifiers.shift,
                i.pointer.button_double_clicked(PointerButton::Primary),
                i.pointer.button_triple_clicked(PointerButton::Primary),
            )
        });
        let hovered = resp.hovered() && !over_scrollbar;
        let command = ctx.input(|i| i.modifiers.command);

        // Links: hover + click (Ctrl+click and middle click ask for a new window).
        let link = pointer.filter(|_| hovered).and_then(|p| {
            self.link_at(doc, hits, p)
                .and_then(|(run, li)| doc.p.texts[run as usize].links.get(li as usize).cloned())
                .or_else(|| self.image_link_at(rect, p))
        });
        if let Some(link) = link {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            out.hovered_link = Some(link.href.clone());
            match &self.hovered_link_since {
                Some((h, _)) if *h == link.href => {}
                _ => self.hovered_link_since = Some((link.href.clone(), now)),
            }
            let middle = resp.clicked_by(PointerButton::Middle);
            if (resp.clicked() && !shift) || middle {
                out.link_new_window = middle || command;
                out.clicked_link = Some(match &link.dest {
                    LinkDest::External(u) => LinkTarget::External(u.clone()),
                    LinkDest::File { path, anchor } => LinkTarget::File {
                        path: path.clone(),
                        anchor: anchor.clone(),
                    },
                    LinkDest::Anchor(a) => LinkTarget::Anchor(a.clone()),
                });
            }
        } else if hovered && let Some(p) = pointer {
            self.hovered_link_since = None;
            if hits
                .iter()
                .any(|h| h.clip.contains(p) && h.rect.contains(p))
            {
                ctx.set_cursor_icon(CursorIcon::Text);
            }
        }

        // Selection.
        if pressed
            && hovered
            && let Some(p) = pointer
        {
            if let Some(pos) = self.hit_pos(hits, p) {
                if shift && let Some(sel) = &mut self.sel {
                    sel.head = pos;
                } else {
                    self.sel = Some(Selection {
                        anchor: pos,
                        head: pos,
                    });
                }
                self.sel_dragging = true;
            } else {
                self.sel = None;
            }
        }
        if self.sel_dragging
            && down
            && let Some(p) = pointer
        {
            // Auto-scroll when dragging past the edges.
            let edge = 24.0;
            let top_edge = rect.top() + self.top_inset;
            if p.y < top_edge + edge {
                self.stick_bottom = false;
                self.scroll_y =
                    (self.scroll_y - ((top_edge + edge - p.y) * 0.5).min(40.0)).max(0.0);
                ctx.request_repaint();
            } else if p.y > rect.bottom() - edge {
                self.scroll_y = (self.scroll_y + ((p.y - rect.bottom() + edge) * 0.5).min(40.0))
                    .min(self.max_scroll());
                ctx.request_repaint();
            }
            let clamped = pos2(
                p.x.clamp(rect.left(), rect.right()),
                p.y.clamp(top_edge, rect.bottom() - 1.0),
            );
            if let Some(pos) = self.hit_pos(hits, clamped)
                && let Some(sel) = &mut self.sel
            {
                sel.head = pos;
            }
        }
        if released {
            self.sel_dragging = false;
        }
        if (double || triple)
            && hovered
            && let Some(p) = pointer
            && let Some(pos) = self.hit_pos(hits, p)
        {
            let rt = &doc.p.texts[pos.run as usize];
            let chars: Vec<char> = rt.text.chars().collect();
            let (s, e) = if triple {
                // The visual line/paragraph: whole run for prose.
                (0, chars.len())
            } else {
                let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '\'';
                let mut s = (pos.ch as usize).min(chars.len());
                let mut e = s;
                while s > 0 && is_word(chars[s - 1]) {
                    s -= 1;
                }
                while e < chars.len() && is_word(chars[e]) {
                    e += 1;
                }
                (s, e)
            };
            self.sel = Some(Selection {
                anchor: SelPos {
                    run: pos.run,
                    ch: s as u32,
                },
                head: SelPos {
                    run: pos.run,
                    ch: e as u32,
                },
            });
            self.sel_dragging = false;
        }
        if let Some(sel) = &self.sel
            && !self.sel_dragging
            && sel.anchor == sel.head
        {
            self.sel = None;
        }
    }

    // -----------------------------------------------------------------------------------------
    // Painting

    #[allow(clippy::too_many_arguments)]
    fn paint(
        &mut self,
        doc: &Document,
        ui: &mut Ui,
        rect: Rect,
        col_left: f32,
        oy: f32,
        a: usize,
        b: usize,
        style: &Style,
        out: &mut DocOutput,
        now: f64,
    ) -> Vec<Action> {
        let painter = ui.painter_at(rect);
        let mut actions = Vec::new();
        let hover_link = out.hovered_link.clone();
        let pal = &style.palette;
        if self.flash.as_ref().is_some_and(|(_, t0)| now - t0 >= 1.2) {
            self.flash = None;
        }
        let matches = &self.find.matches;
        for i in a..b {
            let Some(lb) = self.slots[i].lb.clone() else {
                continue;
            };
            let origin = vec2(col_left, oy + self.tops[i]);
            // Footnote flash.
            if let Some((name, t0)) = &self.flash
                && let Some((_, y, h)) = lb.anchors.iter().find(|(n, _, _)| n == name)
            {
                let age = (now - t0) as f32;
                let alpha = if age < 0.8 {
                    1.0
                } else {
                    1.0 - (age - 0.8) / 0.4
                };
                let r = Rect::from_min_size(
                    pos2(col_left - 8.0, origin.y + y - 4.0),
                    vec2(self.col_w + 16.0, h + 8.0),
                );
                painter.rect_filled(
                    r,
                    6.0,
                    pal.accent_soft.gamma_multiply(alpha.clamp(0.0, 1.0)),
                );
                ui.ctx().request_repaint();
            }
            for item in &lb.items {
                let (y0, y1) = item.y_range();
                if origin.y + y1 < rect.top() - 4.0 || origin.y + y0 > rect.bottom() + 4.0 {
                    continue;
                }
                self.paint_item(
                    doc,
                    ui,
                    &painter,
                    item,
                    origin,
                    rect,
                    style,
                    matches,
                    hover_link.as_deref(),
                    now,
                    &mut actions,
                );
            }
        }
        actions
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_item(
        &self,
        doc: &Document,
        ui: &Ui,
        painter: &Painter,
        item: &Item,
        origin: Vec2,
        clip: Rect,
        style: &Style,
        matches: &[Match],
        hover_link: Option<&str>,
        now: f64,
        actions: &mut Vec<Action>,
    ) {
        let pal = &style.palette;
        match item {
            Item::Text(t) => {
                self.paint_text(doc, ui, painter, t, origin, style, matches, hover_link)
            }
            Item::Label { pos, galley } => {
                painter.add(TextShape::new(*pos + origin, galley.clone(), pal.text));
            }
            Item::Fill {
                rect,
                color,
                radius,
            } => {
                painter.rect_filled(rect.translate(origin), *radius, *color);
            }
            Item::Frame {
                rect,
                stroke,
                radius,
            } => {
                painter.rect_stroke(rect.translate(origin), *radius, *stroke, StrokeKind::Inside);
            }
            Item::Gradient { rect, a, b } => {
                let r = rect.translate(origin);
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(r.left_top(), *a);
                mesh.colored_vertex(r.right_top(), *b);
                mesh.colored_vertex(r.right_bottom(), *b);
                mesh.colored_vertex(r.left_bottom(), *a);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(Shape::mesh(mesh));
            }
            Item::Circle {
                center,
                radius,
                fill,
                stroke,
            } => {
                painter.circle(*center + origin, *radius, *fill, *stroke);
            }
            Item::Icon { icon, rect, color } => {
                icons::paint(painter, *icon, rect.translate(origin), *color, None)
            }
            Item::Checkbox { rect, checked } => {
                let r = rect.translate(origin);
                if *checked {
                    painter.rect_filled(r, 4.0, pal.accent);
                    icons::paint(
                        painter,
                        Icon::Check,
                        r.shrink(2.0),
                        pal.on_accent,
                        Some(2.0),
                    );
                } else {
                    painter.rect_filled(r, 4.0, pal.surface);
                    painter.rect_stroke(
                        r,
                        4.0,
                        Stroke::new(1.5, pal.border_strong),
                        StrokeKind::Inside,
                    );
                }
            }
            Item::Image { rect, uri, .. } => {
                let r = rect.translate(origin);
                paint_image(ui.ctx(), painter, uri, r, 6);
            }
            Item::Chip {
                rect,
                label,
                tooltip,
            } => {
                let r = rect.translate(origin);
                paint_chip(painter, r, label, pal);
                let resp = ui.interact(
                    r,
                    self.id.with(("chip", uri_hash(tooltip), r.top() as i32)),
                    Sense::hover(),
                );
                resp.on_hover_text(tooltip.clone());
            }
            Item::Chevron { rect, open, color } => {
                let r = rect.translate(origin);
                let id = self
                    .id
                    .with(("chev", r.left() as i32, (r.top() - origin.y) as i32));
                let k = ui.ctx().animate_bool_with_time(id, *open, 0.12);
                paint_chevron(painter, r, k, *color);
            }
            Item::Button(btn) => {
                self.paint_button(doc, ui, painter, btn, origin, style, now, actions)
            }
            Item::Scroll(s) => {
                let off = self.hscroll.get(&s.id).copied().unwrap_or(0.0);
                let frame = s.frame.translate(origin);
                let inner_clip = frame.intersect(clip);
                if !inner_clip.is_positive() {
                    return;
                }
                let p2 = painter.with_clip_rect(inner_clip);
                let inner_origin = origin + vec2(s.frame.left() - off, 0.0);
                for it in &s.items {
                    let (y0, y1) = it.y_range();
                    if origin.y + y1 < clip.top() || origin.y + y0 > clip.bottom() {
                        continue;
                    }
                    self.paint_item(
                        doc,
                        ui,
                        &p2,
                        it,
                        inner_origin,
                        inner_clip,
                        style,
                        matches,
                        hover_link,
                        now,
                        actions,
                    );
                }
                let max = s.content_w - s.frame.width();
                if max > 0.5 {
                    // Horizontal thumb on hover.
                    let hovered = ui.rect_contains_pointer(frame);
                    let id = self.id.with(("hthumb", s.id));
                    let alpha = ui.ctx().animate_bool_with_time(id, hovered, 0.15);
                    if alpha > 0.0 {
                        let track_w = frame.width() - 8.0;
                        let tw = (track_w * frame.width() / s.content_w).max(24.0);
                        let tx = frame.left() + 4.0 + (track_w - tw) * (off / max);
                        let tr = Rect::from_min_size(pos2(tx, frame.bottom() - 8.0), vec2(tw, 6.0));
                        painter.rect_filled(tr, 3.0, pal.text.gamma_multiply(0.22 * alpha));
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_text(
        &self,
        doc: &Document,
        ui: &Ui,
        painter: &Painter,
        t: &TextItem,
        origin: Vec2,
        style: &Style,
        matches: &[Match],
        hover_link: Option<&str>,
    ) {
        let pal = &style.palette;
        let g = &t.galley;
        let gpos = t.pos + origin;
        let gv = gpos.to_vec2();
        // Background decorations.
        for d in &t.decos {
            let r = d.rect.translate(gv);
            match d.kind {
                DecoKind::CodeBg => {
                    painter.rect_filled(r, 4.0, pal.icode_bg);
                }
                DecoKind::Mark => {
                    painter.rect_filled(r, 3.0, pal.alert.warning.tint);
                }
                DecoKind::Kbd => {
                    painter.rect_filled(r, 4.0, pal.border_strong);
                    let inner = Rect::from_min_max(r.min + vec2(1.0, 1.0), r.max - vec2(1.0, 2.0));
                    painter.rect_filled(inner, 3.0, pal.surface);
                }
                DecoKind::Line(_) => {}
            }
        }
        let n_chars = g
            .rows
            .iter()
            .map(|r| r.char_count_including_newline().0)
            .sum::<usize>();
        let sel = self.sel_range(t.run, n_chars);
        let lo = matches.partition_point(|m| m.run < t.run);
        let hi = matches.partition_point(|m| m.run <= t.run);
        if sel.is_some() || lo < hi {
            let starts = layout::row_starts(g);
            let line_box = |ri: usize| {
                let row = &g.rows[ri];
                let bl = row.pos.y + t.asc + (row.size.y - t.line_h) + t.shift;
                let top = bl - t.asc - t.shift;
                (top, top + t.line_h)
            };
            if let Some(range) = sel {
                for (ri, x0, x1) in layout::segments(g, &starts, range) {
                    let (top, bottom) = line_box(ri);
                    painter.rect_filled(
                        Rect::from_min_max(pos2(x0, top), pos2(x1, bottom)).translate(gv),
                        0.0,
                        pal.selection,
                    );
                }
            }
            for (mi, m) in matches[lo..hi].iter().enumerate() {
                let current = self.find.current == Some(lo + mi);
                for (ri, x0, x1) in layout::segments(g, &starts, m.start as usize..m.end as usize) {
                    let (top, bottom) = line_box(ri);
                    let r =
                        Rect::from_min_max(pos2(x0 - 1.0, top + 2.0), pos2(x1 + 1.0, bottom - 2.0))
                            .translate(gv);
                    if current {
                        painter.rect_filled(r, 3.0, pal.find_current);
                        painter.rect_stroke(
                            r,
                            3.0,
                            Stroke::new(1.0, pal.find_current_ring),
                            StrokeKind::Outside,
                        );
                    } else {
                        painter.rect_filled(r, 3.0, pal.find_match);
                    }
                }
            }
        }
        // Glyphs (half-leading shift).
        painter.add(TextShape::new(
            gpos + vec2(0.0, t.shift),
            g.clone(),
            pal.text,
        ));
        // Dark theme: the current find match's text turns `bg` (SPEC §5).
        if style.theme.is_dark()
            && let Some(cur) = self.find.current
            && cur >= lo
            && cur < hi
        {
            let m = matches[cur];
            let starts = layout::row_starts(g);
            for (ri, x0, x1) in layout::segments(g, &starts, m.start as usize..m.end as usize) {
                let row = &g.rows[ri];
                let r = Rect::from_min_max(pos2(x0, row.pos.y), pos2(x1, row.pos.y + row.size.y))
                    .translate(gv);
                let clipped = painter.with_clip_rect(r.intersect(painter.clip_rect()));
                clipped.add(
                    TextShape::new(gpos + vec2(0.0, t.shift), g.clone(), pal.bg)
                        .with_override_text_color(pal.bg),
                );
            }
        }
        // Lines.
        for d in &t.decos {
            if let DecoKind::Line(c) = d.kind {
                painter.rect_filled(d.rect.translate(gv), 0.0, c);
            }
        }
        let rt_links = &doc.p.texts[t.run as usize].links;
        for l in &t.links {
            let hovered = hover_link
                .is_some_and(|h| rt_links.get(l.link as usize).is_some_and(|x| x.href == h));
            for u in &l.underline {
                let r = u.translate(gv);
                if hovered {
                    let r = Rect::from_center_size(r.center(), vec2(r.width(), 1.5));
                    painter.rect_filled(r, 0.0, pal.link);
                } else {
                    painter.rect_filled(r, 0.0, pal.link.gamma_multiply(0.4));
                }
            }
        }
        // Inline objects.
        for o in &t.objects {
            let r = o.rect.translate(gv);
            match &o.kind {
                ObjKind::ExternalIcon => icons::paint(
                    painter,
                    Icon::ArrowUpRight,
                    r,
                    pal.link.gamma_multiply(0.7),
                    Some((r.width() / 10.0).max(1.2)),
                ),
                ObjKind::Image { uri } => {
                    if r.width() > 0.0 && r.height() > 0.0 {
                        paint_image(ui.ctx(), painter, uri, r, 0);
                    }
                }
                ObjKind::Chip { label, .. } => paint_chip(painter, r, label, pal),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_button(
        &self,
        doc: &Document,
        ui: &Ui,
        painter: &Painter,
        btn: &layout::ButtonItem,
        origin: Vec2,
        style: &Style,
        now: f64,
        actions: &mut Vec<Action>,
    ) {
        let pal = &style.palette;
        let r = btn.rect.translate(origin);
        match btn.kind {
            ButtonKind::Toggle { id } => {
                let resp = ui.interact(r, self.id.with(("toggle", id)), Sense::click());
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    actions.push(Action::Toggle(id));
                }
            }
            ButtonKind::CopyCode { run, floating } => {
                let copied_at = self.copied.get(&run).copied();
                let copied = copied_at.is_some_and(|t| now - t < 1.5);
                let reveal = btn.reveal.translate(origin);
                let reveal_hovered = ui.rect_contains_pointer(reveal);
                if floating && !reveal_hovered && !copied {
                    return;
                }
                let resp = ui.interact(r, self.id.with(("copy", run)), Sense::click());
                let hovered = resp.hovered();
                if hovered {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    actions.push(Action::Copy(run));
                }
                let (icon, color, label) = if copied {
                    (Icon::Check, pal.alert.tip.fg, Some("Copied"))
                } else if hovered {
                    (Icon::Copy, pal.text, Some("Copy"))
                } else {
                    (Icon::Copy, pal.muted, None)
                };
                let icon_rect = Rect::from_center_size(r.center(), Vec2::splat(14.0));
                let label_g = label.map(|l| {
                    ui.ctx().fonts_mut(|f| {
                        f.layout_no_wrap(
                            l.to_owned(),
                            FontId::new(12.0, crate::fonts::family(crate::fonts::Face::Sans, 500)),
                            color,
                        )
                    })
                });
                let lw = label_g.as_ref().map_or(0.0, |g| g.size().x + 6.0);
                if floating || label_g.is_some() {
                    let bg = Rect::from_min_max(pos2(r.left() - lw, r.top()), r.max);
                    painter.rect_filled(bg, 6.0, pal.code_bg);
                }
                if let Some(g) = label_g {
                    let pos = pos2(r.left() - lw + 4.0, r.center().y - g.size().y / 2.0);
                    painter.add(TextShape::new(pos, g, color));
                }
                icons::paint(painter, icon, icon_rect, color, Some(1.3));
                if copied {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(200));
                }
                let _ = doc;
            }
        }
    }

    fn paint_scrollbar(&mut self, ui: &Ui, rect: Rect, pal: &crate::Palette, now: f64) {
        let max = self.max_scroll();
        if max <= 0.0 {
            return;
        }
        let ctx = ui.ctx().clone();
        let zone = Rect::from_min_max(
            pos2(rect.right() - SCROLLBAR_ZONE, rect.top() + self.top_inset),
            rect.right_bottom(),
        );
        let resp = ui.interact(zone, self.id.with("scrollbar"), Sense::click_and_drag());
        let track = Rect::from_min_max(
            pos2(zone.left(), zone.top() + 4.0),
            pos2(zone.right(), zone.bottom() - 4.0),
        );
        let total = self.top_pad() + self.doc_height() + 0.4 * self.viewport_h;
        let thumb_h = (track.height() * self.viewport_h / total).clamp(32.0, track.height());
        let thumb_y = track.top() + (track.height() - thumb_h) * (self.scroll_y / max);
        let hovered = resp.hovered() || resp.dragged();
        if hovered {
            self.sb.last_active = now;
        }
        // Drag thumb / click track.
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos()
        {
            if p.y >= thumb_y && p.y <= thumb_y + thumb_h {
                self.sb.drag_grab = Some(p.y - thumb_y);
            } else {
                let page = self.viewport_h - 64.0;
                let to = if p.y < thumb_y {
                    self.scroll_y - page
                } else {
                    self.scroll_y + page
                };
                self.animate_to(to.clamp(0.0, max), now, 0.12);
                self.sb.drag_grab = None;
            }
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
            && !(p.y >= thumb_y && p.y <= thumb_y + thumb_h)
        {
            let page = self.viewport_h - 64.0;
            let to = if p.y < thumb_y {
                self.scroll_y - page
            } else {
                self.scroll_y + page
            };
            self.animate_to(to.clamp(0.0, max), now, 0.12);
        }
        if resp.dragged()
            && let (Some(grab), Some(p)) = (self.sb.drag_grab, resp.interact_pointer_pos())
        {
            let k =
                ((p.y - grab - track.top()) / (track.height() - thumb_h).max(1.0)).clamp(0.0, 1.0);
            self.anim = None;
            self.scroll_y = k * max;
            self.stick_bottom = k >= 1.0;
        }
        if resp.drag_stopped() {
            self.sb.drag_grab = None;
        }
        let since = (now - self.sb.last_active) as f32;
        let alpha = if self.sb.drag_grab.is_some() || hovered || since < 0.8 {
            1.0
        } else {
            (1.0 - (since - 0.8) / 0.3).clamp(0.0, 1.0)
        };
        if alpha > 0.0 && alpha < 1.0 || since < 0.8 {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        let wide = ctx.animate_bool_with_time(
            self.id.with("sb-wide"),
            hovered || self.sb.drag_grab.is_some(),
            0.12,
        );
        let w = 6.0 + 4.0 * wide;
        let a = if self.sb.drag_grab.is_some() {
            0.5
        } else if hovered {
            0.38
        } else {
            0.22
        };
        if alpha > 0.0 {
            let thumb =
                Rect::from_min_size(pos2(rect.right() - 2.0 - w, thumb_y), vec2(w, thumb_h));
            ui.painter().rect_filled(
                thumb,
                CornerRadius::same((w / 2.0) as u8),
                pal.text.gamma_multiply(a * alpha),
            );
        }
    }
}

/// Public scroll requests.
pub(crate) enum ScrollReqPub {
    Y(f32),
    Bottom,
    Anchor(String),
    Heading(usize),
}

/// Paint an image through egui's loaders (handles animated GIF frames and SVG rasterizing at
/// the painted pixel size). Nothing is drawn while loading: local content never spins.
fn paint_image(ctx: &egui::Context, painter: &Painter, uri: &str, rect: Rect, radius: u8) {
    let ppp = ctx.pixels_per_point();
    let rect = rect.round_to_pixels(ppp);
    let px = (rect.size() * ppp).round();
    if px.x < 1.0 || px.y < 1.0 {
        return;
    }
    let image = egui::Image::new(uri);
    let tlr = image.source(ctx).clone().load(
        ctx,
        egui::TextureOptions::LINEAR,
        egui::load::SizeHint::Size {
            width: px.x as u32,
            height: px.y as u32,
            maintain_aspect_ratio: false,
        },
    );
    if let Ok(egui::load::TexturePoll::Ready { texture }) = tlr {
        painter.add(
            egui::epaint::RectShape::filled(rect, CornerRadius::same(radius), Color32::WHITE)
                .with_texture(
                    texture.id,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                ),
        );
    }
}

fn uri_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn load_reason(e: &egui::load::LoadError, uri: &str) -> String {
    use egui::load::LoadError as E;
    match e {
        E::NotSupported | E::FormatNotSupported { .. } | E::NoMatchingImageLoader { .. } => {
            "Unsupported".into()
        }
        E::NoImageLoaders | E::NoMatchingBytesLoader | E::NoMatchingTextureLoader => {
            if uri.starts_with("http") {
                "Network".into()
            } else {
                "Unsupported".into()
            }
        }
        E::Loading(msg) => {
            let m = msg.to_ascii_lowercase();
            if m.contains("no such file")
                || m.contains("not found")
                || m.contains("cannot find")
                || m.contains("os error 2")
            {
                "Not found".into()
            } else if m.contains("too large") || m.contains("limit") {
                "Too large".into()
            } else if uri.starts_with("http") {
                "Network".into()
            } else if m.contains("format") || m.contains("decode") || m.contains("unsupported") {
                "Unsupported".into()
            } else {
                "Not found".into()
            }
        }
    }
}

fn paint_chip(painter: &Painter, r: Rect, label: &Arc<Galley>, pal: &crate::Palette) {
    // Dashed 1 px border, radius 6.
    let stroke = Stroke::new(1.0, pal.border_strong);
    let rr = r.shrink(0.5);
    let rad = 6.0;
    let segs = [
        (
            pos2(rr.left() + rad, rr.top()),
            pos2(rr.right() - rad, rr.top()),
        ),
        (
            pos2(rr.right(), rr.top() + rad),
            pos2(rr.right(), rr.bottom() - rad),
        ),
        (
            pos2(rr.right() - rad, rr.bottom()),
            pos2(rr.left() + rad, rr.bottom()),
        ),
        (
            pos2(rr.left(), rr.bottom() - rad),
            pos2(rr.left(), rr.top() + rad),
        ),
    ];
    for (a, b) in segs {
        painter.extend(Shape::dashed_line(&[a, b], stroke, 3.0, 2.5));
    }
    for (c, a0) in [
        (pos2(rr.left() + rad, rr.top() + rad), 180.0f32),
        (pos2(rr.right() - rad, rr.top() + rad), 270.0),
        (pos2(rr.right() - rad, rr.bottom() - rad), 0.0),
        (pos2(rr.left() + rad, rr.bottom() - rad), 90.0),
    ] {
        let pts: Vec<Pos2> = (0..=6)
            .map(|k| {
                let a = (a0 + 15.0 * k as f32).to_radians();
                c + vec2(a.cos(), a.sin()) * rad
            })
            .collect();
        painter.add(Shape::line(pts, stroke));
    }
    let icon = Rect::from_min_size(pos2(r.left() + 8.0, r.center().y - 8.0), Vec2::splat(16.0));
    icons::paint(painter, Icon::ImageOff, icon, pal.muted, None);
    painter.add(TextShape::new(
        pos2(icon.right() + 6.0, r.center().y - label.size().y / 2.0),
        label.clone(),
        pal.muted,
    ));
}

fn paint_chevron(painter: &Painter, r: Rect, open: f32, color: Color32) {
    // Right-pointing chevron rotated by 90° × open.
    let s = r.width() / 24.0;
    let c = r.center();
    let ang = open * std::f32::consts::FRAC_PI_2;
    let (sa, ca) = ang.sin_cos();
    let pts: Vec<Pos2> = [(-3.0f32, -6.0f32), (3.0, 0.0), (-3.0, 6.0)]
        .iter()
        .map(|(x, y)| c + vec2(x * ca - y * sa, x * sa + y * ca) * s)
        .collect();
    painter.add(Shape::line(pts, Stroke::new((1.75 * s).max(1.3), color)));
}

/// Containers (details, front matter) that are closed and contain `run`.
fn collapsed_containers(b: &Block, run: RunId, toggled: &HashSet<u64>) -> Vec<u64> {
    fn contains(b: &Block, run: RunId) -> bool {
        let mut runs = Vec::new();
        collect_runs(b, &mut runs);
        runs.contains(&run)
    }
    fn walk(b: &Block, run: RunId, toggled: &HashSet<u64>, out: &mut Vec<u64>) {
        match &b.kind {
            BlockKind::Details { open, blocks, .. } => {
                let is_open = *open != toggled.contains(&b.id);
                if !is_open && blocks.iter().any(|c| contains(c, run)) {
                    out.push(b.id);
                }
                for c in blocks {
                    walk(c, run, toggled, out);
                }
            }
            BlockKind::FrontMatter(fm) => {
                if fm.run == run && !toggled.contains(&b.id) {
                    out.push(b.id);
                }
            }
            BlockKind::List(l) => {
                for it in &l.items {
                    for c in &it.blocks {
                        walk(c, run, toggled, out);
                    }
                }
            }
            BlockKind::Quote(bs) | BlockKind::Center(bs) | BlockKind::Alert { blocks: bs, .. } => {
                for c in bs {
                    walk(c, run, toggled, out);
                }
            }
            BlockKind::Footnotes(n) => {
                for note in n {
                    for c in &note.blocks {
                        walk(c, run, toggled, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(b, run, toggled, &mut out);
    out
}

fn collect_runs(b: &Block, out: &mut Vec<RunId>) {
    match &b.kind {
        BlockKind::Heading { run, .. } | BlockKind::Paragraph { run, .. } => out.push(*run),
        BlockKind::Code(c) => out.push(c.run),
        BlockKind::List(l) => {
            for it in &l.items {
                for c in &it.blocks {
                    collect_runs(c, out);
                }
            }
        }
        BlockKind::Quote(bs) | BlockKind::Center(bs) | BlockKind::Alert { blocks: bs, .. } => {
            for c in bs {
                collect_runs(c, out);
            }
        }
        BlockKind::Details {
            summary, blocks, ..
        } => {
            out.push(*summary);
            for c in blocks {
                collect_runs(c, out);
            }
        }
        BlockKind::Table(t) => {
            out.extend(t.header.iter().copied());
            for r in &t.rows {
                out.extend(r.iter().copied());
            }
        }
        BlockKind::FrontMatter(f) => out.push(f.run),
        BlockKind::Footnotes(n) => {
            for note in n {
                for c in &note.blocks {
                    collect_runs(c, out);
                }
            }
        }
        BlockKind::Image { .. } | BlockKind::Rule | BlockKind::Anchor(_) => {}
    }
}

/// Map a top-level block of the old document to the new one (SPEC §7 anchor fallbacks):
/// same key → same content anywhere in the same section → same ordinal in the section →
/// the section's heading → proportional position. Returns (index, exact).
fn map_block(old: &Document, new: &Document, i: usize) -> (usize, bool) {
    let n = new.p.blocks.len();
    if n == 0 {
        return (0, false);
    }
    let Some(k) = old.p.top_keys.get(i) else {
        return (0, false);
    };
    let keys = &new.p.top_keys;
    if let Some(j) = keys.iter().position(|nk| nk == k) {
        return (j, true);
    }
    if let Some(j) = keys
        .iter()
        .position(|nk| nk.path == k.path && nk.content == k.content)
    {
        return (j, true);
    }
    if let Some(j) = keys.iter().position(|nk| nk.content == k.content) {
        return (j, true);
    }
    if let Some(j) = keys
        .iter()
        .position(|nk| nk.path == k.path && nk.ordinal == k.ordinal)
    {
        return (j, false);
    }
    // Nearest preceding heading.
    for oi in (0..i).rev() {
        if matches!(old.p.blocks[oi].kind, BlockKind::Heading { .. })
            && let Some(j) = keys.iter().position(|nk| *nk == old.p.top_keys[oi])
        {
            return (j, false);
        }
    }
    let frac = i as f32 / old.p.blocks.len().max(1) as f32;
    (((frac * n as f32) as usize).min(n - 1), false)
}
