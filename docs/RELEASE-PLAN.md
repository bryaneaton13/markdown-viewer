# Release plan: signed macOS, Windows and Linux

Status: proposal. Date: 2026-11-06.

How Markdown Viewer gets from a local, ad-hoc-signed macOS build to signed, notarized macOS releases plus Windows and Linux downloads. It stays a read-only viewer throughout. Builds run locally, one script per platform (no CI for now).

The approach follows [quiet-md](https://github.com/s1dashu/quiet-md), another GPUI app that ships on all three platforms. Files of theirs worth reading are marked *quiet-md:*.

## Where things stand

- **macOS:** `scripts/package.sh` builds an Apple Silicon ZIP, ad-hoc signed, not notarized. Users have to approve it in Privacy & Security.
- **Already in place for later phases** (added alongside this plan):
  - A launch log (`crates/app/src/logging.rs`). It already picks Windows and Linux locations.
  - An update check against GitHub's latest release (`crates/app/src/updates.rs`). It shows a status-bar button that links to the release page.
  - A one-time "What's new" card built from `CHANGELOG.md`.
- **Not portable yet:** nothing in `crates/app` uses `cfg(target_os)`. The settings path, keyboard shortcuts and help text all assume macOS (see phase 1).

## Phases

| Phase | Work | Size | Needs |
|---|---|---|---|
| 0 | Release checklist and version bookkeeping | S | — |
| 1 | Portability work (paths, keys, labels) | M | — |
| 2 | Signed and notarized macOS release | S–M | Apple Developer account ($99/year) |
| 3 | Windows installer | M | Windows 10/11 machine or VM |
| 4 | Linux tarball (experimental) | S–M | Ubuntu 22.04 machine or VM |
| 5 | One publish script for GitHub Releases | S | 2–4 |
| 6 | In-app download and install | M | 2, 5 |

---

## 0. Release checklist and versions

- `Cargo.toml` `[workspace.package].version` is the source of truth.
- `assets/Info.plist` repeats the version by hand (`0.2.0`) and can drift. Have `bundle.sh` set `CFBundleShortVersionString` (and `CFBundleVersion`) from `Cargo.toml` with `plutil -replace`.
- **Changelog:** rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD` before building. The "What's new" card and page read the section whose heading matches the running version. Without the rename, an update shows nothing new.
- Tag `vX.Y.Z`. The update check compares against the latest *published, non-prerelease* GitHub release's `tag_name`, so a draft or prerelease never prompts anyone.

## 1. Portability work

All of this must be done before a Windows or Linux build is worth shipping.

**Paths**
- Settings (`preferences.rs`): `HOME` + `Library/Application Support` → `dirs::config_dir().join("Markdown Viewer")`.
  - On macOS this is the same folder, so existing settings carry over.
  - Keep `MARKDOWN_VIEWER_STATE_DIR`.
- `names.rs` shortens paths to `~` using `HOME`; use `dirs::home_dir()`.

**Keyboard shortcuts**
- Change the `cmd-` bindings in `actions.rs` and `reader.rs` to `secondary-` (⌘ on macOS, Ctrl elsewhere).
- Resolve the conflicts this creates off macOS:
  - `ctrl-1…9` (workspaces) collides with `secondary-1/2` (focus); use Alt+1…9 for workspaces there.
  - `ctrl-tab` now means the same thing as `secondary-tab`.
- `cmd-ctrl-s` and `cmd-q` are macOS-only.

**Text the user sees**
- `WELCOME` / `HELP` in `pane.rs`, the tooltips and the palette footers hard-code ⌘ ⌥ ⇧ ⌃.
  - Add a `keys::label("secondary-shift-f")` helper that returns `⇧⌘F` or `Ctrl+Shift+F`.
  - Build the help table from the binding list so labels can't drift.
- "Reveal in Finder" → "Show in Explorer" / "Show in Folder". `cx.reveal_path` already works everywhere.
- The help page's Finder and Quick Look sections become macOS-only. Its log path line should come from `logging::dir()`.

**Windows-specific**
- `#![cfg_attr(windows, windows_subsystem = "windows")]` in `main.rs`, so no console window opens.
- `crates/app/build.rs`: compile an `.rc` file with the icon and version info (*quiet-md:* `build.rs`). Generate `assets/AppIcon.ico`.
- Single instance: opening a second `.md` should reach the running window.
- Opening with the app: files arrive as command-line arguments. Make sure every one is opened, not just the first.
- The update check already hides curl's console window (`CREATE_NO_WINDOW`).

**Stays macOS-only:** Quick Look (the Swift extension in `bundle.sh`).

## 2. Signed and notarized macOS release

**One-time setup**
1. Join the Apple Developer Program and create a **Developer ID Application** certificate.
2. Store notary credentials:
   ```sh
   xcrun notarytool store-credentials markdown-viewer \
     --apple-id … --team-id … --password <app-specific password>
   ```

**`scripts/package.sh`** (falls back to today's ad-hoc build when no identity is set):
1. Read `SIGNING_IDENTITY` and `NOTARY_PROFILE`.
2. Sign inside-out with the hardened runtime and a timestamp: the Quick Look `.appex` with its entitlements first, then the app. Use `--deep` only to verify.
3. Check that the extension's entitlements include the app sandbox, which Quick Look requires.
4. Zip with `ditto -c -k --keepParent`, then `xcrun notarytool submit --wait --output-format json`. Fail unless the status is `Accepted`.
5. `xcrun stapler staple`, then `stapler validate` and `spctl --assess --type execute`.
6. Zip the stapled app: `Markdown-Viewer-<ver>-macOS-arm64.zip` + `.sha256`.
7. Optional: a universal build (arm64 + x86_64 joined with `lipo`, and the Swift extension built for both).

Then remove the "ad-hoc signed" warning from the README.

*quiet-md:* `scripts/release.sh`.

## 3. Windows installer

Build on Windows 10/11 x64 with the Visual Studio Build Tools (C++) and Rust.

- **Inno Setup**, `packaging/windows/markdown-viewer.iss`:
  - Per-user install, so no admin prompt: `PrivilegesRequired=lowest`, `{localappdata}\Programs\Markdown Viewer`.
  - Start menu shortcut, and an optional desktop shortcut.
  - Link the C runtime statically (`+crt-static`) so no `vcruntime140.dll` is needed.
  - **File association:** register `.md` / `.markdown` under `HKCU\Software\Classes` with `OpenWithProgids`. The app shows up in "Open with" without taking over the default, matching macOS's `LSHandlerRank=Alternate`.
  - A silent `/UPDATE=1` mode that relaunches the app (for phase 6).
  - Uninstalling keeps the user's settings.
- `scripts/package-windows.ps1` builds the app and the installer → `Markdown-Viewer-<ver>-windows-x64-setup.exe` + `.sha256`.
- **Signing:**
  - First ship unsigned, and document SmartScreen's **More info → Run anyway**.
  - Then use **Azure Trusted Signing** (about $10/month, no hardware token).
  - Sign the exe, the installer and the uninstaller.
- **Check:**
  - Windows 10 2004+ (GPUI uses DirectX 11), on a VM with basic display drivers.
  - High DPI.
  - File paths with non-ASCII characters.

*quiet-md:* `scripts/package-windows.ps1`, `scripts/sign-windows-file.ps1`, `packaging/windows/quiet-md.iss`.

## 4. Linux tarball (experimental)

- `scripts/linux-deps.sh`: apt and pacman package lists (xkbcommon, wayland, x11-xcb, vulkan, fontconfig, freetype, xdg-desktop-portal).
- `packaging/linux/com.bryaneaton.MarkdownViewer.desktop` with `MimeType=text/markdown;`, plus a 512px icon.
- `scripts/package-linux.sh`:
  1. Build, then stage `bin/`, `share/applications/` and `share/icons/`.
  2. Fail if `ldd` reports any library "not found".
  3. Output `Markdown-Viewer-<ver>-linux-x86_64.tar.gz` + `.sha256`.
- Build on **Ubuntu 22.04**, because glibc is linked dynamically.
- Test GNOME on Wayland, KDE and X11.
- File watching uses inotify; check the watch limits on large folders.
- Later: AppImage or Flatpak.

*quiet-md:* `scripts/package-linux.sh`, `scripts/linux-deps.sh`.

## 5. Publishing

GitHub Releases stays the only download host.

`scripts/publish.sh <files…>`:
1. Check each file's version against `Cargo.toml`, and its `.sha256`.
2. Sign each package with **minisign** (`-t <file name>` as the trusted comment, so an old package can't pose as a new version). Verify against `packaging/update-key.pub`. The private key stays outside the repo.
3. Write `update-<platform>.json`: `{version, url, size, signature}`.
4. Upload with `gh release create` / `gh release upload`: packages first, then the `.sha256` and `.minisig` files, and the update JSONs **last**, so the app never learns of a download that isn't up yet.
5. Use the matching `CHANGELOG.md` section as the release notes (`markdown_core::release_notes` already extracts it).

The README's Install section links each platform via `releases/latest/download/<name>`.

*quiet-md:* `scripts/publish-downloads.sh`.

## 6. In-app download and install

Today the update button opens the release page. Next:

- Read `update-<platform>.json` from the latest release.
- Download with `curl`, check the size and the minisign signature (`minisign-verify` against a key compiled into the app).
- **macOS:**
  1. Unzip.
  2. Require the new app to have the same Team ID as the running one. This needs phase 2: ad-hoc builds have no Team ID.
  3. A detached helper waits for the app to quit, swaps the bundle and relaunches.
- **Windows:** run the installer with `/VERYSILENT /UPDATE=1` once the app quits.
- **Linux:** keep linking to the release page.
- States: `Available → Downloading → Ready → Installing | Failed`. Extend `updates::State`.

*quiet-md:* `src/update.rs`.

## Not planned

- CI workflows (builds stay local for now).
- Export or print to PDF.
- Anything that edits files.
