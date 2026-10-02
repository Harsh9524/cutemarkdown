//! Reading progress (SPEC §3): 2 px at y = 0, above everything, `grad-a → grad-b`.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Id, LayerId, Order, Rect, pos2};
use engine::Palette;

pub fn show(ctx: &egui::Context, screen: Rect, progress: f32, p: &Palette) {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("progress-line")));
    // The gradient spans the full width; the line reveals it up to `progress`.
    let rect = Rect::from_min_max(
        screen.left_top(),
        pos2(
            screen.left() + screen.width() * progress,
            screen.top() + 2.0,
        ),
    );
    let right = p.grad_a.lerp_to_gamma(p.grad_b, progress);
    let mut mesh = Mesh::default();
    let v = |pos, color| Vertex {
        pos,
        uv: WHITE_UV,
        color,
    };
    mesh.vertices.extend([
        v(rect.left_top(), p.grad_a),
        v(rect.right_top(), right),
        v(rect.right_bottom(), right),
        v(rect.left_bottom(), p.grad_a),
    ]);
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    painter.add(mesh);
}
