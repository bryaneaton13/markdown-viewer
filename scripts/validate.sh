#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/bundle.sh
scripts/check-quicklook.sh
scripts/shot.sh reports/reader-light.png fixtures/Welcome.md properties
scripts/shot.sh reports/reader-dark.png fixtures/Welcome.md dark,nosidebar,find:Markdown
echo "Checks passed. Inspect reports/*.png and reports/quicklook.html."
