//! OS integration: launching things, the title bar and message boxes.
//!
//! Every launcher here is only ever called with targets that passed `links::classify` (or with a
//! file the user opened), so no function in this module decides what is safe to launch.

use std::ffi::OsString;
use std::io;
use std::process::Command;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{
    fatal_message, open_in_editor, open_url, open_with_system, rect_on_screen, reveal,
    style_title_bar,
};

#[cfg(not(windows))]
mod desktop;
#[cfg(not(windows))]
pub use desktop::{
    fatal_message, open_in_editor, open_url, open_with_system, rect_on_screen, reveal,
    style_title_bar,
};

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
