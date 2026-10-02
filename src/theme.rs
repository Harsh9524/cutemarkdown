//! Theme resolution, egui `Visuals` from the engine palette, and the reading measure.

use egui::{Color32, CornerRadius, Shadow, Stroke, Visuals};
use engine::{FontChoice, Palette, Style, ThemeKind};

use crate::settings::{FontPref, Settings, ThemePref, Width};

/// Auto follows the OS theme (via eframe/winit); Light when unknown.
pub fn resolve(pref: ThemePref, system: Option<egui::Theme>) -> ThemeKind {
    match pref {
        ThemePref::Light => ThemeKind::Light,
        ThemePref::Sepia => ThemeKind::Sepia,
        ThemePref::Dark => ThemeKind::Dark,
        ThemePref::Auto => match system {
            Some(egui::Theme::Dark) => ThemeKind::Dark,
            _ => ThemeKind::Light,
        },
    }
}

/// Popover/menu shadow: (0, 8) blur 24 at `shadow` (SPEC §3). The second (0, 1) blur 2 layer is
/// painted by `ui::widgets::card`.
pub fn popup_shadow(p: &Palette) -> Shadow {
    Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: p.shadow,
    }
}

/// egui visuals so built-in widgets (text edit, tooltips, selection) match the theme.
pub fn visuals(kind: ThemeKind, p: &Palette) -> Visuals {
    let mut v = if kind.is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let r8 = CornerRadius::same(8);
    let none = Stroke::NONE;

    v.dark_mode = kind.is_dark();
    v.override_text_color = None;
    v.weak_text_color = Some(p.muted);
    v.hyperlink_color = p.link;
    v.faint_bg_color = p.bg_hover;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.code_bg_color = p.icode_bg;
    v.warn_fg_color = p.alert.warning.fg;
    v.error_fg_color = p.alert.caution.fg;

    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.window_shadow = popup_shadow(p);
    v.popup_shadow = popup_shadow(p);
    // Used by egui tooltips: SPEC §7 tooltips are radius 6.
    v.menu_corner_radius = CornerRadius::same(6);
    v.window_highlight_topmost = false;

    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.text_cursor.stroke = Stroke::new(1.5, p.accent);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r8;

    w.inactive.bg_fill = p.bg_hover;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = none;
    w.inactive.fg_stroke = Stroke::new(1.0, p.muted);
    w.inactive.corner_radius = r8;

    w.hovered.bg_fill = p.bg_hover;
    w.hovered.weak_bg_fill = p.bg_hover;
    w.hovered.bg_stroke = none;
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = r8;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.border;
    w.active.weak_bg_fill = p.border;
    w.active.bg_stroke = none;
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = r8;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.accent_soft;
    w.open.weak_bg_fill = p.accent_soft;
    w.open.bg_stroke = none;
    w.open.fg_stroke = Stroke::new(1.0, p.accent);
    w.open.corner_radius = r8;

    v.image_loading_spinners = false;
    v
}

/// Apply theme visuals plus the shell's spacing and tooltip timing to every egui style (so the
/// result doesn't depend on which theme egui itself thinks is active).
///
/// `animations` off (Windows "Show animations") sets `animation_time` to 0: the engine then
/// jumps instead of animating scrolls, and the chrome skips its transitions (`ui::anim_secs`).
pub fn apply(ctx: &egui::Context, kind: ThemeKind, p: &Palette, animations: bool) {
    let visuals = visuals(kind, p);
    ctx.all_styles_mut(|s| {
        s.visuals = visuals.clone();
        s.interaction.tooltip_delay = 0.6;
        s.interaction.selectable_labels = false;
        s.spacing.menu_margin = egui::Margin::symmetric(10, 6);
        s.spacing.item_spacing = egui::vec2(8.0, 4.0);
        s.animation_time = if animations { 0.12 } else { 0.0 };
    });
}

/// Gutter beside the column (SPEC §3).
pub fn gutter(viewport_w: f32) -> f32 {
    if viewport_w >= 960.0 {
        48.0
    } else if viewport_w >= 640.0 {
        32.0
    } else {
        20.0
    }
}

/// The measure setting in points (Full: `None`).
pub fn nominal_measure(width: Width, text_size: f32) -> Option<f32> {
    width.ems().map(|em| em * text_size)
}

/// Column width for a content area `area_w` wide: min(measure, area − 2 × gutter).
pub fn measure(width: Width, text_size: f32, area_w: f32) -> f32 {
    let avail = (area_w - 2.0 * gutter(area_w)).max(120.0);
    nominal_measure(width, text_size).map_or(avail, |m| m.min(avail))
}

/// The engine style for the current settings. `top_inset` is the height of the chrome over the
/// top of the document: the 44 px app bar, or 0 in Zen.
pub fn engine_style(kind: ThemeKind, s: &Settings, measure: f32, top_inset: f32) -> Style {
    let font = match s.font {
        FontPref::Sans => FontChoice::Sans,
        FontPref::Serif => FontChoice::Serif,
    };
    let mut style = Style::new(kind, font, s.text_size, measure);
    style.wrap_code = s.wrap_code;
    style.top_inset = top_inset;
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_system() {
        assert_eq!(
            resolve(ThemePref::Auto, Some(egui::Theme::Dark)),
            ThemeKind::Dark
        );
        assert_eq!(
            resolve(ThemePref::Auto, Some(egui::Theme::Light)),
            ThemeKind::Light
        );
        assert_eq!(resolve(ThemePref::Auto, None), ThemeKind::Light);
        assert_eq!(
            resolve(ThemePref::Sepia, Some(egui::Theme::Dark)),
            ThemeKind::Sepia
        );
    }

    #[test]
    fn measures_match_spec() {
        // Checklist §9.1: 1280 wide with a 264 docked outline → 736 column.
        assert_eq!(measure(Width::Medium, 16.0, 1280.0 - 264.0), 736.0);
        assert_eq!(measure(Width::Narrow, 16.0, 2000.0), 608.0);
        assert_eq!(measure(Width::Wide, 16.0, 2000.0), 896.0);
        assert_eq!(measure(Width::Full, 16.0, 1000.0), 1000.0 - 96.0);
        assert_eq!(measure(Width::Medium, 16.0, 600.0), 600.0 - 40.0);
        assert_eq!(measure(Width::Medium, 20.0, 3000.0), 920.0);
    }

    #[test]
    fn gutters() {
        assert_eq!(gutter(960.0), 48.0);
        assert_eq!(gutter(959.0), 32.0);
        assert_eq!(gutter(639.0), 20.0);
    }
}
