#!/usr/bin/env bash
# Print the cutemarkdown version from Cargo.toml (single source of truth for the installer,
# the zip name and the release tag check).
set -euo pipefail
# shellcheck source=scripts/_common.sh
. "$(dirname "${BASH_SOURCE[0]}")/_common.sh"
[ -n "$PY" ] || { echo "python3 not found" >&2; exit 1; }
cd "$ROOT"

cargo metadata --no-deps --format-version 1 --offline 2>/dev/null \
  | "$PY" -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="cutemarkdown"))'
