#!/usr/bin/env bash
# Cross-compile cutemarkdown for Windows x64 from Linux and package it.
#
#   scripts/build-windows.sh
#
# Output (in dist/):
#   cutemarkdown-<ver>-setup-x64.exe     installer (per-user, no admin)
#   cutemarkdown-<ver>-portable-x64.zip  exe + README.txt
#
# Prerequisites: rustup target add x86_64-pc-windows-gnu; mingw-w64 (gcc + windres); nsis; python3.
#   Debian/Ubuntu: sudo apt install gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 nsis python3
# Honors CARGO_TARGET_DIR (default: ./target). The release profile uses fat LTO, so the first
# build takes a few minutes.
set -euo pipefail
# shellcheck source=scripts/_common.sh
. "$(dirname "${BASH_SOURCE[0]}")/_common.sh"
cd "$ROOT"

TARGET=x86_64-pc-windows-gnu
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}

missing=0
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1 ($2)" >&2; missing=1; }; }
need cargo "install Rust from https://rustup.rs"
need x86_64-w64-mingw32-gcc "apt install gcc-mingw-w64-x86-64"
need x86_64-w64-mingw32-windres "apt install binutils-mingw-w64-x86-64"
need x86_64-w64-mingw32-objdump "apt install binutils-mingw-w64-x86-64"
need makensis "apt install nsis"
[ -n "$PY" ] || { echo "missing tool: python3" >&2; missing=1; }
if command -v rustup >/dev/null 2>&1 && ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "missing Rust target: run 'rustup target add $TARGET'" >&2; missing=1
fi
[ "$missing" = 0 ] || exit 1

VERSION=$(strip_cr "$(scripts/version.sh)")
echo "==> cutemarkdown $VERSION: cargo build --release --target $TARGET"
cargo build --release --locked --target "$TARGET" --bin cutemarkdown

EXE="$CARGO_TARGET_DIR/$TARGET/release/cutemarkdown.exe"

echo "==> DLL imports"
x86_64-w64-mingw32-objdump -p "$EXE" | grep "DLL Name" | sort -fu
"$PY" scripts/check-imports.py "$EXE" >/dev/null   # fails the build on non-system DLLs
echo "  (all Windows system DLLs)"

scripts/package.sh --exe "$EXE" --version "$VERSION" --out dist

echo
echo "==> Summary"
report "$EXE"
echo "  (installer and zip listed above, in dist/)"
