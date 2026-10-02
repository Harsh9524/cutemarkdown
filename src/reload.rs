//! Live reload (SPEC §7): a background thread polls the file's mtime and size and reports
//! changes once the size has been stable for one short interval (so half-written saves aren't
//! shown), or after 1 s at most. Atomic-rename saves are fine because we poll the path, not a
//! handle. Content that hashes the same is ignored by the app, not here.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant, SystemTime};

const POLL: Duration = Duration::from_millis(500);
/// Faster polling while a change is settling.
const SETTLE_POLL: Duration = Duration::from_millis(150);
const MAX_SETTLE: Duration = Duration::from_secs(1);

/// What identifies a version of the file on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

impl Stamp {
    pub fn of(path: &std::path::Path) -> Option<Self> {
        let m = std::fs::metadata(path).ok()?;
        m.is_file().then(|| Self {
            modified: m.modified().ok(),
            len: m.len(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    /// The file changed (or reappeared): re-read it.
    Changed,
    /// The file is gone; keep showing the last render.
    Missing,
}

/// Pure change detector, fed one observation per poll.
#[derive(Debug)]
pub struct WatchState {
    known: Option<Stamp>,
    missing: bool,
    pending: Option<(Stamp, Instant)>,
}

impl WatchState {
    pub fn new(initial: Option<Stamp>) -> Self {
        Self {
            known: initial,
            missing: initial.is_none(),
            pending: None,
        }
    }

    /// Feed the current stamp (`None` = file missing).
    pub fn observe(&mut self, stamp: Option<Stamp>, now: Instant) -> Option<WatchEvent> {
        let Some(stamp) = stamp else {
            self.pending = None;
            return (!std::mem::replace(&mut self.missing, true)).then_some(WatchEvent::Missing);
        };
        if self.known == Some(stamp) && !self.missing {
            self.pending = None;
            return None;
        }
        match self.pending {
            // Same stamp as last poll (size stable), or we've waited long enough: report.
            Some((p, since)) if p == stamp || now.duration_since(since) >= MAX_SETTLE => {
                self.known = Some(stamp);
                self.missing = false;
                self.pending = None;
                Some(WatchEvent::Changed)
            }
            Some((_, since)) => {
                self.pending = Some((stamp, since));
                None
            }
            None => {
                self.pending = Some((stamp, now));
                None
            }
        }
    }

    pub fn settling(&self) -> bool {
        self.pending.is_some()
    }
}

/// Polls one file on a background thread. Dropping it stops the thread.
pub struct FileWatcher {
    rx: Receiver<WatchEvent>,
    stop: Arc<AtomicBool>,
}

impl FileWatcher {
    /// `initial` is the stamp taken *before* the shown content was read.
    pub fn spawn(path: PathBuf, initial: Option<Stamp>, ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("cutemarkdown-watch".into())
            .spawn(move || watch_loop(&path, initial, &s, &tx, &ctx));
        if let Err(e) = spawned {
            eprintln!("cutemarkdown: live reload unavailable: {e}");
        }
        Self { rx, stop }
    }

    /// Latest event since the last call (older ones are superseded).
    pub fn poll(&self) -> Option<WatchEvent> {
        self.rx.try_iter().last()
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn watch_loop(
    path: &std::path::Path,
    initial: Option<Stamp>,
    stop: &AtomicBool,
    tx: &Sender<WatchEvent>,
    ctx: &egui::Context,
) {
    let mut state = WatchState::new(initial);
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(if state.settling() { SETTLE_POLL } else { POLL });
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(ev) = state.observe(Stamp::of(path), Instant::now()) {
            if tx.send(ev).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(len: u64, secs: u64) -> Option<Stamp> {
        Some(Stamp {
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
            len,
        })
    }

    #[test]
    fn unchanged_file_is_quiet() {
        let t = Instant::now();
        let mut w = WatchState::new(stamp(10, 1));
        assert_eq!(w.observe(stamp(10, 1), t), None);
        assert_eq!(w.observe(stamp(10, 1), t + POLL), None);
    }

    #[test]
    fn change_is_reported_once_size_is_stable() {
        let t = Instant::now();
        let mut w = WatchState::new(stamp(10, 1));
        assert_eq!(w.observe(stamp(20, 2), t), None, "first sighting waits");
        assert_eq!(
            w.observe(stamp(30, 2), t + SETTLE_POLL),
            None,
            "still growing"
        );
        assert_eq!(
            w.observe(stamp(30, 2), t + 2 * SETTLE_POLL),
            Some(WatchEvent::Changed)
        );
        assert_eq!(
            w.observe(stamp(30, 2), t + 3 * SETTLE_POLL),
            None,
            "reported once"
        );
    }

    #[test]
    fn a_file_that_keeps_growing_is_reported_within_a_second() {
        let t = Instant::now();
        let mut w = WatchState::new(stamp(0, 1));
        let mut got = None;
        for i in 1..=10u32 {
            got = got.or(w.observe(stamp(u64::from(i) * 100, 2), t + SETTLE_POLL * i));
        }
        assert_eq!(got, Some(WatchEvent::Changed));
    }

    #[test]
    fn missing_then_reappearing() {
        let t = Instant::now();
        let mut w = WatchState::new(stamp(10, 1));
        assert_eq!(w.observe(None, t), Some(WatchEvent::Missing));
        assert_eq!(w.observe(None, t + POLL), None, "missing reported once");
        // Reappears with the same stamp (e.g. restored): still re-read.
        assert_eq!(w.observe(stamp(10, 1), t + 2 * POLL), None);
        assert_eq!(
            w.observe(stamp(10, 1), t + 2 * POLL + SETTLE_POLL),
            Some(WatchEvent::Changed)
        );
    }

    #[test]
    fn real_file_round_trip() {
        let dir = std::env::temp_dir().join(format!("cutemarkdown-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.md");
        std::fs::write(&path, "one").unwrap();
        let s1 = Stamp::of(&path);
        assert_eq!(s1.unwrap().len, 3);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Stamp::of(&path), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
