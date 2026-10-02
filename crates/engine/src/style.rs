//! Visual tokens shared by the engine (document) and the shell (chrome).
//!
//! Values come from `docs/design/SPEC.md`. Fields may be added; don't rename or remove them,
//! the shell depends on them.

use egui::Color32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeKind {
    #[default]
    Light,
    Dark,
    Sepia,
}

impl ThemeKind {
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontChoice {
    #[default]
    Sans,
    Serif,
}

/// Token colors for code highlighting.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxPalette {
    pub text: Color32,
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub function: Color32,
    pub type_: Color32,
    pub constant: Color32,
    pub operator: Color32,
    pub punctuation: Color32,
    pub diff_add_bg: Color32,
    pub diff_del_bg: Color32,
}

/// All theme colors. Chrome (top bar, sidebar, popovers) uses the same palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    /// Window/page background.
    pub bg: Color32,
    /// Raised surfaces: popovers, find bar, sidebar (if distinct), cards.
    pub surface: Color32,
    /// Body text.
    pub text: Color32,
    /// Headings (may equal `text`).
    pub heading: Color32,
    /// Secondary text: metadata, captions, inactive outline items.
    pub muted: Color32,
    /// Tertiary: placeholders, list markers, indent guides.
    pub faint: Color32,
    /// Hairlines and borders.
    pub border: Color32,
    /// Brand accent (active outline item, progress bar, focus rings, checkboxes).
    pub accent: Color32,
    /// Accent-tinted background (active/hover items).
    pub accent_soft: Color32,
    pub link: Color32,
    pub selection: Color32,
    pub inline_code_bg: Color32,
    pub inline_code_fg: Color32,
    pub code_bg: Color32,
    pub code_border: Color32,
    pub table_header_bg: Color32,
    pub table_stripe_bg: Color32,
    pub quote_bar: Color32,
    pub quote_bg: Color32,
    pub quote_text: Color32,
    pub find_match_bg: Color32,
    pub find_current_bg: Color32,
    pub alert_note: Color32,
    pub alert_tip: Color32,
    pub alert_important: Color32,
    pub alert_warning: Color32,
    pub alert_caution: Color32,
    pub syntax: SyntaxPalette,
}

impl Palette {
    pub fn for_theme(theme: ThemeKind) -> Self {
        match theme {
            ThemeKind::Light => Self::light(),
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Sepia => Self::sepia(),
        }
    }

    // PLACEHOLDER values (old HTML app). The engine team replaces these with SPEC.md tokens.
    pub fn light() -> Self {
        let c = Color32::from_rgb;
        Self {
            bg: c(0xfd, 0xf8, 0xf5),
            surface: c(0xff, 0xff, 0xff),
            text: c(0x3a, 0x33, 0x40),
            heading: c(0x2a, 0x24, 0x30),
            muted: c(0x8a, 0x81, 0x91),
            faint: c(0xc4, 0xb8, 0xc6),
            border: c(0xf0, 0xe3, 0xe8),
            accent: c(0xe0, 0x55, 0x8a),
            accent_soft: c(0xfd, 0xe8, 0xf0),
            link: c(0xc8, 0x3e, 0x76),
            selection: Color32::from_rgba_unmultiplied(0xe0, 0x55, 0x8a, 0x40),
            inline_code_bg: c(0xf8, 0xf0, 0xf4),
            inline_code_fg: c(0xb8, 0x33, 0x6a),
            code_bg: c(0xf8, 0xf0, 0xf4),
            code_border: c(0xf0, 0xe3, 0xe8),
            table_header_bg: c(0xfd, 0xe8, 0xf0),
            table_stripe_bg: c(0xfb, 0xf4, 0xf7),
            quote_bar: c(0xe0, 0x55, 0x8a),
            quote_bg: c(0xff, 0xf3, 0xf8),
            quote_text: c(0x5a, 0x52, 0x60),
            find_match_bg: c(0xff, 0xe5, 0x8a),
            find_current_bg: c(0xff, 0xb0, 0x4a),
            alert_note: c(0x4a, 0x86, 0xe8),
            alert_tip: c(0x2f, 0x9e, 0x6b),
            alert_important: c(0xa1, 0x4f, 0xd6),
            alert_warning: c(0xd9, 0x8a, 0x1f),
            alert_caution: c(0xd6, 0x45, 0x5d),
            syntax: SyntaxPalette {
                text: c(0x3a, 0x33, 0x40),
                keyword: c(0xc0, 0x39, 0x8a),
                string: c(0x2f, 0x8f, 0x6b),
                number: c(0xd9, 0x82, 0x2b),
                comment: c(0xa7, 0x9b, 0xb0),
                function: c(0x5b, 0x5f, 0xd6),
                type_: c(0x9a, 0x5b, 0x13),
                constant: c(0xd9, 0x82, 0x2b),
                operator: c(0x6a, 0x60, 0x70),
                punctuation: c(0x8a, 0x81, 0x91),
                diff_add_bg: c(0xe3, 0xf6, 0xea),
                diff_del_bg: c(0xfd, 0xe6, 0xe8),
            },
        }
    }

    pub fn dark() -> Self {
        let c = Color32::from_rgb;
        Self {
            bg: c(0x1b, 0x17, 0x21),
            surface: c(0x25, 0x1f, 0x2e),
            text: c(0xec, 0xe6, 0xf2),
            heading: c(0xf6, 0xf2, 0xfa),
            muted: c(0x9b, 0x91, 0xa8),
            faint: c(0x5c, 0x54, 0x68),
            border: c(0x35, 0x2e, 0x42),
            accent: c(0xff, 0x8f, 0xb8),
            accent_soft: c(0x3a, 0x26, 0x36),
            link: c(0xff, 0x9f, 0xc4),
            selection: Color32::from_rgba_unmultiplied(0xff, 0x8f, 0xb8, 0x48),
            inline_code_bg: c(0x2c, 0x25, 0x38),
            inline_code_fg: c(0xff, 0xad, 0xc9),
            code_bg: c(0x22, 0x1d, 0x2a),
            code_border: c(0x35, 0x2e, 0x42),
            table_header_bg: c(0x2c, 0x25, 0x38),
            table_stripe_bg: c(0x21, 0x1c, 0x28),
            quote_bar: c(0xff, 0x8f, 0xb8),
            quote_bg: c(0x2a, 0x21, 0x32),
            quote_text: c(0xcf, 0xc6, 0xd8),
            find_match_bg: c(0x6b, 0x5a, 0x1a),
            find_current_bg: c(0xb0, 0x70, 0x20),
            alert_note: c(0x6f, 0xa3, 0xf5),
            alert_tip: c(0x5c, 0xc9, 0x96),
            alert_important: c(0xc0, 0x8a, 0xf5),
            alert_warning: c(0xf0, 0xb0, 0x4a),
            alert_caution: c(0xf5, 0x73, 0x88),
            syntax: SyntaxPalette {
                text: c(0xec, 0xe6, 0xf2),
                keyword: c(0xff, 0x8f, 0xcb),
                string: c(0x7f, 0xdc, 0xae),
                number: c(0xff, 0xb8, 0x6b),
                comment: c(0x7d, 0x73, 0x90),
                function: c(0xa7, 0x9b, 0xff),
                type_: c(0xf5, 0xd0, 0x8a),
                constant: c(0xff, 0xb8, 0x6b),
                operator: c(0xc0, 0xb6, 0xcc),
                punctuation: c(0x9b, 0x91, 0xa8),
                diff_add_bg: c(0x1f, 0x3a, 0x2c),
                diff_del_bg: c(0x43, 0x23, 0x2c),
            },
        }
    }

    pub fn sepia() -> Self {
        let mut p = Self::light();
        p.bg = Color32::from_rgb(0xf6, 0xef, 0xe2);
        p.surface = Color32::from_rgb(0xfb, 0xf6, 0xec);
        p.text = Color32::from_rgb(0x43, 0x36, 0x2a);
        p
    }
}

/// Everything the engine needs to paint a document.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub theme: ThemeKind,
    pub palette: Palette,
    pub font: FontChoice,
    /// Body text size in points (already includes zoom).
    pub text_size: f32,
    /// Maximum width of the text column in points (the reading measure).
    pub measure: f32,
}

impl Style {
    pub fn new(theme: ThemeKind, font: FontChoice, text_size: f32, measure: f32) -> Self {
        Self { theme, palette: Palette::for_theme(theme), font, text_size, measure }
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::new(ThemeKind::Light, FontChoice::Sans, 16.0, 720.0)
    }
}
