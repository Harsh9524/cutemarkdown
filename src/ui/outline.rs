//! Outline sidebar (SPEC §3): docked at 264 px or as a 280 px overlay, with scrollspy.

use egui::{Area, Id, Order, Rect, ScrollArea, Sense, StrokeKind, UiBuilder, pos2, vec2};
use engine::{Heading, Palette};

use super::widgets::{self, tooltip};
use super::{Action, OUTLINE_OVERLAY_W, OUTLINE_W};

/// One outline row.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Index into `Document::headings`.
    pub index: usize,
    /// 0 = top level of this outline.
    pub depth: u8,
    pub text: String,
    /// Heading level (1–3), for collapsing.
    pub level: u8,
}

/// The outline is offered only for documents with ≥ 3 headings at levels 1–3.
pub fn available(headings: &[Heading]) -> bool {
    headings
        .iter()
        .filter(|h| (1..=3).contains(&h.level))
        .count()
        >= 3
}

/// Rows for levels 1–3. A lone H1 that opens the document is the title, not a section, so it's
/// left out and H2 becomes the top level.
pub fn entries(headings: &[Heading]) -> Vec<Entry> {
    let h1s = headings.iter().filter(|h| h.level == 1).count();
    let skip_title = h1s == 1 && headings.first().is_some_and(|h| h.level == 1);
    let items: Vec<(usize, &Heading)> = headings
        .iter()
        .enumerate()
        .filter(|(i, h)| (1..=3).contains(&h.level) && !(skip_title && *i == 0))
        .collect();
    let top = items.iter().map(|(_, h)| h.level).min().unwrap_or(1);
    items
        .into_iter()
        .map(|(index, h)| Entry {
            index,
            depth: h.level - top,
            text: h.text.clone(),
            level: h.level,
        })
        .collect()
}

/// Rows to show. With more than 30 entries, deepest-level rows (H3s) appear only under the active
/// section (the H1/H2 that contains the active heading).
pub fn visible(entries: &[Entry], active: Option<usize>) -> Vec<&Entry> {
    if entries.len() <= 30 {
        return entries.iter().collect();
    }
    let max_depth = entries.iter().map(|e| e.depth).max().unwrap_or(0);
    if max_depth == 0 {
        return entries.iter().collect();
    }
    // The shallower entry at or before the active heading owns the expanded group.
    let owner = active
        .and_then(|a| {
            entries
                .iter()
                .rev()
                .find(|e| e.index <= a && e.depth < max_depth)
        })
        .map(|e| e.index);
    let mut current_owner = None;
    entries
        .iter()
        .filter(|e| {
            if e.depth < max_depth {
                current_owner = Some(e.index);
                true
            } else {
                current_owner.is_some() && current_owner == owner
            }
        })
        .collect()
}

pub struct OutlineProps<'a> {
    pub entries: &'a [Entry],
    pub active: Option<usize>,
    /// "8 min left", if known.
    pub time_left: Option<String>,
}

const ROW_H: f32 = 30.0;

/// The rows and header, painted inside `rect` (which includes the 56 px top padding).
fn contents(
    ui: &mut egui::Ui,
    rect: Rect,
    props: &OutlineProps,
    p: &Palette,
    actions: &mut Vec<Action>,
) {
    let inner = Rect::from_min_max(
        pos2(rect.left() + 12.0, rect.top() + 56.0),
        pos2(rect.right() - 12.0, rect.bottom() - 24.0),
    );
    let painter = ui.painter().clone();
    // Header: CONTENTS … 8 min left
    let header_y = inner.top();
    widgets::overline(
        &painter,
        pos2(inner.left() + 10.0, header_y),
        "Contents",
        p.muted,
    );
    if let Some(t) = &props.time_left {
        painter.text(
            pos2(inner.right() - 10.0, header_y),
            egui::Align2::RIGHT_TOP,
            t,
            widgets::font(11.0),
            p.muted,
        );
    }
    let list_rect = Rect::from_min_max(pos2(inner.left(), header_y + 24.0), inner.max);
    let rows = visible(props.entries, props.active);
    let active_entry = props
        .active
        .and_then(|a| rows.iter().rev().find(|e| e.index <= a))
        .map(|e| e.index);

    let mut child = ui.new_child(UiBuilder::new().max_rect(list_rect).id_salt("outline-list"));
    child.set_clip_rect(list_rect.intersect(ui.clip_rect()));
    let changed_active = {
        let id = Id::new("outline-last-active");
        let last = child
            .ctx()
            .data(|d| d.get_temp::<Option<usize>>(id))
            .flatten();
        child.ctx().data_mut(|d| d.insert_temp(id, active_entry));
        last != active_entry
    };
    ScrollArea::vertical()
        .id_salt("outline-scroll")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for e in rows {
                let (row, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                let is_active = Some(e.index) == active_entry;
                let indent = f32::from(e.depth) * 14.0;
                let pill = Rect::from_min_max(pos2(row.left() + indent, row.top()), row.max);
                let painter = ui.painter();
                let hover_t = ui.ctx().animate_bool_with_time(
                    resp.id.with("h"),
                    resp.hovered(),
                    super::anim_secs(ui.ctx(), super::HOVER_SECS),
                );
                let color = if is_active {
                    painter.rect_filled(pill, 6.0, p.accent_soft);
                    painter.rect_filled(
                        Rect::from_min_size(
                            pill.left_top() + vec2(0.0, 6.0),
                            vec2(2.0, ROW_H - 12.0),
                        ),
                        1.0,
                        p.accent,
                    );
                    p.link
                } else {
                    if hover_t > 0.0 {
                        painter.rect_filled(pill, 6.0, p.bg_hover.gamma_multiply(hover_t));
                    }
                    widgets::lerp(p.muted, p.text, hover_t)
                };
                let text_w = pill.width() - 20.0;
                let g = widgets::galley(painter, &e.text, 13.5, color, Some(text_w));
                let truncated = g.elided;
                painter.galley(
                    pos2(pill.left() + 10.0, pill.center().y - g.size().y / 2.0),
                    g,
                    color,
                );
                if resp.has_focus() {
                    widgets::focus_ring(painter, pill, 6.0, p);
                }
                if is_active && changed_active {
                    // Keep the active row visible; center it if it was off-screen.
                    let off_screen = !ui.clip_rect().contains_rect(row);
                    resp.scroll_to_me(off_screen.then_some(egui::Align::Center));
                }
                let resp = if truncated {
                    tooltip(resp, &e.text, None, p)
                } else {
                    resp
                };
                if resp.clicked() {
                    actions.push(Action::ScrollToHeading(e.index));
                }
            }
        });
}

/// Docked sidebar on `bg`, sliding with `t` (0 hidden … 1 shown).
pub fn show_docked(
    ui: &mut egui::Ui,
    screen: Rect,
    t: f32,
    props: &OutlineProps,
    p: &Palette,
    actions: &mut Vec<Action>,
) {
    if t <= 0.0 {
        return;
    }
    let x = screen.left() - OUTLINE_W * (1.0 - t);
    let rect = Rect::from_min_size(pos2(x, screen.top()), vec2(OUTLINE_W, screen.height()));
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).id_salt("outline-docked"));
    child.set_clip_rect(rect.intersect(screen));
    child.set_opacity(t);
    contents(&mut child, rect, props, p, actions);
}

/// Overlay sidebar: `surface`, shadow, scrim of `text` at 6%. Closes on outside click or row click.
pub fn show_overlay(
    ctx: &egui::Context,
    screen: Rect,
    t: f32,
    props: &OutlineProps,
    p: &Palette,
    actions: &mut Vec<Action>,
) {
    if t <= 0.0 {
        return;
    }
    Area::new(Id::new("outline-overlay"))
        .order(Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let scrim = ui.allocate_rect(screen, Sense::click());
            ui.painter()
                .rect_filled(screen, 0.0, p.text.gamma_multiply(0.06 * t));
            let x = screen.left() - OUTLINE_OVERLAY_W * (1.0 - t);
            let rect = Rect::from_min_size(
                pos2(x, screen.top()),
                vec2(OUTLINE_OVERLAY_W, screen.height()),
            );
            ui.painter()
                .add(crate::theme::popup_shadow(p).as_shape(rect, 0));
            ui.painter().rect(
                rect,
                0.0,
                p.surface,
                egui::Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            let panel = ui.interact(rect, Id::new("outline-overlay-panel"), Sense::click());
            let before = actions.len();
            contents(ui, rect, props, p, actions);
            let row_clicked = actions[before..]
                .iter()
                .any(|a| matches!(a, Action::ScrollToHeading(_)));
            if row_clicked || (scrim.clicked() && !panel.hovered()) {
                actions.push(Action::CloseOutlineOverlay);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(level: u8, text: &str) -> Heading {
        Heading {
            level,
            text: text.into(),
            anchor: text.to_lowercase(),
        }
    }

    #[test]
    fn availability_needs_three_top_headings() {
        assert!(!available(&[h(1, "A"), h(2, "B"), h(4, "C"), h(5, "D")]));
        assert!(available(&[h(2, "A"), h(2, "B"), h(3, "C")]));
    }

    #[test]
    fn lone_leading_h1_is_dropped_and_h2_becomes_top() {
        let e = entries(&[
            h(1, "Title"),
            h(2, "Intro"),
            h(3, "Detail"),
            h(2, "Plan"),
            h(4, "Deep"),
        ]);
        assert_eq!(
            e.iter().map(|e| (e.index, e.depth)).collect::<Vec<_>>(),
            vec![(1, 0), (2, 1), (3, 0)]
        );
    }

    #[test]
    fn several_h1s_are_kept() {
        let e = entries(&[h(1, "One"), h(2, "A"), h(1, "Two")]);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].depth, 0);
        assert_eq!(e[1].depth, 1);
    }

    #[test]
    fn long_outlines_collapse_h3s_outside_the_active_section() {
        let mut hs = vec![h(1, "Title")];
        for s in 0..10 {
            hs.push(h(2, &format!("S{s}")));
            for k in 0..3 {
                hs.push(h(3, &format!("S{s}.{k}")));
            }
        }
        let e = entries(&hs);
        assert_eq!(e.len(), 40);
        // Active = an H3 inside S2 (heading indices: S0=1, S1=5, S2=9, S2.0=10 …).
        let v = visible(&e, Some(11));
        assert_eq!(v.len(), 10 + 3);
        assert!(v.iter().any(|e| e.text == "S2.1"));
        assert!(!v.iter().any(|e| e.text == "S3.0"));
        // No active heading: only the H2s.
        assert_eq!(visible(&e, None).len(), 10);
        // Short outlines never collapse.
        assert_eq!(visible(&e[..20], None).len(), 20);
    }
}
