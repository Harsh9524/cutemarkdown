//! Small chrome building blocks: cards, icon buttons, tooltips, keycaps, segmented controls,
//! switches and text helpers. All colors come from the engine palette.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Galley, Painter, Pos2, Rect, Response,
    Sense, Shadow, Stroke, StrokeKind, Ui, pos2, vec2,
};
use engine::Palette;
use engine::fonts::{self, Face};

use crate::icons::{self, Icon};

/// UI font: Inter 400.
pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Inter 500 (SPEC: recent file names, toasts, keycaps).
pub fn medium(size: f32) -> FontId {
    FontId::new(size, fonts::ui_medium())
}

/// Inter 600 (SPEC: "Aa", overlines, Aa popover labels, the primary button).
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, fonts::ui_semibold())
}

/// Inter at any of the bundled weights (400, 500, 600, 650, 700), e.g. the 20/650 empty-state
/// title.
pub fn weighted(size: f32, weight: u16) -> FontId {
    FontId::new(size, fonts::family(Face::Sans, weight))
}

/// Lay out one line of text, truncated with "…" at `max_width` if given.
pub fn galley(
    painter: &Painter,
    text: &str,
    size: f32,
    color: Color32,
    max_width: Option<f32>,
) -> Arc<Galley> {
    galley_with(painter, text, font(size), color, max_width)
}

/// [`galley`] in any font.
pub fn galley_with(
    painter: &Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: Option<f32>,
) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    if let Some(w) = max_width {
        job.wrap = TextWrapping::truncate_at_width(w.max(1.0));
    }
    painter.layout_job(job)
}

/// Uppercase overline, 11/600 with +0.08em tracking (`CONTENTS`, `RECENT`).
pub fn overline(painter: &Painter, pos: Pos2, text: &str, color: Color32) -> Rect {
    let size = 11.0;
    let format = TextFormat {
        font_id: semibold(size),
        color,
        extra_letter_spacing: 0.08 * size,
        ..Default::default()
    };
    let g = painter.layout_job(LayoutJob::single_section(text.to_uppercase(), format));
    let rect = Rect::from_min_size(pos, g.size());
    painter.galley(pos, g, color);
    rect
}

/// Shorten `text` in the middle ("C:\Users\…\docs") so it fits `max_width`.
pub fn middle_ellipsis(painter: &Painter, text: &str, size: f32, max_width: f32) -> String {
    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font(size), Color32::WHITE)
            .size()
            .x
    };
    if width(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    let candidate = |keep: usize| {
        let head = keep.div_ceil(2);
        let tail = keep / 2;
        let mut s: String = chars[..head].iter().collect();
        s.push('…');
        s.extend(&chars[chars.len() - tail..]);
        s
    };
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&candidate(mid)) <= max_width {
            lo = mid
        } else {
            hi = mid - 1
        }
    }
    candidate(lo)
}

/// Popover/menu/card surface (SPEC §3): (0, 8) blur 24 plus (0, 1) blur 2 shadows, `surface`
/// fill and a 1 px `border`.
pub fn paint_card(painter: &Painter, rect: Rect, p: &Palette, radius: u8) {
    let cr = CornerRadius::same(radius);
    let near = Shadow {
        offset: [0, 1],
        blur: 2,
        spread: 0,
        color: p.shadow.gamma_multiply(0.5),
    };
    painter.add(crate::theme::popup_shadow(p).as_shape(rect, cr));
    painter.add(near.as_shape(rect, cr));
    painter.rect(
        rect,
        cr,
        p.surface,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
}

/// 2 px `accent` focus ring with 2 px offset (SPEC §7 General states).
pub fn focus_ring(painter: &Painter, rect: Rect, radius: f32, p: &Palette) {
    painter.rect_stroke(
        rect.expand(2.0),
        radius + 2.0,
        Stroke::new(2.0, p.accent),
        StrokeKind::Outside,
    );
}

/// Visual state of an icon button.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonState {
    pub toggled: bool,
    pub disabled: bool,
}

/// What a button draws.
pub enum Glyph<'a> {
    Icon(Icon, f32),
    Text(&'a str, FontId),
}

/// A square chrome button (32×32 in the bar, 28×28 in the find bar). SPEC §3 states:
/// rest `muted`; hover `bg-hover` + `text`; pressed `border`; toggled `accent-soft` + `accent`;
/// disabled `faint`.
pub fn icon_button(
    ui: &mut Ui,
    rect: Rect,
    id_salt: &str,
    glyph: Glyph,
    state: ButtonState,
    p: &Palette,
) -> Response {
    let id = ui.id().with(id_salt);
    let sense = if state.disabled {
        Sense::hover()
    } else {
        Sense::click()
    };
    let resp = ui.interact(rect, id, sense);
    let painter = ui.painter();
    let hover_t = ui.ctx().animate_bool_with_time(
        id.with("hover"),
        resp.hovered() && !state.disabled,
        super::anim_secs(ui.ctx(), super::HOVER_SECS),
    );
    let (fill, fg) = if state.disabled {
        (Color32::TRANSPARENT, p.faint)
    } else if resp.is_pointer_button_down_on() {
        (p.border, p.text)
    } else if state.toggled {
        (p.accent_soft, p.accent)
    } else {
        (
            p.bg_hover.gamma_multiply(hover_t),
            lerp(p.muted, p.text, hover_t),
        )
    };
    painter.rect_filled(rect, 8.0, fill);
    match glyph {
        Glyph::Icon(icon, size) => icons::paint(ui, icon, rect, size, fg),
        Glyph::Text(text, font) => {
            painter.text(rect.center(), Align2::CENTER_CENTER, text, font, fg);
        }
    }
    if resp.has_focus() && !state.disabled {
        focus_ring(painter, rect, 8.0, p);
    }
    resp
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// Tooltip: label, then the shortcut in `muted` (SPEC §3).
pub fn tooltip(resp: Response, label: &str, shortcut: Option<&str>, p: &Palette) -> Response {
    let (label, shortcut, text, muted) = (
        label.to_owned(),
        shortcut.map(str::to_owned),
        p.text,
        p.muted,
    );
    let show = move |ui: &mut Ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(egui::RichText::new(&label).font(font(12.5)).color(text));
            if let Some(s) = &shortcut {
                ui.label(egui::RichText::new(s).font(font(12.5)).color(muted));
            }
        });
    };
    if resp.enabled() {
        resp.on_hover_ui(show)
    } else {
        resp.on_disabled_hover_ui(show)
    }
}

fn keycap_font(size: f32) -> FontId {
    medium(size * 0.86)
}

/// Width of a [`keycap`] for `label` next to `size` text.
pub fn keycap_width(painter: &Painter, label: &str, size: f32) -> f32 {
    painter
        .layout_no_wrap(label.to_owned(), keycap_font(size), Color32::WHITE)
        .size()
        .x
        + 12.0
}

/// A `<kbd>`-style keycap (SPEC §6): Inter 500 `text` on `surface`, 1 px `border-strong` with a
/// 2 px bottom edge, radius 4, padding 1×6. Returns its width.
pub fn keycap(painter: &Painter, left_center: Pos2, label: &str, size: f32, p: &Palette) -> f32 {
    let g = painter.layout_no_wrap(label.to_owned(), keycap_font(size), p.text);
    let w = g.size().x + 12.0;
    let h = g.size().y + 2.0;
    let rect = Rect::from_min_size(pos2(left_center.x, left_center.y - h / 2.0), vec2(w, h));
    // Bottom edge: a slightly lower border-strong slab behind the cap.
    painter.rect_filled(rect.translate(vec2(0.0, 1.0)), 4.0, p.border_strong);
    painter.rect(
        rect,
        4.0,
        p.surface,
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - g.size() / 2.0, g, p.text);
    w
}

/// Track and raised-segment fills for segmented controls and steppers. The selected segment
/// should look raised: lighter than its track in every theme.
pub fn track_colors(p: &Palette) -> (Color32, Color32) {
    let dark = p.text.r() > 128;
    if dark {
        (p.bg, p.bg_hover)
    } else {
        (p.bg_hover, p.surface)
    }
}

/// Segmented control; each option is a label and the family it's set in (the Font row shows
/// "Serif" in Literata). Returns the clicked index.
pub fn segmented(
    ui: &mut Ui,
    rect: Rect,
    id_salt: &str,
    options: &[(&str, FontFamily)],
    selected: usize,
    p: &Palette,
) -> Option<usize> {
    let painter = ui.painter().clone();
    let (track, raised) = track_colors(p);
    painter.rect_filled(rect, 8.0, track);
    let seg_w = (rect.width() - 4.0) / options.len() as f32;
    let mut clicked = None;
    for (i, (label, family)) in options.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(rect.left() + 2.0 + i as f32 * seg_w, rect.top() + 2.0),
            vec2(seg_w, rect.height() - 4.0),
        );
        let resp = ui.interact(r, ui.id().with(id_salt).with(i), Sense::click());
        let color = if i == selected {
            painter.rect(
                r,
                6.0,
                raised,
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            p.text_strong
        } else if resp.hovered() {
            p.text
        } else {
            p.muted
        };
        let font = FontId::new(13.0, family.clone());
        painter.text(r.center(), Align2::CENTER_CENTER, *label, font, color);
        if resp.has_focus() {
            focus_ring(&painter, r, 6.0, p);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// 32×18 switch.
pub fn switch(ui: &mut Ui, rect: Rect, id_salt: &str, on: bool, p: &Palette) -> Response {
    let id = ui.id().with(id_salt);
    let resp = ui.interact(rect, id, Sense::click());
    let t = ui
        .ctx()
        .animate_bool_with_time(id, on, super::anim_secs(ui.ctx(), super::HOVER_SECS));
    let painter = ui.painter();
    let track = lerp(p.border_strong, p.accent, t);
    painter.rect_filled(rect, rect.height() / 2.0, track);
    let r = rect.height() / 2.0 - 2.0;
    let x = egui::lerp(rect.left() + 2.0 + r..=rect.right() - 2.0 - r, t);
    painter.circle_filled(pos2(x, rect.center().y), r, lerp(p.surface, p.on_accent, t));
    if resp.has_focus() {
        focus_ring(painter, rect, rect.height() / 2.0, p);
    }
    resp
}

/// Outline of a rounded rectangle as a convex polygon (clockwise in screen space).
pub fn rounded_rect_points(r: Rect, radius: f32) -> Vec<Pos2> {
    const N: usize = 8;
    let corners = [
        (pos2(r.right() - radius, r.top() + radius), -90.0f32),
        (pos2(r.right() - radius, r.bottom() - radius), 0.0),
        (pos2(r.left() + radius, r.bottom() - radius), 90.0),
        (pos2(r.left() + radius, r.top() + radius), 180.0),
    ];
    let mut pts = Vec::with_capacity(4 * (N + 1));
    for (c, start) in corners {
        for i in 0..=N {
            let a = (start + 90.0 * i as f32 / N as f32).to_radians();
            pts.push(c + radius * vec2(a.cos(), a.sin()));
        }
    }
    pts
}
