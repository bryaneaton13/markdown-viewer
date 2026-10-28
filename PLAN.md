# Focused Markdown viewer for macOS

Status: original design with prototype implementation underway. Date: 2026-10-27.
Research and build-versus-use rationale: [RESEARCH.md](RESEARCH.md).

## Implementation decision

The user approved a custom GPUI prototype, with `bible-reader-desktop` as the setup/validation reference. The first implementation is read-only and Markdown-only. Finder Quick Look is promoted into this delivery; Mermaid remains code because it is not a trivial built-in Kit component. One-document navigation/history replaces tabs in the first prototype. See [README.md](README.md) for implemented behavior and repeatable validation commands; the sections below retain the broader design roadmap.

## 1. Product decision

First evaluate Glim. If it meets the workflow, use it rather than recreating it. If its multi-format editing workspace is the wrong experience, build the focused application below using GPUI Kit and selective lessons from Glim.

The intended experience: open a folder of Markdown, move quickly between files, hide the folder pane, and read beautifully rendered content without accidentally changing anything. Double-clicking a `.md` file in Finder should open directly into that experience.

Defaults: read-only; local files; one folder root per window; optional pinned tabs plus one transient preview; system light/dark appearance. Start with Apple Silicon, and add Intel builds if required. Confirm minimum macOS version in the dependency spike before setting the deployment target.

## 2. Window and interaction design

- Native macOS window controls and compact title bar. Toolbar: sidebar toggle, back/forward, filename/breadcrumb, Quick Open, outline, source toggle.
- Left folder pane: approximately 240 points wide, resizable from 180–420 points subject to window width. Hiding it removes it completely, not an icon rail. Preserve width and expansion state.
- Reading pane: selectable native Markdown, roughly 70–90 characters per line, comfortable line height, configurable text size. Wide code and tables scroll horizontally within their blocks.
- Optional tabs appear when there is more than one open document. One unpinned preview slot is visually distinct; pinning is explicit.
- Frontmatter appears as a collapsed Properties disclosure above the body. Outline is a popover initially, avoiding a permanent third pane.
- Empty window offers Open File, Open Folder, and recent locations. Loading, unreadable file, deleted file, and malformed diagram states have inline explanations and useful actions.

### Folder behavior

Open Folder replaces the current window's root. New Window supports a second folder; multi-root workspaces can wait. Show directories and `.md`/`.markdown` files by default. Keep local images usable even when hidden from the tree. Offer “Show other files” and “Show hidden files”; unsupported files open externally.

Load direct children lazily; sort folders first and names naturally. Hide `.git` and common build/dependency directories by default. Respect `.gitignore` for filename search with a visible include-ignored override. Do not recursively follow directory symlinks. Index filenames asynchronously; do not scan the entire root during UI rendering.

Opening a standalone file does not automatically crawl its parent directory. Offer “Browse enclosing folder.” Relative links still resolve against the document's directory.

### Quick viewing and navigation

- Single-click a file to replace the transient preview; Enter or double-click pins it.
- Arrow keys move tree selection and update the preview after a short debounce; keep keyboard focus in the tree. Cancel or disregard stale loads.
- Space in the focused tree gives a temporary reading preview; Escape returns to the tree. In the reader, Space keeps its normal page-down meaning.
- Cmd+P opens a fuzzy filename/path picker with recent files first. Enter opens; Cmd+Enter pins. Escape restores prior focus.
- Local Markdown links open in the current window and create back/forward history with scroll anchors. External HTTP(S) links use the browser. Other file links use the system app.
- Preserve reading position per file. Returning from source mode, navigating back, and refreshing after an external save should retain the nearest stable block anchor.

## 3. Keyboard shortcuts

All commands must also be discoverable in menus or toolbar tooltips. Use named GPUI actions with focus contexts rather than a global key-event interceptor.

| Shortcut | Action | Context / detail |
| --- | --- | --- |
| Cmd+O | Open file | Native file picker |
| Cmd+Shift+O | Open folder | Native directory picker |
| Cmd+N | New viewer window | No document creation |
| Cmd+P | Quick Open | Filename/path search, not Print |
| Cmd+B | Toggle left sidebar | Primary requested shortcut |
| Cmd+Ctrl+S | Toggle left sidebar | macOS-style alternate binding |
| Cmd+1 | Focus folder tree | Show it if hidden |
| Cmd+2 | Focus reader | Preserve tree selection |
| Cmd+F | Find in document | Rendered-text search |
| Cmd+G / Cmd+Shift+G | Next / previous match | Scroll and highlight |
| Cmd+[ / Cmd+] | Back / forward | File/link navigation history |
| Cmd+Shift+P | Command palette | Discover less-common commands |
| Cmd+Shift+M | Toggle source / rendered view | Source is read-only |
| Cmd+Shift+I | Toggle Properties | Frontmatter panel |
| Cmd+Shift+L | Heading outline | Select heading to jump |
| Cmd+= / Cmd+- / Cmd+0 | Increase / decrease / reset text size | Accept Cmd+Plus too |
| Cmd+R | Reload file | Watcher usually handles updates |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab | Pinned and preview tabs |
| Cmd+W | Close document | Close empty window if no documents |
| Cmd+Shift+W | Close window | Leave application running |
| Cmd+, | Settings | Appearance, reader, associations |
| Cmd+Ctrl+F | macOS full screen | OS full-screen behavior |
| Cmd+Q | Quit | Standard Mac behavior |
| Up/Down, Left/Right | Tree navigation | Collapse/expand, parent/child |
| Space / Shift+Space | Page down / up | Reader focus only |
| Cmd+Up / Cmd+Down | Start / end of document | Reader focus only |
| Escape | Dismiss topmost temporary surface | Picker/find/preview, then restore focus |
| Cmd+C / Cmd+A | Copy / select all | Focused reader or input |

Cmd+P intentionally follows developer quick-open conventions. Printing is deferred; if introduced later, give it a menu item and a separate binding. Preserve standard text-input shortcuts while search fields have focus. Avoid Ctrl+Option combinations used by VoiceOver. Validate shortcuts on non-US keyboard layouts and against macOS menu handling.

## 4. Rendering contract

### Required Markdown

Headings, paragraphs, emphasis, links, ordered/unordered lists, blockquotes, horizontal rules, fenced code, GFM tables, strikethrough, and read-only task lists. Include footnotes if the selected Kit renderer supports them correctly. Support local PNG/JPEG/WebP images and heading anchors; treat SVG through an established sanitized image path.

Reuse Kit TextView and its parser rather than creating a second general-purpose Markdown pipeline. Implement a thin document adapter for frontmatter, source offsets, heading IDs, resource resolution, and find indexing. If a second AST parser is needed for ranges, pin compatible behavior and test both against the same fixtures.

Selection/copy across blocks, rendered find, link hit-testing, and heading scrolling are early spike requirements—not assumed free features. If Kit lacks an API, identify and size the small integration change before expanding the app.

Raw HTML receives the renderer's safe non-executing treatment; no scripts or embedded web applications. MDX/JSX execution, wiki links, math, editing, sync, PDF export, and plugins are later scope decisions. Remote images are off by default with a per-document load action; local reading must work offline.

### Frontmatter

1. Detect YAML at document start, allowing BOM and CRLF. Require closing delimiter; never eat an unterminated header silently.
2. Retain source bytes and the body offset. Parse mappings for the Properties UI; preserve nested structures and unknown keys.
3. Invalid YAML shows an error/raw metadata view while still exposing the rest of the document. If boundaries are uncertain, retain complete source rather than guessing.
4. Ordinary horizontal rules and fenced YAML remain body content. Do not interpret metadata as instructions, templates, or executable tags.
5. Add TOML frontmatter only as a separately tested enhancement.

### Mermaid: small optional milestone

Reuse Glim's approach, not its entire multi-format diagram system. Bundle a pinned Mermaid build at packaging time. A private helper process hosts WebKit on its main thread; it receives bounded source jobs, returns sanitized SVG, and the service rasterizes it for GPUI display. No Node runtime is required after installation.

Render only visible/near-visible blocks, with a bounded queue. Cache by source hash, engine version, theme, scale, and rendering options. Time out and terminate stuck jobs; show source and Retry on failure. Invalidate affected diagrams on theme or document changes. Render at Retina scale and rerender when zoom demands it; retain diagram source for accessibility and copying.

Start with flowchart and sequence coverage. Keep strict mode, block external resource fetching and navigation, and limit dimensions/working memory. Preserve license notices. A diagram failure must not block reading prose or opening another file.

## 5. Dependencies and tools

| Dependency / tool | Purpose | Decision |
| --- | --- | --- |
| Rust stable + Cargo | Application | Pin working toolchain after spike |
| `gpui-kit` | GPUI, controls, rich text, themes, focus | Start with observed 0.7.1 baseline; verify stock build |
| Kit syntax features | Code fences | Enable only common languages initially |
| `notify` | External file/folder changes | Coalesce events and handle atomic saves |
| `ignore` | Scoped filename discovery | Background scan, user-visible ignore policy |
| `serde`, `serde_json` | Preferences/session state | Versioned JSON, atomic writes |
| `url` | File URLs, relative links, fragments | Avoid manual URL string manipulation |
| `anyhow` or typed errors | Error boundaries | Pick one modest project convention |
| YAML parser, e.g. `serde_yaml_ng` | Properties | Confirm maintenance; Kit parser may cover some needs |
| `nucleo-matcher` | Fuzzy filenames | Add only if Kit picker does not provide matching |
| `markdown` | AST/ranges if needed | Glim uses 1.0.0; not mandatory alongside Kit |
| Mermaid + bundler | Offline diagrams | Build-time JS assets only |
| WebKit bridge (`wry` or small Swift helper) | Diagram helper | Glim uses wry; reuse proven path after spike |
| `resvg` | SVG-to-image for diagrams | Follow Glim if adopting its diagram pipeline |
| Swift/AppKit bridge | Association APIs, optional Quick Look | Small macOS-only boundary |
| Xcode tooling | Native compilation, bundle/sign/notarize | Verify SDK/toolchain requirements early |
| `tempfile` + Kit test support | Targeted tests | Fixtures, filesystem races, keyboard focus |

Use GPUI's executors first; add Tokio only if a concrete dependency requires it. No database, network service, authentication, or editor engine is needed. Store config under `~/Library/Application Support/<AppName>` and bounded caches under `~/Library/Caches/<AppName>`.

## 6. Architecture and proposed files

Start as one Rust package with clear modules, plus the optional helper. Split crates only when sharing with the Quick Look extension or test isolation warrants it. These paths are proposed; the directory currently has no app source.

```text
Cargo.toml / Cargo.lock / rust-toolchain.toml
src/
  main.rs                 Kit initialization and startup
  app.rs                  windows, open requests, app lifecycle
  actions.rs              named actions and contextual bindings
  menus.rs                macOS menus using the same actions
  workspace.rs            root, tabs, preview, history, focus
  sidebar.rs              lazy tree and selection
  quick_open.rs           picker and filename ranking
  reader.rs               retained TextView and read-only source
  document.rs             source, generation, headings, metadata
  frontmatter.rs          extraction/validation, offset mapping
  files.rs                reads, discovery, canonical identity
  watch.rs                events, debounce, reload generations
  preferences.rs          atomic settings and session persistence
  macos.rs                native integration boundary
  diagrams.rs             optional helper queue and image cache
native/
  Associations.swift      if using a Swift association bridge
  QuickLook/              optional later extension
assets/macos/Info.plist
scripts/bundle.sh
tests/fixtures/            Markdown, metadata, links, diagrams
```

UI owns retained entities and task handles; services perform I/O away from render/layout. `DocumentSnapshot` holds path, original source, generation, metadata/body range, headings and parsed/rendered resources. `WorkspaceState` holds root, expanded paths, selected path, preview document, pinned documents, history, and sidebar width/visibility.

Every asynchronous load carries a document ID and generation; install results only when both still match. Watch the parent directory as needed for editor replace-by-rename saves. Deletion leaves a “File moved or deleted” state rather than a crash. Rescan affected tree nodes after rename; preserve file identity where reliably detectable.

Persist recent files/folders, sidebar state, expanded folders, pinned tabs, and per-file reading anchors with bounded history. Restore asynchronously; an explicit Finder-open request takes priority over old session restoration. Keep the source file completely untouched, including task checkboxes and source-mode keystrokes.

## 7. macOS opening and default associations

Ship a real `.app` bundle with a stable new bundle ID, executable, icon, and version metadata. Register Markdown using `CFBundleDocumentTypes`, Viewer role, and appropriate `LSItemContentTypes`. Resolve conventional Markdown UTIs such as `net.daringfireball.markdown` against the target SDK/OS; use imported type declarations with `.md`/`.markdown`, `text/markdown`, and plain-text conformance if needed. Do not claim ownership of an existing external format or register as the default for `public.plain-text`.

Implement and test:

- Finder double-click and Open With, Dock drops, native Open File, folder drops, and CLI paths.
- GPUI open-URL callbacks for cold/warm launch, queued until windows are ready.
- Correct URL decoding, Unicode/spaces, multiple opens, and duplicate-file activation.
- “Make default for Markdown” in Settings, scoped to Markdown and reporting success/error. Prefer current NSWorkspace content-type APIs where supported; availability-gate any Launch Services fallback.
- A dependable manual fallback: Finder Get Info → Open with → this app → Change All.
- `open -a <AppName> file.md` works after installation; an optional convenience CLI can come later.

For personal development, an ad-hoc bundle is enough to test. Before distributing to other users, add Developer ID signing, hardened runtime, notarization, and stapling. Sign nested diagram helpers consistently. Start with direct distribution; App Store sandbox/bookmark requirements are a separate scope expansion.

### Finder Quick Look, later

Add a separate preview extension only if Finder Space-bar previews are still valuable after the default viewer is fast. Use Apple's Quick Look extension lifecycle, bounded work, and an appropriate sandbox configuration. Share parsing/fixtures where practical; UI code need not be GPUI inside the extension. Basic text/Markdown preview comes first; Mermaid is optional and must respect the host's resource/time limits. Verify extension registration and competition with other installed Markdown preview providers independently from default-app tests.

## 8. Delivery sequence and acceptance gates

### Phase 0 — Glim evaluation and decision

Run the corpus in RESEARCH.md. Record observed gaps, startup/file-switch timings, and preference for its editing model. **Stop custom development if Glim meets the need.** If only localized changes are needed, prefer contributing them.

### Phase 1 — Kit feasibility spike

Reuse Kit's Markdown example and stock dependency release. Open one real file in a minimal window; prove selection, copy, long-document scrolling, frontmatter adapter, heading jumps, find highlighting, and cold/warm open events in a bundle. Establish whether Glim's vendor patches are necessary.

Exit: a small functioning reader and an explicit list of required framework changes. Do not build a polished shell around an unproven renderer.

### Phase 2 — Daily-use reader

Add lazy folder tree, sidebar toggle/focus, Quick Open, preview/pinned tabs, navigation history, local assets/links, Properties, source view, and external-save refresh. Reuse Kit controls and background tasks.

Exit: navigate a real documentation folder entirely with the keyboard; hiding/showing the sidebar preserves reading position; rapid switching never displays stale content; files remain byte-for-byte unchanged.

### Phase 3 — macOS integration and persistence

Add settings/session persistence, association action/manual instructions, app icon and bundle packaging, error states, and VoiceOver/focus checks. Finalize deployment target and architecture support.

Exit: installed app handles cold and warm Finder opens and `.md` defaults correctly, restarts into the expected workspace, and gracefully handles missing files.

### Phase 4 — Mermaid

Adapt the isolated renderer pattern, bundle engine assets/licenses, add cancellable cache and source fallback. Measure first-diagram latency separately from first-text paint.

Exit: flowchart and sequence fixtures render offline in both themes; malformed/large diagrams do not stall the reader; no Node installation is required.

### Phase 5 — Optional Finder preview and public release

Add Quick Look only if wanted. Complete Intel testing if required, signing/notarization, release packaging and third-party notices. Manual updates are sufficient initially.

Planning estimate, not a commitment: a focused prototype is several working days; a dependable daily-use reader with diagrams and packaging is more plausibly a few weeks. Kit compatibility, rendered find/selection, and helper integration are the main uncertainty. Quick Look adds a separate integration/testing effort.

## 9. Verification and performance goals

Targets below are goals to measure in release builds on a documented Apple Silicon Mac, not framework guarantees:

- Cold launch to readable 100 KB local Markdown: under 1 second.
- Warm cached document switch: under 100 ms; show feedback immediately for slower reads.
- Quick Open filtering after indexing: under 50 ms for 10,000 paths.
- Large-folder indexing and diagram rendering never block sidebar input.
- Scroll a 1 MB Markdown fixture smoothly; profile expensive blocks rather than assuming GPU rendering solves parsing/layout.
- Bound caches and helper concurrency; demonstrate memory stabilizes after repeatedly opening/closing files.

Meaningful tests: frontmatter boundaries/BOM/CRLF/malformed YAML; Markdown offset/heading mapping; relative links with spaces/fragments; read-only source preservation; stale-load suppression; atomic-save watcher behavior; tree focus and overlay Escape order; association cold/warm launches; invalid/timeout diagram fallback. Manually check copy across blocks, Retina output, dark mode, reduced motion, VoiceOver, and external-volume failures.

## 10. Defaults to confirm before implementation

1. **Use Glim if it passes the trial; otherwise build the small app.** Avoid a full fork by default.
2. **Read-only viewer.** “Open in Editor” can be a menu command; no autosave or task-checkbox mutation.
3. **YAML frontmatter in MVP; Mermaid in its own milestone.** TOML and math follow demonstrated need.
4. **Quick Open and transient previews in MVP; Finder Quick Look later.** These are separate integrations.
5. **One root per window, direct distribution, Apple Silicon first.** Confirm Intel and minimum macOS support before release planning.
