//! Floating layers: link status pill, drag overlay and the keyboard shortcut card (SPEC §3, §7).

use egui::{Align2, Area, Id, Order, Rect, Sense, Shape, Stroke, pos2, vec2};
use engine::Palette;

use super::Action;
use super::widgets::{self, ButtonState, Glyph, font, icon_button};
use crate::icons::{self, Icon};

/// Link status pill: bottom-left, 8 px inset, 24 px, middle ellipsis, ≤ 60% of the window.
pub fn link_pill(ctx: &egui::Context, screen: Rect, url: &str, p: &Palette) {
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("link-pill")));
    let max_text = screen.width() * 0.6 - 20.0;
    let text = widgets::middle_ellipsis(&painter, url, 12.0, max_text);
    let g = widgets::galley(&painter, &text, 12.0, p.muted, None);
    let rect = Rect::from_min_size(
        pos2(screen.left() + 8.0, screen.bottom() - 8.0 - 24.0),
        vec2(g.size().x + 20.0, 24.0),
    );
    painter.rect(
        rect,
        6.0,
        p.surface,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        pos2(rect.left() + 10.0, rect.center().y - g.size().y / 2.0),
        g,
        p.muted,
    );
}

/// Drag overlay: `accent-soft` at 85%, a dashed `accent` rounded rect inset 16 px, and
/// "Drop to open".
pub fn drag_overlay(ctx: &egui::Context, screen: Rect, p: &Palette) {
    Area::new(Id::new("drag-overlay"))
        .order(Order::Debug)
        .fixed_pos(screen.min)
        .interactable(false)
        .show(ctx, |ui| {
            let painter = ui.painter();
            painter.rect_filled(screen, 0.0, p.accent_soft.gamma_multiply(0.85));
            let inset = screen.shrink(16.0);
            let mut pts = widgets::rounded_rect_points(inset, 16.0);
            pts.push(pts[0]);
            painter.extend(Shape::dashed_line(
                &pts,
                Stroke::new(2.0, p.accent),
                8.0,
                6.0,
            ));
            let c = screen.center();
            icons::paint(
                ui,
                Icon::FileText,
                Rect::from_center_size(c - vec2(0.0, 18.0), vec2(32.0, 32.0)),
                32.0,
                p.link,
            );
            painter.text(
                c + vec2(0.0, 16.0),
                Align2::CENTER_TOP,
                "Drop to open",
                font(16.0),
                p.link,
            );
            ui.allocate_rect(screen, Sense::hover());
        });
}

const SHORTCUTS: [(&str, &str); 21] = [
    ("Open file", "Ctrl+O"),
    ("New window", "Ctrl+N"),
    ("Close window", "Ctrl+W"),
    ("Find", "Ctrl+F"),
    ("Next / previous match", "Enter / Shift+Enter"),
    ("Close popover, find, outline, Zen", "Esc"),
    ("Text size", "Ctrl+= / Ctrl+− / Ctrl+0"),
    ("Back / Forward", "Alt+Left / Alt+Right"),
    ("Previous / next heading", "Ctrl+Up / Ctrl+Down"),
    ("Scroll a page", "PgUp / PgDn / Space"),
    ("Top / bottom", "Home / End"),
    ("Reload", "Ctrl+R / F5"),
    ("Zen mode", "F11"),
    ("Toggle outline", "Ctrl+B"),
    ("Cycle theme", "Ctrl+Shift+L"),
    ("Reading settings", "Ctrl+,"),
    ("Open in editor", "Ctrl+E"),
    ("Reveal in Explorer", "Ctrl+Shift+E"),
    ("Copy Markdown source", "Ctrl+Shift+C"),
    ("Paste Markdown", "Ctrl+V"),
    ("Keyboard shortcuts", "Ctrl+/"),
];

/// Ctrl+/ card listing SPEC §7. Closes on Esc, outside click or ×.
pub fn shortcuts(ctx: &egui::Context, screen: Rect, p: &Palette, actions: &mut Vec<Action>) {
    const COL_W: f32 = 300.0;
    const ROW_H: f32 = 30.0;
    const PAD: f32 = 20.0;
    let rows = SHORTCUTS.len().div_ceil(2);
    let two_cols = screen.width() >= 2.0 * COL_W + 3.0 * PAD + 16.0;
    let (cols, per_col) = if two_cols {
        (2, rows)
    } else {
        (1, SHORTCUTS.len())
    };
    let w = cols as f32 * COL_W + (cols as f32 + 1.0) * PAD;
    let h = (PAD + 32.0 + per_col as f32 * ROW_H + PAD).min(screen.height() - 32.0);
    let card = Rect::from_center_size(screen.center(), vec2(w, h));
    Area::new(Id::new("shortcuts"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let scrim = ui.allocate_rect(screen, Sense::click());
            ui.painter()
                .rect_filled(screen, 0.0, p.text.gamma_multiply(0.06));
            widgets::paint_card(ui.painter(), card, p, 12);
            let inside = ui.interact(card, Id::new("shortcuts-card"), Sense::click());
            let painter = ui.painter().clone();
            painter.text(
                pos2(card.left() + PAD, card.top() + PAD + 10.0),
                Align2::LEFT_CENTER,
                "Keyboard shortcuts",
                font(15.0),
                p.text_strong,
            );
            let close = Rect::from_center_size(
                pos2(card.right() - PAD - 10.0, card.top() + PAD + 10.0),
                vec2(28.0, 28.0),
            );
            let r = icon_button(
                ui,
                close,
                "shortcuts-close",
                Glyph::Icon(Icon::X, 16.0),
                ButtonState::default(),
                p,
            );
            if widgets::tooltip(r, "Close", Some("Esc"), p).clicked() {
                actions.push(Action::ToggleShortcuts);
            }
            let body = painter.with_clip_rect(card.shrink(1.0));
            for (i, (label, keys)) in SHORTCUTS.iter().enumerate() {
                let (col, row) = (i / per_col, i % per_col);
                let x = card.left() + PAD + col as f32 * (COL_W + PAD);
                let y = card.top() + PAD + 32.0 + row as f32 * ROW_H + ROW_H / 2.0;
                body.text(pos2(x, y), Align2::LEFT_CENTER, *label, font(13.0), p.text);
                // Keys as keycaps, right-aligned in the column.
                let mut right = x + COL_W;
                for part in keys.split(" / ").collect::<Vec<_>>().into_iter().rev() {
                    let w = keycap_width(&body, part);
                    right -= w;
                    widgets::keycap(&body, pos2(right, y), part, 13.0, p);
                    right -= 6.0;
                }
            }
            if scrim.clicked() && !inside.hovered() {
                actions.push(Action::ToggleShortcuts);
            }
        });
}

fn keycap_width(painter: &egui::Painter, label: &str) -> f32 {
    widgets::galley(painter, label, 13.0 * 0.86, egui::Color32::WHITE, None)
        .size()
        .x
        + 12.0
}
