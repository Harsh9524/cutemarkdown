//! Local files named by a document: the `file://` URIs egui_extras' file loader understands, and
//! the "no network file access" rule (SPEC §1.7 "Safe by default").
//!
//! A document may show images from its own disk, but opening it must never make Windows
//! authenticate to another machine: a UNC path (`\\host\share\x.png`, `//host/share/x.png`), a
//! `file://host/…` URI or a relative path that egui would read as a host name all make the OS
//! open an SMB (or WebDAV) session and send the user's NTLM credentials. Those resolve to
//! nothing (a broken-image chip, a blocked link). The one exception is a path on the same
//! share as the document itself, which the reader has already opened from there.

use std::path::{Path, PathBuf};

/// The `file://` URI for an absolute local path, in the form egui_extras' `FileLoader` maps back
/// to exactly that path: `file:///C:\docs\img\p.png` on Windows (the loader strips one `/`; a
/// URI without it would become the UNC path `\\C:\…`), `file:///docs/img/p.png` elsewhere.
pub(crate) fn local_file_uri(path: &Path) -> String {
    #[cfg(windows)]
    {
        // Rebuild from components so every separator is `\` (`C:\docs` + `img/p.png`).
        let path: PathBuf = path.components().collect();
        format!("file:///{}", path.display())
    }
    #[cfg(not(windows))]
    {
        format!("file://{}", path.display())
    }
}

/// The local path behind a `file://` URI built by the engine (image URIs), mirroring egui_extras'
/// `FileLoader`. `None` for anything else, including the `file://host/…` form, which the engine
/// never builds.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    if cfg!(windows) {
        rest.strip_prefix('/').map(PathBuf::from)
    } else {
        (!rest.is_empty()).then(|| PathBuf::from(rest))
    }
}

/// Where reading a path goes.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum Location {
    /// A local disk (or a relative path).
    Local,
    /// `\\server\share\…` (lower-cased server and share).
    Share(String, String),
    /// Device and other verbatim namespaces (`\\.\pipe\x`, `\\?\GLOBALROOT\…`): never.
    Device,
}

#[cfg(windows)]
fn location(path: &Path) -> Location {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return Location::Local;
    };
    match prefix.kind() {
        Prefix::Disk(_) | Prefix::VerbatimDisk(_) => Location::Local,
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Location::Share(
            server.to_string_lossy().to_lowercase(),
            share.to_string_lossy().to_lowercase(),
        ),
        Prefix::Verbatim(_) | Prefix::DeviceNS(_) => Location::Device,
    }
}

#[cfg(not(windows))]
fn location(_path: &Path) -> Location {
    // No path makes the OS contact another machine by itself (`//host/x` is `/host/x`).
    Location::Local
}

/// Would reading `path` make the OS contact another machine (a UNC share or device path), other
/// than the share `base` (the document's folder) is on? Always `false` off Windows.
pub fn is_remote_path(path: &Path, base: Option<&Path>) -> bool {
    match location(path) {
        Location::Local => false,
        Location::Device => true,
        share => base.is_none_or(|b| location(b) != share),
    }
}

/// Is `path` on the same UNC share as `base`? (Never off Windows.)
pub(crate) fn on_share_of(path: &Path, base: Option<&Path>) -> bool {
    matches!(location(path), share @ Location::Share(..) if base.is_some_and(|b| location(b) == share))
}

/// Does a document-supplied path start like a UNC path (`\\host`, `//host`, `/\host`)? Rejected
/// on every OS so a document behaves the same everywhere (on Windows the same-share exception
/// is decided by [`is_remote_path`] on the resolved path instead).
pub(crate) fn looks_like_unc(src: &str) -> bool {
    let b = src.as_bytes();
    b.len() >= 2 && matches!(b[0], b'/' | b'\\') && matches!(b[1], b'/' | b'\\')
}

/// The path of a document-supplied `file:` URI: only `file:///path` and `file://localhost/path`
/// name the local machine; any other host is refused. Percent-decoded; `file:///C:/x` gives
/// `C:/x` on Windows.
pub(crate) fn file_uri_local_path(uri: &str) -> Option<String> {
    let rest = uri.get(..7).filter(|s| s.eq_ignore_ascii_case("file://"))?;
    let rest = &uri[rest.len()..];
    let rest = if rest.starts_with('/') {
        rest
    } else {
        let (host, path) = rest.split_at(rest.find('/')?);
        if !host.eq_ignore_ascii_case("localhost") {
            return None;
        }
        path
    };
    let path = crate::parse::percent_decode(rest);
    if cfg!(windows) {
        // `/C:/x` → `C:/x`; `//host/share` (four slashes in the URI) stays UNC and is refused
        // by the caller.
        let b = path.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
            return Some(path[1..].to_owned());
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_file_uris() {
        assert_eq!(
            file_uri_local_path("file:///docs/a%20b.png").as_deref(),
            Some("/docs/a b.png")
        );
        assert_eq!(
            file_uri_local_path("FILE://localhost/docs/a.png").as_deref(),
            Some("/docs/a.png")
        );
        assert_eq!(file_uri_local_path("file://evil/s/x.png"), None);
        assert_eq!(file_uri_local_path("file://evil"), None);
        assert_eq!(file_uri_local_path("https://e.com/x.png"), None);
        assert!(looks_like_unc("//evil/s/x.png"));
        assert!(looks_like_unc(r"\\evil\s\x.png"));
        assert!(looks_like_unc(r"/\evil\s\x.png"));
        assert!(!looks_like_unc("/docs/x.png"));
        assert!(!looks_like_unc("img/x.png"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_uris_round_trip() {
        let p = Path::new("/docs/my img/p.png");
        let uri = local_file_uri(p);
        assert_eq!(uri, "file:///docs/my img/p.png");
        assert_eq!(file_uri_to_path(&uri).as_deref(), Some(p));
        assert!(!is_remote_path(Path::new("//evil/s/x.png"), None));
    }

    #[cfg(windows)]
    #[test]
    fn windows_uris_round_trip() {
        let p = Path::new(r"C:\docs").join("img/p.png");
        let uri = local_file_uri(&p);
        assert_eq!(uri, r"file:///C:\docs\img\p.png");
        assert_eq!(
            file_uri_to_path(&uri).as_deref(),
            Some(Path::new(r"C:\docs\img\p.png"))
        );
        // A document on a share: its images load from the same share.
        let unc = Path::new(r"\\nas\docs\img\p.png");
        let uri = local_file_uri(unc);
        assert_eq!(uri, r"file:///\\nas\docs\img\p.png");
        assert_eq!(file_uri_to_path(&uri).as_deref(), Some(unc));
        assert_eq!(file_uri_to_path(r"file://C:\x.png"), None);
        assert_eq!(
            file_uri_local_path("file:///C:/x%20y.png").as_deref(),
            Some("C:/x y.png")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_remote_paths() {
        let base = Path::new(r"\\NAS\Docs\guide");
        for p in [
            r"\\evil\s\x.png",
            "//evil/s/x.png",
            r"\\nas\other\x.png",
            r"\\.\pipe\x",
        ] {
            assert!(is_remote_path(Path::new(p), None), "{p}");
            assert!(is_remote_path(Path::new(p), Some(base)), "{p}");
        }
        assert!(!is_remote_path(
            Path::new(r"\\nas\docs\img\x.png"),
            Some(base)
        ));
        assert!(!is_remote_path(Path::new("//nas/docs/x.png"), Some(base)));
        assert!(!is_remote_path(Path::new(r"C:\docs\x.png"), None));
        assert!(!is_remote_path(Path::new(r"\\?\C:\docs\x.png"), None));
        assert!(!is_remote_path(Path::new("img/x.png"), None));
    }
}
