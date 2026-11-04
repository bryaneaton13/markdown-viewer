//! One document view. Each tab holds one or two of these. A pane owns its
//! parsed text, back / forward history, find state and Source / Properties
//! toggles, so switching tabs never re-parses and two panes never share state.

use crate::preferences::SavedPane;
use gpui_kit::{
    base::{
        TextView, TextViewState,
        text::{RangeHighlight, RenderedText},
    },
    component::{
        button::*,
        input::{Input, InputEvent, InputState},
        *,
    },
    prelude::FluentBuilder,
    *,
};
use markdown_core::{Document, Link, Stats};
use std::{ops::Range, path::PathBuf};

const LOGO: &[u8] = include_bytes!("../../../assets/logo.png");
pub const WELCOME: &str = "# A little room to read.\n\nOpen a Markdown file or a folder. Your files stay exactly as they are.\n\n### Start here\n\n- **⌘O** — Open a Markdown file\n- **⇧⌘O** — Add a folder as a workspace\n- **⌘P** — Jump to a file (**⌘↵** opens it in a new tab, **⌥↵** beside this one)\n- **⌘T** — New tab · **⌘\\\\** — Split side by side\n- **⌘B** — Hide the sidebar and settle in\n\nTables, code, links, local images, and YAML frontmatter are supported. No accounts. No editing. Just your documents.\n\n> Tip: press **⌘/** for keyboard shortcuts and Finder setup.\n";
pub const HELP: &str = "# Keyboard shortcuts & Finder setup\n\n| Shortcut | Action |\n|---|---|\n| ⌘O / ⇧⌘O | Open Markdown / add folder workspace |\n| ⌘P / ⌘K | Quick Open (⌘↵ new tab, ⌥↵ split) |\n| ⌘T / ⌘W / ⇧⌘T | New tab / close pane or tab / reopen closed |\n| ⌃Tab / ⌃⇧Tab, ⇧⌘] / ⇧⌘[ | Next / previous tab |\n| ⌘\\\\ | Split side by side / unsplit |\n| ⌥⌘← / ⌥⌘→ | Focus left / right pane |\n| ⌃1 … ⌃9 | Switch workspace (in sidebar order) |\n| Click / double-click in tree | Preview (italic tab, reused) / keep open |\n| ⌘-click / ⌥-click in tree | Open in new tab / in the other pane |\n| ⌘B | Hide / show sidebar |\n| ⌘1 / ⌘2 | Focus tree / reader |\n| ↑ ↓ ← → | Navigate focused tree; ← / → close / open folders |\n| Space / Enter in tree | Focus reader / open selection |\n| ⌘F / ⌘G / ⇧⌘G | Find / next / previous |\n| ⌘[ / ⌘] | Back / forward |\n| ⇧⌘M | Read-only source / preview |\n| ⇧⌘I | Properties |\n| ⇧⌘L | Heading outline |\n| ⇧⌘D | Light / dark (palette button: system & themes) |\n| ⌘+ / ⌘− / ⌘0 | Text size |\n| ⌘R | Reload |\n| ⇧⌘W | Close window |\n| Escape | Dismiss picker, find, or help |\n\nA single click in the sidebar previews a file in a reused tab with an italic title; double-click the file or its tab to keep it open. In the sidebar, click a workspace to open or close it without switching; drag workspaces to reorder them. Drag a tab onto the left or right half of the page to read it side by side. Drag a pane's header back to the tab bar to give it its own tab.\n\n## Default Markdown viewer\n\nInstall **Markdown Viewer.app** in Applications. In Finder, select a `.md` file → **Get Info** → **Open with** → **Markdown Viewer** → **Change All**. Repeat for `.markdown` if necessary.\n\n## Finder Quick Look\n\nThe app bundle includes **Markdown Preview.appex**. Enable it under System Settings → General → Login Items & Extensions → Quick Look if required by your macOS version. Select a Markdown file in Finder and press **Space**.\n\nThe extension renders text, lists, code, tables and properties offline. Adjacent images are represented by their alt text in Finder because the extension only receives access to the selected document.\n\n## Scope\n\nOnly `.md` and `.markdown` files, UTF-8, up to 8 MiB. Folder search respects ignore files and hides hidden entries; capped at 20,000 Markdown paths / 32 levels per workspace. A tab shows one document, or two side by side. Mermaid stays visible as code. Remote images are not fetched.\n";

/// Shared reading size, set by the window from preferences.
pub struct FontSize(pub f32);
impl Global for FontSize {}

/// Something about the pane that the window shows (title, history, tabs).
pub enum PaneEvent {
    Changed,
}

pub struct DocPane {
    /// Suffix that keeps this pane's element ids apart from other panes'.
    pub id: u64,
    pub text: Entity<TextViewState>,
    find_input: Entity<InputState>,
    pub path: Option<PathBuf>,
    pub document: Option<Document>,
    /// What the document is made of; computed with it, off the main thread.
    pub stats: Option<Stats>,
    pub source: bool,
    pub properties: bool,
    pub help: bool,
    pub copied: bool,
    pub find: bool,
    pub matches: Vec<Range<usize>>,
    pub match_index: usize,
    searched: Option<RenderedText>,
    pending_anchor: Option<String>,
    history: Vec<PathBuf>,
    history_index: usize,
    pub loading: bool,
    pub error: Option<String>,
    /// The shown text opens with a level-one heading, which gets more room above it.
    leads_with_h1: bool,
    /// Restored from settings but not read yet; loads when first shown.
    dormant: bool,
    generation: usize,
    _load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PaneEvent> for DocPane {}

impl DocPane {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| {
            TextViewState::markdown(WELCOME, cx)
                .selectable(true)
                .scrollable(true)
        });
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in document…"));
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
        Self {
            id: crate::tabs::next_id(),
            text,
            find_input,
            path: None,
            document: None,
            stats: None,
            source: false,
            properties: false,
            help: false,
            copied: false,
            find: false,
            matches: Vec::new(),
            match_index: 0,
            searched: None,
            pending_anchor: None,
            history: Vec::new(),
            history_index: 0,
            loading: false,
            error: None,
            leads_with_h1: true,
            dormant: false,
            generation: 0,
            _load: None,
            _subscriptions: vec![observation, input_subscription],
        }
    }

    /// A pane restored from settings. It reads its file the first time it
    /// is shown, so a long list of saved tabs doesn't slow down launch.
    pub fn restore(saved: &SavedPane, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut pane = Self::new(window, cx);
        pane.source = saved.source;
        if let Some(path) = saved.path.clone().filter(|p| p.is_file()) {
            pane.history = vec![path.clone()];
            pane.path = Some(path);
            pane.dormant = true;
        }
        pane
    }

    pub fn to_saved(&self) -> SavedPane {
        SavedPane {
            path: self.path.clone(),
            source: self.source,
        }
    }

    /// Read a restored pane's file if it hasn't been yet.
    pub fn wake(&mut self, cx: &mut Context<Self>) {
        if self.dormant {
            self.dormant = false;
            if let Some(path) = self.path.clone() {
                self.load_file(path, false, cx);
            }
        }
    }

    pub fn is_blank(&self) -> bool {
        self.path.is_none() && !self.loading
    }

    /// File name, or a placeholder for an empty pane.
    pub fn title(&self) -> String {
        if self.help {
            return "Keyboard Shortcuts".into();
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or("Welcome".into())
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn load_file(&mut self, path: PathBuf, history: bool, cx: &mut Context<Self>) {
        if !markdown_core::is_markdown(&path) {
            self.error = Some("Only .md and .markdown files can be opened.".into());
            cx.notify();
            return;
        }
        self.dormant = false;
        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        self.help = false;
        self.path = Some(path.clone());
        if history && self.history.get(self.history_index) != Some(&path) {
            if !self.history.is_empty() {
                self.history.truncate(self.history_index + 1);
            }
            self.history.push(path.clone());
            self.history_index = self.history.len() - 1;
        }
        let task = cx.background_executor().spawn(async move {
            markdown_core::read_document(&path).map(|doc| {
                let stats = doc.stats();
                (doc, stats)
            })
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    // set_text preserves unchanged block identities and scroll on refresh.
                    Ok((doc, stats)) => {
                        this.document = Some(doc);
                        this.stats = Some(stats);
                        this.present(cx);
                    }
                    Err(e) => {
                        this.error = Some(e);
                        this.document = None;
                        this.stats = None;
                        this.leads_with_h1 = true;
                        this.text.update(cx, |s, cx| s.set_text("# Unable to read this file\n\nCheck that it still exists and is readable, then press **⌘R** to retry.", cx));
                    }
                }
                cx.emit(PaneEvent::Changed);
                cx.notify();
            });
        }));
        cx.emit(PaneEvent::Changed);
        cx.notify();
    }

    /// Re-read the current file (⌘R, or after it changed on disk).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if !self.dormant
            && let Some(path) = self.path.clone()
        {
            self.load_file(path, false, cx);
        }
    }

    pub fn present(&mut self, cx: &mut Context<Self>) {
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
        self.leads_with_h1 = leads_with_h1(&content);
        self.text.update(cx, |s, cx| s.set_text(&content, cx));
        cx.notify();
    }

    pub fn toggle_source(&mut self, cx: &mut Context<Self>) {
        self.source = !self.source;
        self.present(cx);
        cx.emit(PaneEvent::Changed);
    }

    pub fn toggle_help(&mut self, cx: &mut Context<Self>) {
        self.help = !self.help;
        self.present(cx);
        cx.emit(PaneEvent::Changed);
    }

    pub fn copy_source(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = &self.document else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(doc.source_markdown()));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |v, cx| {
                v.copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = true;
        self.find_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    #[cfg(any(test, feature = "screenshot"))]
    pub fn set_find_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.find = true;
        self.find_input
            .update(cx, |s, cx| s.set_value(query, window, cx));
        self.update_find(cx);
    }

    pub fn update_find(&mut self, cx: &mut Context<Self>) {
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

    pub fn next_match(&mut self, backwards: bool, cx: &mut Context<Self>) {
        if !self.matches.is_empty() {
            self.match_index = if backwards {
                (self.match_index + self.matches.len() - 1) % self.matches.len()
            } else {
                (self.match_index + 1) % self.matches.len()
            };
            self.update_find(cx);
        }
    }

    pub fn jump_heading(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(id) = self
            .document
            .as_ref()
            .and_then(|d| d.headings.get(index))
            .map(|h| h.id.clone())
        {
            self.source = false;
            self.help = false;
            self.present(cx);
            self.jump_anchor(&id, cx);
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

    pub fn follow(&mut self, link: &str, cx: &mut Context<Self>) {
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

    pub fn navigate(&mut self, backwards: bool, cx: &mut Context<Self>) {
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

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.text.read(cx).focus_handle().clone().focus(window, cx);
    }

    /// Escape: close find, then help. True when something closed.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        if self.find {
            self.find = false;
            self.text.update(cx, |s, cx| s.clear_range_highlights(cx));
        } else if self.help {
            self.help = false;
            self.present(cx);
            cx.emit(PaneEvent::Changed);
        } else {
            return false;
        }
        cx.notify();
        true
    }

    fn eid(&self, base: &'static str) -> ElementId {
        ElementId::NamedInteger(base.into(), self.id)
    }

    #[cfg(feature = "screenshot")]
    pub fn ready(&self, cx: &App) -> bool {
        !self.loading && !self.text.read(cx).rendered_text().is_empty()
    }
}

impl Render for DocPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.wake(cx);
        let weak = cx.entity().downgrade();
        let image_base = self.path.clone();
        let font_size = cx.try_global::<FontSize>().map_or(16., |f| f.0);
        let shows_properties = self.document.as_ref().is_some_and(|d| d.metadata.is_some())
            && !self.source
            && !self.help;
        let shows_logo = self.path.is_none() && !self.source && !self.help;
        // The list inside TextView can't take padding, so the room above the
        // first line is padding on the view that shrinks as the page scrolls:
        // the opening has space, and a scrolled page still runs to the top.
        let list = self.text.read(cx).list_state().clone();
        let scrolled = -list.scroll_px_offset_for_scrollbar().y;
        let lead = if shows_logo {
            0.
        } else {
            let lead = font_size * if self.leads_with_h1 { 2. } else { 1.5 };
            if shows_properties { lead * 0.5 } else { lead }
        };
        let top = (px(lead) - scrolled).clamp(px(0.), px(lead));
        let reader = TextView::new(&self.text)
            .scrollable(true)
            .size_full()
            // Padding, not margin: the scrollbar stays on the pane's edge.
            .px(px(32.))
            .pt(top)
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
        let theme = cx.theme();
        div()
            .size_full()
            .min_w_0()
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
                        .border_color(theme.border)
                        .child(Input::new(&self.find_input).w(px(260.)))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
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
                            Button::new(self.eid("find-prev"))
                                .ghost()
                                .icon(IconName::ChevronUp)
                                .on_click(cx.listener(|v, _, _, cx| v.next_match(true, cx))),
                        )
                        .child(
                            Button::new(self.eid("find-next"))
                                .ghost()
                                .icon(IconName::ChevronDown)
                                .on_click(cx.listener(|v, _, _, cx| v.next_match(false, cx))),
                        )
                        .child(
                            Button::new(self.eid("close-find"))
                                .ghost()
                                .label("×")
                                .on_click(cx.listener(|v, _, w, cx| {
                                    v.dismiss(cx);
                                    v.focus(w, cx);
                                })),
                        ),
                )
            })
            .when_some(self.error.clone(), |d, error| {
                d.child(div().px_5().py_2().text_sm().bg(theme.muted).child(error))
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
                            .text_color(theme.muted_foreground)
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
                            .px(px(20.))
                            .pt_2()
                            .flex()
                            .flex_col()
                            .items_start()
                            .child(
                                Button::new(self.eid("properties"))
                                    .ghost()
                                    .justify_start()
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
                                        .id(self.eid("metadata"))
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
                    .id(self.eid("reader"))
                    .flex_1()
                    .min_h_0()
                    .text_size(px(font_size))
                    .line_height(relative(1.65))
                    // The list only hears the wheel over its own rows, so the
                    // gutters and the space above pass theirs along.
                    .on_scroll_wheel({
                        let text = self.text.clone();
                        move |event, window, cx| {
                            if list.viewport_bounds().contains(&event.position) {
                                return;
                            }
                            let delta = event.delta.pixel_delta(window.line_height());
                            list.scroll_by(-delta.y);
                            text.update(cx, |_, cx| cx.notify());
                        }
                    })
                    .child(if self.loading && self.document.is_none() {
                        div()
                            .px(px(32.))
                            .pt(px(lead))
                            .text_color(theme.muted_foreground)
                            .child("Opening document…")
                            .into_any_element()
                    } else {
                        reader.into_any_element()
                    }),
            )
    }
}

/// Whether Markdown opens with a level-one heading, ATX or setext.
fn leads_with_h1(text: &str) -> bool {
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    if let Some(rest) = first.trim_start().strip_prefix('#') {
        return rest.is_empty() || rest.starts_with([' ', '\t']);
    }
    lines
        .next()
        .is_some_and(|l| !l.trim().is_empty() && l.trim().chars().all(|c| c == '='))
}

#[cfg(test)]
mod tests {
    use super::leads_with_h1;

    #[test]
    fn spots_an_opening_level_one_heading() {
        assert!(leads_with_h1("# Changelog\n\nText"));
        assert!(leads_with_h1("\n\n  # Indented\n"));
        assert!(leads_with_h1("Title\n=====\n\nBody"));
        assert!(!leads_with_h1("## Section\n"));
        assert!(!leads_with_h1("#hashtag\n"));
        assert!(!leads_with_h1("Just a paragraph.\n"));
        assert!(!leads_with_h1("```markdown\n# Source\n```"));
        assert!(!leads_with_h1(""));
    }
}
