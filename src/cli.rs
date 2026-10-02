//! Command line: `cutemarkdown [FILE ...]` plus QA flags for headless screenshots.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::settings::ThemePref;

pub const USAGE: &str = "\
Usage: cutemarkdown [OPTIONS] [FILE ...]

Opens the first FILE in this window and each other FILE in a new window.

QA options:
  --screenshot OUT.png   Save a screenshot after --frames frames, then exit
  --size WxH             Window size in points (default 1100x860)
  --ppp F                Pixels per point (e.g. 1.5 for 150% scaling)
  --theme T              auto | light | sepia | dark (not saved)
  --scroll PX            Scroll the document to PX before the shot
  --find QUERY           Open the find bar with QUERY
  --open WHAT            aa | menu | recent | outline | shortcuts | drag (open before the shot)
  --empty                Show the empty state (ignore FILE)
  --demo-recents         Show fake recent files (not saved)
  --toast TEXT           Show a toast
  --hover-link URL       Show the link status pill for URL
  --zen                  Start in Zen mode
  --anchor SLUG          Scroll to a heading after opening FILE
  --frames N             Frames to render before the screenshot (default 10)
  --settings PATH        Use this settings file instead of the default
  -h, --help             Show this help";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    Aa,
    Menu,
    Recent,
    Outline,
    Shortcuts,
    Drag,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub files: Vec<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub size: Option<[f32; 2]>,
    pub ppp: Option<f32>,
    pub theme: Option<ThemePref>,
    pub scroll: Option<f32>,
    pub find: Option<String>,
    pub open: Option<Open>,
    pub empty: bool,
    pub demo_recents: bool,
    pub toast: Option<String>,
    pub hover_link: Option<String>,
    pub zen: bool,
    pub anchor: Option<String>,
    pub frames: Option<u32>,
    pub settings: Option<PathBuf>,
    pub help: bool,
}

impl Args {
    /// QA runs never write settings.
    pub fn is_qa(&self) -> bool {
        self.screenshot.is_some() || self.demo_recents
    }
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.into_iter();
    let mut only_files = false;
    while let Some(raw) = it.next() {
        let arg = raw.to_string_lossy().into_owned();
        if only_files || !arg.starts_with('-') || arg == "-" {
            a.files.push(PathBuf::from(raw));
            continue;
        }
        let mut val = |name: &str| -> Result<String, String> {
            it.next()
                .map(|v| v.to_string_lossy().into_owned())
                .ok_or_else(|| format!("missing value for {name}"))
        };
        let num = |name: &str, v: String| {
            v.parse::<f32>()
                .map_err(|_| format!("bad number for {name}: {v}"))
        };
        match arg.as_str() {
            "--" => only_files = true,
            "-h" | "--help" => a.help = true,
            "--screenshot" => a.screenshot = Some(PathBuf::from(val(&arg)?)),
            "--size" => {
                let v = val(&arg)?;
                let (w, h) = v
                    .split_once(['x', 'X'])
                    .ok_or_else(|| format!("--size wants WxH, got {v}"))?;
                a.size = Some([num("--size", w.into())?, num("--size", h.into())?]);
            }
            "--ppp" => a.ppp = Some(num(&arg, val(&arg)?)?.clamp(0.5, 4.0)),
            "--theme" => {
                let v = val(&arg)?;
                a.theme = Some(ThemePref::parse(&v).ok_or_else(|| format!("unknown theme {v}"))?);
            }
            "--scroll" => a.scroll = Some(num(&arg, val(&arg)?)?),
            "--find" => a.find = Some(val(&arg)?),
            "--open" => {
                a.open = Some(match val(&arg)?.as_str() {
                    "aa" => Open::Aa,
                    "menu" => Open::Menu,
                    "recent" => Open::Recent,
                    "outline" => Open::Outline,
                    "shortcuts" => Open::Shortcuts,
                    "drag" => Open::Drag,
                    other => return Err(format!("unknown --open target {other}")),
                })
            }
            "--empty" => a.empty = true,
            "--demo-recents" => a.demo_recents = true,
            "--toast" => a.toast = Some(val(&arg)?),
            "--hover-link" => a.hover_link = Some(val(&arg)?),
            "--zen" => a.zen = true,
            "--anchor" => a.anchor = Some(val(&arg)?),
            "--frames" => {
                a.frames = Some(val(&arg)?.parse().map_err(|_| "bad --frames".to_owned())?)
            }
            "--settings" => a.settings = Some(PathBuf::from(val(&arg)?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Args, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn files_and_flags() {
        let a = p(&[
            "a.md", "--theme", "dark", "--size", "1280x800", "--ppp", "1.5", "b.md", "--open", "aa",
        ])
        .unwrap();
        assert_eq!(a.files, vec![PathBuf::from("a.md"), PathBuf::from("b.md")]);
        assert_eq!(a.theme, Some(ThemePref::Dark));
        assert_eq!(a.size, Some([1280.0, 800.0]));
        assert_eq!(a.ppp, Some(1.5));
        assert_eq!(a.open, Some(Open::Aa));
        assert!(!a.is_qa());
    }

    #[test]
    fn double_dash_ends_options() {
        let a = p(&["--", "--weird-name.md"]).unwrap();
        assert_eq!(a.files, vec![PathBuf::from("--weird-name.md")]);
    }

    #[test]
    fn errors() {
        assert!(p(&["--size", "big"]).is_err());
        assert!(p(&["--theme", "neon"]).is_err());
        assert!(p(&["--frames"]).is_err());
        assert!(p(&["--nope"]).is_err());
    }

    #[test]
    fn qa_mode() {
        assert!(p(&["--screenshot", "x.png"]).unwrap().is_qa());
        assert!(p(&["--demo-recents"]).unwrap().is_qa());
    }
}
