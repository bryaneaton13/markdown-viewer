#!/usr/bin/env bash
# Real Metal pixels; isolated settings and no screen-recording permissions.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: scripts/shot.sh OUT.png [FILE_OR_FOLDER] [dark,properties,nosidebar,source,outline,quick-open,find:query,help]}
file=${2:-"$root/fixtures/Welcome.md"}
actions=${3:-}
cd "$root"
cargo build -p markdown-viewer --features screenshot
state=$(mktemp -d "${TMPDIR:-/tmp}/markdown-shot.XXXXXX")
trap 'rm -rf "$state"' EXIT
mkdir -p "$(dirname "$out")"
MARKDOWN_VIEWER_STATE_DIR="$state" MARKDOWN_VIEWER_SCREENSHOT="$out" MARKDOWN_VIEWER_ACTIONS="$actions" \
  target/debug/markdown-viewer "$file"
