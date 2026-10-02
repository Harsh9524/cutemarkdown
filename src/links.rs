//! Link safety rules (SPEC §7 "Link clicks").
//!
//! [`classify`] is pure apart from the file-system probe, so the rules are unit-tested. The app
//! turns the resulting [`LinkAction`] into navigation, a system launch or a toast. The rule of
//! thumb: Markdown opens in-app, a short allowlist of document types opens with the system
//! handler, and everything else is only ever *revealed*, never launched.

use std::path::{Path, PathBuf};

use engine::LinkTarget;

/// What to do with a clicked link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// Scroll to an anchor in the current document (pushes history).
    Anchor(String),
    /// Open a Markdown file in-app (or in a new window with Ctrl/middle-click).
    OpenDoc {
        path: PathBuf,
        anchor: Option<String>,
    },
    /// Open an allowlisted local file with its system handler.
    OpenWithSystem(PathBuf),
    /// Show a folder (one without a README/index) in Explorer.
    RevealFolder(PathBuf),
    /// A file we refuse to launch: reveal it in Explorer instead ("Revealed in Explorer").
    RevealBlocked(PathBuf),
    /// `http`, `https` or `mailto`: hand to the default browser or mail app.
    OpenUrl(String),
    /// Any other scheme. Holds the lower-cased scheme for the "Blocked link (scheme:)" toast.
    Blocked(String),
    /// The local target doesn't exist ("File not found: path").
    NotFound(PathBuf),
}

/// Extensions opened in-app.
pub const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd", "mdx"];
/// Extensions accepted by drag & drop and the open dialog.
pub const OPENABLE_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd", "mdx", "txt"];
/// Local files that may be opened with the system handler. Everything else is revealed.
const SYSTEM_ALLOWLIST: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tif", "tiff", "avif", "pdf", "txt",
    "log", "csv", "json", "yaml", "yml", "toml", "xml", "html", "htm",
];
const SAFE_SCHEMES: &[&str] = &["http", "https", "mailto"];
const FOLDER_INDEXES: &[&str] = &["README.md", "readme.md", "Readme.md", "index.md"];

/// What exists at a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Missing,
    File,
    Dir,
}

/// The real file system.
pub fn probe(path: &Path) -> PathKind {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => PathKind::Dir,
        Ok(_) => PathKind::File,
        Err(_) => PathKind::Missing,
    }
}

/// Lower-cased extension of the file name, if any. A name containing `:` (an NTFS alternate data
/// stream such as `run.exe:x.pdf`) has no usable extension.
pub fn extension(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if name.contains(':') {
        return None;
    }
    let ext = Path::new(name).extension()?.to_str()?;
    Some(ext.to_ascii_lowercase())
}

pub fn is_markdown(path: &Path) -> bool {
    extension(path).is_some_and(|e| MARKDOWN_EXTS.contains(&e.as_str()))
}

/// Accepted by drag & drop and the open dialog.
pub fn is_openable(path: &Path) -> bool {
    extension(path).is_some_and(|e| OPENABLE_EXTS.contains(&e.as_str()))
}

fn system_allowed(path: &Path) -> bool {
    extension(path).is_some_and(|e| SYSTEM_ALLOWLIST.contains(&e.as_str()))
}

/// The scheme of a URL (`"https"` for `https://…`), lower-cased, if it has a syntactically valid one.
pub fn scheme(url: &str) -> Option<String> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| scheme.to_ascii_lowercase())
}

/// Decide what a link click does. `probe` reports what exists on disk.
pub fn classify(target: &LinkTarget, probe: impl Fn(&Path) -> PathKind) -> LinkAction {
    match target {
        LinkTarget::Anchor(a) => LinkAction::Anchor(a.clone()),
        LinkTarget::External(url) => {
            let url = url.trim();
            match scheme(url) {
                Some(s)
                    if SAFE_SCHEMES.contains(&s.as_str()) && !url.chars().any(char::is_control) =>
                {
                    LinkAction::OpenUrl(url.to_owned())
                }
                Some(s) => LinkAction::Blocked(s),
                None => LinkAction::Blocked(String::new()),
            }
        }
        LinkTarget::File { path, anchor } => classify_file(path, anchor.clone(), &probe),
    }
}

/// [`classify`] for a link in a document from `doc_dir`: a file on another machine (a UNC path
/// other than the document's own share) is blocked before anything touches the file system, so
/// a click can't make Windows authenticate to a host the document names (SPEC §1.7).
pub fn classify_from(
    target: &LinkTarget,
    doc_dir: Option<&Path>,
    probe: impl Fn(&Path) -> PathKind,
) -> LinkAction {
    match target {
        LinkTarget::File { path, .. } if engine::is_remote_path(path, doc_dir) => {
            LinkAction::Blocked("file".into())
        }
        _ => classify(target, probe),
    }
}

fn classify_file(
    path: &Path,
    anchor: Option<String>,
    probe: &impl Fn(&Path) -> PathKind,
) -> LinkAction {
    // Relative paths only reach us from pasted text, which has no folder to resolve against.
    if path.is_relative() {
        return LinkAction::NotFound(path.to_path_buf());
    }
    match probe(path) {
        PathKind::Missing => LinkAction::NotFound(path.to_path_buf()),
        PathKind::Dir => FOLDER_INDEXES
            .iter()
            .map(|name| path.join(name))
            .find(|p| probe(p) == PathKind::File)
            .map(|p| LinkAction::OpenDoc { path: p, anchor })
            .unwrap_or_else(|| LinkAction::RevealFolder(path.to_path_buf())),
        PathKind::File if is_markdown(path) => LinkAction::OpenDoc {
            path: path.to_path_buf(),
            anchor,
        },
        PathKind::File if system_allowed(path) => LinkAction::OpenWithSystem(path.to_path_buf()),
        PathKind::File => LinkAction::RevealBlocked(path.to_path_buf()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\docs")
        } else {
            PathBuf::from("/docs")
        }
    }

    fn fs() -> impl Fn(&Path) -> PathKind {
        let r = root();
        let mut m = HashMap::new();
        m.insert(r.clone(), PathKind::Dir);
        for f in [
            "guide.md",
            "NOTES.MARKDOWN",
            "report.pdf",
            "data.CSV",
            "pic.png",
            "setup.exe",
            "run.bat",
            "x.ps1",
            "page.html",
            "shortcut.lnk",
            "noext",
            "trailing.exe.",
            "tool.js",
        ] {
            m.insert(r.join(f), PathKind::File);
        }
        m.insert(r.join("withreadme"), PathKind::Dir);
        m.insert(r.join("withreadme").join("README.md"), PathKind::File);
        m.insert(r.join("withindex"), PathKind::Dir);
        m.insert(r.join("withindex").join("index.md"), PathKind::File);
        m.insert(r.join("bare"), PathKind::Dir);
        m.insert(r.join("trap.md"), PathKind::Dir);
        move |p: &Path| m.get(p).copied().unwrap_or(PathKind::Missing)
    }

    fn file(name: &str) -> LinkTarget {
        LinkTarget::File {
            path: root().join(name),
            anchor: None,
        }
    }

    #[test]
    fn anchors_pass_through() {
        assert_eq!(
            classify(&LinkTarget::Anchor("rollback-plan".into()), fs()),
            LinkAction::Anchor("rollback-plan".into())
        );
    }

    #[test]
    fn markdown_opens_in_app_with_anchor() {
        let t = LinkTarget::File {
            path: root().join("guide.md"),
            anchor: Some("data-model".into()),
        };
        assert_eq!(
            classify(&t, fs()),
            LinkAction::OpenDoc {
                path: root().join("guide.md"),
                anchor: Some("data-model".into())
            }
        );
        assert!(matches!(
            classify(&file("NOTES.MARKDOWN"), fs()),
            LinkAction::OpenDoc { .. }
        ));
    }

    #[test]
    fn allowlisted_files_open_with_system() {
        for f in ["report.pdf", "data.CSV", "pic.png", "page.html"] {
            assert_eq!(
                classify(&file(f), fs()),
                LinkAction::OpenWithSystem(root().join(f)),
                "{f}"
            );
        }
    }

    #[test]
    fn executables_are_never_launched() {
        for f in [
            "setup.exe",
            "run.bat",
            "x.ps1",
            "shortcut.lnk",
            "noext",
            "trailing.exe.",
            "tool.js",
        ] {
            assert_eq!(
                classify(&file(f), fs()),
                LinkAction::RevealBlocked(root().join(f)),
                "{f}"
            );
        }
    }

    #[test]
    fn alternate_data_streams_have_no_extension() {
        assert_eq!(extension(Path::new("run.exe:x.pdf")), None);
        assert_eq!(extension(Path::new("Report.PDF")).as_deref(), Some("pdf"));
    }

    #[test]
    fn folders_open_their_index_or_are_revealed() {
        assert_eq!(
            classify(&file("withreadme"), fs()),
            LinkAction::OpenDoc {
                path: root().join("withreadme").join("README.md"),
                anchor: None
            }
        );
        assert_eq!(
            classify(&file("withindex"), fs()),
            LinkAction::OpenDoc {
                path: root().join("withindex").join("index.md"),
                anchor: None
            }
        );
        assert_eq!(
            classify(&file("bare"), fs()),
            LinkAction::RevealFolder(root().join("bare"))
        );
        // A folder named like a Markdown file is still a folder.
        assert_eq!(
            classify(&file("trap.md"), fs()),
            LinkAction::RevealFolder(root().join("trap.md"))
        );
    }

    #[test]
    fn missing_and_relative_targets_are_not_found() {
        assert_eq!(
            classify(&file("gone.md"), fs()),
            LinkAction::NotFound(root().join("gone.md"))
        );
        let rel = LinkTarget::File {
            path: PathBuf::from("./x.md"),
            anchor: None,
        };
        assert_eq!(
            classify(&rel, fs()),
            LinkAction::NotFound(PathBuf::from("./x.md"))
        );
    }

    #[test]
    fn only_web_and_mail_schemes_open() {
        let ext = |u: &str| classify(&LinkTarget::External(u.into()), fs());
        assert_eq!(
            ext("https://example.com/a?b=c"),
            LinkAction::OpenUrl("https://example.com/a?b=c".into())
        );
        assert_eq!(
            ext("HTTP://EXAMPLE.COM"),
            LinkAction::OpenUrl("HTTP://EXAMPLE.COM".into())
        );
        assert_eq!(
            ext("mailto:a@b.c"),
            LinkAction::OpenUrl("mailto:a@b.c".into())
        );
        assert_eq!(
            ext("javascript:alert(1)"),
            LinkAction::Blocked("javascript".into())
        );
        assert_eq!(
            ext("file:///C:/Windows/System32/calc.exe"),
            LinkAction::Blocked("file".into())
        );
        assert_eq!(
            ext("ms-settings:privacy"),
            LinkAction::Blocked("ms-settings".into())
        );
        assert_eq!(
            ext("search-ms:query=x"),
            LinkAction::Blocked("search-ms".into())
        );
        assert_eq!(ext("no scheme here"), LinkAction::Blocked(String::new()));
        assert_eq!(
            ext("https://evil\u{0}.com"),
            LinkAction::Blocked("https".into())
        );
    }

    #[test]
    fn remote_files_are_blocked_without_probing() {
        let never = |p: &Path| -> PathKind { panic!("probed {}", p.display()) };
        let unc = LinkTarget::File {
            path: PathBuf::from(r"\\evil\share\a.md"),
            anchor: None,
        };
        if cfg!(windows) {
            assert_eq!(
                classify_from(&unc, Some(&root()), never),
                LinkAction::Blocked("file".into())
            );
        }
        // Local files are classified as usual.
        assert_eq!(
            classify_from(&file("guide.md"), Some(&root()), fs()),
            classify(&file("guide.md"), fs())
        );
    }

    #[test]
    fn scheme_parsing() {
        assert_eq!(scheme("https://x").as_deref(), Some("https"));
        assert_eq!(scheme("C:/x").as_deref(), Some("c"));
        assert_eq!(scheme("1http://x"), None);
        assert_eq!(scheme("plain"), None);
    }

    #[test]
    fn openable_types() {
        for f in ["a.md", "b.MARKDOWN", "c.mdown", "d.mkd", "e.mdx", "f.txt"] {
            assert!(is_openable(Path::new(f)), "{f}");
        }
        assert!(!is_openable(Path::new("g.pdf")));
        assert!(!is_markdown(Path::new("f.txt")));
    }
}
