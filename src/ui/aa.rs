//! The Aa popover (SPEC §8): theme, font, text size, width and code wrapping, applied live.

use egui::{
    Align2, Area, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use engine::{Palette, ThemeKind};

use super::widgets::{self, ButtonState, Glyph, font, icon_button, segmented, switch};
use super::{Action, BAR_H};
use crate::icons::Icon;
use crate::settings::{DEFAULT_TEXT_SIZE, FontPref, Settings, TEXT_SIZES, ThemePref, Width};

const W: f32 = 296.0;
const PAD: f32 = 16.0;
const H: f32 = 372.0;

/// Show the popover anchored below the Aa button. Returns its rect.
/// `theme` is the effective preference (a QA `--theme` override wins over the saved one).
pub fn show(
    ctx: &egui::Context,
    screen: Rect,
    anchor: Rect,
    s: &Settings,
    theme: ThemePref,
    p: &Palette,
    actions: &mut Vec<Action>,
) -> Rect {
    let x = (anchor.right() - W).clamp(
        screen.left() + 8.0,
        (screen.right() - W - 8.0).max(screen.left() + 8.0),
    );
    let rect = Rect::from_min_size(pos2(x, screen.top() + BAR_H + 4.0), vec2(W, H));
    Area::new(Id::new("aa-popover"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            widgets::paint_card(ui.painter(), rect, p, 12);
            let left = rect.left() + PAD;
            let inner_w = W - 2.0 * PAD;
            let mut y = rect.top() + PAD;

            // 1. Theme swatches.
            label(ui, pos2(left, y), "Theme", p);
            y += 24.0;
            let gap = (inner_w - 4.0 * 56.0) / 3.0;
            for (i, pref) in ThemePref::ALL.into_iter().enumerate() {
                let r =
                    Rect::from_min_size(pos2(left + i as f32 * (56.0 + gap), y), vec2(56.0, 40.0));
                let resp = ui.interact(r, Id::new(("theme-swatch", i)), Sense::click());
                let selected = theme == pref;
                paint_swatch(ui, r, pref, p);
                if selected {
                    ui.painter().rect_stroke(
                        r.expand(2.0),
                        10.0,
                        Stroke::new(2.0, p.accent),
                        StrokeKind::Outside,
                    );
                } else if resp.hovered() {
                    ui.painter().rect_stroke(
                        r.expand(2.0),
                        10.0,
                        Stroke::new(2.0, p.border_strong),
                        StrokeKind::Outside,
                    );
                }
                let caption = if selected { p.text } else { p.muted };
                ui.painter().text(
                    pos2(r.center().x, r.bottom() + 6.0),
                    Align2::CENTER_TOP,
                    pref.label(),
                    font(11.0),
                    caption,
                );
                if widgets::tooltip(resp, pref.label(), None, p).clicked() {
                    actions.push(Action::SetTheme(pref));
                }
            }
            y += 40.0 + 6.0 + 14.0 + 16.0;

            // 2. Font.
            label(ui, pos2(left, y), "Font", p);
            y += 24.0;
            let row = Rect::from_min_size(pos2(left, y), vec2(inner_w, 32.0));
            let sel = usize::from(s.font == FontPref::Serif);
            if let Some(i) = segmented(ui, row, "font", &["Sans", "Serif"], sel, p) {
                actions.push(Action::SetFont(if i == 0 {
                    FontPref::Sans
                } else {
                    FontPref::Serif
                }));
            }
            y += 32.0 + 16.0;

            // 3. Text size stepper.
            label(ui, pos2(left, y), "Text size", p);
            y += 24.0;
            let track = Rect::from_min_size(pos2(left, y), vec2(140.0, 32.0));
            ui.painter()
                .rect_filled(track, 8.0, widgets::track_colors(p).0);
            let minus = Rect::from_min_size(track.min + vec2(2.0, 2.0), vec2(28.0, 28.0));
            let plus = Rect::from_min_size(
                pos2(track.right() - 30.0, track.top() + 2.0),
                vec2(28.0, 28.0),
            );
            let at_min = s.text_size <= TEXT_SIZES[0];
            let at_max = s.text_size >= TEXT_SIZES[TEXT_SIZES.len() - 1];
            let st = |disabled| ButtonState {
                toggled: false,
                disabled,
            };
            let r = icon_button(
                ui,
                minus,
                "size-down",
                Glyph::Icon(Icon::Minus, 16.0),
                st(at_min),
                p,
            );
            if widgets::tooltip(r, "Smaller", Some("Ctrl+−"), p).clicked() {
                actions.push(Action::StepTextSize(-1));
            }
            let r = icon_button(
                ui,
                plus,
                "size-up",
                Glyph::Icon(Icon::Plus, 16.0),
                st(at_max),
                p,
            );
            if widgets::tooltip(r, "Larger", Some("Ctrl++"), p).clicked() {
                actions.push(Action::StepTextSize(1));
            }
            ui.painter().text(
                track.center(),
                Align2::CENTER_CENTER,
                format!("{} px", s.text_size),
                font(13.0),
                p.text,
            );
            if s.text_size != DEFAULT_TEXT_SIZE {
                let g = widgets::galley(ui.painter(), "Default", 12.5, p.link, None);
                let r = Rect::from_min_size(
                    pos2(
                        rect.right() - PAD - g.size().x,
                        track.center().y - g.size().y / 2.0,
                    ),
                    g.size(),
                );
                let resp = ui.interact(r.expand(4.0), Id::new("size-default"), Sense::click());
                if resp.hovered() {
                    ui.painter()
                        .hline(r.x_range(), r.bottom(), Stroke::new(1.0, p.link));
                }
                ui.painter().galley(r.min, g, p.link);
                if widgets::tooltip(resp, "Reset to 16 px", Some("Ctrl+0"), p).clicked() {
                    actions.push(Action::SetTextSize(DEFAULT_TEXT_SIZE));
                }
            }
            y += 32.0 + 16.0;

            // 4. Width.
            label(ui, pos2(left, y), "Width", p);
            y += 24.0;
            let row = Rect::from_min_size(pos2(left, y), vec2(inner_w, 32.0));
            let labels: Vec<&str> = Width::ALL.iter().map(|w| w.label()).collect();
            let sel = Width::ALL.iter().position(|&w| w == s.width).unwrap_or(1);
            if let Some(i) = segmented(ui, row, "width", &labels, sel, p) {
                actions.push(Action::SetWidth(Width::ALL[i]));
            }
            y += 32.0 + 16.0;

            // 5. Wrap long code lines.
            let row = Rect::from_min_size(pos2(left, y), vec2(inner_w, 18.0));
            label(
                ui,
                pos2(left, row.center().y - 8.0),
                "Wrap long code lines",
                p,
            );
            let sw = Rect::from_min_size(pos2(row.right() - 32.0, row.top()), vec2(32.0, 18.0));
            if switch(ui, sw, "wrap", s.wrap_code, p).clicked() {
                actions.push(Action::SetWrap(!s.wrap_code));
            }
        });
    rect
}

fn label(ui: &Ui, pos: Pos2, text: &str, p: &Palette) {
    ui.painter()
        .text(pos, Align2::LEFT_TOP, text, font(12.0), p.muted);
}

/// A theme swatch: its own `bg`, "Aa" in its `text` and a 6 px `accent` dot. Auto is split
/// diagonally, light top-left and dark bottom-right.
fn paint_swatch(ui: &Ui, r: Rect, pref: ThemePref, current: &Palette) {
    let painter = ui.painter();
    let light = Palette::light();
    let dark = Palette::dark();
    let pal = match pref {
        ThemePref::Light | ThemePref::Auto => light.clone(),
        ThemePref::Sepia => Palette::for_theme(ThemeKind::Sepia),
        ThemePref::Dark => dark.clone(),
    };
    painter.rect_filled(r, 8.0, pal.bg);
    let text_galley = |c: Color32| painter.layout_no_wrap("Aa".into(), font(15.0), c);
    let g = text_galley(pal.text);
    let text_pos = pos2(
        r.center().x - (g.size().x + 9.0) / 2.0,
        r.center().y - g.size().y / 2.0,
    );
    let dot = pos2(text_pos.x + g.size().x + 6.0, r.center().y + 3.0);

    if pref == ThemePref::Auto {
        // Dark half as a clipped rounded-rect polygon, so the diagonal edge is anti-aliased.
        let a = r.right_top();
        let b = r.left_bottom();
        let dark_side = |q: Pos2| (b - a).x * (q - a).y - (b - a).y * (q - a).x <= 0.0;
        let poly = clip_half_plane(&widgets::rounded_rect_points(r, 8.0), a, b, dark_side);
        painter.add(Shape::convex_polygon(poly, dark.bg, Stroke::NONE));
        // "Aa" in each half's text color, drawn through one-pixel column clips along the diagonal.
        let px = 1.0 / ui.ctx().pixels_per_point();
        let dark_g = text_galley(dark.text);
        let mut x = text_pos.x;
        while x < text_pos.x + g.size().x {
            let t = (x + px / 2.0 - r.left()) / r.width();
            let y_split = egui::lerp(r.bottom()..=r.top(), t);
            let col = |y0: f32, y1: f32| Rect::from_min_max(pos2(x, y0), pos2(x + px, y1));
            painter
                .with_clip_rect(col(r.top(), y_split))
                .galley(text_pos, g.clone(), light.text);
            painter.with_clip_rect(col(y_split, r.bottom())).galley(
                text_pos,
                dark_g.clone(),
                dark.text,
            );
            x += px;
        }
        let in_dark = dark_side(dot);
        painter.circle_filled(dot, 3.0, if in_dark { dark.accent } else { light.accent });
    } else {
        painter.galley(text_pos, g, pal.text);
        painter.circle_filled(dot, 3.0, pal.accent);
    }
    // Keep light swatches visible on a light surface.
    painter.rect_stroke(r, 8.0, Stroke::new(1.0, current.border), StrokeKind::Inside);
}

/// Sutherland–Hodgman clip of a convex polygon to the side of line a→b where `keep` holds.
fn clip_half_plane(poly: &[Pos2], a: Pos2, b: Pos2, keep: impl Fn(Pos2) -> bool) -> Vec<Pos2> {
    let intersect = |p: Pos2, q: Pos2| {
        let d = b - a;
        let cross = |v: egui::Vec2| d.x * v.y - d.y * v.x;
        let (fp, fq) = (cross(p - a), cross(q - a));
        p + (q - p) * (fp / (fp - fq))
    };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (cur, next) = (poly[i], poly[(i + 1) % poly.len()]);
        match (keep(cur), keep(next)) {
            (true, true) => out.push(next),
            (true, false) => out.push(intersect(cur, next)),
            (false, true) => {
                out.push(intersect(cur, next));
                out.push(next);
            }
            (false, false) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_plane_clip_keeps_one_triangle_of_a_square() {
        let sq = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
        ];
        let (a, b) = (pos2(10.0, 0.0), pos2(0.0, 10.0));
        let side = |q: Pos2| (b - a).x * (q - a).y - (b - a).y * (q - a).x <= 0.0;
        assert!(
            side(pos2(10.0, 10.0)) && !side(pos2(0.0, 0.0)),
            "bottom-right side is kept"
        );
        let out = clip_half_plane(&sq, a, b, side);
        // Bottom-right triangle: area 50.
        let area: f32 = (0..out.len())
            .map(|i| {
                let (p, q) = (out[i], out[(i + 1) % out.len()]);
                p.x * q.y - q.x * p.y
            })
            .sum::<f32>()
            .abs()
            / 2.0;
        assert!((area - 50.0).abs() < 1e-3, "{area}");
    }
}
