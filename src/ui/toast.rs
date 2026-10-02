//! Toasts (SPEC §3): one slot, bottom center, inverted colors, fade + 8 px rise.

use egui::{Area, Id, Order, Rect, pos2, vec2};
use engine::Palette;

use super::Toast;
use super::widgets;
use crate::icons;

const FADE: f32 = 0.16;

/// Paint the toast. Returns false once it has expired.
pub fn show(ctx: &egui::Context, screen: Rect, toast: &Toast, p: &Palette) -> bool {
    let age = toast.born.elapsed().as_secs_f32();
    let life = toast.lifetime();
    if age >= life {
        return false;
    }
    let fade_in = (age / FADE).min(1.0);
    let fade_out = ((life - age) / FADE).clamp(0.0, 1.0);
    let alpha = egui::emath::easing::cubic_out(fade_in).min(fade_out);
    let rise = 8.0 * (1.0 - egui::emath::easing::cubic_out(fade_in));

    let (bg, fg) = (p.text, p.bg);
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::new("toast-measure")));
    let g = widgets::galley(&painter, &toast.text, 13.0, fg, Some(screen.width() * 0.8));
    let icon_w = if toast.icon.is_some() {
        14.0 + 8.0
    } else {
        0.0
    };
    let size = vec2(g.size().x + icon_w + 28.0, 32.0);
    let rect = Rect::from_min_size(
        pos2(
            screen.center().x - size.x / 2.0,
            screen.bottom() - 24.0 - size.y + rise,
        ),
        size,
    );
    Area::new(Id::new("toast"))
        .order(Order::Tooltip)
        .fixed_pos(rect.min)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            let painter = ui.painter();
            painter.add(crate::theme::popup_shadow(p).as_shape(rect, egui::CornerRadius::same(16)));
            painter.rect_filled(rect, 16.0, bg);
            let mut x = rect.left() + 14.0;
            if let Some(icon) = toast.icon {
                icons::paint(
                    ui,
                    icon,
                    Rect::from_min_size(pos2(x, rect.center().y - 7.0), vec2(14.0, 14.0)),
                    14.0,
                    fg,
                );
                x += icon_w;
            }
            painter.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
            ui.allocate_rect(rect, egui::Sense::hover());
        });
    ctx.request_repaint();
    true
}
