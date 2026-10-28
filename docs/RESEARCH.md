# macOS Markdown viewer: research and findings

Research date: 2026-10-27. Companion: [recommended application plan](PLAN.md).

## Recommendation

**Try Glim against your actual Markdown library before committing to a new app.** It already uses the requested stack and implements much more of the requested behavior than a superficial reading of its description suggests. If it feels good enough, use it and propose small improvements upstream. If a deliberately read-only, distraction-free reader is the goal, build a small GPUI Kit application and selectively adapt Glim's integration patterns rather than forking its entire multi-format workspace.

A full fork is the least attractive default: it inherits editing, autosave, many document formats, native bridges, updates, and vendored framework patches. Those costs are justified only if you want Glim's broader product.

## Evidence and limits

- The destination project directory was empty when inspected; there is no existing implementation to preserve or extend.
- Inspected Glim source at commit `79f734b5e3125416faae06d7ce3fd2d584386e4e`; its workspace manifest reports version `0.2.12`. This is a source snapshot, not a claim about the latest installed release.
- Read upstream GPUI Kit documentation, Glim's source/manifests/documentation, and Apple's default-app instructions.
- **No Glim binary was installed or run.** Rendering quality, startup latency, memory use, shortcut ergonomics, and OS integration remain runtime evaluation items. Upstream performance claims are not measurements from this research.

## 1. The stack is viable—and simpler than two independent frameworks

[GPUI](https://www.gpui.rs/) supplies Rust entities, windowing, actions, focus, layout, and GPU rendering. [GPUI Kit](https://github.com/longbridge/gpui-kit) supplies reusable behavior and styled components on top.

The current Kit documentation describes:

```text
gpui-kit
  ├── gpui-base       behavior, state, rich text
  └── gpui-component  styled controls
       └── matching pinned GPUI snapshot
```

Use `gpui-kit` as the app dependency and its GPUI re-exports. Do not independently add Zed's latest Git GPUI dependency: different GPUI versions can produce incompatible types and duplicated framework dependencies.

Glim pins `gpui-kit = "=0.7.1"`. Its vendored `gpui-base` 0.7.1 depends on `gpui-pre = "=0.3.8"`. Treat that as an observed compatible baseline, not a promise that an unmodified application needs Glim's patches. Pin the chosen Kit version and commit Cargo.lock after a clean spike.

Kit already has native Markdown rendering, `TextView`/`TextViewState`, virtualized content infrastructure, selectable text behavior, themes, and accessibility primitives. Start there rather than writing a Markdown layout engine. Initialize Kit before mounting components and follow its documented root/window entry point.

Optional GPUI Fast and JavaScript extension infrastructure exist, but neither is needed for this app's first release.

## 2. What Glim already implements

| Requested capability | Evidence in Glim | Remaining evaluation |
| --- | --- | --- |
| GPUI + GPUI Kit | Root Cargo.toml pins Kit 0.7.1 | Clean build, patch necessity |
| Folder sidebar | Lazy explorer, expansion state, watchers, multiple roots | Markdown-only browsing ergonomics |
| Sidebar shortcut | `ToggleSidebar`, `secondary-b` binding in app/mod.rs | Cmd+B on macOS; collapsed state retains an icon rail |
| Fast file access | Cmd+O, Cmd+Shift+O, filename/path search, tabs, navigation history | Preview-slot behavior and latency |
| Markdown | Native TextView, tables, task lists, links, images | Selection/find behavior on representative files |
| Frontmatter | Format guide advertises it; underlying parser has frontmatter extension/tests | Exact syntax, collapsed metadata UX, malformed input |
| Mermaid | Bundled engine and isolated rendering helper | Diagram fidelity, dark mode, large diagrams |
| Default file viewer | Viewer bundle registration and native association settings | Cold/warm Finder opens on target macOS |
| External changes | Coalesced watchers, obsolete-result suppression | Atomic editor saves, deletion, rename behavior |
| Read-only product | Not its general model: text editing and autosave exist | Whether that mismatch is acceptable |
| Resume reading | Recent folders; docs explicitly limit full tab/scroll restoration | Dedicated reader likely needs stronger restoration |
| Finder Quick Look provider | No provider established by this inspection | Separate extension needed; embedded Quick Look is not one |

Relevant source areas, relative to the [pinned Glim tree](https://github.com/glimapp/glim/tree/79f734b5e3125416faae06d7ce3fd2d584386e4e):

- `crates/glim-app/src/app/mod.rs`: keyboard bindings and `application.on_open_urls`.
- `crates/glim-app/src/views/explorer.rs`: directory browsing.
- `crates/glim-app/src/views/reader/markup.rs`: native TextView integration.
- `crates/glim-services/src/markdown_preview.rs`: preview parsing, resource preparation, diagram handling.
- `crates/glim-app/src/app/diagram_engine`: diagram engine integration; inspect exact module layout when adapting.
- `scripts/markdown/mermaid-engine.js`: Mermaid initialization and SVG generation.
- `assets/macos/Info.plist`: Viewer role and file associations.
- `native/Preview.swift`: association UI and Launch Services calls.
- `docs/architecture.md`: task lifetimes, source/rendered state, caching, native boundaries.

### Important implementation discoveries

**Markdown is native; Mermaid is not a Rust-only renderer.** Glim uses native TextView for prose. Bundled JavaScript engines run in an isolated WebKit-based helper, generate SVG, and the service rasterizes bounded output for display. Node, Java, and a hosted renderer are not required on the user's machine. This is a useful design to reuse.

**Frontmatter needs explicit verification.** The vendored Markdown parser includes a test named `yaml_frontmatter_is_disabled_by_default` and an extension-enabled test for non-mapping YAML falling back to code. Do not assume creating a Markdown TextView automatically produces the desired metadata panel.

**Glim patches the toolkit.** Its root manifest overrides `gpui-pre` and `gpui-base` with vendored copies for frame recovery, visible-only measurement, and task-checkbox callbacks. A small reader should first try stock Kit and add only demonstrated necessary fixes. Task-checkbox mutation is unnecessary in a read-only viewer.

**Native-looking does not imply fully validated accessibility.** Glim's accessibility statement explicitly says full VoiceOver, keyboard-only navigation, high contrast, and reduced-motion coverage have not been validated. Kit's AccessKit support is a foundation, not an app acceptance test.

**Distribution is still a consideration.** Glim's README says downloads are ad-hoc signed and not notarized. Its broad format support includes network-capable HTML previews and optional telemetry. A focused local reader can have a simpler runtime and release policy.

## 3. Frontmatter recommendation

Ship YAML frontmatter in the first useful release. Recognize an opening `---` delimiter only at the beginning of the document, allowing a UTF-8 BOM; preserve original bytes and offsets. A later horizontal rule must remain Markdown.

Present valid mapping metadata in a collapsed “Properties” section, with expandable nested values and a raw view. Do not replace the filename identity with the `title` value or render an extra title if the body already has one. Invalid metadata must never make the body unreadable or silently disappear.

TOML `+++` frontmatter is a sensible follow-up, but should be advertised only after testing. Use a parser for values, not ad hoc splitting on colons. `serde_yaml_ng` is used by Glim; confirm maintenance and desired YAML semantics before choosing the app's parser. No template expansion or tag-triggered execution is needed.

## 4. Mermaid recommendation

Include Mermaid after the basic reader works. Start with flowchart and sequence diagrams as the acceptance corpus; add class, state, and ER examples next. Do not promise complete parity with every Mermaid release.

Adapt Glim's offline-helper approach: pinned bundled Mermaid, strict mode, HTML labels disabled where necessary for pure SVG, bounded render jobs, cancellation, and a cache. Keep source available for every failure. A WebKit helper is additional engineering even though it uses the system WebKit runtime.

Glim's inspected JavaScript package manifest pins Mermaid `12.1.0`. Revalidate that version when implementing; keep engine version in cache keys and preserve its third-party notices. Avoid a runtime dependency on `mmdc`, Node, a remote diagram API, or downloading JS from a CDN.

## 5. Quick viewing means three distinct features

1. **Quick Open inside the app:** Cmd+P searches filenames/paths and opens immediately. This belongs in MVP.
2. **Transient in-app preview:** single selection replaces one preview slot; Enter/double-click pins it. This prevents tab clutter while browsing folders and also belongs in MVP.
3. **Finder Space-bar Quick Look:** requires a separately packaged Quick Look preview extension. Becoming the default `.md` app does not make Finder use its renderer for Quick Look. Schedule this as an optional later milestone, likely using Swift/AppKit and a lightweight HTML preview rather than embedding a complete GPUI window in the extension.

## 6. Making it the default `.md` viewer

An app bundle needs a stable bundle ID and Markdown document declarations with the **Viewer** role. Glim uses `CFBundleDocumentTypes` and `LSHandlerRank=Alternate`; registration alone does not make it the user's default.

Apple's reliable manual path is: select a `.md` file in Finder → Get Info → Open with → choose the installed app → Change All. Test `.markdown` separately because type resolution can vary with installed applications.

An in-app “Make default for Markdown” action can use the supported NSWorkspace content-type API where available, with availability checks. Glim demonstrates a Launch Services path using `LSSetDefaultRoleHandlerForContentType`; verify the deployment-target API choice rather than blindly copying it. Resolve the actual Markdown content type(s), report completion/errors, and never set the default handler for all plain text.

Handle Finder/Dock Open Documents events on both cold and warm launch, queue requests received before the first window exists, and deduplicate files. CLI argument handling alone is insufficient.

## 7. Build/use/fork decision

| Route | Best reason to choose it | Cost / mismatch |
| --- | --- | --- |
| Use Glim unchanged | Get a capable reader immediately | Broader UI, editing/autosave, unmeasured reading feel |
| Improve Glim upstream | A few small changes would solve the need | Upstream roadmap and review timing |
| Full fork | Want most of its workspace and formats | Ongoing merge, patch, packaging, and dependency maintenance |
| Small custom Kit app | Want a focused read-only product and own UX | Must implement navigation, state, find, packaging, and diagram bridge |

**Decision rule:** spend a time-boxed half-day evaluating Glim with real files. Use it if Markdown browsing, search, selection, associations, and diagrams satisfy the workflow. If the failures are two or three localized UI gaps, prefer upstream changes. Build the focused app when the central product model—not just styling—is the mismatch. The companion plan describes that focused app without making a full fork a prerequisite.

Suggested evaluation corpus: a real README, a long generated plan, YAML metadata with nested values, malformed frontmatter, local images and relative links, wide tables, Unicode filenames, flowchart/sequence diagrams, and a folder with 10,000 entries. Exercise read-only expectations explicitly: a preview task checkbox in Glim can be wired to source editing/autosave.

## Sources

- [GPUI](https://www.gpui.rs/): rendering framework and key-dispatch documentation links.
- [GPUI Kit README](https://github.com/longbridge/gpui-kit): dependency layering, initialization, rich content, examples, licensing.
- [Glim pinned source](https://github.com/glimapp/glim/tree/79f734b5e3125416faae06d7ce3fd2d584386e4e): manifests and implementation inspected above.
- [Glim architecture](https://github.com/glimapp/glim/blob/79f734b5e3125416faae06d7ce3fd2d584386e4e/docs/architecture.md).
- [Glim format support](https://github.com/glimapp/glim/blob/79f734b5e3125416faae06d7ce3fd2d584386e4e/docs/file-format-support.md).
- [Glim user guide](https://github.com/glimapp/glim/blob/79f734b5e3125416faae06d7ce3fd2d584386e4e/docs/user-guide.md).
- [Glim accessibility statement](https://github.com/glimapp/glim/blob/79f734b5e3125416faae06d7ce3fd2d584386e4e/ACCESSIBILITY.md).
- [Apple: choose the app used to open files](https://support.apple.com/guide/mac-help/choose-an-app-to-open-a-file-on-mac-mh35597/mac).
- [Apple: QLPreviewingController](https://developer.apple.com/documentation/quicklook/qlpreviewingcontroller): API reference for the optional extension; dynamically rendered documentation was not extractable in this session, so extension details require SDK verification.

Glim is MIT-licensed; GPUI Kit source is Apache-2.0. Preserve applicable copyright/license notices for copied code and separately inventory bundled engines, icons, and dependencies.
