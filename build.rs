//! Embeds the Windows resources into `cutemarkdown.exe`:
//! the application icon, version info and the application manifest
//! (per-monitor-v2 DPI, UTF-8 code page, Common Controls v6, long paths).
//!
//! Build scripts run on the *host*, so we look at the *target* (`CARGO_CFG_TARGET_OS`).
//! On non-Windows targets this is a no-op.
//!
//! * `x86_64-pc-windows-msvc`: resources are compiled with the Windows SDK `rc.exe`.
//! * `x86_64-pc-windows-gnu`:  resources are compiled with `windres`
//!   (`x86_64-w64-mingw32-windres` when cross-compiling from Linux).

use std::env;
use std::path::PathBuf;

const ICON: &str = "assets/brand/app.ico";
const MANIFEST: &str = "assets/windows/app.manifest";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed={MANIFEST}");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // Absolute paths: the resource compiler is not run from the package directory.
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = root.join(ICON);
    let manifest = root.join(MANIFEST);

    let mut res = winresource::WindowsResource::new();
    // FileVersion / ProductVersion / the numeric version quad come from Cargo.toml's `version`.
    res.set("ProductName", "cutemarkdown")
        .set("FileDescription", "cutemarkdown \u{2014} Markdown reader")
        .set("InternalName", "cutemarkdown")
        .set("OriginalFilename", "cutemarkdown.exe")
        .set("CompanyName", "cutemarkdown contributors")
        .set(
            "LegalCopyright",
            "\u{a9} cutemarkdown contributors. Released under the MIT License.",
        )
        .set(
            "Comments",
            &env::var("CARGO_PKG_REPOSITORY").unwrap_or_default(),
        )
        .set_icon(icon.to_str().expect("icon path is not valid UTF-8"))
        .set_manifest_file(manifest.to_str().expect("manifest path is not valid UTF-8"));

    if let Err(e) = res.compile() {
        panic!(
            "failed to embed Windows resources: {e}\n\
             (MSVC target: needs the Windows SDK `rc.exe`; GNU target: needs `windres`, \
             e.g. `apt install binutils-mingw-w64-x86-64` when cross-compiling from Linux)"
        );
    }
}
