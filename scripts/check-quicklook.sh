#!/usr/bin/env bash
# Test the exact Swift -> Rust renderer linked into the Finder extension.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
cargo build -p markdown-core
mkdir -p reports
xcrun swiftc -O -parse-as-library native/QuickLook/Renderer.swift native/QuickLook/Smoke.swift \
  -L target/debug -lmarkdown_core -o target/debug/quicklook-smoke
target/debug/quicklook-smoke "$root/fixtures/Welcome.md" "$root/reports/quicklook.html"
if [[ -d "dist/Markdown Viewer.app" ]]; then
  codesign --verify --deep --strict "dist/Markdown Viewer.app"
  pluginkit -m -A -D -i com.bryaneaton.MarkdownViewer.Preview || true
fi
