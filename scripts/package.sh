#!/usr/bin/env bash
# Build an ad-hoc-signed release app and package it for GitHub Releases.
# Uses the host architecture; no Apple Developer account is required.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [[ -n ${CARGO_BUILD_TARGET:-} || -n ${CARGO_TARGET_DIR:-} ]]; then
  echo "Unset CARGO_BUILD_TARGET and CARGO_TARGET_DIR before packaging." >&2
  exit 1
fi
"$root/scripts/bundle.sh" --release
app="$root/dist/Markdown Viewer.app"
arch=$(lipo -archs "$app/Contents/MacOS/markdown-viewer")
case "$arch" in
  arm64|x86_64) ;;
  *) echo "Unexpected binary architecture: $arch" >&2; exit 1 ;;
esac
codesign --verify --deep --strict "$app"
archive="Markdown-Viewer-$version-macOS-$arch.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$root/dist/$archive"
cd "$root/dist"
shasum -a 256 "$archive" > "$archive.sha256"
echo "==> Packaged $root/dist/$archive"
cat "$archive.sha256"
