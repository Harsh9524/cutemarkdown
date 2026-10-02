//! App state and per-frame orchestration: documents, history, live reload, input, chrome layout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Event, Id, Key, Modifiers, MouseWheelUnit, PointerButton, Rect, UiBuilder, pos2};
use engine::{DocOutput, DocView, Document, LinkTarget, Palette, ThemeKind};

use crate::cli::{Args, Open};
use crate::decode::{content_hash, decode};
use crate::history::{History, Location};
use crate::icons::Icon;
use crate::links::{self, LinkAction};
use crate::platform;
use crate::reload::{FileWatcher, Stamp, WatchEvent};
use crate::renderer::Renderer;
use crate::settings::{self, DEFAULT_TEXT_SIZE, RecentEntry, SettingsStore, WindowGeom};
use crate::theme;
use crate::ui::{self, Action, OUTLINE_W, PANEL_SECS, Popover, Toast, UiState, outline};

/// Set once a frame has been shown; `main` uses it to tell renderer-init failures apart.
pub static FIRST_FRAME_SHOWN: AtomicBool = AtomicBool::new(false);

const FIND_DEBOUNCE: Duration = Duration::from_millis(80);

/// The open document.
pub struct Doc {
    pub view: DocView,
    pub location: Location,
    hash: u64,
    /// The file is gone; we keep showing the last render.
    pub missing: bool,
    watcher: Option<FileWatcher>,
    pub outline: Vec<outline::Entry>,
    pub outline_available: bool,
}

impl Doc {
    fn new(view: DocView, location: Location, hash: u64, watcher: Option<FileWatcher>) -> Self {
        let mut d = Self {
            view,
            location,
            hash,
            missing: false,
            watcher,
            outline: Vec::new(),
            outline_available: false,
        };
        d.refresh_outline();
        d
    }

    fn refresh_outline(&mut self) {
        let headings = self.view.document().headings();
        self.outline_available = outline::available(headings);
        self.outline = outline::entries(headings);
    }

    pub fn path(&self) -> Option<&Path> {
        match &self.location {
            Location::File(p) => Some(p),
            Location::Pasted(_) => None,
        }
    }
}

/// Throttle for Ctrl+wheel text size (one step per notch, at most every 100 ms).
#[derive(Default)]
struct WheelZoom {
    accum: f32,
    last_step: Option<Instant>,
}

pub struct App {
    settings: SettingsStore,
    args: Args,
    doc: Option<Doc>,
    history: History,
    ui: UiState,
    theme: Option<ThemeKind>,
    palette: Palette,
    out: DocOutput,
    renderer: Renderer,
    frame_no: u32,
    title: String,
    wheel: WheelZoom,
    autohide: ui::autohide::AutoHide,
    /// Last user scroll input (wheel, scroll keys, pointer drag) and last app-driven scroll.
    user_scroll_at: Option<Instant>,
    programmatic_at: Option<Instant>,
    /// Modifiers at the end of the previous frame (to replay this frame's changes in order).
    modifiers: Modifiers,
    docked_fits: bool,
    bar: ui::app_bar::BarOut,
    demo_recents: Option<Vec<RecentEntry>>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        args: Args,
        settings: SettingsStore,
        renderer: Renderer,
    ) -> Self {
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        engine::fonts::install(ctx);
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        if let Some(ppp) = args.ppp {
            ctx.set_pixels_per_point(ppp);
        }
        let demo_recents = args.demo_recents.then(demo_recents);
        let mut app = Self {
            settings,
            doc: None,
            history: History::default(),
            ui: UiState::default(),
            theme: None,
            palette: Palette::light(),
            out: DocOutput::default(),
            renderer,
            frame_no: 0,
            title: String::new(),
            wheel: WheelZoom::default(),
            autohide: Default::default(),
            user_scroll_at: None,
            programmatic_at: None,
            modifiers: Modifiers::NONE,
            docked_fits: true,
            bar: Default::default(),
            demo_recents,
            args,
        };
        if !app.args.empty {
            // Screenshot runs open every file here in turn, so Back/Forward can be checked;
            // normal launches open the others in new windows (see `main`).
            let files: Vec<PathBuf> = if app.args.screenshot.is_some() {
                app.args.files.clone()
            } else {
                app.args.files.iter().take(1).cloned().collect()
            };
            for f in files {
                let anchor = app.args.anchor.clone();
                app.open_path(ctx, &f, anchor, true);
            }
        }
        app
    }

    fn recents(&self) -> &[RecentEntry] {
        self.demo_recents
            .as_deref()
            .unwrap_or(&self.settings.data.recent)
    }

    fn toast(&mut self, toast: Toast) {
        self.ui.toast = Some(toast);
    }

    fn current_scroll(&self) -> f32 {
        self.doc.as_ref().map_or(0.0, |d| d.view.scroll_offset())
    }

    // ---- documents -------------------------------------------------------------------------

    fn load_file(ctx: &egui::Context, path: &Path) -> Result<Doc, String> {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let stamp = Stamp::of(&path);
        let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("File not found: {}", path.display()),
            _ => format!("Couldn't open {}: {e}", ui::menu::file_name(&path)),
        })?;
        let text = decode(&bytes);
        let view = DocView::new(Document::parse(&text, path.parent()));
        let watcher = FileWatcher::spawn(path.clone(), stamp, ctx.clone());
        Ok(Doc::new(
            view,
            Location::File(path),
            content_hash(&text),
            Some(watcher),
        ))
    }

    fn pasted_doc(text: Arc<str>) -> Doc {
        let view = DocView::new(Document::parse(&text, None));
        let hash = content_hash(&text);
        Doc::new(view, Location::Pasted(text), hash, None)
    }

    fn install_doc(&mut self, doc: Doc, anchor: Option<String>) {
        self.programmatic_at = Some(Instant::now());
        self.doc = Some(doc);
        self.ui.outline_overlay = false;
        self.ui.hovered_link = None;
        self.out = DocOutput::default();
        self.rerun_find();
        if let (Some(a), Some(d)) = (anchor, self.doc.as_mut()) {
            d.view.scroll_to_anchor(&a);
        }
    }

    /// Open a file in this window. Returns false (and toasts) if it can't be read.
    fn open_path(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        anchor: Option<String>,
        push_history: bool,
    ) -> bool {
        match Self::load_file(ctx, path) {
            Ok(doc) => {
                if push_history {
                    let scroll = self.current_scroll();
                    self.history.push(doc.location.clone(), scroll);
                }
                if let Some(p) = doc.path() {
                    self.settings.data.add_recent(p);
                    self.settings.mark_dirty();
                }
                self.install_doc(doc, anchor);
                true
            }
            Err(msg) => {
                self.toast(Toast::error(msg));
                false
            }
        }
    }

    fn open_pasted(&mut self, text: String) {
        let text: Arc<str> = Arc::from(text);
        let scroll = self.current_scroll();
        self.history.push(Location::Pasted(text.clone()), scroll);
        self.install_doc(Self::pasted_doc(text), None);
    }

    /// Re-read the current file. Returns whether the content changed.
    fn reload_from_disk(&mut self) -> Result<bool, String> {
        self.programmatic_at = Some(Instant::now());
        let Some(doc) = self.doc.as_mut() else {
            return Ok(false);
        };
        let Some(path) = doc.path().map(Path::to_path_buf) else {
            return Ok(false);
        };
        let bytes =
            std::fs::read(&path).map_err(|_| format!("File not found: {}", path.display()))?;
        doc.missing = false;
        let text = decode(&bytes);
        let hash = content_hash(&text);
        if hash == doc.hash {
            return Ok(false);
        }
        doc.view
            .set_document(Document::parse(&text, path.parent()), true);
        doc.hash = hash;
        doc.refresh_outline();
        self.rerun_find();
        Ok(true)
    }

    fn poll_watcher(&mut self) {
        let event = self
            .doc
            .as_ref()
            .and_then(|d| d.watcher.as_ref())
            .and_then(FileWatcher::poll);
        match event {
            Some(WatchEvent::Changed) => {
                // Automatic reloads never toast (SPEC §3).
                if let Err(e) = self.reload_from_disk() {
                    eprintln!("cutemarkdown: reload failed: {e}");
                }
            }
            Some(WatchEvent::Missing) => {
                if let Some(d) = self.doc.as_mut() {
                    d.missing = true;
                }
            }
            None => {}
        }
    }

    fn go_history(&mut self, ctx: &egui::Context, back: bool) {
        let scroll = self.current_scroll();
        let entry = if back {
            self.history.back(scroll)
        } else {
            self.history.forward(scroll)
        }
        .cloned();
        let Some(entry) = entry else { return };
        let same = self
            .doc
            .as_ref()
            .is_some_and(|d| d.location == entry.location);
        if !same {
            let loaded = match &entry.location {
                Location::File(p) => Self::load_file(ctx, p),
                Location::Pasted(t) => Ok(Self::pasted_doc(t.clone())),
            };
            match loaded {
                Ok(doc) => self.install_doc(doc, None),
                Err(msg) => {
                    // Undo the move so history keeps matching what's shown.
                    if back {
                        self.history.forward(0.0)
                    } else {
                        self.history.back(0.0)
                    };
                    self.toast(Toast::error(msg));
                    return;
                }
            }
        }
        if let Some(d) = self.doc.as_mut() {
            d.view.set_scroll_offset(entry.scroll);
        }
    }

    fn follow_link(&mut self, ctx: &egui::Context, target: &LinkTarget, new_window: bool) {
        match links::classify(target, links::probe) {
            LinkAction::Anchor(a) => self.jump_to_anchor(&a),
            LinkAction::OpenDoc { path, anchor } => {
                if new_window {
                    self.spawn_window(Some(&path), anchor.as_deref());
                } else if self
                    .doc
                    .as_ref()
                    .and_then(Doc::path)
                    .is_some_and(|p| settings::same_path(p, &path))
                {
                    match anchor {
                        Some(a) => self.jump_to_anchor(&a),
                        None => {
                            if let Some(d) = self.doc.as_mut() {
                                self.history
                                    .push(d.location.clone(), d.view.scroll_offset());
                                d.view.scroll_to_top();
                            }
                        }
                    }
                } else {
                    self.open_path(ctx, &path, anchor, true);
                }
            }
            LinkAction::OpenWithSystem(p) => {
                if let Err(e) = platform::open_with_system(&p) {
                    self.toast(Toast::error(format!(
                        "Couldn't open {}: {e}",
                        ui::menu::file_name(&p)
                    )));
                }
            }
            LinkAction::RevealFolder(p) => {
                if let Err(e) = platform::reveal(&p) {
                    self.toast(Toast::error(format!("Couldn't show folder: {e}")));
                }
            }
            LinkAction::RevealBlocked(p) => match platform::reveal(&p) {
                Ok(()) => self.toast(Toast::info(if cfg!(windows) {
                    "Revealed in Explorer"
                } else {
                    "Revealed in folder"
                })),
                Err(e) => self.toast(Toast::error(format!("Couldn't reveal file: {e}"))),
            },
            LinkAction::OpenUrl(u) => {
                if let Err(e) = platform::open_url(&u) {
                    self.toast(Toast::error(format!("Couldn't open link: {e}")));
                }
            }
            LinkAction::Blocked(scheme) => {
                self.toast(Toast::error(format!("Blocked link ({scheme}:)")))
            }
            LinkAction::NotFound(p) => {
                self.toast(Toast::error(format!("File not found: {}", p.display())))
            }
        }
    }

    fn jump_to_anchor(&mut self, anchor: &str) {
        let Some(d) = self.doc.as_mut() else { return };
        let before = d.view.scroll_offset();
        if d.view.scroll_to_anchor(anchor) {
            self.history.push(d.location.clone(), before);
        } else {
            self.toast(Toast::error(format!("Section not found: #{anchor}")));
        }
    }

    fn spawn_window(&mut self, path: Option<&Path>, anchor: Option<&str>) {
        let mut args: Vec<std::ffi::OsString> = Vec::new();
        if let Some(s) = &self.args.settings {
            args.extend(["--settings".into(), s.clone().into()]);
        }
        if let Some(a) = anchor {
            args.extend(["--anchor".into(), a.into()]);
        }
        if let Some(p) = path {
            args.extend(["--".into(), p.as_os_str().to_owned()]);
        }
        if let Err(e) = platform::spawn_window(&args) {
            self.toast(Toast::error(format!("Couldn't open a new window: {e}")));
        }
    }

    // ---- find ------------------------------------------------------------------------------

    fn send_find_query(&mut self) {
        self.programmatic_at = Some(Instant::now());
        let f = &mut self.ui.find;
        f.edited_at = None;
        let Some(d) = self.doc.as_mut() else { return };
        if f.query.is_empty() {
            d.view.clear_find();
            f.status = Default::default();
            f.sent.clear();
        } else {
            f.status = d.view.set_find_query(&f.query);
            f.sent = f.query.clone();
        }
    }

    /// After a document change, re-run an active search.
    fn rerun_find(&mut self) {
        if self.ui.find.open && !self.ui.find.query.is_empty() {
            self.send_find_query();
        }
    }

    fn find_step(&mut self, forward: bool) {
        if self.ui.find.sent != self.ui.find.query {
            self.send_find_query();
        }
        let Some(d) = self.doc.as_mut() else { return };
        if self.ui.find.sent.is_empty() {
            return;
        }
        let before = self.ui.find.status.current;
        let st = if forward {
            d.view.find_next()
        } else {
            d.view.find_prev()
        };
        let wrapped = match (before, st.current) {
            (Some(b), Some(a)) => (forward && a < b) || (!forward && a > b),
            _ => false,
        };
        if wrapped {
            self.ui.find.wrapped_until = Some(Instant::now() + Duration::from_millis(1200));
        }
        self.ui.find.status = st;
    }

    // ---- actions ---------------------------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, frame: &eframe::Frame, action: Action) {
        // App-driven scrolls never count as the reader scrolling (app bar auto-hide).
        if matches!(
            action,
            Action::Back
                | Action::Forward
                | Action::Link(..)
                | Action::ScrollToHeading(_)
                | Action::HeadingStep(_)
                | Action::FindNext
                | Action::FindPrev
        ) {
            self.programmatic_at = Some(Instant::now());
        }
        match action {
            Action::OpenDialog => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Open")
                    .add_filter("Markdown", links::OPENABLE_EXTS);
                if let Some(dir) = self.doc.as_ref().and_then(Doc::path).and_then(Path::parent) {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.set_parent(frame).pick_file() {
                    self.open_path(ctx, &path, None, true);
                }
            }
            Action::Open(path) => {
                self.ui.popover = None;
                self.open_path(ctx, &path, None, true);
            }
            Action::Paste(text) => self.open_pasted(text),
            Action::OpenInNewWindow(path, anchor) => {
                self.spawn_window(Some(&path), anchor.as_deref())
            }
            Action::RemoveRecent(path) => {
                if let Some(demo) = self.demo_recents.as_mut() {
                    demo.retain(|r| r.path != path);
                }
                self.settings.data.remove_recent(&path);
                self.settings.mark_dirty();
            }
            Action::NewWindow => self.spawn_window(None, None),
            Action::CloseWindow => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Back => self.go_history(ctx, true),
            Action::Forward => self.go_history(ctx, false),
            Action::Reload => {
                if self.doc.as_ref().and_then(Doc::path).is_some() {
                    match self.reload_from_disk() {
                        Ok(_) => self.toast(Toast::info("Reloaded")),
                        Err(e) => self.toast(Toast::error(e)),
                    }
                }
            }
            Action::CopySource => {
                if let Some(d) = &self.doc {
                    ctx.copy_text(d.view.document().source().to_owned());
                    self.toast(Toast::info("Copied Markdown source").with_icon(Icon::Check));
                }
            }
            Action::OpenInEditor => {
                if let Some(p) = self.doc.as_ref().and_then(Doc::path).map(Path::to_path_buf)
                    && let Err(e) = platform::open_in_editor(&p)
                {
                    self.toast(Toast::error(format!("Couldn't open an editor: {e}")));
                }
            }
            Action::Reveal => {
                if let Some(p) = self.doc.as_ref().and_then(Doc::path).map(Path::to_path_buf)
                    && let Err(e) = platform::reveal(&p)
                {
                    self.toast(Toast::error(format!("Couldn't reveal file: {e}")));
                }
            }
            Action::Link(target, new_window) => self.follow_link(ctx, &target, new_window),
            Action::ScrollToHeading(i) => {
                // Outline clicks don't push history (SPEC §7).
                if let Some(d) = self.doc.as_mut() {
                    d.view.scroll_to_heading(i);
                }
            }
            Action::HeadingStep(dir) => self.heading_step(dir),
            Action::ToggleOutline => {
                let available = self.doc.as_ref().is_some_and(|d| d.outline_available);
                if !available {
                    if self.doc.is_some() {
                        self.toast(Toast::info("No outline for short documents"));
                    }
                } else if self.docked_fits {
                    self.settings.data.outline_open = !self.settings.data.outline_open;
                    self.settings.mark_dirty();
                } else {
                    self.ui.outline_overlay = !self.ui.outline_overlay;
                }
            }
            Action::CloseOutlineOverlay => self.ui.outline_overlay = false,
            Action::OpenFind => {
                if self.doc.is_some() {
                    let f = &mut self.ui.find;
                    if !f.open && !f.query.is_empty() {
                        f.edited_at = Some(Instant::now() - FIND_DEBOUNCE);
                    }
                    f.open = true;
                    f.focus = true;
                    self.ui.popover = None;
                }
            }
            Action::CloseFind => {
                self.ui.find.open = false;
                self.ui.find.sent.clear();
                self.ui.find.status = Default::default();
                if let Some(d) = self.doc.as_mut() {
                    d.view.clear_find();
                }
            }
            Action::FindNext => self.find_step(true),
            Action::FindPrev => self.find_step(false),
            Action::TogglePopover(p) => {
                self.ui.popover = if self.ui.popover == Some(p) {
                    None
                } else {
                    Some(p)
                };
            }
            Action::ClosePopover => self.ui.popover = None,
            Action::ToggleZen => self.set_zen(ctx, !self.ui.zen),
            Action::ToggleShortcuts => {
                self.ui.popover = None;
                self.ui.shortcuts = !self.ui.shortcuts;
            }
            Action::About => {
                self.ui.popover = None;
                self.toast(Toast::info(format!(
                    "cutemarkdown {} · MIT license",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            Action::SetTheme(t) => {
                self.args.theme = None;
                self.settings.data.theme = t;
                self.settings.mark_dirty();
            }
            Action::CycleTheme => {
                let next = self
                    .args
                    .theme
                    .take()
                    .unwrap_or(self.settings.data.theme)
                    .next();
                self.settings.data.theme = next;
                self.settings.mark_dirty();
                self.toast(Toast::info(format!("Theme: {}", next.label())));
            }
            Action::SetFont(f) => {
                self.settings.data.font = f;
                self.settings.mark_dirty();
            }
            Action::SetTextSize(size) => self.set_text_size(size),
            Action::StepTextSize(dir) => {
                self.set_text_size(settings::step_text_size(self.settings.data.text_size, dir))
            }
            Action::SetWidth(w) => {
                self.settings.data.width = w;
                self.settings.mark_dirty();
            }
            Action::SetWrap(on) => {
                self.settings.data.wrap_code = on;
                self.settings.mark_dirty();
            }
        }
    }

    fn set_text_size(&mut self, size: f32) {
        let size = settings::snap_text_size(size);
        self.settings.data.text_size = size;
        self.settings.mark_dirty();
        self.toast(Toast::info(format!(
            "Text {}%",
            (size / DEFAULT_TEXT_SIZE * 100.0).round()
        )));
    }

    fn set_zen(&mut self, ctx: &egui::Context, on: bool) {
        if self.doc.is_none() && on {
            return;
        }
        self.ui.zen = on;
        self.ui.popover = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    /// Ctrl+↑/↓: previous/next H1–H3 relative to the active section.
    fn heading_step(&mut self, dir: i32) {
        let active = self.out.active_heading;
        let Some(d) = self.doc.as_mut() else { return };
        let targets: Vec<usize> = d
            .view
            .document()
            .headings()
            .iter()
            .enumerate()
            .filter(|(_, h)| h.level <= 3)
            .map(|(i, _)| i)
            .collect();
        let next = match (dir > 0, active) {
            (true, Some(a)) => targets.iter().copied().find(|&i| i > a),
            (true, None) => targets.first().copied(),
            (false, Some(a)) => targets.iter().copied().rev().find(|&i| i < a),
            (false, None) => None,
        };
        match next {
            Some(i) => d.view.scroll_to_heading(i),
            None if dir < 0 => d.view.scroll_to_top(),
            None => {}
        }
    }

    // ---- input -----------------------------------------------------------------------------

    fn handle_input(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut actions = Vec::new();
        let typing = ctx.egui_wants_keyboard_input();
        let nothing_focused = ctx.memory(|m| m.focused().is_none());
        let has_doc = self.doc.is_some();
        ctx.input_mut(|i| {
            // Reader-driven scrolling (for app bar auto-hide): wheel, scroll keys, drags.
            let user_scroll = i.pointer.primary_down()
                || i.raw.events.iter().any(|e| match e {
                    Event::MouseWheel { modifiers, .. } => !modifiers.command,
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        !typing
                            && !modifiers.command
                            && matches!(
                                key,
                                Key::ArrowUp
                                    | Key::ArrowDown
                                    | Key::PageUp
                                    | Key::PageDown
                                    | Key::Space
                                    | Key::Home
                                    | Key::End
                            )
                    }
                    _ => false,
                });
            if user_scroll {
                self.user_scroll_at = Some(Instant::now());
            }
            let ctrl = Modifiers::COMMAND;
            let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
            // Shift variants first: consume_key ignores extra Shift.
            // egui-winit turns Ctrl+(Shift+)C into a bare Copy event; with Shift it means
            // "copy the Markdown source", so take the event away from the document.
            // Replay modifier changes in event order: a fast press can begin and end in one frame.
            let mut mods = self.modifiers;
            let mut shift_copy = false;
            for e in &i.events {
                match e {
                    Event::ModifiersChanged(m) => mods = *m,
                    Event::Copy => shift_copy |= mods.command && mods.shift,
                    _ => {}
                }
            }
            self.modifiers = i.modifiers;
            if shift_copy {
                i.events.retain(|e| !matches!(e, Event::Copy));
                actions.push(Action::CopySource);
            }
            let mut key = |mods: Modifiers, k: Key| i.consume_key(mods, k);
            if key(ctrl_shift, Key::L) {
                actions.push(Action::CycleTheme);
            }
            if key(ctrl_shift, Key::E) {
                actions.push(Action::Reveal);
            }
            if key(ctrl, Key::O) {
                actions.push(Action::OpenDialog);
            }
            if key(ctrl, Key::N) {
                actions.push(Action::NewWindow);
            }
            if key(ctrl, Key::W) {
                actions.push(Action::CloseWindow);
            }
            if key(ctrl, Key::F) {
                actions.push(Action::OpenFind);
            }
            if key(ctrl, Key::R) || key(Modifiers::NONE, Key::F5) {
                actions.push(Action::Reload);
            }
            if key(ctrl, Key::B) {
                actions.push(Action::ToggleOutline);
            }
            if key(ctrl, Key::E) {
                actions.push(Action::OpenInEditor);
            }
            if key(ctrl, Key::Comma) {
                actions.push(Action::TogglePopover(Popover::Aa));
            }
            if key(ctrl, Key::Slash) {
                actions.push(Action::ToggleShortcuts);
            }
            if key(ctrl, Key::Equals) || key(ctrl, Key::Plus) {
                actions.push(Action::StepTextSize(1));
            }
            if key(ctrl, Key::Minus) {
                actions.push(Action::StepTextSize(-1));
            }
            if key(ctrl, Key::Num0) {
                actions.push(Action::SetTextSize(DEFAULT_TEXT_SIZE));
            }
            if has_doc && key(ctrl, Key::ArrowUp) {
                actions.push(Action::HeadingStep(-1));
            }
            if has_doc && key(ctrl, Key::ArrowDown) {
                actions.push(Action::HeadingStep(1));
            }
            if key(Modifiers::ALT, Key::ArrowLeft) {
                actions.push(Action::Back);
            }
            if key(Modifiers::ALT, Key::ArrowRight) {
                actions.push(Action::Forward);
            }
            if key(Modifiers::NONE, Key::F11) {
                actions.push(Action::ToggleZen);
            }
            if self.ui.find.open {
                if key(Modifiers::SHIFT, Key::F3) {
                    actions.push(Action::FindPrev);
                } else if key(Modifiers::NONE, Key::F3) {
                    actions.push(Action::FindNext);
                }
                // Enter steps matches from anywhere, unless a button has focus (Tab + Enter).
                // While the input is focused, the find bar handles Enter itself.
                if nothing_focused {
                    if key(Modifiers::SHIFT, Key::Enter) {
                        actions.push(Action::FindPrev);
                    } else if key(Modifiers::NONE, Key::Enter) {
                        actions.push(Action::FindNext);
                    }
                }
            } else if has_doc && key(Modifiers::NONE, Key::F3) {
                actions.push(Action::OpenFind);
            }
            if key(Modifiers::NONE, Key::Escape)
                && let Some(a) = self.escape_action()
            {
                actions.push(a);
            }
            if i.pointer.button_pressed(PointerButton::Extra1) {
                actions.push(Action::Back);
            }
            if i.pointer.button_pressed(PointerButton::Extra2) {
                actions.push(Action::Forward);
            }

            // Ctrl+wheel: text size, one step per notch, throttled to 100 ms.
            for e in &i.raw.events {
                if let Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } = e
                    && modifiers.command
                {
                    self.wheel.accum += match unit {
                        MouseWheelUnit::Line => delta.y,
                        MouseWheelUnit::Point => delta.y / 50.0,
                        MouseWheelUnit::Page => delta.y * 3.0,
                    };
                }
            }
            if self.wheel.accum.abs() >= 1.0 {
                let ready = self
                    .wheel
                    .last_step
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(100));
                if ready {
                    actions.push(Action::StepTextSize(self.wheel.accum.signum() as i32));
                    self.wheel.last_step = Some(Instant::now());
                }
                self.wheel.accum = 0.0;
            }

            // Paste Markdown when no text field has focus.
            if !typing {
                for e in &i.events {
                    if let Event::Paste(text) = e
                        && !text.trim().is_empty()
                    {
                        actions.push(Action::Paste(text.clone()));
                    }
                }
            }

            // Drag & drop: the first Markdown file opens here, the rest in new windows.
            let mut first = true;
            let mut rejected = false;
            for f in &i.raw.dropped_files {
                let path = f.path();
                if path.as_os_str().is_empty() {
                    continue;
                }
                if !links::is_openable(path) {
                    rejected = true;
                } else if std::mem::take(&mut first) {
                    actions.push(Action::Open(path.to_path_buf()));
                } else {
                    actions.push(Action::OpenInNewWindow(path.to_path_buf(), None));
                }
            }
            if rejected && first {
                self.ui.toast = Some(Toast::error("Not a Markdown file"));
            }
        });
        actions
    }

    /// Esc closes, in order: popover or menu → shortcut overlay → find → overlay outline → Zen.
    fn escape_action(&self) -> Option<Action> {
        if self.ui.popover.is_some() {
            Some(Action::ClosePopover)
        } else if self.ui.shortcuts {
            Some(Action::ToggleShortcuts)
        } else if self.ui.find.open {
            Some(Action::CloseFind)
        } else if self.ui.outline_overlay {
            Some(Action::CloseOutlineOverlay)
        } else if self.ui.zen {
            Some(Action::ToggleZen)
        } else {
            None
        }
    }

    // ---- per-frame helpers -----------------------------------------------------------------

    fn sync_theme(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let pref = self.args.theme.unwrap_or(self.settings.data.theme);
        let kind = theme::resolve(pref, ctx.system_theme());
        if self.theme != Some(kind) {
            self.theme = Some(kind);
            self.palette = Palette::for_theme(kind);
            theme::apply(ctx, kind, &self.palette);
            platform::style_title_bar(frame, self.palette.bg, self.palette.text, kind.is_dark());
        }
    }

    fn window_title(&self) -> String {
        match &self.doc {
            None => "cutemarkdown".into(),
            Some(d) => match &d.location {
                Location::Pasted(_) => "Pasted text — cutemarkdown".into(),
                Location::File(p) => {
                    let missing = if d.missing { " (missing)" } else { "" };
                    format!("{}{missing} — cutemarkdown", ui::menu::file_name(p))
                }
            },
        }
    }

    fn track_geometry(&mut self, ctx: &egui::Context) {
        if self.args.is_qa() || self.ui.zen {
            return;
        }
        let (outer, inner, maximized, minimized, fullscreen) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.outer_rect,
                v.inner_rect,
                v.maximized.unwrap_or(false),
                v.minimized.unwrap_or(false),
                v.fullscreen.unwrap_or(false),
            )
        });
        if minimized || fullscreen {
            return;
        }
        let mut g = self.settings.data.window.unwrap_or(WindowGeom {
            x: 0.0,
            y: 0.0,
            w: 1100.0,
            h: 860.0,
            maximized: false,
        });
        if maximized {
            g.maximized = true;
        } else if let (Some(o), Some(i)) = (outer, inner) {
            g = WindowGeom {
                x: o.min.x,
                y: o.min.y,
                w: i.width(),
                h: i.height(),
                maximized: false,
            };
        } else {
            return;
        }
        if self.settings.data.window != Some(g) {
            self.settings.data.window = Some(g);
            self.settings.mark_dirty();
        }
    }

    /// QA setup applied on the first frame.
    fn qa_setup(&mut self, ctx: &egui::Context) {
        if self.args.zen {
            self.set_zen(ctx, true);
        }
        if let Some(q) = self.args.find.clone() {
            self.ui.find.open = true;
            self.ui.find.query = q;
            self.send_find_query();
        }
        match self.args.open {
            Some(Open::Aa) => self.ui.popover = Some(Popover::Aa),
            Some(Open::Menu | Open::Recent) => self.ui.popover = Some(Popover::Menu),
            Some(Open::Outline) => {
                self.settings.data.outline_open = true;
                self.ui.outline_overlay = true;
            }
            Some(Open::Shortcuts) => self.ui.shortcuts = true,
            Some(Open::Drag) | None => {}
        }
        if let Some(t) = self.args.toast.clone() {
            self.toast(Toast::info(t).with_icon(Icon::Check));
        }
        if let Some(url) = self.args.hover_link.clone() {
            self.ui.hovered_link = Some((url, Instant::now() - Duration::from_secs(1)));
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.args.screenshot.clone() else {
            return;
        };
        if self.frame_no == self.args.frames.unwrap_or(20) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let saved = image::RgbaImage::from_raw(
                image.width() as u32,
                image.height() as u32,
                image.as_raw().to_vec(),
            )
            .ok_or_else(|| "bad screenshot buffer".to_owned())
            .and_then(|img| img.save(&path).map_err(|e| e.to_string()));
            match saved {
                Ok(()) => {
                    eprintln!(
                        "saved {} ({}x{})",
                        path.display(),
                        image.width(),
                        image.height()
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("screenshot failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        ctx.request_repaint();
    }
}

/// "8 min left" (230 words per minute; "<1 min left" near the end). Until the engine reports
/// `words_remaining`, estimate it from the total and the reading progress.
fn time_left(out: &DocOutput, total_words: usize) -> String {
    let words = if out.words_remaining > 0 || out.progress >= 0.999 {
        out.words_remaining
    } else {
        (total_words as f32 * (1.0 - out.progress)).round() as usize
    };
    if words < 230 {
        "<1 min left".into()
    } else {
        format!("{} min left", (words as f32 / 230.0).round() as usize)
    }
}

fn demo_recents() -> Vec<RecentEntry> {
    let here = std::env::current_dir().unwrap_or_default();
    let samples = here.join("samples");
    let entry = |p: PathBuf| RecentEntry {
        path: p,
        opened: settings::now_rfc3339(),
        hash: None,
        anchor: None,
    };
    let fake_home = if cfg!(windows) {
        PathBuf::from(r"C:\Users\harsh\Documents")
    } else {
        PathBuf::from("/home/harsh/Documents")
    };
    vec![
        entry(samples.join("ai-report.md")),
        entry(samples.join("architecture.md")),
        entry(fake_home.join("notes").join("weekly-sync-2026-09-28.md")),
        entry(samples.join("commonmark-edge.md")),
        entry(
            fake_home
                .join("projects")
                .join("cutemarkdown")
                .join("docs")
                .join("design")
                .join("very-long-folder-name-for-ellipsis")
                .join("SPEC.md"),
        ),
        entry(samples.join("long.md")),
    ]
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        FIRST_FRAME_SHOWN.store(true, Ordering::Relaxed);
        let ctx = root.ctx().clone();
        self.frame_no += 1;
        if self.frame_no == 1 {
            if self.settings.data.renderer != self.renderer {
                self.settings.data.renderer = self.renderer;
                self.settings.mark_dirty();
            }
            self.qa_setup(&ctx);
        }
        if self.frame_no == 3
            && let (Some(px), Some(d)) = (self.args.scroll, self.doc.as_mut())
        {
            d.view.set_scroll_offset(px);
            self.programmatic_at = Some(Instant::now());
        }
        self.sync_theme(&ctx, frame);
        self.poll_watcher();
        if let Some(t) = self.ui.find.edited_at {
            let left = FIND_DEBOUNCE.saturating_sub(t.elapsed());
            if left.is_zero() {
                self.send_find_query();
            } else {
                ctx.request_repaint_after(left);
            }
        }
        for a in self.handle_input(&ctx) {
            self.apply(&ctx, frame, a);
        }

        let p = self.palette.clone();
        let kind = self.theme.unwrap_or_default();
        let screen = root.max_rect();
        root.painter().rect_filled(screen, 0.0, p.bg);
        let mut actions = Vec::new();

        // Layout: docked outline when the window fits sidebar + measure + 2×48 gutters.
        let s = &self.settings.data;
        let nominal = theme::nominal_measure(s.width, s.text_size).unwrap_or(46.0 * s.text_size);
        self.docked_fits = screen.width() >= OUTLINE_W + nominal + 96.0;
        let outline_ok = self.doc.as_ref().is_some_and(|d| d.outline_available) && !self.ui.zen;
        if self.docked_fits {
            self.ui.outline_overlay = false;
        }
        let docked_on = outline_ok && self.docked_fits && s.outline_open;
        let overlay_on = outline_ok && !self.docked_fits && self.ui.outline_overlay;
        let docked_t =
            ctx.animate_bool_with_time(Id::new("outline-docked-t"), docked_on, PANEL_SECS);
        let overlay_t =
            ctx.animate_bool_with_time(Id::new("outline-overlay-t"), overlay_on, PANEL_SECS);
        let doc_rect = Rect::from_min_max(
            pos2(screen.left() + OUTLINE_W * docked_t, screen.top()),
            screen.max,
        );
        let measure = theme::measure(s.width, s.text_size, doc_rect.width());
        let style = theme::engine_style(kind, s, measure);

        // Document (or empty state), then the docked outline beside it.
        let mut scroll_y = 0.0;
        if let Some(doc) = self.doc.as_mut() {
            let mut child = root.new_child(UiBuilder::new().max_rect(doc_rect).id_salt("doc-view"));
            child.set_clip_rect(doc_rect);
            let out = doc.view.show(&mut child, &style);
            scroll_y = doc.view.scroll_offset();
            if let Some(t) = &out.clicked_link {
                let new_window = ctx.input(|i| {
                    i.modifiers.command || i.pointer.button_released(PointerButton::Middle)
                });
                actions.push(Action::Link(t.clone(), new_window));
            }
            if out.copied {
                self.ui.toast = Some(Toast::info("Copied").with_icon(Icon::Check));
            }
            let props = outline::OutlineProps {
                entries: &doc.outline,
                active: out.active_heading,
                time_left: Some(time_left(&out, doc.view.document().word_count())),
            };
            outline::show_docked(root, screen, docked_t, &props, &p, &mut actions);
            outline::show_overlay(&ctx, screen, overlay_t, &props, &p, &mut actions);
            self.out = out;
        } else {
            ui::empty::show(root, screen, self.recents(), &p, &mut actions);
            self.out = DocOutput::default();
        }

        // App bar, auto-hiding while reading (always shown on the empty state).
        let has_doc = self.doc.is_some();
        let now = Instant::now();
        let recent = |t: Option<Instant>, ms: u64| {
            t.is_some_and(|t| now.duration_since(t) < Duration::from_millis(ms))
        };
        let (pointer_y, alt) = ctx.input(|i| {
            (
                i.pointer.hover_pos().map(|p| p.y - screen.top()),
                i.modifiers.alt,
            )
        });
        let bar_input = ui::autohide::BarInput {
            now,
            scroll_y,
            user_scrolling: recent(self.user_scroll_at, 400) && !recent(self.programmatic_at, 600),
            pointer_y,
            hovered: self.bar.hovered,
            pinned: self.ui.bar_pinned(),
            alt,
            zen: self.ui.zen,
        };
        let shown = self.autohide.update(&bar_input) || !has_doc;
        if !shown && pointer_y.is_some_and(|y| y <= 56.0) {
            ctx.request_repaint_after(Duration::from_millis(50)); // top-edge dwell
        }
        let shown_t = ctx.animate_bool_with_time(Id::new("bar-shown"), shown, PANEL_SECS);
        let border_t = ctx.animate_bool_with_time(Id::new("bar-border"), scroll_y > 0.0, 0.12);
        let props = ui::app_bar::BarProps {
            has_doc,
            outline_available: outline_ok,
            outline_on: docked_on || overlay_on,
            has_history: self.history.has_history(),
            can_back: self.history.can_back(),
            can_forward: self.history.can_forward(),
            find_open: self.ui.find.open,
            popover: self.ui.popover,
            missing: self.doc.as_ref().is_some_and(|d| d.missing),
            border_t,
            shown_t,
        };
        self.bar = ui::app_bar::show(&ctx, screen, &props, &p, &mut actions);

        // Find bar.
        if self.ui.find.open && has_doc {
            ui::find_bar::show(&ctx, doc_rect, &mut self.ui.find, &p, &mut actions);
        }

        // Popovers: close on a click outside them (and outside their button).
        let mut inside = Vec::new();
        match self.ui.popover {
            Some(Popover::Aa) => {
                let theme_pref = self.args.theme.unwrap_or(self.settings.data.theme);
                inside.push(ui::aa::show(
                    &ctx,
                    screen,
                    self.bar.aa_button,
                    &self.settings.data,
                    theme_pref,
                    &p,
                    &mut actions,
                ));
                inside.push(self.bar.aa_button);
            }
            Some(Popover::Menu) => {
                let props = ui::menu::MenuProps {
                    has_file: self.doc.as_ref().and_then(Doc::path).is_some(),
                    has_doc,
                    recent: self.recents(),
                    words: self.doc.as_ref().map(|d| d.view.document().word_count()),
                    force_recent: self.args.open == Some(Open::Recent)
                        && self.args.screenshot.is_some(),
                };
                inside.extend(ui::menu::show(
                    &ctx,
                    screen,
                    self.bar.menu_button,
                    &props,
                    &p,
                    &mut actions,
                ));
                inside.push(self.bar.menu_button);
                // Menu commands close the menu.
                if actions
                    .iter()
                    .any(|a| !matches!(a, Action::TogglePopover(_)))
                {
                    actions.insert(0, Action::ClosePopover);
                }
            }
            None => {}
        }
        if self.ui.popover.is_some() {
            let clicked_outside = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|pos| !inside.iter().any(|r| r.contains(pos)))
            });
            if clicked_outside {
                actions.push(Action::ClosePopover);
            }
        }

        // Toast and progress line.
        if let Some(t) = &self.ui.toast
            && !ui::toast::show(&ctx, screen, t, &p)
        {
            self.ui.toast = None;
        }
        if has_doc {
            ui::progress::show(&ctx, screen, self.out.progress, &p);
        }

        // Link status pill: 300 ms after hovering a link, gone as soon as the pointer leaves.
        let hovered = self
            .out
            .hovered_link
            .clone()
            .or_else(|| self.args.hover_link.clone());
        match hovered {
            Some(url)
                if self
                    .ui
                    .hovered_link
                    .as_ref()
                    .is_some_and(|(u, _)| *u == url) => {}
            Some(url) => self.ui.hovered_link = Some((url, now)),
            None => self.ui.hovered_link = None,
        }
        if let Some((url, since)) = &self.ui.hovered_link {
            let wait = Duration::from_millis(300).saturating_sub(since.elapsed());
            if wait.is_zero() {
                ui::overlays::link_pill(&ctx, screen, url, &p);
            } else {
                ctx.request_repaint_after(wait);
            }
        }

        if self.ui.shortcuts {
            ui::overlays::shortcuts(&ctx, screen, &p, &mut actions);
        }
        let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if dragging || self.args.open == Some(Open::Drag) {
            ui::overlays::drag_overlay(&ctx, screen, &p);
        }

        for a in actions {
            self.apply(&ctx, frame, a);
        }

        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        self.track_geometry(&ctx);
        if let Some(wait) = self.settings.tick() {
            ctx.request_repaint_after(wait);
        }
        self.screenshot(&ctx);
    }

    fn on_exit(&mut self) {
        self.settings.flush();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.bg.to_normalized_gamma_f32()
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}
