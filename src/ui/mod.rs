//! Chrome: app bar, outline, popovers, find bar, toasts, empty state and overlays.
//!
//! Components paint themselves and report what the user asked for as [`Action`]s; `app.rs`
//! applies them after the frame's UI has run, so components never borrow app state mutably.

pub mod aa;
pub mod app_bar;
pub mod autohide;
pub mod empty;
pub mod find_bar;
pub mod menu;
pub mod outline;
pub mod overlays;
pub mod progress;
pub mod toast;
pub mod widgets;

use std::path::PathBuf;
use std::time::Instant;

use engine::LinkTarget;

use crate::icons::Icon;
use crate::settings::{FontPref, ThemePref, Width};

/// App bar height (SPEC §3).
pub const BAR_H: f32 = 44.0;
/// Docked outline width.
pub const OUTLINE_W: f32 = 264.0;
/// Overlay outline width.
pub const OUTLINE_OVERLAY_W: f32 = 280.0;
/// Panel animation time.
pub const PANEL_SECS: f32 = 0.16;
/// Hover transition time (SPEC §7 General states).
pub const HOVER_SECS: f32 = 0.12;

/// `secs`, or 0 when animations are off (Windows "Show animations", SPEC §7). The app sets
/// egui's `animation_time` to 0 in that case; the engine reads the same flag.
pub fn anim_secs(ctx: &egui::Context, secs: f32) -> f32 {
    if ctx.global_style().animation_time <= 0.0 {
        0.0
    } else {
        secs
    }
}

/// Something the user asked for this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenDialog,
    Open(PathBuf),
    Paste(String),
    OpenInNewWindow(PathBuf, Option<String>),
    RemoveRecent(PathBuf),
    NewWindow,
    CloseWindow,
    Back,
    Forward,
    Reload,
    CopySource,
    OpenInEditor,
    /// "Open in editor here" from a heading's context menu: 1-based source line.
    OpenInEditorAt(usize),
    Reveal,
    Link(LinkTarget, bool),
    ScrollToHeading(usize),
    ToggleOutline,
    CloseOutlineOverlay,
    OpenFind,
    CloseFind,
    /// Esc in the find bar: the current match becomes the selection, then find closes.
    EscapeFind,
    FindNext,
    FindPrev,
    /// The find bar's `Aa` match-case toggle.
    ToggleFindCase,
    TogglePopover(Popover),
    ClosePopover,
    ToggleZen,
    ToggleShortcuts,
    About,
    SetTheme(ThemePref),
    CycleTheme,
    SetFont(FontPref),
    SetTextSize(f32),
    StepTextSize(i32),
    SetWidth(Width),
    SetWrap(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popover {
    Aa,
    Menu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub icon: Option<Icon>,
    pub kind: ToastKind,
    pub born: Instant,
}

impl Toast {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            icon: None,
            kind: ToastKind::Info,
            born: Instant::now(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            icon: Some(Icon::CircleAlert),
            kind: ToastKind::Error,
            born: Instant::now(),
        }
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// How long it stays (SPEC §3: 1.6 s, errors 4 s).
    pub fn lifetime(&self) -> f32 {
        match self.kind {
            ToastKind::Info => 1.6,
            ToastKind::Error => 4.0,
        }
    }
}

/// Find bar state. The query is debounced (80 ms) before it reaches the engine.
#[derive(Debug, Default)]
pub struct FindState {
    pub open: bool,
    pub query: String,
    /// Query last sent to the engine.
    pub sent: String,
    pub edited_at: Option<Instant>,
    pub status: engine::FindStatus,
    /// Focus the input (and select its text) on the next frame.
    pub focus: bool,
    /// Show "Wrapped" instead of the count until then.
    pub wrapped_until: Option<Instant>,
    /// Match case (the `Aa` toggle). Kept for the window's lifetime, across documents.
    pub case_sensitive: bool,
}

/// Transient chrome state (not persisted).
#[derive(Debug, Default)]
pub struct UiState {
    pub popover: Option<Popover>,
    pub find: FindState,
    pub outline_overlay: bool,
    pub toast: Option<Toast>,
    pub zen: bool,
    pub shortcuts: bool,
    /// Link under the pointer and since when (status pill after 300 ms).
    pub hovered_link: Option<(String, Instant)>,
}

impl UiState {
    /// Anything floating that forces the app bar visible.
    pub fn bar_pinned(&self) -> bool {
        self.popover.is_some() || self.find.open
    }
}
