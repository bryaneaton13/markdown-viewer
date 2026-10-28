#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
profile=debug
install=false
for arg in "$@"; do
  case "$arg" in
    --release) profile=release ;;
    --install) install=true ;;
    *) echo "Usage: scripts/bundle.sh [--release] [--install]" >&2; exit 2 ;;
  esac
done
if [[ "$profile" == release ]]; then
  cargo build --workspace --release
else
  cargo build --workspace
fi
app="$root/dist/Markdown Viewer.app"
extension="$app/Contents/PlugIns/Markdown Preview.appex"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$extension/Contents/MacOS"
cp "target/$profile/markdown-viewer" "$app/Contents/MacOS/"
cp assets/Info.plist "$app/Contents/Info.plist"
cp native/QuickLook/Info.plist "$extension/Contents/Info.plist"
arch=$(uname -m)
xcrun swiftc -O -parse-as-library -module-name MarkdownPreview \
  -target "$arch-apple-macosx13.0" -application-extension \
  native/QuickLook/Renderer.swift native/QuickLook/PreviewProvider.swift \
  -L "target/$profile" -lmarkdown_core -framework Cocoa -framework QuickLookUI -framework UniformTypeIdentifiers \
  -Xlinker -e -Xlinker _NSExtensionMain -o "$extension/Contents/MacOS/MarkdownPreview"
codesign --force --sign - --entitlements native/QuickLook/entitlements.plist "$extension"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
plutil -lint "$app/Contents/Info.plist" "$extension/Contents/Info.plist"
echo "Built: $app"
if $install; then
  dest="$HOME/Applications/Markdown Viewer.app"
  mkdir -p "$HOME/Applications"
  ditto "$app" "$dest"
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$dest"
  pluginkit -a "$dest/Contents/PlugIns/Markdown Preview.appex"
  echo "Installed: $dest"
  echo "If needed, enable Markdown Preview in System Settings → General → Login Items & Extensions → Quick Look."
fi
