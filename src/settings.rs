//! Persisted settings (SPEC §8): `%APPDATA%\cutemarkdown\settings.json` on Windows,
//! `$XDG_CONFIG_HOME/cutemarkdown/settings.json` (or `~/.config/…`) elsewhere.
//!
//! Loading is lenient: unknown keys are ignored and a bad value only resets that key. A file that
//! isn't a JSON object at all is moved aside as `settings.bad.json` and defaults are used. Writes
//! are debounced (500 ms) and atomic (temp file + rename).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::renderer::Renderer;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    #[default]
    Auto,
    Light,
    Sepia,
    Dark,
}

impl ThemePref {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Light, Self::Sepia, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Light => "Light",
            Self::Sepia => "Sepia",
            Self::Dark => "Dark",
        }
    }

    /// Ctrl+Shift+L order: Auto → Light → Sepia → Dark → Auto.
    pub fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|&t| t == self).unwrap_or(0) + 1) % Self::ALL.len()]
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.label().eq_ignore_ascii_case(s))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontPref {
    #[default]
    Sans,
    Serif,
}

/// Reading measure (SPEC §3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Width {
    Narrow,
    #[default]
    Medium,
    Wide,
    Full,
}

impl Width {
    pub const ALL: [Self; 4] = [Self::Narrow, Self::Medium, Self::Wide, Self::Full];

    pub fn label(self) -> &'static str {
        match self {
            Self::Narrow => "Narrow",
            Self::Medium => "Medium",
            Self::Wide => "Wide",
            Self::Full => "Full",
        }
    }

    /// Measure in em of the sans text size; `None` for Full (viewport minus gutters).
    pub fn ems(self) -> Option<f32> {
        match self {
            Self::Narrow => Some(38.0),
            Self::Medium => Some(46.0),
            Self::Wide => Some(56.0),
            Self::Full => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeom {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub maximized: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: PathBuf,
    /// RFC 3339 UTC timestamp.
    #[serde(default)]
    pub opened: String,
    /// Reserved for resume-at-anchor (P2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub theme: ThemePref,
    pub font: FontPref,
    pub text_size: f32,
    pub width: Width,
    pub wrap_code: bool,
    pub outline_open: bool,
    pub outline_width: f32,
    pub window: Option<WindowGeom>,
    pub recent: Vec<RecentEntry>,
    /// Custom editor command (P2); kept so it round-trips.
    pub editor: Option<String>,
    /// GPU backend that worked last time (see `renderer.rs`).
    pub renderer: Renderer,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePref::Auto,
            font: FontPref::Sans,
            text_size: DEFAULT_TEXT_SIZE,
            width: Width::Medium,
            wrap_code: false,
            outline_open: true,
            outline_width: 264.0,
            window: None,
            recent: Vec::new(),
            editor: None,
            renderer: Renderer::Auto,
        }
    }
}

pub const DEFAULT_TEXT_SIZE: f32 = 16.0;
/// SPEC §4 text size steps.
pub const TEXT_SIZES: [f32; 12] = [
    12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0,
];
pub const RECENT_STORED: usize = 12;
pub const RECENT_SHOWN: usize = 8;

/// Snap to the nearest allowed text size.
pub fn snap_text_size(size: f32) -> f32 {
    TEXT_SIZES
        .into_iter()
        .min_by(|a, b| (a - size).abs().total_cmp(&(b - size).abs()))
        .unwrap_or(DEFAULT_TEXT_SIZE)
}

/// One step up (`dir > 0`) or down from `size`, clamped to the ends.
pub fn step_text_size(size: f32, dir: i32) -> f32 {
    let i = TEXT_SIZES
        .iter()
        .position(|&s| s == snap_text_size(size))
        .unwrap_or(4) as i32;
    TEXT_SIZES[(i + dir.signum()).clamp(0, TEXT_SIZES.len() as i32 - 1) as usize]
}

impl Settings {
    /// Parse leniently. `Err` only if the text isn't a JSON object.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let Value::Object(obj) = value else {
            return Err("settings is not a JSON object".into());
        };
        let mut s = Self::default();
        // Each key is taken only if it parses, so one bad value never resets the rest.
        macro_rules! take {
            ($($field:ident),*) => {$(
                if let Some(v) = obj.get(stringify!($field)) {
                    if let Ok(x) = serde_json::from_value(v.clone()) {
                        s.$field = x;
                    }
                }
            )*};
        }
        take!(
            theme,
            font,
            text_size,
            width,
            wrap_code,
            outline_open,
            outline_width,
            window,
            editor,
            renderer
        );
        if let Some(Value::Array(items)) = obj.get("recent") {
            s.recent = items
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect();
        }
        s.sanitize();
        Ok(s)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serialize")
    }

    fn sanitize(&mut self) {
        self.text_size = if self.text_size.is_finite() {
            snap_text_size(self.text_size)
        } else {
            DEFAULT_TEXT_SIZE
        };
        self.outline_width = if self.outline_width.is_finite() {
            self.outline_width.clamp(200.0, 400.0)
        } else {
            264.0
        };
        if let Some(w) = self.window {
            let finite = [w.x, w.y, w.w, w.h].iter().all(|v| v.is_finite());
            if !finite || w.w < 200.0 || w.h < 150.0 {
                self.window = None;
            }
        }
        self.recent.retain(|r| !r.path.as_os_str().is_empty());
        self.recent.truncate(RECENT_STORED);
    }

    /// Move `path` to the top of the recent list.
    pub fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|r| !same_path(&r.path, path));
        self.recent.insert(
            0,
            RecentEntry {
                path: path.to_path_buf(),
                opened: now_rfc3339(),
                hash: None,
                anchor: None,
            },
        );
        self.recent.truncate(RECENT_STORED);
    }

    pub fn remove_recent(&mut self, path: &Path) {
        self.recent.retain(|r| !same_path(&r.path, path));
    }
}

/// Path equality as the OS sees it (case-insensitive on Windows).
pub fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.as_os_str().to_string_lossy().to_lowercase()
            == b.as_os_str().to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Default settings file location.
pub fn default_path() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(x)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config")
    };
    Some(dir.join("cutemarkdown").join("settings.json"))
}

/// Why loading fell back to defaults (for logging).
#[derive(Debug, PartialEq)]
pub enum LoadIssue {
    /// The file was unreadable as settings and was moved to this path.
    Corrupt { backup: PathBuf, reason: String },
    /// The file exists but couldn't be read; we won't overwrite it.
    Unreadable(String),
}

/// Settings plus where and when to save them.
pub struct SettingsStore {
    pub data: Settings,
    path: Option<PathBuf>,
    /// When false nothing is written (QA runs, unreadable file).
    writable: bool,
    dirty_since: Option<Instant>,
    saved: Option<String>,
}

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

impl SettingsStore {
    /// In-memory settings that are never written.
    pub fn memory(data: Settings) -> Self {
        Self {
            data,
            path: None,
            writable: false,
            dirty_since: None,
            saved: None,
        }
    }

    pub fn load(path: PathBuf) -> (Self, Option<LoadIssue>) {
        let (data, issue, writable) = match std::fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None, true),
            Err(e) => (
                Settings::default(),
                Some(LoadIssue::Unreadable(e.to_string())),
                false,
            ),
            Ok(bytes) => match Settings::from_json(&String::from_utf8_lossy(&bytes)) {
                Ok(s) => (s, None, true),
                Err(reason) => {
                    let backup = path.with_file_name("settings.bad.json");
                    let _ = std::fs::remove_file(&backup);
                    let _ = std::fs::rename(&path, &backup);
                    (
                        Settings::default(),
                        Some(LoadIssue::Corrupt { backup, reason }),
                        true,
                    )
                }
            },
        };
        let saved = Some(data.to_json());
        (
            Self {
                data,
                path: Some(path),
                writable,
                dirty_since: None,
                saved,
            },
            issue,
        )
    }

    pub fn set_writable(&mut self, writable: bool) {
        self.writable = writable && self.path.is_some();
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Call after changing `data`; the write happens ≥ 500 ms later.
    pub fn mark_dirty(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    /// Save if the debounce has elapsed. Returns how long until the next check is needed.
    pub fn tick(&mut self) -> Option<Duration> {
        let since = self.dirty_since?;
        let elapsed = since.elapsed();
        if elapsed >= SAVE_DEBOUNCE {
            self.flush();
            None
        } else {
            Some(SAVE_DEBOUNCE - elapsed)
        }
    }

    /// Write now if anything changed since the last write.
    pub fn flush(&mut self) {
        self.dirty_since = None;
        let (true, Some(path)) = (self.writable, &self.path) else {
            return;
        };
        let json = self.data.to_json();
        if self.saved.as_deref() == Some(json.as_str()) {
            return;
        }
        match write_atomic(path, json.as_bytes()) {
            Ok(()) => self.saved = Some(json),
            Err(e) => eprintln!(
                "cutemarkdown: couldn't save settings to {}: {e}",
                path.display()
            ),
        }
    }
}

/// Write via a temp file in the same folder, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Current time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_rfc3339() -> String {
    rfc3339(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
}

fn rfc3339(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let secs = unix_secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("cutemarkdown-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip() {
        let mut s = Settings {
            theme: ThemePref::Sepia,
            text_size: 18.0,
            width: Width::Wide,
            ..Default::default()
        };
        s.window = Some(WindowGeom {
            x: 120.0,
            y: 80.0,
            w: 1100.0,
            h: 860.0,
            maximized: true,
        });
        s.renderer = Renderer::Vulkan;
        s.add_recent(Path::new("/tmp/a.md"));
        let back = Settings::from_json(&s.to_json()).unwrap();
        assert_eq!(back, s);
        assert!(s.to_json().contains("\"theme\": \"sepia\""));
        assert!(s.to_json().contains("\"renderer\": \"vulkan\""));
    }

    #[test]
    fn spec_example_parses() {
        let json = r#"{ "theme": "auto", "font": "sans", "text_size": 16, "width": "medium", "wrap_code": false,
          "outline_open": true, "outline_width": 264,
          "window": { "x": 120, "y": 80, "w": 1100, "h": 860, "maximized": false },
          "recent": [{ "path": "C:\\docs\\design.md", "opened": "2026-10-02T09:12:00Z", "hash": "abc", "anchor": "x" }],
          "editor": null }"#;
        let s = Settings::from_json(json).unwrap();
        assert_eq!(s.recent.len(), 1);
        assert_eq!(s.recent[0].anchor.as_deref(), Some("x"));
        assert_eq!(s.window.unwrap().w, 1100.0);
    }

    #[test]
    fn bad_values_reset_only_themselves() {
        let s = Settings::from_json(
            r#"{"theme":"purple","text_size":19.4,"width":"wide","outline_width":9000,"recent":[{"nope":1},{"path":"/x.md"}],"extra":true}"#,
        )
        .unwrap();
        assert_eq!(s.theme, ThemePref::Auto);
        assert_eq!(s.text_size, 20.0);
        assert_eq!(s.width, Width::Wide);
        assert_eq!(s.outline_width, 400.0);
        assert_eq!(s.recent.len(), 1);
    }

    #[test]
    fn non_object_is_corrupt() {
        assert!(Settings::from_json("{ broken").is_err());
        assert!(Settings::from_json("[1,2]").is_err());
        assert!(Settings::from_json("").is_err());
    }

    #[test]
    fn corrupt_file_is_backed_up_and_defaults_used() {
        let dir = temp_dir("corrupt");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        let (store, issue) = SettingsStore::load(path.clone());
        assert_eq!(store.data, Settings::default());
        assert!(matches!(issue, Some(LoadIssue::Corrupt { .. })));
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.bad.json")).unwrap(),
            "{ this is not json"
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn store_writes_atomically_and_reloads() {
        let dir = temp_dir("store");
        let path = dir.join("nested").join("settings.json");
        let (mut store, issue) = SettingsStore::load(path.clone());
        assert!(issue.is_none());
        store.data.theme = ThemePref::Dark;
        store.mark_dirty();
        assert!(store.tick().is_some(), "debounced");
        store.flush();
        let (again, _) = SettingsStore::load(path.clone());
        assert_eq!(again.data.theme, ThemePref::Dark);
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temp files left behind");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn read_only_store_never_writes() {
        let dir = temp_dir("ro");
        let path = dir.join("settings.json");
        let (mut store, _) = SettingsStore::load(path.clone());
        store.set_writable(false);
        store.data.wrap_code = true;
        store.flush();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recents_dedupe_and_cap() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.add_recent(Path::new(&format!("/d/{i}.md")));
        }
        s.add_recent(Path::new("/d/5.md"));
        assert_eq!(s.recent.len(), RECENT_STORED);
        assert_eq!(s.recent[0].path, Path::new("/d/5.md"));
        assert_eq!(
            s.recent
                .iter()
                .filter(|r| r.path == Path::new("/d/5.md"))
                .count(),
            1
        );
        s.remove_recent(Path::new("/d/5.md"));
        assert!(s.recent.iter().all(|r| r.path != Path::new("/d/5.md")));
    }

    #[test]
    fn text_size_steps() {
        assert_eq!(step_text_size(16.0, 1), 17.0);
        assert_eq!(step_text_size(18.0, 1), 20.0);
        assert_eq!(step_text_size(12.0, -1), 12.0);
        assert_eq!(step_text_size(28.0, 1), 28.0);
        assert_eq!(snap_text_size(0.0), 12.0);
    }

    #[test]
    fn theme_cycle() {
        assert_eq!(ThemePref::Auto.next(), ThemePref::Light);
        assert_eq!(ThemePref::Dark.next(), ThemePref::Auto);
        assert_eq!(ThemePref::parse("SEPIA"), Some(ThemePref::Sepia));
    }

    #[test]
    fn timestamps() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_790_932_320), "2026-10-02T09:12:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }
}
