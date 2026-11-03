<p align="center">
  <img src="assets/logo.png" alt="Markdown Viewer logo" width="96">
</p>

<h1 align="center">Markdown Viewer</h1>

<p align="center">
  A fast, native, read-only Markdown reader for macOS, with a Finder Quick Look extension.
</p>

<p align="center">
  <a href="https://github.com/bryaneaton13/markdown-viewer/actions/workflows/ci.yml"><img src="https://github.com/bryaneaton13/markdown-viewer/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/macOS-13%2B-lightgrey.svg" alt="macOS 13+">
</p>

<p align="center">
  <img src="docs/screenshots/light.png" alt="Markdown Viewer in light mode with a folder tree and a rendered document" width="800">
</p>

Open a folder of Markdown, jump between files, and read them rendered. Nothing is editable, so you can't change a file by accident. It is built in Rust with [GPUI Kit](https://github.com/longbridge/gpui-kit), has no web view or Electron runtime, and works fully offline.

> **Status: early release (v0.1).** It works and is tested, but the download is ad-hoc signed, not notarized. See [Limitations](#limitations).

## Features

- Selectable, natively rendered Markdown: headings, emphasis, lists, tables, task lists, code, links, footnotes and local images.
- Folder tree and filtered **Quick Open** (⌘P) for `.md` and `.markdown` files.
- Find against the *rendered* text, with match highlighting and next/previous.
- Source view, YAML frontmatter shown in a collapsible Properties panel, and a heading outline.
- **Workspaces** for several folders, listed together in the sidebar as folders that open and close, with **tabs** that can mix files from any of them, and a **side-by-side split** of two documents (⌘\\). Drag to reorder workspaces or sort them by name or recent use.
- Editor-style **preview tabs**: a click previews a file in a reused italic tab, a double-click keeps it. Same-named files show the folder that tells them apart.
- Back/forward history per pane, light/dark/system appearance with chrome themes (Classic, Paper, Fjord, plus the colorful Synthwave, Campfire, Lagoon and Bubblegum), adjustable text size, and automatic reload when a file changes on disk.
- Finder and Dock "open with" support. Preferences persist between launches.
- **Finder Quick Look**: press Space on a Markdown file for an offline rendered preview.

<p align="center">
  <img src="docs/screenshots/dark.png" alt="Dark mode with find highlighting" width="800">
</p>

<p align="center">
  <img src="docs/screenshots/split.png" alt="Two documents side by side in a split, with the Fjord theme" width="800">
</p>

<p align="center">
  <img src="docs/screenshots/theme-synthwave.png" alt="Synthwave theme" width="395">
  <img src="docs/screenshots/theme-lagoon.png" alt="Lagoon theme in light mode" width="395">
</p>
<p align="center">
  <img src="docs/screenshots/theme-campfire.png" alt="Campfire theme" width="395">
  <img src="docs/screenshots/preview-tab.png" alt="A preview tab with an italic title" width="395">
</p>

## Install

### Download (Apple Silicon)

Download the ZIP from [GitHub Releases](https://github.com/bryaneaton13/markdown-viewer/releases/latest), unzip it, and drag **Markdown Viewer.app** into Applications. The build is ad-hoc signed, not notarized, so if macOS blocks it, open **System Settings → Privacy & Security → Open Anyway**. Only approve a download you trust.

### Build from source

You need:

- macOS 13 or later (Apple Silicon or Intel)
- [Rust](https://rustup.rs). The pinned toolchain in `rust-toolchain.toml` installs automatically.
- Xcode or the Xcode Command Line Tools (`xcode-select --install`), which provide the Swift compiler used for the Quick Look extension

```sh
git clone https://github.com/bryaneaton13/markdown-viewer.git
cd markdown-viewer
scripts/bundle.sh --release --install
open "$HOME/Applications/Markdown Viewer.app"
```

This builds the app, signs it ad hoc, copies it to `~/Applications`, and registers the Quick Look extension with Finder.

### Make it your default `.md` app

Select a `.md` file in Finder, choose **Get Info → Open with → Markdown Viewer → Change All**. Repeat for `.markdown` if needed. The installer never changes your default app for you.

### Enable Quick Look

Select a Markdown file in Finder and press Space. If the preview is plain text, enable **Markdown Preview** in **System Settings → General → Login Items & Extensions → Quick Look**. Other Markdown Quick Look plugins can compete for the same file type; to check that this one is registered:

```sh
pluginkit -m -A -D -i com.bryaneaton.MarkdownViewer.Preview   # "+" means enabled
```

## Usage

```sh
cargo run -- fixtures/Welcome.md      # open a file
cargo run -- /path/to/your/notes      # open a folder
open -a "Markdown Viewer" notes.md    # after installing
```

### Keyboard shortcuts

Press ⌘/ in the app for this list and the Finder setup notes.

| Shortcut | Action |
| --- | --- |
| ⌘O / ⇧⌘O | Open file / add folder workspace |
| ⌘P | Quick Open (⌘↵ new tab, ⌥↵ other pane) |
| ⌘T / ⌘W / ⇧⌘T | New tab / close pane or tab / reopen closed |
| ⌃Tab, ⇧⌘] / ⇧⌘[ | Next / previous tab |
| ⌘\\ | Split side by side / separate into tabs |
| ⌥⌘← / ⌥⌘→ | Focus left / right pane |
| ⌃1 … ⌃9, ⌃⌘] / ⌃⌘[ | Switch workspace (in sidebar order) |
| Click / double-click in tree | Preview in a reused tab / keep open |
| ⌘-click / ⌥-click in tree | Open in new tab / other pane |
| ⌘B or ⌃⌘S | Show/hide sidebar |
| ⌘1 / ⌘2 | Focus tree / document |
| ↑ ↓ ← → | Navigate the focused folder tree |
| Enter / Space in tree | Open selection / focus reader |
| ⌘F / ⌘G / ⇧⌘G | Find / next / previous match |
| ⌘[ / ⌘] | Back / forward |
| ⇧⌘M | Source / preview |
| ⇧⌘I / ⇧⌘L | Properties / heading outline |
| ⇧⌘D | Toggle light/dark (palette button: Match System and themes) |
| ⌘+ / ⌘− / ⌘0 | Text size |
| ⌘R | Reload |
| ⌃⌘F | Full screen |
| ⇧⌘W / ⌘Q | Close window / quit |

Drag a tab, or a file from the sidebar, to the left or right edge of the page to read it beside the current one, and drag a pane header back to the tab bar to separate them. A tab holds at most two panes. Tabs belong to the window, not to a folder, so files from different workspaces can sit side by side; each tab names its folder once they're mixed.

You can also drag Markdown files in from Finder. Drop one near the left or right edge of the page to split, or in the middle to open it in place (Back returns to the previous document). On a split, a file dropped on either pane replaces it. Drop two on an empty tab to read them side by side. Files dropped on the tab strip open as new tabs. Folders dropped on the sidebar become workspaces.

The app keeps running after you close its window; quit with ⌘Q. Settings, including open workspaces and tabs, live in `~/Library/Application Support/Markdown Viewer/settings.json`.

## Limitations

- At most two documents side by side per tab. No editing. Themes are a few built-in ones for the chrome, not custom.
- Mermaid blocks render as code. Remote images are never fetched.
- Files must be UTF-8 and at most 8 MiB.
- Folder discovery respects ignore rules, skips hidden files, does not follow directory symlinks, and stops at 20,000 files or 32 levels deep.
- Quick Look shows images as alt text (the sandbox only receives the selected file), omits raw HTML, and blocks scripts and network access.
- Not notarized: downloads need **Open Anyway** on first launch, and the Quick Look extension may need enabling by hand.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
scripts/validate.sh        # all of the above, plus bundle, Quick Look and screenshot checks
```

```text
crates/markdown-core/   parsing, frontmatter, links, find, safe HTML, Quick Look C ABI
crates/app/             GPUI application, reader, folder discovery, preferences
native/QuickLook/       Swift Quick Look preview provider and bridge smoke test
assets/                 Info.plist, app icon, logo
fixtures/               sample Markdown and a local image for tests and screenshots
scripts/                bundle/install, packaging, screenshots, validation
docs/                   original research and design plan
```

`scripts/shot.sh OUT.png FILE STATES` captures the real GPUI window with Metal `render_to_image`, with no screen-recording permission needed. `STATES` is a comma-separated list of `light`, `dark`, `browse` (add the file's folder as a workspace), `workspace:/path`, `switch:N` (activate the Nth workspace, from 0), `collapse:N` (open or close the Nth), `sidebar-focus`, `tab:relative/path.md`, `split:relative/path.md` (paths relative to the workspace; `~` for a space), `nosidebar`, `properties`, `source`, `outline`, `quick-open`, `find:query`, `help`, `preview:relative/path.md` (open in the preview tab), `appearance` (open the theme panel), `palette:name` (`classic`, `paper`, `fjord`, `synthwave`, `campfire`, `lagoon`, `bubblegum`). Set `MARKDOWN_VIEWER_STATE_DIR=/path/to/scratch` to keep experiments away from your real settings.

`scripts/check-quicklook.sh` compiles and calls the same Swift-to-Rust bridge the extension uses and writes `reports/quicklook.html`. It checks rendering, not Finder's choice of preview provider, so test that with an installed bundle.

## Contributing

Issues and pull requests are welcome. Before opening a PR, run `scripts/validate.sh` (CI runs the same checks) and add a test for behavior changes. Background and design intent are in [docs/RESEARCH.md](docs/RESEARCH.md) and [docs/PLAN.md](docs/PLAN.md).

Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## License

[MIT](LICENSE). Built on [GPUI Kit](https://github.com/longbridge/gpui-kit) (Apache-2.0).
