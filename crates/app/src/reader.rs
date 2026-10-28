use crate::{actions::*, files, preferences::Preferences};
use gpui_kit::{
    base::{
        TextView, TextViewState,
        text::{RangeHighlight, RenderedText},
    },
    component::{
        button::*,
        command::{Command, CommandGroup, CommandItem, CommandState},
        input::{Input, InputEvent, InputState},
        *,
    },
    prelude::FluentBuilder,
    *,
};
use markdown_core::{Document, Link};
use notify::Watcher;
use std::{collections::HashSet, ops::Range, path::PathBuf, time::Duration};

const TOOLBAR_HEIGHT: f32 = 44.;
const LOGO: &[u8] = include_bytes!("../../../assets/logo.png");
const WELCOME: &str = "# A little room to read.\n\nOpen a Markdown file or a folder. Your files stay exactly as they are.\n\n### Start here\n\n- **⌘O** — Open a Markdown file\n- **⇧⌘O** — Browse a folder\n- **⌘P** — Jump to a file\n- **⌘B** — Hide the sidebar and settle in\n\nTables, code, links, local images, and YAML frontmatter are supported. No accounts. No editing. Just your documents.\n\n> Tip: press **⌘/** for keyboard shortcuts and Finder setup.\n";
const HELP: &str = "# Keyboard shortcuts & Finder setup\n\n| Shortcut | Action |\n|---|---|\n| ⌘O / ⇧⌘O | Open Markdown / folder |\n| ⌘P | Quick Open |\n| ⌘B | Hide / show sidebar |\n| ⌘1 / ⌘2 | Focus tree / reader |\n| ↑ ↓ ← → | Navigate focused tree |\n| Space / Enter in tree | Focus reader / open selection |\n| ⌘F / ⌘G / ⇧⌘G | Find / next / previous |\n| ⌘[ / ⌘] | Back / forward |\n| ⇧⌘M | Read-only source / preview |\n| ⇧⌘I | Properties |\n| ⇧⌘L | Heading outline |\n| ⇧⌘D | Light / dark |\n| ⌘+ / ⌘− / ⌘0 | Text size |\n| ⌘R | Reload |\n| Escape | Dismiss picker, find, or help |\n\n## Default Markdown viewer\n\nInstall **Markdown Viewer.app** in Applications. In Finder, select a `.md` file → **Get Info** → **Open with** → **Markdown Viewer** → **Change All**. Repeat for `.markdown` if necessary.\n\n## Finder Quick Look\n\nThe app bundle includes **Markdown Preview.appex**. Enable it under System Settings → General → Login Items & Extensions → Quick Look if required by your macOS version. Select a Markdown file in Finder and press **Space**.\n\nThe extension renders text, lists, code, tables and properties offline. Adjacent images are represented by their alt text in Finder because the extension only receives access to the selected document.\n\n## Prototype scope\n\nOnly `.md` and `.markdown` files, UTF-8, up to 8 MiB. Folder search respects ignore files and hides hidden entries; capped at 20,000 Markdown paths / 32 levels. One document at a time with back/forward history. Mermaid stays visible as code. Remote images are not fetched.\n";

pub struct Reader {
    prefs: Preferences,
    focus: FocusHandle,
    tree_focus: FocusHandle,
    text: Entity<TextViewState>,
    find_input: Entity<InputState>,
    palette: Entity<CommandState>,
    pub path: Option<PathBuf>,
    document: Option<Document>,
    root: Option<PathBuf>,
    files: Vec<PathBuf>,
    rows: Vec<files::Row>,
    expanded: HashSet<PathBuf>,
    selected: usize,
    tree_scroll: UniformListScrollHandle,
    source: bool,
    properties: bool,
    help: bool,
    find: bool,
    picker: Option<bool>, // false: files, true: headings
    matches: Vec<Range<usize>>,
    match_index: usize,
    searched: Option<RenderedText>,
    pending_anchor: Option<String>,
    history: Vec<PathBuf>,
    history_index: usize,
    loading: bool,
    indexing: bool,
    limited: bool,
    error: Option<String>,
    generation: usize,
    folder_generation: usize,
    _load: Option<Task<()>>,
    _scan: Option<Task<()>>,
    _watch: Option<notify::RecommendedWatcher>,
    _watch_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Reader {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = Preferences::load();
        if let Some(dark) = prefs.dark {
            Theme::change(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                Some(window),
                cx,
            );
        }
        let text = cx.new(|cx| {
            TextViewState::markdown(WELCOME, cx)
                .selectable(true)
                .scrollable(true)
        });
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in document…"));
        let palette = cx.new(|cx| CommandState::new(window, cx));
        let observation = cx.observe(&text, |this, _, cx| {
            let snapshot = this.text.read(cx).rendered_text();
            if this.searched.as_ref() != Some(&snapshot) {
                this.searched = Some(snapshot);
                if this.find {
                    this.update_find(cx);
                }
                if let Some(anchor) = this.pending_anchor.take() {
                    this.jump_anchor(&anchor, cx);
                }
            }
            cx.notify();
        });
        let input_subscription = cx.subscribe(&find_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.match_index = 0;
                this.update_find(cx);
            }
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = Self {
            prefs,
            focus,
            tree_focus: cx.focus_handle(),
            text,
            find_input,
            palette,
            path: None,
            document: None,
            root: None,
            files: Vec::new(),
            rows: Vec::new(),
            expanded: HashSet::new(),
            selected: 0,
            tree_scroll: UniformListScrollHandle::new(),
            source: false,
            properties: false,
            help: false,
            find: false,
            picker: None,
            matches: Vec::new(),
            match_index: 0,
            searched: None,
            pending_anchor: None,
            history: Vec::new(),
            history_index: 0,
            loading: false,
            indexing: false,
            limited: false,
            error: None,
            generation: 0,
            folder_generation: 0,
            _load: None,
            _scan: None,
            _watch: None,
            _watch_task: None,
            _subscriptions: vec![observation, input_subscription],
        };
        if let Some(path) = path {
            this.open_path(path, cx);
        } else {
            if let Some(folder) = this.prefs.folder.clone().filter(|p| p.is_dir()) {
                this.open_folder(folder, cx);
            }
            if let Some(file) = this.prefs.file.clone().filter(|p| p.is_file()) {
                this.load_file(file, true, cx);
            }
        }
        this
    }

    pub fn open_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        if path.is_dir() {
            self.open_folder(path, cx);
        } else {
            self.load_file(path, true, cx);
        }
    }

    fn open_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.root = Some(path.clone());
        self.prefs.folder = Some(path);
        self.prefs.sidebar = true;
        self.expanded.clear();
        self.scan(cx);
        self.watch(cx);
        self.prefs.save();
    }

    fn scan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.folder_generation += 1;
        let generation = self.folder_generation;
        self.indexing = true;
        let task = cx
            .background_executor()
            .spawn(async move { files::discover(&root) });
        self._scan = Some(cx.spawn(async move |this, cx| {
            let (files, limited) = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.folder_generation == generation {
                    this.files = files;
                    this.limited = limited;
                    this.indexing = false;
                    this.rebuild_rows();
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    fn rebuild_rows(&mut self) {
        self.rows = self
            .root
            .as_ref()
            .map(|r| files::rows(r, &self.files, &self.expanded))
            .unwrap_or_default();
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    fn load_file(&mut self, path: PathBuf, history: bool, cx: &mut Context<Self>) {
        if !markdown_core::is_markdown(&path) {
            self.error = Some("Only .md and .markdown files can be opened.".into());
            cx.notify();
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        self.help = false;
        self.path = Some(path.clone());
        self.prefs.file = Some(path.clone());
        if history && self.history.get(self.history_index) != Some(&path) {
            if !self.history.is_empty() {
                self.history.truncate(self.history_index + 1);
            }
            self.history.push(path.clone());
            self.history_index = self.history.len() - 1;
        }
        let task = cx
            .background_executor()
            .spawn(async move { markdown_core::read_document(&path) });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation { return; }
                this.loading = false;
                match result {
                    Ok(doc) => {
                        // set_text preserves unchanged block identities and scroll on refresh.
                        this.document = Some(doc); this.present(cx); this.watch(cx); this.prefs.save();
                    },
                    Err(e) => { this.error = Some(e); this.document = None; this.text.update(cx, |s, cx| s.set_text("# Unable to read this file\n\nCheck that it still exists and is readable, then press **⌘R** to retry.", cx)); },
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn present(&mut self, cx: &mut Context<Self>) {
        let content = if self.help {
            HELP.to_owned()
        } else if let Some(doc) = &self.document {
            if self.source {
                doc.source_markdown()
            } else {
                doc.body.clone()
            }
        } else {
            WELCOME.to_owned()
        };
        self.text.update(cx, |s, cx| s.set_text(&content, cx));
        cx.notify();
    }

    fn watch(&mut self, cx: &mut Context<Self>) {
        let (tx, rx) = async_channel::bounded(1);
        let Ok(mut watcher) =
            notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                if event.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_))) {
                    let _ = tx.try_send(());
                }
            })
        else {
            return;
        };
        if let Some(root) = &self.root {
            let _ = watcher.watch(root, notify::RecursiveMode::Recursive);
        }
        if let Some(parent) = self.path.as_ref().and_then(|p| p.parent())
            && self.root.as_ref().is_none_or(|r| !parent.starts_with(r))
        {
            let _ = watcher.watch(parent, notify::RecursiveMode::NonRecursive);
        }
        self._watch = Some(watcher);
        self._watch_task = Some(cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                cx.background_executor()
                    .timer(Duration::from_millis(350))
                    .await;
                while rx.try_recv().is_ok() {}
                let _ = this.update(cx, |this, cx| {
                    this.scan(cx);
                    if let Some(path) = this.path.clone() {
                        this.load_file(path, false, cx);
                    }
                });
            }
        }));
    }

    fn prompt(&mut self, folder: bool, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some(
                if folder {
                    "Open Folder"
                } else {
                    "Open Markdown"
                }
                .into(),
            ),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| this.open_path(path, cx));
            }
        })
        .detach();
    }

    fn update_find(&mut self, cx: &mut Context<Self>) {
        let query = self.find_input.read(cx).value();
        let rendered = self.text.read(cx).rendered_text();
        self.matches = markdown_core::find_ranges(rendered.as_str(), &query);
        self.match_index = self.match_index.min(self.matches.len().saturating_sub(1));
        let ranges = self.matches.clone();
        let selected = self.match_index;
        self.text.update(cx, |state, cx| {
            let _ = state.set_range_highlights(
                ranges.iter().enumerate().map(|(i, r)| {
                    RangeHighlight::new(
                        r.clone(),
                        if i == selected {
                            rgba(0xf4b94299)
                        } else {
                            rgba(0xf4d57755)
                        },
                    )
                }),
                cx,
            );
            if let Some(range) = ranges.get(selected) {
                let _ = state.reveal_range(range.clone(), cx);
            }
        });
        cx.notify();
    }

    fn next_match(&mut self, backwards: bool, cx: &mut Context<Self>) {
        if !self.matches.is_empty() {
            self.match_index = if backwards {
                (self.match_index + self.matches.len() - 1) % self.matches.len()
            } else {
                (self.match_index + 1) % self.matches.len()
            };
            self.update_find(cx);
        }
    }

    fn jump_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) {
        let Some(doc) = &self.document else {
            return;
        };
        if let Some(heading) = doc.headings.iter().find(|h| h.id == anchor) {
            let snapshot = self.text.read(cx).rendered_text();
            if snapshot.source() != doc.body {
                self.pending_anchor = Some(anchor.to_owned());
                return;
            }
            if let Some(range) = snapshot.range_for_source(heading.range.clone()) {
                self.text.update(cx, |s, cx| {
                    let _ = s.reveal_range(range, cx);
                });
            }
        }
    }

    fn follow(&mut self, link: &str, cx: &mut Context<Self>) {
        let Some(path) = &self.path else {
            return;
        };
        match markdown_core::resolve_link(path, link) {
            Link::Local(p, anchor) if markdown_core::is_markdown(&p) => {
                if Some(&p) == self.path.as_ref() {
                    if let Some(anchor) = anchor {
                        self.jump_anchor(&anchor, cx);
                    }
                } else {
                    self.pending_anchor = anchor;
                    self.load_file(p, true, cx);
                }
            }
            Link::External(url) => cx.open_url(&url),
            _ => {
                self.error = Some("This reader opens Markdown links and web links only.".into());
                cx.notify();
            }
        }
    }

    fn navigate(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let next = if backwards {
            self.history_index.checked_sub(1)
        } else {
            Some(self.history_index + 1)
        };
        if let Some(index) = next.filter(|i| *i < self.history.len()) {
            self.history_index = index;
            self.load_file(self.history[index].clone(), false, cx);
        }
    }

    fn focus_reader(&self, window: &mut Window, cx: &mut App) {
        self.text.read(cx).focus_handle().clone().focus(window, cx);
    }
    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
        } else if self.find {
            self.find = false;
            self.text.update(cx, |s, cx| s.clear_range_highlights(cx));
        } else if self.help {
            self.help = false;
            self.present(cx);
        }
        self.focus_reader(window, cx);
        cx.notify();
    }
    fn open_picker(&mut self, outline: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = Some(outline);
        self.palette.update(cx, |s, cx| {
            s.set_query("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }
    fn tree_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rows.len() - 1);
        self.tree_scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        if !self.rows[self.selected].directory {
            self.load_file(self.rows[self.selected].path.clone(), true, cx);
        }
        cx.notify();
    }
    fn tree_open(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(self.selected).cloned() {
            if row.directory {
                if !self.expanded.remove(&row.path) {
                    self.expanded.insert(row.path);
                }
                self.rebuild_rows();
                cx.notify();
            } else {
                self.load_file(row.path, true, cx);
            }
        }
    }
    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.sidebar = !self.prefs.sidebar;
        if !self.prefs.sidebar && self.tree_focus.is_focused(window) {
            self.focus_reader(window, cx);
        }
        self.prefs.save();
        cx.notify();
    }
    fn zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.prefs.font_size = if delta == 0. {
            16.
        } else {
            (self.prefs.font_size + delta).clamp(12., 26.)
        };
        self.prefs.save();
        cx.notify();
    }

    fn toolbar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        // 44px toolbar doubles as the title bar; traffic lights are centered in it (main.rs).
        TitleBar::new()
            .h(px(TOOLBAR_HEIGHT))
            .bg(theme.background)
            .border_color(theme.border)
            .when(window.is_fullscreen(), |d| d.pl_2())
            .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .w_full()
                .pr_2()
                .child(
                    Button::new("sidebar")
                        .ghost()
                        .icon(IconName::PanelLeft)
                        .tooltip("Toggle sidebar · ⌘B")
                        .on_click(cx.listener(|v, _, w, cx| v.toggle_sidebar(w, cx))),
                )
                .child(
                    Button::new("back")
                        .ghost()
                        .icon(IconName::ChevronLeft)
                        .disabled(self.history_index == 0)
                        .tooltip("Back · ⌘[")
                        .on_click(cx.listener(|v, _, _, cx| v.navigate(true, cx))),
                )
                .child(
                    Button::new("forward")
                        .ghost()
                        .icon(IconName::ChevronRight)
                        .disabled(self.history_index + 1 >= self.history.len())
                        .tooltip("Forward · ⌘]")
                        .on_click(cx.listener(|v, _, _, cx| v.navigate(false, cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .overflow_hidden()
                        .child(
                            self.path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or("Markdown Viewer".into()),
                        ),
                )
                .child(
                    Button::new("quick-open")
                        .ghost()
                        .icon(IconName::Search)
                        .label("Quick Open")
                        .tooltip("Find a file · ⌘P")
                        .on_click(cx.listener(|v, _, w, cx| v.open_picker(false, w, cx))),
                )
                .child(
                    Button::new("outline")
                        .ghost()
                        .label("Outline")
                        .tooltip("Heading outline · ⇧⌘L")
                        .on_click(cx.listener(|v, _, w, cx| v.open_picker(true, w, cx))),
                )
                .child(
                    Button::new("source")
                        .ghost()
                        .label(if self.source { "Preview" } else { "Source" })
                        .tooltip("Read-only source · ⇧⌘M")
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.source = !v.source;
                            v.present(cx);
                        })),
                )
                .child(
                    Button::new("theme")
                        .ghost()
                        .icon(IconName::Sun)
                        .tooltip("Toggle theme · ⇧⌘D")
                        .on_click(
                            cx.listener(|_, _, w, cx| w.dispatch_action(Box::new(ToggleTheme), cx)),
                        ),
                )
                .child(
                    Button::new("help")
                        .ghost()
                        .icon(IconName::Info)
                        .tooltip("Shortcuts & Finder setup · ⌘/")
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.help = !v.help;
                            v.present(cx);
                        })),
                ),
        )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("folder-pane")
            .key_context("MarkdownTree")
            .track_focus(&self.tree_focus)
            .w(px(242.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .px_4()
                    .pt_5()
                    .pb_3()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child("WORKSPACE"),
            )
            .child(
                div().px_3().pb_3().child(
                    Button::new("open-folder")
                        .ghost()
                        .icon(IconName::FolderOpen)
                        .label(
                            self.root
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or("Open folder…".into()),
                        )
                        .on_click(cx.listener(|v, _, _, cx| v.prompt(true, cx))),
                ),
            )
            .child(
                uniform_list(
                    "file-tree",
                    self.rows.len(),
                    cx.processor(|this, range: Range<usize>, _, cx| {
                        range
                            .map(|index| {
                                let row = &this.rows[index];
                                let directory = row.directory;
                                let current = if directory {
                                    this.selected == index
                                } else {
                                    this.path.as_ref() == Some(&row.path)
                                };
                                div()
                                    .id(("file", index))
                                    .h(px(32.))
                                    .pl(px(12. + row.depth as f32 * 14.))
                                    .pr_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_sm()
                                    .cursor_pointer()
                                    .bg(if current {
                                        cx.theme().accent
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .child(
                                        Icon::new(if directory {
                                            if this.expanded.contains(&row.path) {
                                                IconName::ChevronDown
                                            } else {
                                                IconName::ChevronRight
                                            }
                                        } else {
                                            IconName::FileText
                                        })
                                        .size_4()
                                        .text_color(cx.theme().muted_foreground),
                                    )
                                    .child(
                                        div().overflow_hidden().child(
                                            row.path
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .into_owned(),
                                        ),
                                    )
                                    .on_click(cx.listener(move |v, _, w, cx| {
                                        v.selected = index;
                                        v.tree_focus.focus(w, cx);
                                        v.tree_open(cx);
                                    }))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.tree_scroll)
                .flex_1(),
            )
            .when(self.rows.is_empty(), |d| {
                d.child(
                    div()
                        .px_4()
                        .py_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.indexing {
                            "Finding Markdown files…"
                        } else {
                            "Open a folder to browse its Markdown files."
                        }),
                )
            })
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.indexing {
                        "Indexing…".to_owned()
                    } else {
                        format!(
                            "{} Markdown files{}",
                            self.files.len(),
                            if self.limited {
                                " · limit reached"
                            } else {
                                ""
                            }
                        )
                    }),
            )
    }

    fn picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let outline = self.picker == Some(true);
        let items: Vec<CommandItem> = if outline {
            self.document
                .as_ref()
                .map(|d| {
                    d.headings
                        .iter()
                        .enumerate()
                        .map(|(index, h)| {
                            CommandItem::new()
                                .label(h.text.clone())
                                .action(Box::new(JumpHeading { index }))
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            self.files
                .iter()
                .map(|path| {
                    CommandItem::new()
                        .label(
                            path.strip_prefix(
                                self.root.as_deref().unwrap_or(std::path::Path::new("/")),
                            )
                            .unwrap_or(path)
                            .to_string_lossy()
                            .into_owned(),
                        )
                        .action(Box::new(OpenPath { path: path.clone() }))
                })
                .collect()
        };
        let weak = cx.entity().downgrade();
        let cancel = weak.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .pt(px(80.))
            .bg(rgba(0x00000035))
            .child(
                div()
                    .w(px(570.))
                    .h(px(420.))
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .shadow_lg()
                    .child(
                        Command::new(&self.palette)
                            .placeholder(if outline {
                                "Jump to heading…"
                            } else {
                                "Find a Markdown file…"
                            })
                            .max_h(px(360.))
                            .group(
                                CommandGroup::new()
                                    .label(if outline { "HEADINGS" } else { "FILES" })
                                    .items(items),
                            )
                            .on_confirm(move |_, w, cx| {
                                let _ = weak.update(cx, |v, cx| v.close_overlay(w, cx));
                            })
                            .on_cancel(move |w, cx| {
                                let _ = cancel.update(cx, |v, cx| v.close_overlay(w, cx));
                            })
                            .footer(|_, _, cx| {
                                div()
                                    .px_3()
                                    .py_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("↑ ↓ Navigate    ↵ Open    Esc Close")
                            }),
                    ),
            )
    }

    #[cfg(feature = "screenshot")]
    pub fn ready(&self, cx: &App) -> bool {
        !self.loading && !self.indexing && !self.text.read(cx).rendered_text().is_empty()
    }
    #[cfg(feature = "screenshot")]
    pub fn debug_actions(&mut self, actions: &str, window: &mut Window, cx: &mut Context<Self>) {
        for action in actions.split(',') {
            match action {
                "dark" => Theme::change(ThemeMode::Dark, Some(window), cx),
                "light" => Theme::change(ThemeMode::Light, Some(window), cx),
                "nosidebar" => self.prefs.sidebar = false,
                "browse" => {
                    if let Some(parent) = self
                        .path
                        .as_ref()
                        .and_then(|p| p.parent())
                        .map(|p| p.to_path_buf())
                    {
                        self.open_folder(parent, cx);
                    }
                }
                "properties" => self.properties = true,
                "source" => {
                    self.source = true;
                    self.present(cx);
                }
                "help" => {
                    self.help = true;
                    self.present(cx);
                }
                "quick-open" => self.open_picker(false, window, cx),
                "outline" => self.open_picker(true, window, cx),
                a if a.starts_with("find:") => {
                    self.find = true;
                    self.find_input
                        .update(cx, |s, cx| s.set_value(&a[5..], window, cx));
                    self.update_find(cx);
                }
                _ => {}
            }
        }
        cx.notify();
    }
}

impl Render for Reader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let image_base = self.path.clone();
        let reader = TextView::new(&self.text)
            .scrollable(true)
            .size_full()
            .image_source(move |uri| {
                if let Some(base) = &image_base
                    && let Link::Local(path, _) = markdown_core::resolve_link(base, uri.as_ref())
                {
                    return ImageSource::from(path);
                }
                ImageSource::from(PathBuf::from("/nonexistent/remote-images-disabled"))
            })
            .on_link_click(move |url, _, _, cx| {
                let _ = weak.update(cx, |v, cx| v.follow(url, cx));
            });
        div()
            .id("viewer")
            .key_context("Viewer")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|v, _: &OpenFile, _, cx| v.prompt(false, cx)))
            .on_action(cx.listener(|v, _: &OpenFolder, _, cx| v.prompt(true, cx)))
            .on_action(cx.listener(|v, _: &ToggleSidebar, w, cx| v.toggle_sidebar(w, cx)))
            .on_action(cx.listener(|v, _: &FocusSidebar, w, cx| {
                v.prefs.sidebar = true;
                v.tree_focus.focus(w, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &FocusReader, w, cx| v.focus_reader(w, cx)))
            .on_action(cx.listener(|v, _: &QuickOpen, w, cx| v.open_picker(false, w, cx)))
            .on_action(cx.listener(|v, _: &Outline, w, cx| v.open_picker(true, w, cx)))
            .on_action(cx.listener(|v, a: &OpenPath, _, cx| {
                v.picker = None;
                v.load_file(a.path.clone(), true, cx);
            }))
            .on_action(cx.listener(|v, a: &JumpHeading, _, cx| {
                if let Some(id) = v
                    .document
                    .as_ref()
                    .and_then(|d| d.headings.get(a.index))
                    .map(|h| h.id.clone())
                {
                    v.source = false;
                    v.help = false;
                    v.present(cx);
                    v.jump_anchor(&id, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &Find, w, cx| {
                v.find = true;
                v.find_input.update(cx, |s, cx| s.focus(w, cx));
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &NextMatch, _, cx| v.next_match(false, cx)))
            .on_action(cx.listener(|v, _: &PrevMatch, _, cx| v.next_match(true, cx)))
            .on_action(cx.listener(|v, _: &Escape, w, cx| v.close_overlay(w, cx)))
            .on_action(cx.listener(|v, _: &ToggleSource, _, cx| {
                v.source = !v.source;
                v.present(cx);
            }))
            .on_action(cx.listener(|v, _: &ToggleProperties, _, cx| {
                v.properties = !v.properties;
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &ToggleTheme, w, cx| {
                let dark = !cx.theme().mode.is_dark();
                v.prefs.dark = Some(dark);
                Theme::change(
                    if dark {
                        ThemeMode::Dark
                    } else {
                        ThemeMode::Light
                    },
                    Some(w),
                    cx,
                );
                v.prefs.save();
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &Larger, _, cx| v.zoom(1., cx)))
            .on_action(cx.listener(|v, _: &Smaller, _, cx| v.zoom(-1., cx)))
            .on_action(cx.listener(|v, _: &ResetSize, _, cx| v.zoom(0., cx)))
            .on_action(cx.listener(|v, _: &Back, _, cx| v.navigate(true, cx)))
            .on_action(cx.listener(|v, _: &Forward, _, cx| v.navigate(false, cx)))
            .on_action(cx.listener(|v, _: &Reload, _, cx| {
                if let Some(p) = v.path.clone() {
                    v.load_file(p, false, cx);
                }
                v.scan(cx);
            }))
            .on_action(cx.listener(|_, _: &CloseWindow, w, _| w.remove_window()))
            .on_action(cx.listener(|_, _: &FullScreen, w, _| w.toggle_fullscreen()))
            .on_action(cx.listener(|v, _: &Help, _, cx| {
                v.help = !v.help;
                v.present(cx);
            }))
            .on_action(cx.listener(|v, _: &TreeUp, _, cx| v.tree_move(-1, cx)))
            .on_action(cx.listener(|v, _: &TreeDown, _, cx| v.tree_move(1, cx)))
            .on_action(cx.listener(|v, _: &TreeOpen, _, cx| v.tree_open(cx)))
            .on_action(cx.listener(|v, _: &TreeLeft, _, cx| {
                if let Some(row) = v.rows.get(v.selected) {
                    if v.expanded.remove(&row.path) {
                        v.rebuild_rows();
                    } else if let Some(parent) = row.path.parent()
                        && let Some(index) = v.rows.iter().position(|r| r.path == parent)
                    {
                        v.selected = index;
                    }
                }
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &TreeRight, _, cx| {
                if let Some(row) = v.rows.get(v.selected) {
                    if row.directory && !v.expanded.contains(&row.path) {
                        v.tree_open(cx);
                    } else {
                        v.tree_move(1, cx);
                    }
                }
            }))
            .child(self.toolbar(window, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(self.prefs.sidebar, |d| d.child(self.sidebar(cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .when(self.find, |d| {
                                d.child(
                                    div()
                                        .px_4()
                                        .py_2()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .border_b_1()
                                        .border_color(cx.theme().border)
                                        .child(Input::new(&self.find_input).w(px(300.)))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!(
                                                    "{} / {}",
                                                    if self.matches.is_empty() {
                                                        0
                                                    } else {
                                                        self.match_index + 1
                                                    },
                                                    self.matches.len()
                                                )),
                                        )
                                        .child(
                                            Button::new("find-prev")
                                                .ghost()
                                                .icon(IconName::ChevronUp)
                                                .on_click(cx.listener(|v, _, _, cx| {
                                                    v.next_match(true, cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("find-next")
                                                .ghost()
                                                .icon(IconName::ChevronDown)
                                                .on_click(cx.listener(|v, _, _, cx| {
                                                    v.next_match(false, cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("close-find").ghost().label("×").on_click(
                                                cx.listener(|v, _, w, cx| v.close_overlay(w, cx)),
                                            ),
                                        ),
                                )
                            })
                            .when_some(self.error.clone(), |d, error| {
                                d.child(
                                    div()
                                        .px_5()
                                        .py_2()
                                        .text_sm()
                                        .bg(cx.theme().muted)
                                        .child(error),
                                )
                            })
                            .when_some(
                                self.document
                                    .as_ref()
                                    .and_then(|d| d.metadata_error.clone()),
                                |d, error| {
                                    d.child(
                                        div()
                                            .px_5()
                                            .py_2()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(error),
                                    )
                                },
                            )
                            .when(
                                self.document.as_ref().is_some_and(|d| d.metadata.is_some())
                                    && !self.source
                                    && !self.help,
                                |d| {
                                    d.child(
                                        div()
                                            .px_5()
                                            .pt_2()
                                            .child(
                                                Button::new("properties")
                                                    .ghost()
                                                    .icon(if self.properties {
                                                        IconName::ChevronDown
                                                    } else {
                                                        IconName::ChevronRight
                                                    })
                                                    .label("Properties")
                                                    .on_click(cx.listener(|v, _, _, cx| {
                                                        v.properties = !v.properties;
                                                        cx.notify();
                                                    })),
                                            )
                                            .when(self.properties, |d| {
                                                d.child(
                                                    div()
                                                        .id("metadata")
                                                        .max_h(px(160.))
                                                        .overflow_y_scroll()
                                                        .px_3()
                                                        .py_2()
                                                        .text_sm()
                                                        .font_family("Menlo")
                                                        .child(
                                                            self.document
                                                                .as_ref()
                                                                .and_then(|d| d.metadata.clone())
                                                                .unwrap_or_default(),
                                                        ),
                                                )
                                            }),
                                    )
                                },
                            )
                            .when(self.path.is_none() && !self.source && !self.help, |d| {
                                d.child(
                                    div().px(px(32.)).pt(px(24.)).child(
                                        img(std::sync::Arc::new(Image::from_bytes(
                                            ImageFormat::Png,
                                            LOGO.to_vec(),
                                        )))
                                        .size(px(72.)),
                                    ),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .px(px(32.))
                                    .py(px(24.))
                                    .text_size(px(self.prefs.font_size))
                                    .line_height(relative(1.65))
                                    .child(if self.loading {
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Opening document…")
                                            .into_any_element()
                                    } else {
                                        reader.into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .px_5()
                                    .h(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if self.source {
                                        "SOURCE · READ ONLY"
                                    } else {
                                        "MARKDOWN · READ ONLY"
                                    })
                                    .child(format!(
                                        "{}   ·   {} pt",
                                        self.document
                                            .as_ref()
                                            .map(|d| format!(
                                                "{} words",
                                                d.body.split_whitespace().count()
                                            ))
                                            .unwrap_or("Local files".into()),
                                        self.prefs.font_size
                                    )),
                            ),
                    )
                    .when(self.picker.is_some(), |d| d.child(self.picker(cx))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::{base::Root, test::TestWindowExt};

    fn open(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Reader>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::actions::bind(cx);
            let (w, v) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1120.), px(820.)),
                    })),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Reader::new(None, w, cx)),
            )
            .unwrap();
            (w.downcast::<Root>().unwrap(), v)
        })
    }
    fn interact(
        w: WindowHandle<Root>,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut Window, &mut App),
    ) {
        cx.update_window(w.into(), |_, w, cx| {
            w.render_frame(cx);
            f(w, cx);
            w.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    #[gpui_kit::test]
    fn sidebar_shortcuts_keep_reader_focus_and_source_is_read_only(cx: &mut TestAppContext) {
        let (w, view) = open(cx);
        interact(w, cx, |w, cx| {
            w.press("cmd-1", cx);
            w.press("cmd-b", cx);
        });
        assert!(!view.read_with(cx, |v, _| v.prefs.sidebar));
        interact(w, cx, |w, cx| w.press("cmd-b", cx));
        assert!(view.read_with(cx, |v, _| v.prefs.sidebar));
        view.update(cx, |v, cx| {
            v.document = Some(Document::parse("# Original\n- [ ] untouched".into()));
            v.present(cx);
        });
        cx.run_until_parked();
        interact(w, cx, |w, cx| {
            w.click("source", cx);
            w.press("cmd-2", cx);
            w.input("must not edit", cx);
        });
        assert!(view.read_with(cx, |v, _| v.source));
        assert_eq!(
            view.read_with(cx, |v, _| v.document.as_ref().unwrap().source.clone()),
            "# Original\n- [ ] untouched"
        );
    }
    #[gpui_kit::test]
    fn find_matches_rendered_text_and_escape_dismisses(cx: &mut TestAppContext) {
        let (w, view) = open(cx);
        view.update(cx, |v, cx| {
            v.document = Some(Document::parse(
                "# Hello **world**\n\nAnother WORLD.".into(),
            ));
            v.present(cx);
        });
        cx.run_until_parked();
        interact(w, cx, |w, cx| {
            w.press("cmd-f", cx);
            w.render_frame(cx);
            w.input("world", cx);
        });
        assert_eq!(view.read_with(cx, |v, _| v.matches.len()), 2);
        interact(w, cx, |w, cx| w.press("cmd-g", cx));
        assert_eq!(view.read_with(cx, |v, _| v.match_index), 1);
        interact(w, cx, |w, cx| w.press("escape", cx));
        assert!(!view.read_with(cx, |v, _| v.find));
    }
    #[gpui_kit::test]
    fn quick_open_filters_and_opens_with_keyboard(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("Alpha.md");
        let second = tmp.path().join("Beta.md");
        std::fs::write(&first, "# Alpha").unwrap();
        std::fs::write(&second, "# Beta").unwrap();
        let (w, view) = open(cx);
        view.update(cx, |v, _| {
            v.root = Some(tmp.path().to_owned());
            v.files = vec![first, second.clone()];
            v.rebuild_rows();
        });
        interact(w, cx, |w, cx| {
            w.press("cmd-p", cx);
            w.render_frame(cx);
            w.input("Beta", cx);
        });
        interact(w, cx, |w, cx| w.press("enter", cx));
        assert_eq!(view.read_with(cx, |v, _| v.path.clone()), Some(second));
        assert!(view.read_with(cx, |v, _| v.picker.is_none()));
    }

    #[gpui_kit::test]
    fn rapid_navigation_keeps_latest_file_and_handles_deletion(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("First.md");
        let second = tmp.path().join("Second.md");
        std::fs::write(&first, "# First").unwrap();
        std::fs::write(&second, "# Second\n- [ ] untouched").unwrap();
        let (_, view) = open(cx);
        view.update(cx, |v, cx| {
            v.load_file(first.clone(), true, cx);
            v.load_file(second.clone(), true, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |v, _| v.document.as_ref().map(|d| d.body.clone())),
            Some("# Second\n- [ ] untouched".into())
        );
        view.update(cx, |v, cx| v.navigate(true, cx));
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |v, _| v.path.clone()),
            Some(first.clone())
        );
        std::fs::remove_file(&first).unwrap();
        view.update(cx, |v, cx| v.load_file(first, false, cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| v.error.is_some()));
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            "# Second\n- [ ] untouched"
        );
    }
}
