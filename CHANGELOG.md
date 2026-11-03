# Changelog

All notable changes to Markdown Viewer are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/) (`0.MINOR.PATCH` until 1.0).

## [Unreleased]

Planned for 0.2.0.

### Added
- **Workspaces.** Each added folder becomes a workspace with its own file tree. The sidebar lists every workspace as a folder you can open and close, so several projects can be browsed at once. Opening a folder never switches workspaces. The active workspace (marked with a dot) follows the focused document and scopes Quick Open; when closed it still shows the current file name and its number of open tabs. Switch with ⌃1–9 (in sidebar order) or ⌃⌘[ / ⌃⌘], which also brings forward one of its tabs. Files outside every folder are listed under "Open Files". Close Workspace closes the tabs showing its files (⇧⌘T brings them back).
- **Organizing the sidebar.** Drag folders to reorder them, or sort by name or recent use. Collapse All closes every workspace but the active one (press it again to close that too). Each folder's ⋯ / right-click menu has Reveal in Finder, Collapse Subfolders and Close Workspace. The arrow keys move across all open workspaces: ← closes a folder or goes to its parent, → opens it, ↵ opens a file in its workspace.
- **Tabs.** Tabs belong to the window and can mix files from any workspace; once they do, each tab shows its folder name. ⌘T opens a new tab, ⌘W closes a pane or tab, ⇧⌘T reopens the last closed one, and ⌃Tab / ⇧⌘] / ⇧⌘[ switch tabs. Tabs can be pinned, reordered by dragging, and closed from a context menu. The tab strip only shows with more than one tab.
- **Side-by-side split.** ⌘\ shows a document twice, or separates a split back into two tabs. Drag a tab, or a file from the sidebar tree, to the left or right edge of the page to read it beside the current one, and drag a pane header to the tab bar to separate it again. Splits stop at two panes. In narrow windows a split shows one pane at a time with a switcher.
- **Drag from Finder or the sidebar.** Drop a Markdown file near the left or right edge of the page to split (the edge is a quarter of the width, 72–240 pt), or in the middle to open it in place; on a split, it replaces the pane it lands on. Sidebar files dropped on the tab strip open as new tabs. Drop two files on an empty tab to read them side by side. Files dropped on the tab strip open as new tabs, and folders dropped on the sidebar (or anywhere else) become workspaces.
- ⌘-click a file in the tree, or press ⌘↵ in Quick Open, to open it in a new tab. ⌥-click or ⌥↵ opens it in the other pane.
- Workspaces, tabs, splits and Source mode are restored at launch. Restored tabs read their file the first time they are shown.

### Changed
- ⌘W now closes the focused pane or tab. Close Window moved to ⇧⌘W.
- "Open Folder…" is now "Add Folder Workspace…". Settings saved by 0.1 (one folder and one file) are migrated into a single workspace.
- The folders of the active workspace and of every workspace open in the sidebar are watched for changes. Closed workspaces rescan when they are opened again.

## [0.1.0] - 2026-10-27

First release: a native, read-only GPUI Markdown reader with a Finder Quick Look extension.

### Added
- Folder tree, Quick Open (⌘P), find in rendered text, source view, frontmatter Properties panel, heading outline, back/forward history, light/dark mode, adjustable text size, live reload and Finder "open with" support.
- Finder Quick Look extension for an offline rendered preview.
- A Copy button in Source view, with copied feedback.
- A simple website in `docs/` and this changelog.
- A downloadable Apple Silicon ZIP on GitHub Releases (`scripts/package.sh`). It is ad-hoc signed, not notarized.

### Changed
- Quick Open is an auto-height card with a contextual empty state, and clicking outside closes it.
- The Open folder and Properties buttons are left-aligned.

[Unreleased]: https://github.com/bryaneaton13/markdown-viewer/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/bryaneaton13/markdown-viewer/releases/tag/v0.1.0
