//! Empty state (SPEC §3): mascot, title, keycap hint, "Open file…" and recent files.

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use engine::Palette;

use super::widgets::{self, ButtonState, Glyph, font, icon_button};
use super::{Action, BAR_H};
use crate::icons::{self, Icon};
use crate::settings::{RECENT_SHOWN, RecentEntry};

const COL_W: f32 = 400.0;
const ROW_H: f32 = 44.0;

pub fn show(
    ui: &mut Ui,
    screen: Rect,
    recent: &[RecentEntry],
    p: &Palette,
    actions: &mut Vec<Action>,
) {
    let rows = recent.iter().take(RECENT_SHOWN).collect::<Vec<_>>();
    let mut total = 64.0 + 20.0 + 26.0 + 6.0 + 20.0 + 20.0 + 36.0;
    if !rows.is_empty() {
        total += 32.0 + 14.0 + 8.0 + rows.len() as f32 * ROW_H;
    }
    let top =
        (screen.top() + screen.height() * 0.42 - total / 2.0).max(screen.top() + BAR_H + 16.0);
    let col = Rect::from_min_size(
        pos2(screen.center().x - COL_W / 2.0, top),
        vec2(COL_W, total),
    );
    let cx = col.center().x;
    let painter = ui.painter().clone();

    mascot(
        &painter,
        Rect::from_min_size(pos2(cx - 32.0, top), vec2(64.0, 64.0)),
        p,
    );
    let mut y = top + 64.0 + 20.0;
    painter.text(
        pos2(cx, y),
        Align2::CENTER_TOP,
        "Open a Markdown file",
        font(20.0),
        p.text_strong,
    );
    y += 26.0 + 6.0;
    hint_line(&painter, pos2(cx, y + 10.0), p);
    y += 20.0 + 20.0;

    // Primary button.
    let label = widgets::galley(&painter, "Open file…", 14.0, p.on_accent, None);
    let btn = Rect::from_min_size(
        pos2(cx - (label.size().x + 32.0) / 2.0, y),
        vec2(label.size().x + 32.0, 36.0),
    );
    let resp = ui.interact(btn, ui.id().with("open-file"), Sense::click());
    let hover_t = ui
        .ctx()
        .animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    painter.rect_filled(btn, 8.0, darken(p.accent, 0.06 * hover_t));
    painter.galley(btn.center() - label.size() / 2.0, label, p.on_accent);
    if resp.has_focus() {
        widgets::focus_ring(&painter, btn, 8.0, p);
    }
    if widgets::tooltip(resp, "Open a file", Some("Ctrl+O"), p).clicked() {
        actions.push(Action::OpenDialog);
    }
    y += 36.0;

    if rows.is_empty() {
        return;
    }
    y += 32.0;
    widgets::overline(&painter, pos2(col.left() + 12.0, y), "Recent", p.muted);
    y += 14.0 + 8.0;
    for (i, entry) in rows.into_iter().enumerate() {
        let row = Rect::from_min_size(pos2(col.left(), y), vec2(COL_W, ROW_H));
        recent_row(ui, row, i, entry, p, actions);
        y += ROW_H;
    }
}

fn recent_row(
    ui: &mut Ui,
    row: Rect,
    i: usize,
    entry: &RecentEntry,
    p: &Palette,
    actions: &mut Vec<Action>,
) {
    let resp = ui.interact(row, ui.id().with(("recent", i)), Sense::click());
    let hovered = ui.rect_contains_pointer(row);
    let painter = ui.painter().clone();
    if hovered {
        painter.rect_filled(row, 8.0, p.bg_hover);
    }
    if resp.has_focus() {
        widgets::focus_ring(&painter, row, 8.0, p);
    }
    let missing = !entry.path.exists();
    icons::paint(
        ui,
        Icon::FileText,
        Rect::from_min_size(
            pos2(row.left() + 12.0, row.center().y - 8.0),
            vec2(16.0, 16.0),
        ),
        16.0,
        p.muted,
    );
    let text_left = row.left() + 40.0;
    let text_w = row.width() - 40.0 - 44.0;
    let name = super::menu::file_name(&entry.path);
    let name_g = widgets::galley(
        &painter,
        &name,
        14.0,
        if missing { p.muted } else { p.text },
        Some(text_w - if missing { 60.0 } else { 0.0 }),
    );
    let name_size = name_g.size();
    painter.galley(pos2(text_left, row.top() + 5.0), name_g, p.text);
    if missing {
        painter.text(
            pos2(
                text_left + name_size.x + 8.0,
                row.top() + 5.0 + name_size.y / 2.0,
            ),
            Align2::LEFT_CENTER,
            "Missing",
            font(11.0),
            p.muted,
        );
    }
    let folder =
        widgets::middle_ellipsis(&painter, &super::menu::folder(&entry.path), 12.0, text_w);
    painter.text(
        pos2(text_left, row.bottom() - 6.0),
        Align2::LEFT_BOTTOM,
        folder,
        font(12.0),
        p.muted,
    );

    let mut removed = false;
    if hovered {
        let x = Rect::from_center_size(pos2(row.right() - 22.0, row.center().y), vec2(24.0, 24.0));
        let r = icon_button(
            ui,
            x,
            &format!("recent-x-{i}"),
            Glyph::Icon(Icon::X, 14.0),
            ButtonState::default(),
            p,
        );
        if widgets::tooltip(r, "Remove from Recent", None, p).clicked() {
            actions.push(Action::RemoveRecent(entry.path.clone()));
            removed = true;
        }
    }
    if !removed && widgets::tooltip(resp, &entry.path.display().to_string(), None, p).clicked() {
        actions.push(Action::Open(entry.path.clone()));
    }
}

/// "Drop a file here, press [Ctrl O], or paste with [Ctrl V]" centered on `center`.
fn hint_line(painter: &egui::Painter, center: Pos2, p: &Palette) {
    enum Run {
        Text(&'static str),
        Key(&'static str),
    }
    let runs = [
        Run::Text("Drop a file here, press "),
        Run::Key("Ctrl O"),
        Run::Text(", or paste with "),
        Run::Key("Ctrl V"),
    ];
    let size = 14.0;
    let text_w = |s: &str| widgets::galley(painter, s, size, p.muted, None).size().x;
    let key_w = |s: &str| {
        widgets::galley(painter, s, size * 0.86, p.text, None)
            .size()
            .x
            + 12.0
    };
    let total: f32 = runs
        .iter()
        .map(|r| match r {
            Run::Text(t) => text_w(t),
            Run::Key(k) => key_w(k) + 4.0,
        })
        .sum();
    let mut x = center.x - total / 2.0;
    for run in runs {
        match run {
            Run::Text(t) => {
                let r = painter.text(
                    pos2(x, center.y),
                    Align2::LEFT_CENTER,
                    t,
                    font(size),
                    p.muted,
                );
                x = r.right();
            }
            Run::Key(k) => {
                x += 2.0;
                x += widgets::keycap(painter, pos2(x, center.y), k, size, p) + 2.0;
            }
        }
    }
}

/// The smiling page (SPEC §3): 2 px `accent` stroke, `accent-soft` fill, eyes and smile in
/// `text`, blush in `accent` at 35%. Drawn on a 64×64 grid.
fn mascot(painter: &egui::Painter, r: Rect, p: &Palette) {
    let s = r.width() / 64.0;
    let at = |x: f32, y: f32| r.min + vec2(x, y) * s;
    let stroke = Stroke::new(2.0 * s, p.accent);
    // Page with a folded top-right corner.
    let page = vec![
        at(14.0, 6.0),
        at(40.0, 6.0),
        at(52.0, 18.0),
        at(52.0, 58.0),
        at(14.0, 58.0),
    ];
    painter.add(PathShape {
        points: page,
        closed: true,
        fill: p.accent_soft,
        stroke: PathStroke::from(stroke),
    });
    let fold = vec![at(40.0, 6.0), at(40.0, 18.0), at(52.0, 18.0)];
    painter.add(PathShape {
        points: fold,
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: PathStroke::from(stroke),
    });
    // Eyes and smile.
    painter.circle_filled(at(26.0, 33.0), 2.4 * s, p.text);
    painter.circle_filled(at(40.0, 33.0), 2.4 * s, p.text);
    let smile = CubicBezierShape::from_points_stroke(
        [
            at(28.5, 40.0),
            at(30.5, 44.5),
            at(35.5, 44.5),
            at(37.5, 40.0),
        ],
        false,
        Color32::TRANSPARENT,
        Stroke::new(2.0 * s, p.text),
    );
    painter.add(smile);
    // Blush.
    let blush = p.accent.gamma_multiply(0.35);
    painter.circle_filled(at(20.5, 40.5), 3.0 * s, blush);
    painter.circle_filled(at(45.5, 40.5), 3.0 * s, blush);
}

fn darken(c: Color32, amount: f32) -> Color32 {
    let k = 1.0 - amount;
    Color32::from_rgba_premultiplied(
        (f32::from(c.r()) * k) as u8,
        (f32::from(c.g()) * k) as u8,
        (f32::from(c.b()) * k) as u8,
        c.a(),
    )
}
