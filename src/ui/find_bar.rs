//! Find bar (SPEC §3): a 340×40 card at the top right of the content, below the bar:
//! `search` icon · input · count · ↑ ↓ · `Aa` match case · ×.

use std::time::Instant;

use egui::text::CCursor;
use egui::text_selection::CCursorRange;
use egui::{Align2, Area, Frame, Id, Margin, Order, Rect, RichText, Sense, TextEdit, pos2, vec2};
use engine::Palette;

use super::widgets::{self, ButtonState, Glyph, font, icon_button, tooltip};
use super::{Action, BAR_H, FindState};
use crate::icons::{self, Icon};

const W: f32 = 340.0;
const H: f32 = 40.0;
const BTN: f32 = 28.0;
pub const INPUT_ID: &str = "find-input";

/// The count text: "3 of 17", "No results", "Wrapped" or nothing.
pub fn count_label(state: &FindState, now: Instant) -> Option<(String, bool)> {
    if state.sent.is_empty() {
        return None;
    }
    if state.wrapped_until.is_some_and(|t| now < t) {
        return Some(("Wrapped".into(), false));
    }
    let st = state.status;
    if st.total == 0 {
        return Some(("No results".into(), true));
    }
    Some((
        format!("{} of {}", st.current.map_or(0, |c| c + 1), st.total),
        false,
    ))
}

/// `content` is the content viewport (right of a docked outline). Returns the card rect.
pub fn show(
    ctx: &egui::Context,
    content: Rect,
    state: &mut FindState,
    p: &Palette,
    actions: &mut Vec<Action>,
) -> Rect {
    let rect = Rect::from_min_size(
        pos2(
            (content.right() - 16.0 - W).max(content.left() + 8.0),
            content.top() + BAR_H + 8.0,
        ),
        vec2(W, H),
    );
    let input_id = Id::new(INPUT_ID);
    Area::new(Id::new("find-bar"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            widgets::paint_card(ui.painter(), rect, p, 10);
            let cy = rect.center().y;
            icons::paint(
                ui,
                Icon::Search,
                Rect::from_center_size(pos2(rect.left() + 20.0, cy), vec2(16.0, 16.0)),
                16.0,
                p.muted,
            );

            let close =
                Rect::from_center_size(pos2(rect.right() - 6.0 - BTN / 2.0, cy), vec2(BTN, BTN));
            let case = close.translate(vec2(-(BTN + 2.0), 0.0));
            let down = case.translate(vec2(-(BTN + 2.0), 0.0));
            let up = down.translate(vec2(-(BTN + 2.0), 0.0));
            let st = ButtonState::default();
            let has_query = !state.sent.is_empty();
            let nav = ButtonState {
                toggled: false,
                disabled: !has_query || state.status.total == 0,
            };
            let r = icon_button(
                ui,
                up,
                "find-prev",
                Glyph::Icon(Icon::ChevronUp, 16.0),
                nav,
                p,
            );
            if tooltip(r, "Previous match", Some("Shift+Enter"), p).clicked() {
                actions.push(Action::FindPrev);
            }
            let r = icon_button(
                ui,
                down,
                "find-next",
                Glyph::Icon(Icon::ChevronDown, 16.0),
                nav,
                p,
            );
            if tooltip(r, "Next match", Some("Enter"), p).clicked() {
                actions.push(Action::FindNext);
            }
            let case_st = ButtonState {
                toggled: state.case_sensitive,
                disabled: false,
            };
            let aa = Glyph::Text("Aa", widgets::semibold(13.0));
            let r = icon_button(ui, case, "find-case", aa, case_st, p);
            if tooltip(r, "Match case", None, p).clicked() {
                actions.push(Action::ToggleFindCase);
            }
            let r = icon_button(ui, close, "find-close", Glyph::Icon(Icon::X, 16.0), st, p);
            if tooltip(r, "Close", Some("Esc"), p).clicked() {
                actions.push(Action::CloseFind);
            }

            let mut count_left = up.left() - 8.0;
            if let Some((text, bad)) = count_label(state, Instant::now()) {
                let color = if bad { p.alert.caution.fg } else { p.muted };
                let r = ui.painter().text(
                    pos2(count_left, cy),
                    Align2::RIGHT_CENTER,
                    text,
                    font(12.0),
                    color,
                );
                count_left = r.left() - 8.0;
            }

            let input_rect = Rect::from_min_max(
                pos2(rect.left() + 36.0, cy - 10.0),
                pos2(count_left, cy + 10.0),
            );
            let edit = TextEdit::singleline(&mut state.query)
                .id(input_id)
                .frame(Frame::NONE)
                .margin(Margin::ZERO)
                .font(font(14.0))
                .text_color(p.text)
                .hint_text(RichText::new("Find").font(font(14.0)).color(p.muted))
                .desired_width(input_rect.width())
                .return_key(None);
            let resp = ui.put(input_rect, edit);
            if resp.changed() {
                state.edited_at = Some(Instant::now());
            }
            if state.focus {
                state.focus = false;
                resp.request_focus();
                if let Some(mut ts) = TextEdit::load_state(ctx, input_id) {
                    let len = state.query.chars().count();
                    ts.cursor.set_char_range(Some(CCursorRange::two(
                        CCursor::new(0),
                        CCursor::new(len),
                    )));
                    ts.store(ctx, input_id);
                }
            }
            if resp.has_focus() {
                let (enter, shift) =
                    ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift));
                if enter {
                    actions.push(if shift {
                        Action::FindPrev
                    } else {
                        Action::FindNext
                    });
                }
            }
        });
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::FindStatus;
    use std::time::Duration;

    #[test]
    fn count_labels() {
        let now = Instant::now();
        let mut s = FindState::default();
        assert_eq!(count_label(&s, now), None);
        s.sent = "the".into();
        s.status = FindStatus {
            total: 17,
            current: Some(2),
        };
        assert_eq!(count_label(&s, now), Some(("3 of 17".into(), false)));
        s.status = FindStatus {
            total: 0,
            current: None,
        };
        assert_eq!(count_label(&s, now), Some(("No results".into(), true)));
        s.wrapped_until = Some(now + Duration::from_secs(1));
        assert_eq!(count_label(&s, now), Some(("Wrapped".into(), false)));
    }
}
