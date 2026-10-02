//! Visual tokens shared by the engine (document) and the shell (chrome).
//!
//! Values are the tokens in `docs/design/SPEC.md` §5. Fields may be added; don't rename or
//! remove them, the shell depends on them.

use egui::Color32;

/// Concrete theme. ("Auto" is a shell setting that resolves to Light or Dark.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeKind {
    #[default]
    Light,
    Sepia,
    Dark,
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

/// Foreground (icon/title), tint (fill) and border of one alert kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlertColors {
    pub fg: Color32,
    pub tint: Color32,
    pub border: Color32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlertPalette {
    pub note: AlertColors,
    pub tip: AlertColors,
    pub important: AlertColors,
    pub warning: AlertColors,
    pub caution: AlertColors,
}

/// Code token colors (on `code_bg`). Plain text/variables use `Palette::text`.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxPalette {
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub constant: Color32,
    pub function: Color32,
    pub type_: Color32,
    pub comment: Color32,
    pub operator: Color32,
    pub diff_add_bg: Color32,
    pub diff_del_bg: Color32,
}

/// All theme colors (SPEC §5). Chrome uses the same palette as the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub bg_hover: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_2: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub accent_soft: Color32,
    pub link: Color32,
    pub grad_a: Color32,
    pub grad_b: Color32,
    pub selection: Color32,
    pub icode_bg: Color32,
    pub icode_fg: Color32,
    pub code_bg: Color32,
    pub code_border: Color32,
    pub table_head: Color32,
    pub table_zebra: Color32,
    pub quote_bar: Color32,
    pub find_match: Color32,
    pub find_current: Color32,
    pub find_current_ring: Color32,
    pub shadow: Color32,
    /// Semantic orange used for 🟠 🔥 tints.
    pub orange: Color32,
    pub alert: AlertPalette,
    pub syntax: SyntaxPalette,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const fn alert(fg: u32, tint: u32, border: u32) -> AlertColors {
    AlertColors { fg: hex(fg), tint: hex(tint), border: hex(border) }
}

impl Palette {
    pub fn for_theme(theme: ThemeKind) -> Self {
        match theme {
            ThemeKind::Light => Self::light(),
            ThemeKind::Sepia => Self::sepia(),
            ThemeKind::Dark => Self::dark(),
        }
    }

    pub fn light() -> Self {
        Self {
            bg: hex(0xFCFAF9),
            surface: hex(0xFFFFFF),
            bg_hover: hex(0xF4EEF0),
            text: hex(0x2A2430),
            text_strong: hex(0x1D1822),
            text_2: hex(0x4F4755),
            muted: hex(0x6B6271),
            faint: hex(0xA0979F),
            border: hex(0xEDE5E8),
            border_strong: hex(0xDDD2D7),
            accent: hex(0xC2407A),
            on_accent: hex(0xFFFFFF),
            accent_soft: hex(0xF9E8EF),
            link: hex(0xB8336C),
            grad_a: hex(0xE0558A),
            grad_b: hex(0x7B5CE6),
            selection: hex(0xF6D2E1),
            icode_bg: hex(0xF5EDF0),
            icode_fg: hex(0x9A2E5E),
            code_bg: hex(0xF7F3F4),
            code_border: hex(0xEEE6E9),
            table_head: hex(0xF5F0F2),
            table_zebra: hex(0xFAF7F8),
            quote_bar: hex(0xE2C3D1),
            find_match: hex(0xFFE7A1),
            find_current: hex(0xFFC266),
            find_current_ring: hex(0xE0951A),
            shadow: Color32::from_rgba_unmultiplied(0x3C, 0x19, 0x2D, 31),
            orange: hex(0xC25A12),
            alert: AlertPalette {
                note: alert(0x2864BE, 0xEEF3FB, 0xC1D0E8),
                tip: alert(0x1D7748, 0xEBF6EF, 0xBED5C7),
                important: alert(0x7A4ED6, 0xF3EEFC, 0xD8CAEF),
                warning: alert(0x946000, 0xFCF3E3, 0xDFCFB3),
                caution: alert(0xC3304B, 0xFCECEF, 0xECC1C8),
            },
            syntax: SyntaxPalette {
                keyword: hex(0xB02D67),
                string: hex(0x2D7A50),
                number: hex(0xA2560A),
                constant: hex(0x8C4A00),
                function: hex(0x5D45C9),
                type_: hex(0x1C6B8A),
                comment: hex(0x6F6875),
                operator: hex(0x5E5664),
                diff_add_bg: hex(0xE2F2E7),
                diff_del_bg: hex(0xFBE3E7),
            },
        }
    }

    pub fn sepia() -> Self {
        Self {
            bg: hex(0xF6EFE4),
            surface: hex(0xFBF7F0),
            bg_hover: hex(0xEEE4D6),
            text: hex(0x382D25),
            text_strong: hex(0x2A211A),
            text_2: hex(0x54473C),
            muted: hex(0x6B5D50),
            faint: hex(0xA79784),
            border: hex(0xE6DCCD),
            border_strong: hex(0xD6C8B5),
            accent: hex(0xA84470),
            on_accent: hex(0xFFFFFF),
            accent_soft: hex(0xF0DFD9),
            link: hex(0xA13A60),
            grad_a: hex(0xB8507A),
            grad_b: hex(0x6E58C9),
            selection: hex(0xEACDC8),
            icode_bg: hex(0xEDE2D3),
            icode_fg: hex(0x8A3354),
            code_bg: hex(0xF0E7DA),
            code_border: hex(0xE5D9C8),
            table_head: hex(0xEEE4D6),
            table_zebra: hex(0xF3EBE0),
            quote_bar: hex(0xD9BDB6),
            find_match: hex(0xF9DC85),
            find_current: hex(0xF2A94A),
            find_current_ring: hex(0xE0951A),
            shadow: Color32::from_rgba_unmultiplied(0x50, 0x32, 0x1A, 36),
            orange: hex(0xA9500F),
            alert: AlertPalette {
                note: alert(0x2A60AD, 0xE6E9EA, 0xBDC7D5),
                tip: alert(0x2F6E3A, 0xE5ECDB, 0xBECBB4),
                important: alert(0x6A47BE, 0xEDE5EA, 0xCFC0D9),
                warning: alert(0x875700, 0xF4E5C8, 0xD7C4A4),
                caution: alert(0xAE2D45, 0xF4DFD8, 0xE2B9B7),
            },
            syntax: SyntaxPalette {
                keyword: hex(0x9E2C5A),
                string: hex(0x3B6A2A),
                number: hex(0x8F4F0B),
                constant: hex(0x7F3F14),
                function: hex(0x5640A8),
                type_: hex(0x1E5D76),
                comment: hex(0x73665A),
                operator: hex(0x5A4D42),
                diff_add_bg: hex(0xDCE8CC),
                diff_del_bg: hex(0xF1D6CF),
            },
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: hex(0x18151C),
            surface: hex(0x221E27),
            bg_hover: hex(0x2A2530),
            text: hex(0xE9E4EC),
            text_strong: hex(0xF7F3F9),
            text_2: hex(0xCFC7D4),
            muted: hex(0xA59CAD),
            faint: hex(0x6E6575),
            border: hex(0x2E2834),
            border_strong: hex(0x3E3645),
            accent: hex(0xF07AAB),
            on_accent: hex(0x18151C),
            accent_soft: hex(0x3A2332),
            link: hex(0xF59AC0),
            grad_a: hex(0xF07AAB),
            grad_b: hex(0xA893FF),
            selection: hex(0x5B2A45),
            icode_bg: hex(0x2A2230),
            icode_fg: hex(0xF6A9CA),
            code_bg: hex(0x1F1B24),
            code_border: hex(0x2C2631),
            table_head: hex(0x24202A),
            table_zebra: hex(0x1C1920),
            quote_bar: hex(0x4D3443),
            find_match: hex(0x5E4A12),
            find_current: hex(0xE0A33A),
            find_current_ring: hex(0xE0951A),
            shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 128),
            orange: hex(0xF4A261),
            alert: AlertPalette {
                note: alert(0x7EB0FF, 0x242837, 0x394765),
                tip: alert(0x6CD39E, 0x222C2C, 0x335246),
                important: alert(0xB7A0FF, 0x2B2637, 0x4B4165),
                warning: alert(0xE9B45A, 0x312823, 0x5B4830),
                caution: alert(0xFF8A9C, 0x34232B, 0x623A45),
            },
            syntax: SyntaxPalette {
                keyword: hex(0xF58DB9),
                string: hex(0x8CD6A6),
                number: hex(0xF2B46E),
                constant: hex(0xF59E7A),
                function: hex(0xB9A7FF),
                type_: hex(0x7CC9E0),
                comment: hex(0x958C9E),
                operator: hex(0xBCB3C4),
                diff_add_bg: hex(0x1F3328),
                diff_del_bg: hex(0x3A1E27),
            },
        }
    }
}

/// Everything the engine needs to paint a document.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub theme: ThemeKind,
    pub palette: Palette,
    pub font: FontChoice,
    /// Body text size T in points (SPEC §4; default 16). Content em-sizes derive from it.
    pub text_size: f32,
    /// Maximum width of the text column in points (the reading measure, SPEC §3).
    pub measure: f32,
    /// Soft-wrap long code lines instead of horizontal scrolling.
    pub wrap_code: bool,
}

impl Style {
    pub fn new(theme: ThemeKind, font: FontChoice, text_size: f32, measure: f32) -> Self {
        Self { theme, palette: Palette::for_theme(theme), font, text_size, measure, wrap_code: false }
    }
}

impl Default for Style {
    /// Light, Sans, 16 pt, Medium measure (46em = 736 pt).
    fn default() -> Self {
        Self::new(ThemeKind::Light, FontChoice::Sans, 16.0, 736.0)
    }
}
