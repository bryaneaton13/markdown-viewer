# Changelog

All notable changes to Markdown Viewer are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/) (`0.MINOR.PATCH` until 1.0).

## [Unreleased]

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
