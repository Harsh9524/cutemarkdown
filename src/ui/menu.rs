//! The ⋯ menu (SPEC §8): the remaining commands, an Open recent submenu and a word-count footer.

use std::path::{Path, PathBuf};

use egui::{Align2, Area, Id, Order, Rect, Sense, Stroke, pos2, vec2};
use engine::Palette;

use super::widgets::{self, font};
use super::{Action, BAR_H};
use crate::icons::{self, Icon};
use crate::settings::{RECENT_SHOWN, RecentEntry};

const W: f32 = 256.0;
const PAD: f32 = 6.0;
const ITEM_H: f32 = 30.0;
const SEP_H: f32 = 13.0;
const FOOTER_H: f32 = 32.0;

pub struct MenuProps<'a> {
    /// A file (not pasted text) is open.
    pub has_file: bool,
    pub has_doc: bool,
    pub recent: &'a [RecentEntry],
    pub words: Option<usize>,
    /// Open the recent submenu without hovering (QA).
    pub force_recent: bool,
}

enum Item {
    Cmd {
        label: &'static str,
        shortcut: Option<&'static str>,
        enabled: bool,
        action: Action,
    },
    Recent {
        enabled: bool,
    },
    Sep,
}

/// Show the menu below the ⋯ button. Returns the rects that count as "inside".
pub fn show(
    ctx: &egui::Context,
    screen: Rect,
    anchor: Rect,
    props: &MenuProps,
    p: &Palette,
    actions: &mut Vec<Action>,
) -> Vec<Rect> {
    let reveal_label = if cfg!(windows) {
        "Reveal in Explorer"
    } else {
        "Show in folder"
    };
    let items = [
        Item::Cmd {
            label: "Open…",
            shortcut: Some("Ctrl+O"),
            enabled: true,
            action: Action::OpenDialog,
        },
        Item::Recent {
            enabled: !props.recent.is_empty(),
        },
        Item::Sep,
        Item::Cmd {
            label: "Open in editor",
            shortcut: Some("Ctrl+E"),
            enabled: props.has_file,
            action: Action::OpenInEditor,
        },
        Item::Cmd {
            label: reveal_label,
            shortcut: Some("Ctrl+Shift+E"),
            enabled: props.has_file,
            action: Action::Reveal,
        },
        Item::Cmd {
            label: "Copy Markdown source",
            shortcut: Some("Ctrl+Shift+C"),
            enabled: props.has_doc,
            action: Action::CopySource,
        },
        Item::Cmd {
            label: "Reload",
            shortcut: Some("Ctrl+R"),
            enabled: props.has_file,
            action: Action::Reload,
        },
        Item::Sep,
        Item::Cmd {
            label: "Zen mode",
            shortcut: Some("F11"),
            enabled: props.has_doc,
            action: Action::ToggleZen,
        },
        Item::Cmd {
            label: "Keyboard shortcuts",
            shortcut: Some("Ctrl+/"),
            enabled: true,
            action: Action::ToggleShortcuts,
        },
        Item::Cmd {
            label: "About cutemarkdown",
            shortcut: None,
            enabled: true,
            action: Action::About,
        },
    ];
    let content_h: f32 = items
        .iter()
        .map(|i| {
            if matches!(i, Item::Sep) {
                SEP_H
            } else {
                ITEM_H
            }
        })
        .sum();
    let footer = props.words.map(footer_text);
    let h = 2.0 * PAD + content_h + if footer.is_some() { FOOTER_H } else { 0.0 };
    let x = (anchor.right() - W).max(screen.left() + 8.0);
    let rect = Rect::from_min_size(pos2(x, screen.top() + BAR_H + 4.0), vec2(W, h));
    let sub_id = Id::new("menu-recent-open");
    let mut sub_open =
        props.force_recent || ctx.data(|d| d.get_temp::<bool>(sub_id).unwrap_or(false));
    let mut recent_row = Rect::NOTHING;
    let mut inside = vec![rect];

    Area::new(Id::new("more-menu"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            widgets::paint_card(ui.painter(), rect, p, 12);
            let mut y = rect.top() + PAD;
            for (i, item) in items.into_iter().enumerate() {
                let row =
                    Rect::from_min_size(pos2(rect.left() + PAD, y), vec2(W - 2.0 * PAD, ITEM_H));
                match item {
                    Item::Sep => {
                        ui.painter().hline(
                            rect.left() + PAD..=rect.right() - PAD,
                            y + SEP_H / 2.0,
                            Stroke::new(1.0, p.border),
                        );
                        y += SEP_H;
                        continue;
                    }
                    Item::Cmd {
                        label,
                        shortcut,
                        enabled,
                        action,
                    } => {
                        let resp = menu_row(
                            ui,
                            row,
                            ("menu-item", i),
                            label,
                            shortcut,
                            enabled,
                            false,
                            p,
                        );
                        if resp.hovered() && !props.force_recent {
                            sub_open = false;
                        }
                        if resp.clicked() {
                            actions.push(action);
                        }
                    }
                    Item::Recent { enabled } => {
                        recent_row = row;
                        let resp = menu_row(
                            ui,
                            row,
                            "menu-recent",
                            "Open recent",
                            None,
                            enabled,
                            sub_open,
                            p,
                        );
                        if enabled && (resp.hovered() || resp.clicked()) {
                            sub_open = true;
                        }
                        icons::paint(
                            ui,
                            Icon::ChevronRight,
                            Rect::from_min_size(
                                pos2(row.right() - 26.0, row.top()),
                                vec2(20.0, ITEM_H),
                            ),
                            14.0,
                            if enabled { p.muted } else { p.faint },
                        );
                    }
                }
                y += ITEM_H;
            }
            if let Some(text) = &footer {
                let line_y = y + PAD;
                ui.painter().hline(
                    rect.left()..=rect.right(),
                    line_y,
                    Stroke::new(1.0, p.border),
                );
                ui.painter().text(
                    pos2(rect.left() + PAD + 10.0, (line_y + rect.bottom()) / 2.0),
                    Align2::LEFT_CENTER,
                    text,
                    font(12.0),
                    p.muted,
                );
            }
        });

    if sub_open && !props.recent.is_empty() {
        inside.push(recent_submenu(
            ctx,
            screen,
            rect,
            recent_row,
            props.recent,
            p,
            actions,
        ));
    }
    ctx.data_mut(|d| d.insert_temp(sub_id, sub_open));
    inside
}

/// A 30 px menu row with right-aligned shortcut. `highlight` keeps the hover fill (open submenu).
#[allow(clippy::too_many_arguments)]
fn menu_row(
    ui: &mut egui::Ui,
    row: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    shortcut: Option<&str>,
    enabled: bool,
    highlight: bool,
    p: &Palette,
) -> egui::Response {
    let resp = ui.interact(
        row,
        Id::new(id),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let painter = ui.painter();
    if enabled && (resp.hovered() || highlight) {
        painter.rect_filled(row, 6.0, p.bg_hover);
    }
    if resp.has_focus() {
        widgets::focus_ring(painter, row, 6.0, p);
    }
    let color = if enabled { p.text } else { p.faint };
    painter.text(
        pos2(row.left() + 10.0, row.center().y),
        Align2::LEFT_CENTER,
        label,
        font(13.5),
        color,
    );
    if let Some(s) = shortcut {
        painter.text(
            pos2(row.right() - 10.0, row.center().y),
            Align2::RIGHT_CENTER,
            s,
            font(12.0),
            if enabled { p.muted } else { p.faint },
        );
    }
    resp
}

fn recent_submenu(
    ctx: &egui::Context,
    screen: Rect,
    menu: Rect,
    row: Rect,
    recent: &[RecentEntry],
    p: &Palette,
    actions: &mut Vec<Action>,
) -> Rect {
    const SW: f32 = 300.0;
    let n = recent.len().min(RECENT_SHOWN);
    let h = 2.0 * PAD + n as f32 * ITEM_H;
    // Open to the left of the menu (it sits at the right edge), else to the right.
    let x = if menu.left() - SW - 4.0 >= screen.left() {
        menu.left() - SW - 4.0
    } else {
        menu.right() + 4.0
    };
    let y = (row.top() - PAD)
        .min(screen.bottom() - h - 8.0)
        .max(screen.top() + 8.0);
    let rect = Rect::from_min_size(pos2(x, y), vec2(SW, h));
    Area::new(Id::new("more-menu-recent"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            widgets::paint_card(ui.painter(), rect, p, 12);
            for (i, entry) in recent.iter().take(n).enumerate() {
                let r = Rect::from_min_size(
                    pos2(rect.left() + PAD, rect.top() + PAD + i as f32 * ITEM_H),
                    vec2(SW - 2.0 * PAD, ITEM_H),
                );
                let resp = ui.interact(r, Id::new(("recent-item", i)), Sense::click());
                let painter = ui.painter();
                if resp.hovered() {
                    painter.rect_filled(r, 6.0, p.bg_hover);
                }
                let missing = !entry.path.exists();
                let name = file_name(&entry.path);
                let name_g = widgets::galley(
                    painter,
                    &name,
                    13.5,
                    if missing { p.muted } else { p.text },
                    Some(r.width() * 0.6),
                );
                let name_w = name_g.size().x;
                painter.galley(
                    pos2(r.left() + 10.0, r.center().y - name_g.size().y / 2.0),
                    name_g,
                    p.text,
                );
                let folder = folder(&entry.path);
                let avail = r.width() - 30.0 - name_w;
                let folder = if missing {
                    "Missing".to_owned()
                } else {
                    widgets::middle_ellipsis(painter, &folder, 12.0, avail)
                };
                painter.text(
                    pos2(r.left() + 20.0 + name_w, r.center().y + 0.5),
                    Align2::LEFT_CENTER,
                    folder,
                    font(12.0),
                    p.muted,
                );
                if widgets::tooltip(resp, &entry.path.display().to_string(), None, p).clicked() {
                    actions.push(Action::Open(entry.path.clone()));
                }
            }
        });
    rect
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

pub fn folder(path: &Path) -> String {
    path.parent()
        .map(PathBuf::from)
        .unwrap_or_default()
        .display()
        .to_string()
}

/// "2,340 words · 11 min read" (230 words per minute).
pub fn footer_text(words: usize) -> String {
    let minutes = words.div_ceil(230).max(1);
    format!("{} words · {minutes} min read", thousands(words))
}

pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer() {
        assert_eq!(footer_text(2340), "2,340 words · 11 min read");
        assert_eq!(footer_text(12), "12 words · 1 min read");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(999), "999");
    }
}
