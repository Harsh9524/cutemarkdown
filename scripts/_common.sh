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

# Absolute path in the form native tools expect: C:/x/y under Git Bash on Windows, plain POSIX elsewhere.
native() {
  local p
  p=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$p"; else printf '%s' "$p"; fi
}

# Print "<size> bytes  <path>" for each file.
report() {
  local f
  for f in "$@"; do printf '  %10d bytes  %s\n' "$(wc -c < "$f")" "$f"; done
}
