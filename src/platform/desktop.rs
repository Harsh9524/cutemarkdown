//! Linux and other non-Windows desktops: `xdg-open` for everything (development and CI builds).

use std::io;
use std::path::Path;
use std::process::Command;

use super::spawn_detached;

/// Open an `http(s)`/`mailto` URL with the default handler.
pub fn open_url(url: &str) -> io::Result<()> {
    spawn_detached(Command::new("xdg-open").arg(url))
}

/// Open an allowlisted local file with its associated app.
pub fn open_with_system(path: &Path) -> io::Result<()> {
    spawn_detached(Command::new("xdg-open").arg(path))
}

/// Open the folder containing `path` (file managers can't reliably select a file).
pub fn reveal(path: &Path) -> io::Result<()> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    spawn_detached(Command::new("xdg-open").arg(dir))
}

pub fn open_in_editor(path: &Path) -> io::Result<()> {
    spawn_detached(Command::new("xdg-open").arg(path))
}

pub fn rect_on_screen(_x: f32, _y: f32, _w: f32, _h: f32) -> bool {
    true
}

pub fn style_title_bar(
    _frame: &eframe::Frame,
    _caption: egui::Color32,
    _text: egui::Color32,
    _dark: bool,
) {
}

/// Last-resort error report when no window can be shown.
pub fn fatal_message(title: &str, text: &str) {
    eprintln!("{title}: {text}");
}
