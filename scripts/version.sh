#!/usr/bin/env bash
# Print the cutemarkdown version from Cargo.toml (single source of truth for the installer,
# the zip name and the release tag check). Output is the bare version and a "\n", never "\r".
set -euo pipefail
# shellcheck source=scripts/_common.sh
. "$(dirname "${BASH_SOURCE[0]}")/_common.sh"
[ -n "$PY" ] || { echo "python3 not found" >&2; exit 1; }
cd "$ROOT"

# On Windows, Python's print() ends the line with "\r\n" (text-mode stdout) and Git Bash's $(...)
# keeps the "\r", which used to end up in the release tag comparison and in makensis' -DVERSION.
# So: write the bytes ourselves (works on every Python 3), drop any "\r" anyway, and refuse
# anything that is not a plain version.
version=$(cargo metadata --no-deps --format-version 1 --offline 2>/dev/null \
  | "$PY" -c 'import json,sys; v=next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="cutemarkdown"); sys.stdout.buffer.write((v + "\n").encode("ascii"))' \
  | tr -d '\r')
version=$(strip_cr "$version")   # second guard, in bash itself
check_version "$version"
printf '%s\n' "$version"
