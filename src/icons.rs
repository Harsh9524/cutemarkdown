//! Lucide icons (`assets/icons/`, ISC), embedded and drawn tinted via egui_extras' SVG loader.

use egui::{Color32, ImageSource, Rect, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    PanelLeft,
    ArrowLeft,
    ArrowRight,
    Search,
    Ellipsis,
    ChevronUp,
    ChevronDown,
    ChevronRight,
    X,
    FileText,
    Check,
    CircleAlert,
    Minus,
    Plus,
}

impl Icon {
    pub fn source(self) -> ImageSource<'static> {
        match self {
            Self::PanelLeft => egui::include_image!("../assets/icons/panel-left.svg"),
            Self::ArrowLeft => egui::include_image!("../assets/icons/arrow-left.svg"),
            Self::ArrowRight => egui::include_image!("../assets/icons/arrow-right.svg"),
            Self::Search => egui::include_image!("../assets/icons/search.svg"),
            Self::Ellipsis => egui::include_image!("../assets/icons/ellipsis.svg"),
            Self::ChevronUp => egui::include_image!("../assets/icons/chevron-up.svg"),
            Self::ChevronDown => egui::include_image!("../assets/icons/chevron-down.svg"),
            Self::ChevronRight => egui::include_image!("../assets/icons/chevron-right.svg"),
            Self::X => egui::include_image!("../assets/icons/x.svg"),
            Self::FileText => egui::include_image!("../assets/icons/file-text.svg"),
            Self::Check => egui::include_image!("../assets/icons/check.svg"),
            Self::CircleAlert => egui::include_image!("../assets/icons/circle-alert.svg"),
            Self::Minus => egui::include_image!("../assets/icons/minus.svg"),
            Self::Plus => egui::include_image!("../assets/icons/plus.svg"),
        }
    }
}

/// Paint `icon` at `size` points, centered in `rect`.
pub fn paint(ui: &Ui, icon: Icon, rect: Rect, size: f32, tint: Color32) {
    let r = Rect::from_center_size(rect.center(), egui::vec2(size, size));
    egui::Image::new(icon.source()).tint(tint).paint_at(ui, r);
}
