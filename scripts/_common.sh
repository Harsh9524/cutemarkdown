# shellcheck shell=bash disable=SC2034
# Shared helpers, sourced by the other scripts (not meant to be run directly).

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# A Python 3 that really runs. (On Windows, `python3` can be a Microsoft Store stub that exists but fails.)
PY=""
for _c in python3 python; do
  if command -v "$_c" >/dev/null 2>&1 && "$_c" -c 'import sys; sys.exit(sys.version_info < (3, 6))' >/dev/null 2>&1; then
    PY=$_c
    break
  fi
done
unset _c

# Windows programs (python.exe, makensis.exe, ...) end their lines with "\r\n". Git Bash's $(...)
# only strips the "\n", so a captured value keeps a trailing "\r". Never use such a value as is.
strip_cr() { printf '%s' "${1//$'\r'/}"; }

# A release version: 1.2.3, optionally with a pre-release suffix (1.2.3-rc.1). The installer script
# derives its numeric version resource from it, so anything else (including a stray "\r") must stop
# the build here, with a readable message, instead of failing inside makensis.
check_version() {
  if [[ ! $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
    printf 'error: not a valid version: [%s]\n' "$(printf '%s' "$1" | cat -v)" >&2
    return 1
  fi
}

# Absolute path in the form native tools expect: C:\x\y (backslashes) under Git Bash on Windows,
# plain POSIX elsewhere. Backslashes, not C:/x/y: makensis.exe splits the File command's path on
# "\" only, so 'File "C:/x/y.exe"' reports "no files found" even though the file exists.
native() {
  local p
  p=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
  if command -v cygpath >/dev/null 2>&1; then p=$(cygpath -w "$p"); fi
  strip_cr "$p"
}

# Print "<size> bytes  <path>" for each file.
report() {
  local f
  for f in "$@"; do printf '  %10d bytes  %s\n' "$(wc -c < "$f")" "$f"; done
}
