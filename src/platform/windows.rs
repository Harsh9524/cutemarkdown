//! Windows: Shell and DWM calls via `windows-sys`.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_OK, MessageBoxW, SW_SHOWNORMAL,
};

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

use super::spawn_detached;

/// Open an `http(s)`/`mailto` URL with the default handler.
pub fn open_url(url: &str) -> io::Result<()> {
    shell_execute("open", url.as_ref())
}

/// Open an allowlisted local file with its associated app.
pub fn open_with_system(path: &Path) -> io::Result<()> {
    shell_execute("open", path.as_os_str())
}

/// Show `path` selected in Explorer.
pub fn reveal(path: &Path) -> io::Result<()> {
    // explorer parses its own command line; `/select,"path"` must be passed verbatim.
    spawn_detached(Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())))
}

/// The shell `edit` verb (the user's editor for that type), else Notepad.
pub fn open_in_editor(path: &Path) -> io::Result<()> {
    shell_execute("edit", path.as_os_str())
        .or_else(|_| spawn_detached(Command::new("notepad.exe").arg(path)))
}

/// Last-resort error report when no window can be shown.
pub fn fatal_message(title: &str, text: &str) {
    eprintln!("{title}: {text}");
    message_box(title, text);
}

/// `ShellExecuteW(verb, file)`. Values ≤ 32 are errors.
fn shell_execute(verb: &str, file: &OsStr) -> io::Result<()> {
    let verb = wide(verb.as_ref());
    let file = wide(file);
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if r as usize > 32 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "ShellExecuteW failed ({})",
            r as usize
        )))
    }
}

/// Whether a window rectangle (logical points) overlaps any monitor.
pub fn rect_on_screen(x: f32, y: f32, w: f32, h: f32) -> bool {
    // SAFETY: no arguments.
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    // Require a reasonable grab area (the top strip of the window) to be visible.
    let rect = RECT {
        left: (x * scale) as i32,
        top: (y * scale) as i32,
        right: ((x + w.min(200.0)) * scale) as i32,
        bottom: ((y + h.min(40.0)) * scale) as i32,
    };
    // SAFETY: `rect` is a valid RECT for the duration of the call.
    !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_null()
}

fn hwnd(frame: &eframe::Frame) -> Option<HWND> {
    match frame.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

fn colorref(c: egui::Color32) -> u32 {
    u32::from(c.r()) | (u32::from(c.g()) << 8) | (u32::from(c.b()) << 16)
}

/// Caption color = `bg` (Windows 11) and immersive dark mode in Dark (Windows 10 20H1+).
pub fn style_title_bar(
    frame: &eframe::Frame,
    caption: egui::Color32,
    text: egui::Color32,
    dark: bool,
) {
    let Some(hwnd) = hwnd(frame) else { return };
    let dark = i32::from(dark);
    let (caption, text) = (colorref(caption), colorref(text));
    // SAFETY: valid HWND from eframe; each pointer refers to a live 4-byte value. Failures (e.g.
    // DWMWA_CAPTION_COLOR before Windows 11) are harmless and ignored.
    unsafe {
        let set = |attr: i32, value: *const u32| {
            DwmSetWindowAttribute(hwnd, attr as u32, value.cast(), 4);
        };
        set(DWMWA_USE_IMMERSIVE_DARK_MODE, (&raw const dark).cast());
        set(DWMWA_CAPTION_COLOR, &raw const caption);
        set(DWMWA_TEXT_COLOR, &raw const text);
    }
}

fn message_box(title: &str, text: &str) {
    let (title, text) = (wide(title.as_ref()), wide(text.as_ref()));
    // SAFETY: NUL-terminated UTF-16 buffers that outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
