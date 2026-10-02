#!/usr/bin/env bash
# Package a built cutemarkdown.exe into the two release artifacts:
#   <out>/cutemarkdown-<ver>-setup-x64.exe      NSIS installer (per-user, no admin)
#   <out>/cutemarkdown-<ver>-portable-x64.zip   cutemarkdown.exe + README.txt
#
# This is the single place that knows how to invoke makensis. It is used by
# scripts/build-windows.sh (Linux cross build) and by the GitHub workflows (Windows, Git Bash).
#
#   scripts/package.sh --exe path/to/cutemarkdown.exe [--version 1.2.3] [--out dist]
#
# Needs: makensis (NSIS 3) and Python 3. On Windows: `choco install nsis -y`.
set -euo pipefail
# shellcheck source=scripts/_common.sh
. "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

EXE="" VERSION="" OUT="$ROOT/dist"
while [ $# -gt 0 ]; do
  case "$1" in
    --exe)     EXE=$2; shift 2 ;;
    --version) VERSION=$2; shift 2 ;;
    --out)     OUT=$2; shift 2 ;;
    -h|--help) sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$EXE" ] || { echo "error: --exe is required" >&2; exit 2; }
[ -f "$EXE" ] || { echo "error: $EXE does not exist (build it first)" >&2; exit 1; }
[ -n "$PY" ] || { echo "error: python3 not found" >&2; exit 1; }
[ -n "$VERSION" ] || VERSION=$("$ROOT/scripts/version.sh")

# makensis: PATH first, then the default NSIS install locations on Windows.
MAKENSIS=$(command -v makensis || true)
if [ -z "$MAKENSIS" ]; then
  for c in "/c/Program Files (x86)/NSIS/makensis.exe" "/c/Program Files/NSIS/makensis.exe"; do
    if [ -x "$c" ]; then MAKENSIS=$c; break; fi
  done
fi
[ -n "$MAKENSIS" ] || { echo "error: makensis not found (Linux: apt install nsis, Windows: choco install nsis -y)" >&2; exit 1; }

mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
SETUP="$OUT/cutemarkdown-$VERSION-setup-x64.exe"
ZIP="$OUT/cutemarkdown-$VERSION-portable-x64.zip"
rm -f "$SETUP" "$ZIP"

echo "==> Installer ($("$MAKENSIS" -VERSION 2>/dev/null || true))"
# Absolute paths: makensis resolves relative ones against the .nsi's directory.
# MSYS would otherwise rewrite anything that looks like a POSIX path in the arguments.
MSYS2_ARG_CONV_EXCL='*' MSYS_NO_PATHCONV=1 "$MAKENSIS" -V2 \
  "-DVERSION=$VERSION" \
  "-DEXE_PATH=$(native "$EXE")" \
  "-DOUTFILE=$(native "$SETUP")" \
  "$(native "$ROOT/installer/cutemarkdown.nsi")"

echo "==> Portable zip"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp "$EXE" "$STAGE/cutemarkdown.exe"
sed "s/@VERSION@/$VERSION/g; s/\$/\r/" "$ROOT/installer/portable-README.txt" > "$STAGE/README.txt"
"$PY" - "$(native "$ZIP")" "$(native "$STAGE/cutemarkdown.exe")" "$(native "$STAGE/README.txt")" <<'PYEOF'
import os, sys, zipfile
out, *files = sys.argv[1:]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for f in files:
        z.write(f, os.path.basename(f))
PYEOF

echo "==> Done"
report "$SETUP" "$ZIP"
