//! OS integration: launching things, the title bar and message boxes.
//!
//! Every launcher here is only ever called with targets that passed `links::classify` (or with a
//! file the user opened), so no function in this module decides what is safe to launch.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;

#[cfg(windows)]
mod windows;

/// Open an `http(s)`/`mailto` URL with the default handler.
pub fn open_url(url: &str) -> io::Result<()> {
    #[cfg(windows)]
    return windows::shell_execute("open", url.as_ref());
    #[cfg(not(windows))]
    return spawn_detached(Command::new("xdg-open").arg(url));
}

/// Open an allowlisted local file with its associated app.
pub fn open_with_system(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    return windows::shell_execute("open", path.as_os_str());
    #[cfg(not(windows))]
    return spawn_detached(Command::new("xdg-open").arg(path));
}

/// Show `path` selected in Explorer (on Linux: open its folder).
pub fn reveal(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer parses its own command line; `/select,"path"` must be passed verbatim.
        return spawn_detached(
            Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())),
        );
    }
    #[cfg(not(windows))]
    {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        spawn_detached(Command::new("xdg-open").arg(dir))
    }
}

/// Open in the user's editor: the shell `edit` verb, falling back to Notepad (Windows), or
/// `xdg-open` elsewhere.
pub fn open_in_editor(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        if windows::shell_execute("edit", path.as_os_str()).is_ok() {
            return Ok(());
        }
        return spawn_detached(Command::new("notepad.exe").arg(path));
    }
    #[cfg(not(windows))]
    spawn_detached(Command::new("xdg-open").arg(path))
}

/// Start another cutemarkdown window (a new process of this exe).
pub fn spawn_window(args: &[OsString]) -> io::Result<()> {
    spawn_detached(Command::new(std::env::current_exe()?).args(args))
}

/// Spawn and reap in the background so no zombie is left behind.
fn spawn_detached(cmd: &mut Command) -> io::Result<()> {
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Whether a window rectangle (logical points) overlaps any monitor. Used to re-center windows
/// whose saved position falls off every screen.
pub fn rect_on_screen(x: f32, y: f32, w: f32, h: f32) -> bool {
    #[cfg(windows)]
    return windows::rect_on_screen(x, y, w, h);
    #[cfg(not(windows))]
    {
        let _ = (x, y, w, h);
        true
    }
}

/// Native title bar to match the theme (Windows 11: caption color; 10/11: dark mode).
pub fn style_title_bar(
    frame: &eframe::Frame,
    caption: egui::Color32,
    text: egui::Color32,
    dark: bool,
) {
    #[cfg(windows)]
    windows::style_title_bar(frame, caption, text, dark);
    #[cfg(not(windows))]
    let _ = (frame, caption, text, dark);
}

/// Last-resort error report when no window can be shown.
pub fn fatal_message(title: &str, text: &str) {
    eprintln!("{title}: {text}");
    #[cfg(windows)]
    windows::message_box(title, text);
}
