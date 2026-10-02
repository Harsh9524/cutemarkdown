//! OS integration: launching things, the title bar and message boxes.
//!
//! Every launcher here is only ever called with targets that passed `links::classify` (or with a
//! file the user opened), so no function in this module decides what is safe to launch.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{
    animations_enabled, fatal_message, open_in_editor, open_url, open_with_system, rect_on_screen,
    reveal, style_title_bar,
};

#[cfg(not(windows))]
mod desktop;
#[cfg(not(windows))]
pub use desktop::{
    animations_enabled, fatal_message, open_in_editor, open_url, open_with_system, rect_on_screen,
    reveal, style_title_bar,
};

/// VS Code's command-line launcher (`bin\code.cmd` on Windows).
const VS_CODE: &[&str] = if cfg!(windows) {
    &["code.cmd", "code.exe"]
} else {
    &["code"]
};

/// "Open in editor here" (a heading's context menu): VS Code at that line (`code -g file:line`)
/// when `code` is on PATH, else the normal Ctrl+E editor, which opens the file without a line.
pub fn open_in_editor_at(path: &Path, line: usize) -> io::Result<()> {
    if let Some(code) = find_on_path(VS_CODE) {
        let mut target = path.as_os_str().to_owned();
        target.push(format!(":{line}"));
        let mut cmd = Command::new(code);
        cmd.arg("-g").arg(target);
        #[cfg(windows)]
        {
            // `code.cmd` is a batch file: don't flash a console window.
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if spawn_detached(&mut cmd).is_ok() {
            return Ok(());
        }
    }
    open_in_editor(path)
}

/// The first of `names` found in an absolute `PATH` directory (relative entries are skipped, so
/// the current directory never supplies the program).
fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
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
