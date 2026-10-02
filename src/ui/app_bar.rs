//! The 44 px app bar (SPEC §3): outline, Back, Forward on the left; file-missing chip, Find, Aa
//! and More on the right. It overlays the content at `bg` 94%.

use egui::{Area, Id, Order, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use engine::Palette;

use super::widgets::{self, ButtonState, Glyph, icon_button, tooltip};
use super::{Action, BAR_H, Popover};
use crate::icons::Icon;

pub struct BarProps {
    pub has_doc: bool,
    pub outline_available: bool,
    pub outline_on: bool,
    pub has_history: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub find_open: bool,
    pub popover: Option<Popover>,
    pub missing: bool,
    /// 0..1: bottom border (shown once scrolled).
    pub border_t: f32,
    /// 0..1: 1 = fully shown, 0 = slid up and faded out.
    pub shown_t: f32,
}

/// Rects other layers anchor to.
#[derive(Clone, Copy, Debug)]
pub struct BarOut {
    pub rect: Rect,
    pub aa_button: Rect,
    pub menu_button: Rect,
    pub hovered: bool,
}

impl Default for BarOut {
    fn default() -> Self {
        Self {
            rect: Rect::NOTHING,
            aa_button: Rect::NOTHING,
            menu_button: Rect::NOTHING,
            hovered: false,
        }
    }
}

const BTN: f32 = 32.0;
const GAP: f32 = 4.0;
const PAD: f32 = 8.0;

pub fn show(
    ctx: &egui::Context,
    screen: Rect,
    props: &BarProps,
    p: &Palette,
    actions: &mut Vec<Action>,
) -> BarOut {
    let y = screen.top() - BAR_H * (1.0 - props.shown_t);
    let rect = Rect::from_min_size(pos2(screen.left(), y), vec2(screen.width(), BAR_H));
    let mut out = BarOut {
        rect,
        ..Default::default()
    };
    if props.shown_t <= 0.0 {
        return out;
    }
    Area::new(Id::new("app-bar"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_opacity(props.shown_t);
            let bg = ui.allocate_rect(rect, Sense::click());
            out.hovered = bg.hovered() || ui.rect_contains_pointer(rect);
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, p.bg.gamma_multiply(0.94));
            if props.border_t > 0.0 {
                painter.hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0, p.border.gamma_multiply(props.border_t)),
                );
            }
            let top = rect.top() + (BAR_H - BTN) / 2.0;
            let button = |x: f32| Rect::from_min_size(pos2(x, top), vec2(BTN, BTN));

            // Left cluster.
            if props.has_doc {
                let mut x = rect.left() + PAD;
                let st = ButtonState {
                    toggled: props.outline_on,
                    disabled: !props.outline_available,
                };
                let r = icon_button(
                    ui,
                    button(x),
                    "outline",
                    Glyph::Icon(Icon::PanelLeft, 18.0),
                    st,
                    p,
                );
                let label = if props.outline_available {
                    "Outline"
                } else {
                    "No outline for short documents"
                };
                let shortcut = props.outline_available.then_some("Ctrl+B");
                if tooltip(r, label, shortcut, p).clicked() {
                    actions.push(Action::ToggleOutline);
                }
                x += BTN + GAP;
                if props.has_history {
                    let st = ButtonState {
                        toggled: false,
                        disabled: !props.can_back,
                    };
                    let r = icon_button(
                        ui,
                        button(x),
                        "back",
                        Glyph::Icon(Icon::ArrowLeft, 18.0),
                        st,
                        p,
                    );
                    if tooltip(r, "Back", Some("Alt+←"), p).clicked() {
                        actions.push(Action::Back);
                    }
                    x += BTN + GAP;
                    let st = ButtonState {
                        toggled: false,
                        disabled: !props.can_forward,
                    };
                    let r = icon_button(
                        ui,
                        button(x),
                        "forward",
                        Glyph::Icon(Icon::ArrowRight, 18.0),
                        st,
                        p,
                    );
                    if tooltip(r, "Forward", Some("Alt+→"), p).clicked() {
                        actions.push(Action::Forward);
                    }
                }
            }

            // Right cluster, laid out right to left.
            let mut x = rect.right() - PAD - BTN;
            out.menu_button = button(x);
            let st = ButtonState {
                toggled: props.popover == Some(Popover::Menu),
                disabled: false,
            };
            let r = icon_button(
                ui,
                out.menu_button,
                "more",
                Glyph::Icon(Icon::Ellipsis, 18.0),
                st,
                p,
            );
            if tooltip(r, "More", None, p).clicked() {
                actions.push(Action::TogglePopover(Popover::Menu));
            }
            x -= BTN + GAP;
            out.aa_button = button(x);
            let st = ButtonState {
                toggled: props.popover == Some(Popover::Aa),
                disabled: false,
            };
            let r = icon_button(ui, out.aa_button, "aa", Glyph::Text("Aa", 15.0), st, p);
            if tooltip(r, "Reading settings", Some("Ctrl+,"), p).clicked() {
                actions.push(Action::TogglePopover(Popover::Aa));
            }
            if props.has_doc {
                x -= BTN + GAP;
                let st = ButtonState {
                    toggled: props.find_open,
                    disabled: false,
                };
                let r = icon_button(
                    ui,
                    button(x),
                    "find",
                    Glyph::Icon(Icon::Search, 18.0),
                    st,
                    p,
                );
                if tooltip(r, "Find", Some("Ctrl+F"), p).clicked() {
                    actions.push(if props.find_open {
                        Action::CloseFind
                    } else {
                        Action::OpenFind
                    });
                }
                if props.missing {
                    missing_chip(ui, pos2(x - 8.0, rect.center().y), p);
                }
            }
        });
    out
}

/// "File missing" chip: WARNING fg on WARNING tint, 22 px tall, 12 px text, fully rounded.
fn missing_chip(ui: &mut egui::Ui, right_center: egui::Pos2, p: &Palette) {
    let w = &p.alert.warning;
    let g = widgets::galley(ui.painter(), "File missing", 12.0, w.fg, None);
    let size = vec2(g.size().x + 20.0, 22.0);
    let rect = Rect::from_min_size(
        pos2(right_center.x - size.x, right_center.y - size.y / 2.0),
        size,
    );
    let painter = ui.painter();
    painter.rect(
        rect,
        11.0,
        w.tint,
        Stroke::new(1.0, w.border),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - g.size() / 2.0, g, w.fg);
    let r = ui.interact(rect, ui.id().with("missing-chip"), Sense::hover());
    tooltip(
        r,
        "The file was moved or deleted. Showing the last version.",
        None,
        p,
    );
}
