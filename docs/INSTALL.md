# Installing cutemarkdown

cutemarkdown is a single native `.exe` for **Windows 10 and 11 (64-bit)**. It needs no Visual C++
Redistributable, no Edge/WebView and no other runtime. Get it from the
[Releases page](https://github.com/harsh9524/cutemarkdown/releases):

| File | What it is |
|---|---|
| `cutemarkdown-<version>-setup-x64.exe` | Installer (recommended) |
| `cutemarkdown-<version>-portable-x64.zip` | Just the `.exe` and a README, nothing to install |

## Install

1. Run `cutemarkdown-<version>-setup-x64.exe`.
2. Click **Install**, then **Finish**. That is all: no administrator rights, no UAC prompt.

It installs for **your account only**, into `%LOCALAPPDATA%\Programs\cutemarkdown`, adds a Start Menu
shortcut, and lists cutemarkdown under *Settings > Apps* and *Default apps*.

> The installer is not code-signed yet, so Windows SmartScreen may say *"Windows protected your PC"*.
> Click **More info > Run anyway**.

**Silent install** (scripts, deployment tools):

```bat
cutemarkdown-1.0.0-setup-x64.exe /S
cutemarkdown-1.0.0-setup-x64.exe /S /D=C:\Tools\cutemarkdown   :: custom folder; /D must be last
```

**Update:** run the newer installer over the old one. It upgrades in place and does not close anything:
if cutemarkdown is open, the new version starts the next time you launch it.

## Make it your default Markdown app

Windows does not let any program quietly take over a file type, so this is one extra click for you:

* At the end of the installer, tick **Make cutemarkdown my default Markdown app**. It opens Windows
  Settings on cutemarkdown's entry (Windows 11) or the Default apps page (Windows 10).
* Or, any time: right-click a `.md` file > **Open with** > **Choose another app** > **cutemarkdown** >
  **Always**.
* Or: *Settings > Apps > Default apps*, search for `cutemarkdown` and set the Markdown extensions
  (`.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`, `.mdwn`, `.mdtext`).

If nothing else on your PC handles a Markdown extension, the installer makes cutemarkdown the handler
for it. It never replaces an app you already chose.

## Uninstall

*Settings > Apps > Installed apps > cutemarkdown > Uninstall*, or run
`%LOCALAPPDATA%\Programs\cutemarkdown\uninstall.exe` (add `/S` for silent).

It removes the files, the Start Menu shortcut and everything it registered. If cutemarkdown is
running it asks you to close it first. It also offers to delete the settings the app itself keeps in
`%APPDATA%\cutemarkdown` (kept by default, and always kept on a silent uninstall).

## Portable

1. Unzip `cutemarkdown-<version>-portable-x64.zip` anywhere (a USB stick is fine).
2. Double-click `cutemarkdown.exe`, or drag a `.md` file onto it.

It does not write to the registry and needs no installation. Delete the folder to remove it.
To open `.md` files with it by double-click, use *Open with > Choose another app > Always* once.

## Build from source

You need [Rust](https://rustup.rs) (stable, edition 2024).

### On Windows

```powershell
cargo build --release
```

The result is `target\release\cutemarkdown.exe`. The MSVC target links the C runtime statically
(`.cargo/config.toml`), and `build.rs` embeds the icon, version info and manifest. Requirements:
Visual Studio Build Tools (C++ workload, includes the Windows SDK).

To also build the installer and the zip, install [NSIS](https://nsis.sourceforge.io)
(`choco install nsis -y`) and Python 3, then from Git Bash:

```bash
bash scripts/package.sh --exe target/release/cutemarkdown.exe
```

Both files end up in `dist/`.

### On Linux (cross-compile)

```bash
sudo apt install gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 nsis python3
rustup target add x86_64-pc-windows-gnu
scripts/build-windows.sh
```

This builds `x86_64-pc-windows-gnu` in release mode (fat LTO, so the first build takes a few
minutes), checks that the `.exe` only imports Windows system DLLs, and writes the installer and the
portable zip to `dist/`.

### Notes

* The check `python scripts/check-imports.py <exe>` lists the DLLs an `.exe` imports and fails on
  anything that is not part of Windows (VC++ or MinGW runtimes).
* Installer artwork is generated from `assets/brand/logo.svg` by
  `python installer/make_images.py` (needs `pip install cairosvg pillow`). The generated `.bmp` files
  are committed, so a normal build does not need Python imaging libraries.
* Releases: bump `version` in `Cargo.toml`, then push a tag `vX.Y.Z` that matches it. GitHub Actions
  builds on Windows and attaches the installer, the zip and `SHA256SUMS.txt` to the release.
