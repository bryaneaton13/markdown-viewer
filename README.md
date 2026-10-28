# Markdown Viewer

A local, read-only macOS Markdown reader built with **GPUI Kit 0.7.1**, plus a **Finder Quick Look preview extension**. The application setup and offscreen validation workflow follow the patterns in `bible-reader-desktop`.

## Run it

Requires macOS 13+, Rust (the pinned toolchain installs via rustup), and Xcode/Apple command-line tooling. Building the Finder extension uses the macOS SDK and Swift compiler.

```sh
cargo run -- fixtures/Welcome.md
cargo run -- /path/to/your/notes
scripts/bundle.sh
open "dist/Markdown Viewer.app" --args "$PWD/fixtures/Welcome.md"
```

For a local installation with Finder registration:

```sh
scripts/bundle.sh --install
open "$HOME/Applications/Markdown Viewer.app"
```

The default bundle is a development build. Use `scripts/bundle.sh --release --install` for an optimized build. Bundles are ad-hoc signed for local development, not notarized distribution builds.

## Included

- Native, selectable Markdown: headings, emphasis, lists, tables, task lists, code, links, footnotes and local images.
- Folder tree and filtered Quick Open; only `.md` and `.markdown` files.
- Sidebar keyboard navigation and show/hide, back/forward file history.
- Find against **rendered text**, match highlighting and next/previous navigation.
- Read-only source mode; YAML frontmatter with a Properties disclosure and diagnostics.
- Light/dark mode, adjustable text size, automatic reload on external changes.
- Native file/folder pickers and Finder/Dock document-open events.
- Persisted last file/folder, sidebar visibility, text size and theme.
- Offline Finder Quick Look extension with shared frontmatter/parser logic.

This prototype shows **one document at a time**. Tabs, custom themes, editing, Mermaid rendering, multi-root workspaces and full session/scroll restoration are not implemented. Mermaid fences remain code. Files must be UTF-8 and at most 8 MiB. Folder discovery respects ignore rules, hides hidden files, does not follow directory symlinks, and is bounded to 20,000 Markdown files / 32 levels. Remote images are not fetched.

## Keyboard

| Shortcut | Action |
| --- | --- |
| ⌘O / ⇧⌘O | Open file / folder |
| ⌘P | Quick Open |
| ⌘B or ⌃⌘S | Show/hide sidebar |
| ⌘1 / ⌘2 | Focus tree / document |
| ↑ ↓ ← → | Navigate focused folder tree |
| Enter / Space in tree | Open selection / focus reader |
| ⌘F / ⌘G / ⇧⌘G | Find / next / previous match |
| ⌘[ / ⌘] | Back / forward |
| ⇧⌘M | Source / preview |
| ⇧⌘I / ⇧⌘L | Properties / heading outline |
| ⇧⌘D | Toggle light/dark |
| ⌘+ / ⌘− / ⌘0 | Text size |
| ⌘R | Reload |
| ⌘/ | Shortcuts and Finder setup |
| ⌃⌘F | Full screen |
| ⌘W / ⌘Q | Close window / quit |

## Default app for `.md`

Install the app, then select a `.md` file in Finder → **Get Info** → **Open with** → **Markdown Viewer** → **Change All**. Repeat for `.markdown` if necessary. The bundle registers a Markdown **Viewer**, but deliberately does not change your existing default application during installation.

Opening by application name also works after registration:

```sh
open -a "Markdown Viewer" "$PWD/fixtures/Welcome.md"
```

## Finder Quick Look

The bundle contains `Contents/PlugIns/Markdown Preview.appex`, a sandboxed, data-based `QLPreviewProvider`. The provider links the same Rust core as the application and returns offline HTML to macOS. It does not spawn a renderer or need Node.

After installation, enable **Markdown Preview** in **System Settings → General → Login Items & Extensions → Quick Look** if your macOS version requires it. Select a Markdown file in Finder and press **Space**. Other installed Markdown providers can compete for the same type; default-app selection and Quick Look provider selection are independent.

Quick Look supports text, tables, code and frontmatter. Images use their alt text because the sandbox receives the selected file, not arbitrary neighboring files. Raw HTML is excluded, and the preview blocks scripts and network resources.

```sh
# Inspect provider registration (a + indicates enabled)
pluginkit -m -A -D -i com.bryaneaton.MarkdownViewer.Preview
# Exercise the system Quick Look host interactively
qlmanage -p "$PWD/fixtures/Welcome.md"
```

## Validate and view

```sh
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
scripts/check-quicklook.sh
scripts/shot.sh reports/reader-light.png fixtures/Welcome.md properties
scripts/shot.sh reports/reader-dark.png fixtures/Welcome.md dark,nosidebar,find:Markdown
# All of the above, plus bundle checks:
scripts/validate.sh
```

`shot.sh` captures the real GPUI window using Metal `render_to_image`, not a web mockup. It isolates application settings via `MARKDOWN_VIEWER_STATE_DIR`; no screen-recording permission is needed. Available scripted states: `light`, `dark`, `nosidebar`, `properties`, `source`, `outline`, `quick-open`, `find:query`, `help` (comma-separated).

Tests cover frontmatter edge cases, Unicode find offsets, links, HTML preview sanitization, unchanged source bytes, folder filtering, sidebar/focus shortcuts, rendered search and Quick Open keyboard navigation. `check-quicklook.sh` compiles and calls the **same Swift → Rust bridge** used by the extension and writes `reports/quicklook.html`; this validates rendering, not Finder's provider selection. Test Finder separately using the installed bundle.

Use `MARKDOWN_VIEWER_STATE_DIR=/path/to/scratch` for isolated live experiments. Normal preferences are in `~/Library/Application Support/Markdown Viewer/settings.json`. The app stays running after closing its window; use ⌘Q to quit.

## Source map

```text
crates/markdown-core/   parsing, metadata, links, find, safe HTML, Quick Look C ABI
crates/app/             GPUI application, reader, folder discovery, preferences
native/QuickLook/       Swift preview provider + bridge smoke test
assets/Info.plist       Markdown document type registration
fixtures/              representative Markdown and local image
scripts/               bundle/install, screenshots, validation
```

See [RESEARCH.md](RESEARCH.md) and [PLAN.md](PLAN.md) for the original research and broader design. Current implementation scope is described here.
