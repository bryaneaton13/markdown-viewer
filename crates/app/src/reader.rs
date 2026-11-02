//! The window: title-bar toolbar, workspace sidebar, tab strip, and one or
//! two document panes side by side. Document state lives in `DocPane`; this
//! view routes actions to the focused pane and owns workspaces and tabs.
//! The sidebar lists every workspace as a folder that opens and closes
//! (layout in `sidebar.rs`); the active one owns the tab strip.

use crate::{
    actions::*,
    files,
    pane::{DocPane, FontSize, PaneEvent},
    preferences::{Preferences, SavedPane, SavedTab, SavedWorkspace},
    sidebar::{self, Item, Sort},
    tabs::{Tab, TabSet},
    workspace::{Workspace, home_for, now_millis},
};
use gpui_kit::{
    assets::IconName as Lucide,
    component::{
        button::*,
        command::{Command, CommandGroup, CommandItem, CommandState},
        menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
        resizable::{ResizableState, h_resizable, resizable_panel},
        *,
    },
    prelude::FluentBuilder,
    *,
};
use notify::Watcher;
use std::{
    collections::HashSet,
    ffi::OsString,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

const TOOLBAR_HEIGHT: f32 = 44.;
const SIDEBAR_WIDTH: f32 = 242.;
const SIDEBAR_ROW: f32 = 30.;
/// Below this content width a split tab shows one pane at a time.
const SPLIT_MIN_WIDTH: f32 = 720.;

/// Where a file opens.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    /// Replace the focused pane's document.
    Focused,
    /// A new tab beside the active one.
    NewTab,
    /// The other pane of the active tab, splitting it first if needed.
    Split,
}

/// Where a drag over the panes would land.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DropZone {
    /// The visible pane as a whole: an empty tab, or a narrow split.
    Whole,
    Left,
    Right,
}

/// Markdown files and folders among paths dragged in from Finder; anything
/// else is ignored.
fn droppable(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for path in paths {
        if path.is_dir() {
            dirs.push(path.clone());
        } else if markdown_core::is_markdown(path) {
            files.push(path.clone());
        }
    }
    (files, dirs)
}

/// Payload while a tab is dragged.
#[derive(Clone)]
struct DraggedTab {
    from: usize,
}

/// Payload while a workspace's folder row is dragged to reorder.
#[derive(Clone)]
struct DraggedWorkspace {
    ws: usize,
}

/// The sidebar row the keyboard is on: a workspace's folder row (`path` is
/// `None`) or a path in its tree. Kept by path so it survives rescans.
#[derive(Clone, PartialEq, Debug)]
struct Cursor {
    ws: usize,
    path: Option<PathBuf>,
}

/// Payload while a pane header is dragged out to the tab strip.
#[derive(Clone)]
struct DraggedPane {
    right: bool,
}

/// The floating label that follows the pointer during a drag.
struct Ghost(SharedString);
impl Render for Ghost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .text_sm()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .child(self.0.clone())
    }
}

pub struct Reader {
    prefs: Preferences,
    focus: FocusHandle,
    tree_focus: FocusHandle,
    palette: Entity<CommandState>,
    workspaces: Vec<Workspace>,
    active: usize,
    picker: Option<bool>, // false: files, true: headings
    cursor: Option<Cursor>,
    tree_scroll: UniformListScrollHandle,
    split_state: Entity<ResizableState>,
    /// While a tab or Finder files are dragged over the panes: where they
    /// would land, and what dropping there does.
    drop_zone: Option<DropZone>,
    drop_hint: SharedString,
    watched: Vec<(PathBuf, bool)>,
    _watch: Option<notify::RecommendedWatcher>,
    _watch_task: Option<Task<()>>,
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
        cx.set_global(FontSize(prefs.font_size));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = Self {
            focus,
            tree_focus: cx.focus_handle(),
            palette: cx.new(|cx| CommandState::new(window, cx)),
            workspaces: Vec::new(),
            active: 0,
            picker: None,
            cursor: None,
            tree_scroll: UniformListScrollHandle::new(),
            split_state: cx.new(|_| ResizableState::default()),
            drop_zone: None,
            drop_hint: SharedString::default(),
            watched: Vec::new(),
            _watch: None,
            _watch_task: None,
            prefs,
        };
        let saved = std::mem::take(&mut this.prefs.workspaces);
        for ws in &saved {
            if ws.root.as_ref().is_some_and(|r| !r.is_dir())
                || this.workspaces.iter().any(|w| w.root == ws.root)
            {
                continue;
            }
            let tabs = ws
                .tabs
                .iter()
                .map(|t| {
                    let mut tab = Tab::new(Self::make_pane(Some(&t.left), window, cx));
                    tab.right = t
                        .right
                        .as_ref()
                        .map(|r| Self::make_pane(Some(r), window, cx));
                    tab.focus_right = t.focus_right && tab.right.is_some();
                    tab.pinned = t.pinned;
                    tab
                })
                .collect();
            let tabs = TabSet::restore(tabs, ws.active_tab, || Self::make_pane(None, window, cx));
            let mut workspace = Workspace::new(ws.root.clone(), tabs);
            workspace.expanded = ws.expanded.iter().cloned().collect();
            workspace.open = !ws.collapsed;
            workspace.last_used = ws.last_used;
            this.workspaces.push(workspace);
        }
        if this.workspaces.is_empty() {
            this.push_workspace(None, window, cx);
        }
        this.active = this.prefs.active_workspace.min(this.workspaces.len() - 1);
        for ix in 0..this.workspaces.len() {
            if ix == this.active || this.workspaces[ix].open {
                this.scan(ix, cx);
            }
        }
        this.refresh_loose(cx);
        this.rewatch(cx);
        if let Some(path) = path {
            this.open_path(path, window, cx);
        }
        this
    }

    fn make_pane(
        saved: Option<&SavedPane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<DocPane> {
        let pane = cx.new(|cx| match saved {
            Some(saved) => DocPane::restore(saved, window, cx),
            None => DocPane::new(window, cx),
        });
        cx.subscribe(&pane, |this, _, _: &PaneEvent, cx| this.pane_changed(cx))
            .detach();
        pane
    }

    fn pane_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_loose(cx);
        self.rewatch(cx);
        self.save(cx);
        cx.notify();
    }

    // MARK: Accessors

    fn ws(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    fn ws_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active]
    }

    fn tab(&self) -> &Tab<Entity<DocPane>> {
        self.ws().tabs.active()
    }

    /// The pane navigation and the toolbar apply to.
    pub fn focused_pane(&self) -> Entity<DocPane> {
        self.tab().focused().clone()
    }

    fn with_pane<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut DocPane, &mut Context<DocPane>) -> R,
    ) -> R {
        self.focused_pane().update(cx, f)
    }

    // MARK: Opening

    /// Open from Finder, the command line, or the Open panel: a folder
    /// becomes a workspace; a file opens in the workspace it belongs to.
    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        if path.is_dir() {
            self.add_folder(path, window, cx);
        } else {
            self.open_file(path, window, cx);
        }
    }

    fn push_workspace(
        &mut self,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tabs = TabSet::new(Tab::new(Self::make_pane(None, window, cx)));
        self.workspaces.push(Workspace::new(root, tabs));
    }

    /// Add a folder as a workspace and switch to it. A loose-files workspace
    /// whose documents all live in that folder is adopted instead.
    fn add_folder(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.sidebar = true;
        if let Some(ix) = self
            .workspaces
            .iter()
            .position(|w| w.root.as_ref() == Some(&path))
        {
            self.activate_workspace(ix, window, cx);
            return;
        }
        let adopt = self.ws().root.is_none()
            && self.ws().tabs.panes().all(|p| {
                p.read(cx)
                    .path
                    .as_ref()
                    .is_none_or(|f| f.starts_with(&path))
            });
        if adopt {
            let ws = self.ws_mut();
            ws.root = Some(path);
            ws.expanded.clear();
            ws.scanned = false;
        } else {
            self.push_workspace(Some(path), window, cx);
            self.active = self.workspaces.len() - 1;
        }
        self.after_workspace_change(window, cx);
    }

    /// Open a file in its workspace: switch to a tab already showing it, use
    /// an empty focused pane, or open a new tab.
    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let roots: Vec<_> = self.workspaces.iter().map(|w| w.root.as_deref()).collect();
        let ix = match home_for(&roots, &path) {
            Some(ix) => ix,
            None => {
                self.push_workspace(None, window, cx);
                self.workspaces.len() - 1
            }
        };
        if ix != self.active {
            self.active = ix;
            self.after_workspace_change(window, cx);
        }
        if self.show_existing(&path, window, cx) {
        } else if self.focused_pane().read(cx).is_blank() {
            self.open_in(path, Target::Focused, window, cx);
        } else {
            self.open_in(path, Target::NewTab, window, cx);
        }
    }

    /// Bring forward the tab and pane already showing `path` in the active
    /// workspace, if any.
    fn show_existing(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let showing = self.ws().tabs.tabs.iter().enumerate().find_map(|(i, t)| {
            let right = t
                .panes()
                .position(|p| p.read(cx).path.as_deref() == Some(path))?;
            Some((i, right == 1))
        });
        let Some((tab, right)) = showing else {
            return false;
        };
        let tabs = &mut self.ws_mut().tabs;
        tabs.activate(tab);
        tabs.active_mut().focus_right = right;
        self.after_tab_change(window, cx);
        true
    }

    /// Open a file from a workspace's tree in the sidebar, making that
    /// workspace active. A plain click goes to a tab already showing it.
    fn open_from_tree(
        &mut self,
        ws: usize,
        path: PathBuf,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ws != self.active {
            self.active = ws;
            self.after_workspace_change(window, cx);
        }
        if target == Target::Focused && self.show_existing(&path, window, cx) {
            return;
        }
        self.open_in(path, target, window, cx);
    }

    /// Open a file from the tree or Quick Open.
    pub fn open_in(
        &mut self,
        path: PathBuf,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let load = |pane: &Entity<DocPane>, cx: &mut Context<Self>| {
            pane.update(cx, |p, cx| p.load_file(path.clone(), true, cx))
        };
        match target {
            Target::Focused => load(&self.focused_pane(), cx),
            Target::NewTab => {
                let pane = Self::make_pane(None, window, cx);
                load(&pane, cx);
                self.ws_mut().tabs.insert_after_active(Tab::new(pane));
                self.after_tab_change(window, cx);
            }
            Target::Split => {
                if self.tab().is_split() {
                    let tab = self.ws_mut().tabs.active_mut();
                    tab.focus_right = !tab.focus_right;
                    load(&self.focused_pane(), cx);
                } else {
                    let pane = Self::make_pane(None, window, cx);
                    load(&pane, cx);
                    self.ws_mut().tabs.active_mut().split_with(pane, true);
                }
                self.after_tab_change(window, cx);
            }
        }
        self.save(cx);
        cx.notify();
    }

    fn prompt(&mut self, folder: bool, window: &Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some(
                if folder {
                    "Add Folder"
                } else {
                    "Open Markdown"
                }
                .into(),
            ),
        });
        let window = window.window_handle();
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = window.update(cx, |_, window, cx| {
                    this.update(cx, |this, cx| this.open_path(path, window, cx))
                });
            }
        })
        .detach();
    }

    // MARK: Dropping from Finder

    /// Files and folders dropped outside the panes and tab strip: the same
    /// as opening them from Finder.
    fn drop_anywhere(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        self.drop_zone = None;
        let (files, dirs) = droppable(paths);
        for path in dirs.into_iter().chain(files) {
            self.open_path(path, window, cx);
        }
    }

    /// Files dropped on the tab strip open as new tabs here.
    fn drop_on_tabs(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let (files, dirs) = droppable(paths);
        if !dirs.is_empty() {
            return self.drop_anywhere(paths, window, cx);
        }
        for path in files {
            self.open_in(path, Target::NewTab, window, cx);
        }
    }

    /// What dropping `paths` at the given spot of the panes would do, or
    /// `None` when there is nothing to open.
    fn plan_drop(
        &self,
        paths: &[PathBuf],
        right_half: bool,
        narrow: bool,
        cx: &App,
    ) -> Option<(DropZone, SharedString)> {
        let (files, dirs) = droppable(paths);
        let more = |n: usize| match n {
            0 => String::new(),
            1 => " (+1 tab)".into(),
            n => format!(" (+{n} tabs)"),
        };
        if !dirs.is_empty() {
            let hint = match (dirs.len(), files.len()) {
                (1, 0) => "Add folder as workspace".to_owned(),
                (n, 0) => format!("Add {n} folders as workspaces"),
                _ => "Open folders and files".to_owned(),
            };
            return Some((DropZone::Whole, hint.into()));
        }
        let n = files.len();
        if n == 0 {
            return None;
        }
        let tab = self.tab();
        if tab.is_split() && narrow {
            return Some((DropZone::Whole, format!("Open here{}", more(n - 1)).into()));
        }
        if !tab.is_split() && self.focused_pane().read(cx).is_blank() {
            let hint = if n == 1 {
                "Open".to_owned()
            } else {
                format!("Open side by side{}", more(n - 2))
            };
            return Some((DropZone::Whole, hint.into()));
        }
        let (zone, side) = if right_half {
            (DropZone::Right, "right")
        } else {
            (DropZone::Left, "left")
        };
        let verb = if tab.is_split() {
            "Replace"
        } else {
            "Open on the"
        };
        let hint = if tab.is_split() {
            format!("{verb} {side} pane{}", more(n - 1))
        } else {
            format!("{verb} {side}{}", more(n - 1))
        };
        Some((zone, hint.into()))
    }

    /// Files dropped on the panes: into the side they were dropped on,
    /// splitting the tab if needed. Extra files open as background tabs.
    fn drop_on_panes(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let Some(zone) = self.drop_zone.take() else {
            return;
        };
        let (files, dirs) = droppable(paths);
        if !dirs.is_empty() {
            return self.drop_anywhere(paths, window, cx);
        }
        let mut files = files.into_iter();
        let Some(first) = files.next() else {
            return;
        };
        let keep = self.ws().tabs.active;
        match zone {
            DropZone::Whole => {
                let fill_split = !self.tab().is_split();
                self.open_in(first, Target::Focused, window, cx);
                if fill_split && let Some(second) = files.next() {
                    self.open_in(second, Target::Split, window, cx);
                }
            }
            DropZone::Left | DropZone::Right => {
                let right = zone == DropZone::Right;
                if self.tab().is_split() {
                    self.ws_mut().tabs.active_mut().focus_right = right;
                    self.open_in(first, Target::Focused, window, cx);
                } else {
                    let pane = Self::make_pane(None, window, cx);
                    pane.update(cx, |p, cx| p.load_file(first, true, cx));
                    self.ws_mut().tabs.active_mut().split_with(pane, right);
                }
            }
        }
        for path in files {
            self.open_in(path, Target::NewTab, window, cx);
        }
        self.ws_mut().tabs.activate(keep);
        self.after_tab_change(window, cx);
    }

    // MARK: Workspaces

    fn activate_workspace(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.workspaces.len() && ix != self.active {
            self.active = ix;
            self.after_workspace_change(window, cx);
        }
    }

    /// Workspaces as the sidebar shows them, skipping a hidden "Open Files".
    fn shown_order(&self) -> Vec<usize> {
        let summaries = self.summaries();
        sidebar::order(&summaries, self.prefs.sidebar_sort)
            .into_iter()
            .filter(|&ws| sidebar::visible(&summaries[ws]) || ws == self.active)
            .collect()
    }

    /// ⌃1–9: the nth workspace in sidebar order, opened and scrolled to.
    fn switch_to_nth(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&ws) = self.shown_order().get(n) else {
            return;
        };
        if ws == self.active && !self.ws().open {
            self.toggle_open(ws, cx);
        }
        self.activate_workspace(ws, window, cx);
        if let Some(ix) = self.layout().iter().position(|&i| i == Item::Header(ws)) {
            self.tree_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    fn cycle_workspace(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.shown_order();
        let Some(at) = order.iter().position(|&ws| ws == self.active) else {
            return;
        };
        let next = (at as isize + delta).rem_euclid(order.len() as isize) as usize;
        self.switch_to_nth(next, window, cx);
    }

    fn close_workspace(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.workspaces.len() {
            return;
        }
        self.workspaces.remove(ix);
        if self.workspaces.is_empty() {
            self.push_workspace(None, window, cx);
        }
        if self.active > ix || self.active >= self.workspaces.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.cursor = self.cursor.take().and_then(|mut c| {
            if c.ws == ix {
                return None;
            }
            c.ws -= (c.ws > ix) as usize;
            Some(c)
        });
        self.after_workspace_change(window, cx);
    }

    fn after_workspace_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        self.drop_zone = None;
        let ws = self.ws_mut();
        ws.open = true;
        ws.last_used = now_millis();
        // Unwatched folders may have changed since they were last scanned.
        let root = self.ws().root.clone();
        if !self.ws().scanned || !self.watched.iter().any(|(p, _)| Some(p) == root.as_ref()) {
            self.scan(self.active, cx);
        }
        self.rewatch(cx);
        self.after_tab_change(window, cx);
    }

    /// Open or close a workspace's folder in the sidebar. This never changes
    /// which workspace is active.
    fn toggle_open(&mut self, ix: usize, cx: &mut Context<Self>) {
        let ws = &mut self.workspaces[ix];
        ws.open = !ws.open;
        if ws.open && !ws.scanned {
            self.scan(ix, cx);
        }
        if !self.workspaces[ix].open
            && let Some(c) = self.cursor.as_mut()
            && c.ws == ix
        {
            c.path = None;
        }
        self.rewatch(cx);
        self.save(cx);
        cx.notify();
    }

    /// Close every workspace but the active one; if that's already the
    /// case, close the active one too.
    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let active = self.active;
        let others_open = self
            .workspaces
            .iter()
            .enumerate()
            .any(|(i, w)| i != active && w.open);
        for (i, ws) in self.workspaces.iter_mut().enumerate() {
            if i != active || !others_open {
                ws.open = false;
            }
        }
        if let Some(c) = self.cursor.as_mut()
            && !self.workspaces[c.ws].open
        {
            c.path = None;
        }
        self.rewatch(cx);
        self.save(cx);
        cx.notify();
    }

    fn set_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
        self.prefs.sidebar_sort = sort;
        self.save(cx);
        cx.notify();
    }

    /// Drag a workspace onto another: it takes that place, and the sidebar
    /// switches to (and keeps) the resulting manual order.
    fn move_workspace(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let order = sidebar::order(&self.summaries(), self.prefs.sidebar_sort);
        let order = sidebar::reorder(&order, from, to);
        let mut old: Vec<Option<Workspace>> = self.workspaces.drain(..).map(Some).collect();
        self.workspaces = order.iter().filter_map(|&i| old[i].take()).collect();
        let remap = |ix: usize| order.iter().position(|&i| i == ix).unwrap_or(ix);
        self.active = remap(self.active);
        if let Some(c) = self.cursor.as_mut() {
            c.ws = remap(c.ws);
        }
        self.prefs.sidebar_sort = Sort::Manual;
        self.save(cx);
        cx.notify();
    }

    /// The loose-files workspace lists the documents open in its tabs.
    fn refresh_loose(&mut self, cx: &App) {
        for ws in &mut self.workspaces {
            if ws.root.is_none() {
                let paths: Vec<PathBuf> = ws
                    .tabs
                    .panes()
                    .filter_map(|p| p.read(cx).path.clone())
                    .collect();
                ws.set_loose_rows(paths);
            }
        }
    }

    // MARK: Tabs and panes

    /// Keep keyboard focus inside the visible content after tabs or panes
    /// change, so window actions keep reaching this view.
    fn after_tab_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tree_focus.is_focused(window) {
            self.focus_reader(window, cx);
        }
        self.refresh_loose(cx);
        self.save(cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = Self::make_pane(None, window, cx);
        self.ws_mut().tabs.insert_after_active(Tab::new(pane));
        self.after_tab_change(window, cx);
        if self.ws().root.is_some() {
            self.open_picker(false, window, cx);
        }
    }

    /// ⌘W: close the focused pane of a split, else the tab. Closing the last,
    /// empty tab closes the window.
    fn close_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().is_split() {
            let right = self.tab().focus_right;
            self.close_pane(right, window, cx);
        } else if self.ws().tabs.len() > 1 {
            let ix = self.ws().tabs.active;
            self.close_tab(ix, window, cx);
        } else if !self.focused_pane().read(cx).is_blank() {
            let pane = Self::make_pane(None, window, cx);
            let tabs = &mut self.ws_mut().tabs;
            let old = std::mem::replace(&mut tabs.active_mut().left, pane);
            tabs.remember_pane(old);
            self.after_tab_change(window, cx);
        } else {
            window.remove_window();
        }
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = &mut self.ws_mut().tabs;
        if tabs.len() == 1 {
            self.close_focused(window, cx);
        } else if tabs.close(ix) {
            self.after_tab_change(window, cx);
        }
    }

    fn close_pane(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = &mut self.ws_mut().tabs;
        if let Some(pane) = tabs.active_mut().take_pane(right) {
            tabs.remember_pane(pane);
            self.after_tab_change(window, cx);
        }
    }

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.ws_mut().tabs.activate(ix) {
            self.after_tab_change(window, cx);
        }
    }

    /// ⌘\: show the focused document again beside itself, or split the two
    /// panes back into separate tabs.
    fn toggle_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().is_split() {
            self.ws_mut().tabs.pane_to_tab(true, false);
        } else {
            let pane = Self::make_pane(None, window, cx);
            if let Some(path) = self.focused_pane().read(cx).path.clone() {
                pane.update(cx, |p, cx| p.load_file(path, true, cx));
            }
            self.ws_mut().tabs.active_mut().split_with(pane, true);
        }
        self.after_tab_change(window, cx);
    }

    fn focus_pane(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.ws_mut().tabs.active_mut();
        if tab.is_split() && tab.focus_right != right {
            tab.focus_right = right;
            self.after_tab_change(window, cx);
        }
    }

    // MARK: Folder scanning and watching

    fn scan(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(ws) = self.workspaces.get_mut(ix) else {
            return;
        };
        let Some(root) = ws.root.clone() else {
            ws.scanned = true;
            return;
        };
        ws.scan_generation += 1;
        let generation = ws.scan_generation;
        ws.indexing = true;
        let walk = root.clone();
        let task = cx
            .background_executor()
            .spawn(async move { files::discover(&walk) });
        ws._scan = Some(cx.spawn(async move |this, cx| {
            let (files, limited) = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(ws) = this
                    .workspaces
                    .iter_mut()
                    .find(|w| w.root.as_ref() == Some(&root))
                    && ws.scan_generation == generation
                {
                    ws.files = files;
                    ws.limited = limited;
                    ws.indexing = false;
                    ws.scanned = true;
                    ws.rebuild_rows();
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    /// Watch the folders of the active workspace and of every workspace
    /// open in the sidebar, plus the folders of the active workspace's open
    /// files that live outside it. Closed workspaces rescan when reopened.
    fn rewatch(&mut self, cx: &mut Context<Self>) {
        let mut wanted: Vec<(PathBuf, bool)> = Vec::new();
        for (ix, ws) in self.workspaces.iter().enumerate() {
            if let Some(root) = &ws.root
                && (ix == self.active || ws.open)
                && !wanted.iter().any(|(p, _)| p == root)
            {
                wanted.push((root.clone(), true));
            }
        }
        let ws = self.ws();
        for pane in ws.tabs.panes() {
            if let Some(parent) = pane.read(cx).path.as_ref().and_then(|p| p.parent())
                && !ws.contains(parent)
                && !wanted.iter().any(|(p, _)| p == parent)
            {
                wanted.push((parent.to_owned(), false));
            }
        }
        if wanted == self.watched {
            return;
        }
        self.watched = wanted.clone();
        // FSEvents would wake the deterministic test scheduler from its own
        // thread; tests drive `files_changed` directly instead.
        if cfg!(test) {
            return;
        }
        let (tx, rx) = async_channel::unbounded::<Vec<PathBuf>>();
        let Ok(mut watcher) =
            notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                if let Ok(e) = event
                    && !matches!(e.kind, notify::EventKind::Access(_))
                {
                    let _ = tx.try_send(e.paths);
                }
            })
        else {
            return;
        };
        for (dir, recursive) in &wanted {
            let mode = if *recursive {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };
            let _ = watcher.watch(dir, mode);
        }
        self._watch = Some(watcher);
        self._watch_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                cx.background_executor()
                    .timer(Duration::from_millis(350))
                    .await;
                let mut changed = first;
                while let Ok(more) = rx.try_recv() {
                    changed.extend(more);
                }
                let _ = this.update(cx, |this, cx| this.files_changed(&changed, cx));
            }
        }));
    }

    /// Rescan the watched folders the changes fall under, and reload every
    /// pane showing a changed file. Panes are matched by name, since the
    /// watcher may report paths through a different (resolved) prefix.
    fn files_changed(&mut self, changed: &[PathBuf], cx: &mut Context<Self>) {
        let names: HashSet<OsString> = changed
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_owned()))
            .collect();
        let affected: Vec<usize> = (0..self.workspaces.len())
            .filter(|&ix| {
                let Some(root) = &self.workspaces[ix].root else {
                    return false;
                };
                let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
                self.watched.iter().any(|(p, _)| p == root)
                    && changed
                        .iter()
                        .any(|p| p.starts_with(root) || p.starts_with(&real))
            })
            .collect();
        if affected.is_empty() {
            self.scan(self.active, cx);
        }
        for ix in affected {
            self.scan(ix, cx);
        }
        let panes: Vec<_> = self
            .workspaces
            .iter()
            .flat_map(|ws| ws.tabs.panes().cloned())
            .collect();
        for pane in panes {
            let hit = pane
                .read(cx)
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .is_some_and(|n| names.is_empty() || names.contains(n));
            if hit {
                pane.update(cx, |p, cx| p.reload(cx));
            }
        }
    }

    // MARK: Persistence

    fn save(&mut self, cx: &App) {
        self.prefs.workspaces = self
            .workspaces
            .iter()
            .map(|ws| {
                let mut expanded: Vec<_> = ws.expanded.iter().cloned().collect();
                expanded.sort();
                SavedWorkspace {
                    root: ws.root.clone(),
                    expanded,
                    tabs: ws
                        .tabs
                        .tabs
                        .iter()
                        .map(|t| SavedTab {
                            left: t.left.read(cx).to_saved(),
                            right: t.right.as_ref().map(|p| p.read(cx).to_saved()),
                            focus_right: t.focus_right,
                            pinned: t.pinned,
                        })
                        .collect(),
                    active_tab: ws.tabs.active,
                    collapsed: !ws.open,
                    last_used: ws.last_used,
                }
            })
            .collect();
        self.prefs.active_workspace = self.active;
        self.prefs.save();
    }

    // MARK: Tree, focus, overlays

    fn focus_reader(&self, window: &mut Window, cx: &mut App) {
        let handle = self
            .focused_pane()
            .read(cx)
            .text
            .read(cx)
            .focus_handle()
            .clone();
        handle.focus(window, cx);
    }

    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_none() {
            self.with_pane(cx, |p, cx| p.dismiss(cx));
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

    /// ⌘↵ / ⌥↵: open the highlighted Quick Open result or tree row elsewhere.
    fn open_selection(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let path = match self.picker {
            Some(false) => {
                let row = self.palette.read(cx).selected_index().map(|ix| ix.row);
                let path = row.and_then(|r| self.ws().files.get(r).cloned());
                self.close_overlay(window, cx);
                path
            }
            Some(true) => return,
            None => {
                let items = self.layout();
                let hit = self
                    .cursor_index(&items, cx)
                    .and_then(|ix| match items[ix] {
                        Item::Entry { ws, row } => {
                            let row = &self.workspaces[ws].rows[row];
                            (!row.directory).then(|| (ws, row.path.clone()))
                        }
                        _ => None,
                    });
                if let Some((ws, path)) = hit {
                    self.open_from_tree(ws, path, target, window, cx);
                }
                return;
            }
        };
        if let Some(path) = path {
            self.open_in(path, target, window, cx);
        }
    }

    // MARK: Sidebar tree

    fn summaries(&self) -> Vec<sidebar::Summary> {
        self.workspaces.iter().map(Workspace::summary).collect()
    }

    /// Workspace indices in sidebar order.
    fn order(&self) -> Vec<usize> {
        sidebar::order(&self.summaries(), self.prefs.sidebar_sort)
    }

    /// The sidebar's rows, top to bottom.
    fn layout(&self) -> Vec<Item> {
        let summaries = self.summaries();
        sidebar::layout(
            &summaries,
            &sidebar::order(&summaries, self.prefs.sidebar_sort),
        )
    }

    fn row(&self, item: Item) -> Option<&files::Row> {
        match item {
            Item::Entry { ws, row } => self.workspaces.get(ws)?.rows.get(row),
            _ => None,
        }
    }

    fn cursor_at(&self, item: Item) -> Option<Cursor> {
        match item {
            Item::Header(ws) => Some(Cursor { ws, path: None }),
            Item::Entry { ws, .. } => Some(Cursor {
                ws,
                path: Some(self.row(item)?.path.clone()),
            }),
            Item::Empty(_) => None,
        }
    }

    fn is_cursor(&self, item: Item) -> bool {
        let Some(c) = &self.cursor else {
            return false;
        };
        match item {
            Item::Header(ws) => c.ws == ws && c.path.is_none(),
            Item::Entry { ws, .. } => {
                c.ws == ws && c.path.as_ref() == self.row(item).map(|r| &r.path)
            }
            Item::Empty(_) => false,
        }
    }

    /// The keyboard's row: the cursor, else the current file in the active
    /// workspace, else the active workspace's folder row.
    fn cursor_index(&self, items: &[Item], cx: &App) -> Option<usize> {
        let current = self.focused_pane().read(cx).path.clone();
        items
            .iter()
            .position(|&i| self.is_cursor(i))
            .or_else(|| {
                items.iter().position(|&i| {
                    i.ws() == self.active
                        && self
                            .row(i)
                            .is_some_and(|r| Some(&r.path) == current.as_ref())
                })
            })
            .or_else(|| items.iter().position(|&i| i == Item::Header(self.active)))
    }

    fn set_cursor(&mut self, items: &[Item], ix: usize) {
        self.cursor = self.cursor_at(items[ix]);
        self.tree_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
    }

    /// ↑ / ↓ across every open workspace. Landing on a file in the active
    /// workspace previews it; other workspaces wait for ↵ or a click.
    fn tree_move(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.layout();
        if items.is_empty() {
            return;
        }
        let step = if delta < 0 { -1 } else { 1 };
        let mut ix = match self.cursor_index(&items, cx) {
            Some(ix) => ix as isize + delta,
            None => 0,
        };
        while ix >= 0
            && items
                .get(ix as usize)
                .is_some_and(|i| matches!(i, Item::Empty(_)))
        {
            ix += step;
        }
        let Some(&item) = usize::try_from(ix).ok().and_then(|ix| items.get(ix)) else {
            return;
        };
        self.set_cursor(&items, ix as usize);
        if item.ws() == self.active
            && let Some(row) = self.row(item).filter(|r| !r.directory).cloned()
        {
            self.open_from_tree(item.ws(), row.path, Target::Focused, window, cx);
        }
        cx.notify();
    }

    fn toggle_dir(&mut self, ws: usize, dir: PathBuf, cx: &mut Context<Self>) {
        let ws = &mut self.workspaces[ws];
        if !ws.expanded.remove(&dir) {
            ws.expanded.insert(dir);
        }
        ws.rebuild_rows();
        self.save(cx);
        cx.notify();
    }

    /// ↵ or a click: open or close a folder, or open a file.
    fn tree_open(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.layout();
        let Some(ix) = self.cursor_index(&items, cx) else {
            return;
        };
        match items[ix] {
            Item::Header(ws) => self.toggle_open(ws, cx),
            item @ Item::Entry { ws, .. } => {
                let Some(row) = self.row(item).cloned() else {
                    return;
                };
                if row.directory {
                    self.toggle_dir(ws, row.path, cx);
                } else {
                    self.open_from_tree(ws, row.path, target, window, cx);
                }
            }
            Item::Empty(_) => {}
        }
    }

    /// ←: close the folder under the cursor, else go up to its parent.
    fn tree_left(&mut self, cx: &mut Context<Self>) {
        let items = self.layout();
        let Some(ix) = self.cursor_index(&items, cx) else {
            return;
        };
        match items[ix] {
            Item::Header(ws) => {
                if self.workspaces[ws].open {
                    self.toggle_open(ws, cx);
                }
            }
            item @ Item::Entry { ws, .. } => {
                let Some(row) = self.row(item).cloned() else {
                    return;
                };
                if row.directory && self.workspaces[ws].expanded.contains(&row.path) {
                    return self.toggle_dir(ws, row.path, cx);
                }
                let parent = row.path.parent();
                let up = items
                    .iter()
                    .position(|&i| {
                        i.ws() == ws
                            && row.depth > 0
                            && self.row(i).map(|r| r.path.as_path()) == parent
                    })
                    .or_else(|| items.iter().position(|&i| i == Item::Header(ws)));
                if let Some(up) = up {
                    self.set_cursor(&items, up);
                }
                cx.notify();
            }
            Item::Empty(_) => {}
        }
    }

    /// →: open the folder under the cursor, else move down.
    fn tree_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.layout();
        let Some(ix) = self.cursor_index(&items, cx) else {
            return;
        };
        let closed = match items[ix] {
            Item::Header(ws) => !self.workspaces[ws].open,
            item @ Item::Entry { ws, .. } => self
                .row(item)
                .is_some_and(|r| r.directory && !self.workspaces[ws].expanded.contains(&r.path)),
            Item::Empty(_) => false,
        };
        if closed {
            self.cursor = self.cursor_at(items[ix]);
            self.tree_open(Target::Focused, window, cx);
        } else {
            self.tree_move(1, window, cx);
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
        cx.set_global(FontSize(self.prefs.font_size));
        self.prefs.save();
        cx.notify();
    }

    // MARK: Rendering

    fn toolbar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let pane = self.focused_pane();
        let pane = pane.read(cx);
        let (source, help, has_doc, copied) =
            (pane.source, pane.help, pane.document.is_some(), pane.copied);
        let workspace = self.ws().root.as_ref().map(|_| self.ws().name());
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
                            .disabled(!pane.can_go_back())
                            .tooltip("Back · ⌘[")
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.with_pane(cx, |p, cx| p.navigate(true, cx))
                            })),
                    )
                    .child(
                        Button::new("forward")
                            .ghost()
                            .icon(IconName::ChevronRight)
                            .disabled(!pane.can_go_forward())
                            .tooltip("Forward · ⌘]")
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.with_pane(cx, |p, cx| p.navigate(false, cx))
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            // Which workspace this tab strip belongs to.
                            .when_some(workspace, |d, name| {
                                d.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{name}  /")),
                                )
                            })
                            .child(div().min_w_0().truncate().child(
                                if pane.path.is_some() || help {
                                    pane.title()
                                } else {
                                    "Markdown Viewer".into()
                                },
                            )),
                    )
                    .child(
                        Button::new("quick-open")
                            .ghost()
                            .icon(IconName::Search)
                            .label("Quick Open")
                            .tooltip("Find a file · ⌘P or ⌘K")
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
                        Button::new("split")
                            .ghost()
                            .icon(IconName::PanelRight)
                            .selected(self.tab().is_split())
                            .tooltip("Split side by side · ⌘\\")
                            .on_click(cx.listener(|v, _, w, cx| v.toggle_split(w, cx))),
                    )
                    .when(source && !help && has_doc, |d| {
                        d.child(
                            Button::new("copy-source")
                                .ghost()
                                .icon(if copied {
                                    IconName::Check
                                } else {
                                    IconName::Copy
                                })
                                .label(if copied { "Copied" } else { "Copy" })
                                .tooltip("Copy Markdown source")
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.with_pane(cx, |p, cx| p.copy_source(cx))
                                })),
                        )
                    })
                    .child(
                        Button::new("source")
                            .ghost()
                            .label(if source { "Preview" } else { "Source" })
                            .tooltip("Read-only source · ⇧⌘M")
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.with_pane(cx, |p, cx| p.toggle_source(cx))
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .ghost()
                            .icon(IconName::Sun)
                            .tooltip("Toggle theme · ⇧⌘D")
                            .on_click(cx.listener(|_, _, w, cx| {
                                w.dispatch_action(Box::new(ToggleTheme), cx)
                            })),
                    )
                    .child(
                        Button::new("help")
                            .ghost()
                            .icon(IconName::Info)
                            .tooltip("Shortcuts & Finder setup · ⌘/")
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.with_pane(cx, |p, cx| p.toggle_help(cx))
                            })),
                    ),
            )
    }

    /// The title strip of the sidebar: sort, collapse all, add folder.
    fn sidebar_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let sort = self.prefs.sidebar_sort;
        let folders = self.workspaces.iter().filter(|w| w.root.is_some()).count();
        let any_open = self.workspaces.iter().any(|w| w.open);
        div()
            .pl_4()
            .pr_2()
            .pt_4()
            .pb_2()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child("WORKSPACES"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        Button::new("workspace-sort")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::ArrowDownUp))
                            .disabled(folders < 2)
                            .tooltip("Sort workspaces")
                            .dropdown_menu(move |mut menu, _, _| {
                                menu = menu.min_w(px(200.));
                                for option in Sort::ALL {
                                    let weak = weak.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(option.label())
                                            .checked(option == sort)
                                            .on_click(move |_, _, cx| {
                                                let _ =
                                                    weak.update(cx, |v, cx| v.set_sort(option, cx));
                                            }),
                                    );
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new("workspace-collapse")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::ChevronsDownUp))
                            .disabled(!any_open)
                            .tooltip("Collapse all")
                            .on_click(cx.listener(|v, _, _, cx| v.collapse_all(cx))),
                    )
                    .child(
                        Button::new("workspace-add")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::FolderPlus))
                            .tooltip("Add folder · ⇧⌘O")
                            .on_click(cx.listener(|v, _, w, cx| v.prompt(true, w, cx))),
                    ),
            )
    }

    /// Actions for one workspace, from its ⋯ button or a right-click.
    fn workspace_menu(
        menu: PopupMenu,
        ws: usize,
        folder: bool,
        weak: WeakEntity<Self>,
    ) -> PopupMenu {
        let item =
            |label: &'static str, f: fn(&mut Reader, usize, &mut Window, &mut Context<Reader>)| {
                let weak = weak.clone();
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |v, cx| f(v, ws, window, cx));
                })
            };
        let menu = if folder {
            menu.item(item("Reveal in Finder", |v, ws, _, cx| {
                if let Some(root) = &v.workspaces[ws].root {
                    cx.reveal_path(root);
                }
            }))
            .item(item("Collapse Subfolders", |v, ws, _, cx| {
                let ws = &mut v.workspaces[ws];
                ws.expanded.clear();
                ws.rebuild_rows();
                v.save(cx);
                cx.notify();
            }))
            .separator()
        } else {
            menu
        };
        menu.item(item("Close Workspace", |v, ws, w, cx| {
            v.close_workspace(ws, w, cx)
        }))
    }

    /// One sidebar row: a workspace's folder row, an entry in its tree, or
    /// the placeholder under an empty folder.
    fn sidebar_row(
        &self,
        item: Item,
        ix: usize,
        positions: &Rc<Vec<usize>>,
        keyboard: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (hover, accent, ring) = (theme.sidebar_accent, theme.sidebar_accent, theme.ring);
        let muted = theme.muted_foreground;
        let cursor = keyboard == Some(ix);
        let current = self.focused_pane().read(cx).path.clone();
        let row = div()
            .h(px(SIDEBAR_ROW))
            .w_full()
            .pr_1()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .text_sm()
            .border_1()
            .border_color(if cursor { ring } else { transparent_black() });
        match item {
            Item::Header(w) => {
                let ws = &self.workspaces[w];
                let (active, open, folder) = (w == self.active, ws.open, ws.root.is_some());
                let name: SharedString = ws.name().into();
                let ghost = name.clone();
                // A closed active workspace still says which file is showing.
                let showing = (active && !open)
                    .then(|| current.as_ref()?.file_name())
                    .flatten()
                    .map(|n| n.to_string_lossy().into_owned());
                let docs = ws
                    .tabs
                    .tabs
                    .iter()
                    .filter(|t| t.panes().any(|p| p.read(cx).path.is_some()))
                    .count();
                let group: SharedString = format!("workspace-row-{w}").into();
                let positions = positions.clone();
                let target = positions[w];
                let drag = theme.drag_border;
                let (menu_weak, context_weak) = (cx.entity().downgrade(), cx.entity().downgrade());
                row.id(("workspace", w))
                    .group(group.clone())
                    .pl_2()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(
                        Icon::new(match (folder, open) {
                            (false, _) => IconName::Inbox,
                            (true, true) => IconName::FolderOpen,
                            (true, false) => IconName::Folder,
                        })
                        .size_4()
                        .flex_shrink_0()
                        .text_color(if active {
                            theme.primary
                        } else {
                            muted
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .gap_1p5()
                            .whitespace_nowrap()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .max_w(px(140.))
                                    .truncate()
                                    .when(active, |d| d.font_weight(FontWeight::SEMIBOLD))
                                    .child(name),
                            )
                            .when_some(showing, |d, file| {
                                d.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(file),
                                )
                            }),
                    )
                    .when(!open && docs > 0, |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(muted)
                                .child(docs.to_string()),
                        )
                    })
                    .when(active, |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .size(px(6.))
                                .rounded_full()
                                .bg(theme.primary),
                        )
                    })
                    .child(
                        div()
                            .id(("workspace-menu", w))
                            .flex_shrink_0()
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            // Keep the click from also opening or closing the folder.
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                Button::new(("workspace-actions", w))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Ellipsis)
                                    .dropdown_menu(move |menu, _, _| {
                                        Self::workspace_menu(menu, w, folder, menu_weak.clone())
                                    }),
                            ),
                    )
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.cursor = Some(Cursor { ws: w, path: None });
                        v.toggle_open(w, cx);
                    }))
                    .when(folder, |d| {
                        d.on_drag(DraggedWorkspace { ws: w }, move |_, _, _, cx| {
                            cx.new(|_| Ghost(ghost.clone()))
                        })
                        .drag_over::<DraggedWorkspace>(move |style, dragged, _, _| {
                            // A line on the side the dragged folder will land.
                            let from = positions.get(dragged.ws).copied().unwrap_or(target);
                            match from.cmp(&target) {
                                std::cmp::Ordering::Less => style.border_b_2().border_color(drag),
                                std::cmp::Ordering::Greater => {
                                    style.border_t_2().border_color(drag)
                                }
                                std::cmp::Ordering::Equal => style,
                            }
                        })
                        .on_drop(cx.listener(
                            move |v, dragged: &DraggedWorkspace, _, cx| {
                                v.move_workspace(dragged.ws, w, cx)
                            },
                        ))
                    })
                    .context_menu(move |menu, _, _| {
                        Self::workspace_menu(menu, w, folder, context_weak.clone())
                    })
                    .into_any_element()
            }
            Item::Entry { ws, .. } => {
                let Some(entry) = self.row(item) else {
                    return div().id(("entry", ix)).into_any_element();
                };
                let selected = ws == self.active && current.as_ref() == Some(&entry.path);
                let directory = entry.directory;
                let icon = if !directory {
                    IconName::FileText
                } else if self.workspaces[ws].expanded.contains(&entry.path) {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                };
                let path = entry.path.clone();
                row.id(("entry", ix))
                    .pl(px(22. + entry.depth as f32 * 14.))
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(accent).text_color(theme.sidebar_accent_foreground)
                    })
                    .when(!selected, |d| d.hover(move |s| s.bg(hover)))
                    .child(Icon::new(icon).size_4().flex_shrink_0().text_color(muted))
                    .child(
                        div().min_w_0().truncate().child(
                            entry
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    )
                    .on_click(cx.listener(move |v, e: &ClickEvent, w, cx| {
                        let m = e.modifiers();
                        let target = if m.platform {
                            Target::NewTab
                        } else if m.alt {
                            Target::Split
                        } else {
                            Target::Focused
                        };
                        v.cursor = Some(Cursor {
                            ws,
                            path: Some(path.clone()),
                        });
                        v.tree_focus.focus(w, cx);
                        v.tree_open(target, w, cx);
                    }))
                    .into_any_element()
            }
            Item::Empty(ws) => {
                let ws = &self.workspaces[ws];
                row.id(("empty", ix))
                    .pl(px(46.))
                    .text_xs()
                    .text_color(muted)
                    .child(if ws.indexing || !ws.scanned {
                        "Finding Markdown files…"
                    } else {
                        "No Markdown files"
                    })
                    .into_any_element()
            }
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.ws();
        let items = Rc::new(self.layout());
        let mut positions = vec![0; self.workspaces.len()];
        for (p, &w) in self.order().iter().enumerate() {
            positions[w] = p;
        }
        let positions = Rc::new(positions);
        div()
            .id("folder-pane")
            .key_context("MarkdownTree")
            .track_focus(&self.tree_focus)
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            // Folders dropped here become workspaces; files open in theirs.
            .drag_over::<ExternalPaths>({
                let accent = cx.theme().accent;
                move |style, paths: &ExternalPaths, _, _| {
                    let (files, dirs) = droppable(paths.paths());
                    if files.is_empty() && dirs.is_empty() {
                        style
                    } else {
                        style.bg(accent)
                    }
                }
            })
            .on_drop(cx.listener(|this, dragged: &ExternalPaths, window, cx| {
                this.drop_anywhere(dragged.paths(), window, cx)
            }))
            .child(self.sidebar_header(cx))
            .when(items.is_empty(), |d| {
                d.child(
                    div()
                        .flex_1()
                        .px_4()
                        .py_2()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Add a folder to browse its Markdown files. Each folder keeps its own tabs.")
                        .child(
                            Button::new("empty-add-folder")
                                .small()
                                .icon(IconName::FolderOpen)
                                .label("Add Folder…")
                                .on_click(cx.listener(|v, _, w, cx| v.prompt(true, w, cx))),
                        ),
                )
            })
            .when(!items.is_empty(), |d| {
                d.child(
                    uniform_list(
                        "workspace-tree",
                        items.len(),
                        cx.processor(move |this, range: Range<usize>, window, cx| {
                            // While the sidebar has focus, ring the keyboard's row.
                            let keyboard = this
                                .tree_focus
                                .is_focused(window)
                                .then(|| this.cursor_index(&items, cx))
                                .flatten();
                            range
                                .map(|ix| {
                                    div()
                                        .w_full()
                                        .px_1()
                                        .child(this.sidebar_row(items[ix], ix, &positions, keyboard, cx))
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.tree_scroll)
                    .pb_2()
                    .flex_1(),
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
                    .truncate()
                    .child(if ws.indexing {
                        format!("{} · Indexing…", ws.name())
                    } else if ws.root.is_none() {
                        match ws.rows.len() {
                            0 => "No folder open".to_owned(),
                            1 => "1 open file".to_owned(),
                            n => format!("{n} open files"),
                        }
                    } else {
                        format!(
                            "{} · {} Markdown files{}",
                            ws.name(),
                            ws.files.len(),
                            if ws.limited { " · limit reached" } else { "" }
                        )
                    }),
            )
    }

    fn tab_label(tab: &Tab<Entity<DocPane>>, cx: &App) -> String {
        match &tab.right {
            Some(right) => format!(
                "{}  |  {}",
                tab.left.read(cx).title(),
                right.read(cx).title()
            ),
            None => tab.left.read(cx).title(),
        }
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (accent, muted) = (theme.accent, theme.muted);
        let tabs = &self.ws().tabs;
        let last = tabs.len() - 1;
        let weak = cx.entity().downgrade();
        let mut strip = div().id("tab-strip").flex().min_w_0().overflow_x_scroll();
        for (ix, tab) in tabs.tabs.iter().enumerate() {
            let selected = ix == tabs.active;
            let (pinned, split) = (tab.pinned, tab.is_split());
            let label: SharedString = Self::tab_label(tab, cx).into();
            let ghost = label.clone();
            let weak = weak.clone();
            strip = strip.child(
                div()
                    .id(("tab", ix))
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .gap_1()
                    .h(px(31.))
                    .pl_3()
                    .pr(px(if pinned { 12. } else { 6. }))
                    .text_sm()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(theme.border)
                    .when(selected, |d| {
                        d.bg(theme.background).text_color(theme.foreground)
                    })
                    .when(!selected, |d| {
                        d.text_color(theme.muted_foreground)
                            .hover(move |d| d.bg(muted))
                    })
                    .when(pinned, |d| {
                        d.child(Icon::new(IconName::Star).small().text_color(theme.primary))
                    })
                    .when(split, |d| {
                        d.child(
                            Icon::new(IconName::PanelRight)
                                .small()
                                .text_color(theme.muted_foreground),
                        )
                    })
                    .child(
                        div()
                            .max_w(px(if split {
                                340.
                            } else if pinned {
                                120.
                            } else {
                                200.
                            }))
                            .truncate()
                            .child(label),
                    )
                    .when(!pinned, |d| {
                        d.child(
                            Button::new(("close-tab", ix))
                                .icon(IconName::Close)
                                .ghost()
                                .xsmall()
                                .tooltip("Close tab · ⌘W")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close_tab(ix, window, cx);
                                })),
                        )
                    })
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.activate_tab(ix, window, cx)),
                    )
                    .on_drag(DraggedTab { from: ix }, move |_, _, _, cx| {
                        cx.new(|_| Ghost(ghost.clone()))
                    })
                    .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(move |this, dragged: &DraggedTab, window, cx| {
                        this.ws_mut().tabs.move_tab(dragged.from, ix);
                        this.after_tab_change(window, cx);
                    }))
                    .drag_over::<DraggedPane>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedPane, window, cx| {
                        this.ws_mut().tabs.pane_to_tab(dragged.right, true);
                        this.after_tab_change(window, cx);
                    }))
                    .context_menu(move |menu, _, _| {
                        let item = |label: &'static str,
                                    f: fn(
                            &mut Reader,
                            usize,
                            &mut Window,
                            &mut Context<Reader>,
                        )| {
                            let weak = weak.clone();
                            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                let _ = weak.update(cx, |v, cx| f(v, ix, window, cx));
                            })
                        };
                        menu.item(item(
                            if pinned { "Unpin Tab" } else { "Pin Tab" },
                            |v, ix, w, cx| {
                                v.ws_mut().tabs.toggle_pin(ix);
                                v.after_tab_change(w, cx);
                            },
                        ))
                        .item(item(
                            if split {
                                "Separate Panes into Tabs"
                            } else {
                                "Split Side by Side"
                            },
                            |v, ix, w, cx| {
                                v.ws_mut().tabs.activate(ix);
                                v.toggle_split(w, cx);
                            },
                        ))
                        .separator()
                        .item(item("Close Tab", |v, ix, w, cx| v.close_tab(ix, w, cx)))
                        .item(item("Close Other Tabs", |v, ix, w, cx| {
                            v.ws_mut().tabs.close_others(ix);
                            v.after_tab_change(w, cx);
                        }))
                    }),
            );
        }
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .h(px(32.))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(strip)
            // Dropping on the empty space moves the tab to the end.
            .child(
                div()
                    .id("tab-bar-end")
                    .flex_1()
                    .h_full()
                    .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(move |this, dragged: &DraggedTab, window, cx| {
                        this.ws_mut().tabs.move_tab(dragged.from, last);
                        this.after_tab_change(window, cx);
                    }))
                    .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &ExternalPaths, window, cx| {
                        this.drop_on_tabs(dragged.paths(), window, cx)
                    }))
                    .drag_over::<DraggedPane>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedPane, window, cx| {
                        this.ws_mut().tabs.pane_to_tab(dragged.right, true);
                        this.after_tab_change(window, cx);
                    })),
            )
            .child(
                Button::new("tab-bar-new")
                    .icon(IconName::Plus)
                    .ghost()
                    .xsmall()
                    .tooltip("New tab · ⌘T")
                    .on_click(cx.listener(|this, _, window, cx| this.new_tab(window, cx))),
            )
            .pr_2()
    }

    fn pane_id(base: &'static str, right: bool) -> SharedString {
        if right {
            format!("{base}-right").into()
        } else {
            base.into()
        }
    }

    /// Title strip over each pane of a split. The accent line marks the pane
    /// navigation applies to; drag it to the tab bar to separate the panes.
    fn pane_header(&self, right: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tab = self.tab();
        let focused = tab.focus_right == right;
        let title: SharedString = tab
            .pane(right)
            .map(|p| p.read(cx).title())
            .unwrap_or_default()
            .into();
        let ghost = title.clone();
        div()
            .id(Self::pane_id("pane-header", right))
            .relative()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .h(px(30.))
            .pl_4()
            .pr_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(if focused {
                theme.background
            } else {
                theme.sidebar
            })
            .when(focused, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(2.))
                        .bg(theme.primary),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .when(!focused, |d| d.text_color(theme.muted_foreground))
                    .child(title),
            )
            .child(
                Button::new(Self::pane_id("swap-panes", right))
                    .icon(Icon::new(gpui_kit::assets::IconName::ArrowLeftRight))
                    .ghost()
                    .xsmall()
                    .tooltip("Swap panes")
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.ws_mut().tabs.active_mut().swap();
                        this.after_tab_change(window, cx);
                    })),
            )
            .child(
                Button::new(Self::pane_id("close-pane", right))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("Close pane · ⌘W")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_pane(right, window, cx);
                    })),
            )
            .on_drag(DraggedPane { right }, move |_, _, _, cx| {
                cx.new(|_| Ghost(ghost.clone()))
            })
    }

    /// A pane with its header (when split). Pressing anywhere in it, text
    /// selection included, gives it focus.
    fn pane_frame(&self, right: bool, header: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = self.tab().pane(right).cloned();
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .capture_any_mouse_down(
                cx.listener(move |this, _, window, cx| this.focus_pane(right, window, cx)),
            )
            .when(header, |d| d.child(self.pane_header(right, cx)))
            .children(pane)
    }

    fn panes(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let available = f32::from(window.viewport_size().width)
            - if self.prefs.sidebar {
                SIDEBAR_WIDTH
            } else {
                0.
            };
        let theme = cx.theme();
        let (border, primary) = (theme.border, theme.primary);
        let tab = self.tab();
        let split = tab.is_split();
        let body = if !split {
            self.pane_frame(false, false, cx).into_any_element()
        } else if available < SPLIT_MIN_WIDTH {
            // Too narrow for two: a switcher over the focused pane.
            let mut switcher = div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap_1()
                .p_1()
                .border_b_1()
                .border_color(border);
            for right in [false, true] {
                let title = tab
                    .pane(right)
                    .map(|p| p.read(cx).title())
                    .unwrap_or_default();
                switcher = switcher.child(
                    Button::new(Self::pane_id("pane-switch", right))
                        .label(title)
                        .small()
                        .ghost()
                        .selected(tab.focus_right == right)
                        .on_click(cx.listener(move |this, _, w, cx| this.focus_pane(right, w, cx))),
                );
            }
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(switcher)
                .child(self.pane_frame(tab.focus_right, true, cx))
                .into_any_element()
        } else {
            let panel = |right: bool, cx: &mut Context<Self>| {
                resizable_panel().size_range(px(280.)..Pixels::MAX).child(
                    div()
                        .size_full()
                        .flex()
                        .child(self.pane_frame(right, true, cx)),
                )
            };
            h_resizable("split-panes")
                .with_state(&self.split_state)
                .child(panel(false, cx))
                .child(panel(true, cx))
                .into_any_element()
        };
        let narrow = split && available < SPLIT_MIN_WIDTH;
        // A drag that left the window leaves no stale highlight behind.
        let zone = self.drop_zone.filter(|_| cx.has_active_drag());
        let hint = self.drop_hint.clone();
        let (popover, popover_border) = (cx.theme().popover, cx.theme().border);
        div()
            .id("panes")
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .on_drag_move::<DraggedTab>(cx.listener(
                |this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    let from = event.drag(cx).from;
                    let inside = event.bounds.contains(&event.event.position);
                    let zone = (inside && this.ws().tabs.can_split_with(from)).then(|| {
                        if event.event.position.x > event.bounds.center().x {
                            DropZone::Right
                        } else {
                            DropZone::Left
                        }
                    });
                    if this.drop_zone != zone {
                        this.drop_zone = zone;
                        this.drop_hint = SharedString::default();
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(|this, dragged: &DraggedTab, window, cx| {
                if let Some(zone) = this.drop_zone.take() {
                    this.ws_mut()
                        .tabs
                        .merge_into_active(dragged.from, zone == DropZone::Right);
                }
                this.after_tab_change(window, cx);
            }))
            .on_drag_move::<ExternalPaths>(cx.listener(
                move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let inside = event.bounds.contains(&event.event.position);
                    let right_half = event.event.position.x > event.bounds.center().x;
                    let plan = inside
                        .then(|| this.plan_drop(event.drag(cx).paths(), right_half, narrow, cx))
                        .flatten();
                    let zone = plan.as_ref().map(|p| p.0);
                    if this.drop_zone != zone {
                        this.drop_zone = zone;
                        this.drop_hint = plan.map(|p| p.1).unwrap_or_default();
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(|this, dragged: &ExternalPaths, window, cx| {
                this.drop_on_panes(dragged.paths(), window, cx)
            }))
            .child(body)
            .when_some(zone, |d, zone| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .map(|d| match zone {
                            DropZone::Whole => d.left_0().right_0(),
                            DropZone::Left => d.left_0().w(relative(0.5)),
                            DropZone::Right => d.right_0().w(relative(0.5)),
                        })
                        .p_2()
                        .child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_lg()
                                .border_2()
                                .border_color(primary)
                                .bg(primary.opacity(0.08))
                                .when(!hint.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .bg(popover)
                                            .border_1()
                                            .border_color(popover_border)
                                            .shadow_md()
                                            .child(hint),
                                    )
                                }),
                        ),
                )
            })
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = self.focused_pane();
        let pane = pane.read(cx);
        div()
            .px_5()
            .h(px(28.))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().border)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(if pane.source {
                "SOURCE · READ ONLY"
            } else {
                "MARKDOWN · READ ONLY"
            })
            .child(format!(
                "{}   ·   {} pt",
                pane.document
                    .as_ref()
                    .map(|d| format!("{} words", d.body.split_whitespace().count()))
                    .unwrap_or("Local files".into()),
                self.prefs.font_size
            ))
    }

    fn picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let outline = self.picker == Some(true);
        let ws = self.ws();
        let items: Vec<CommandItem> = if outline {
            self.focused_pane()
                .read(cx)
                .document
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
            ws.files
                .iter()
                .map(|path| {
                    CommandItem::new()
                        .label(
                            path.strip_prefix(ws.root.as_deref().unwrap_or(Path::new("/")))
                                .unwrap_or(path)
                                .to_string_lossy()
                                .into_owned(),
                        )
                        .action(Box::new(OpenPath { path: path.clone() }))
                })
                .collect()
        };
        let empty_msg = if outline {
            "No headings in this document."
        } else if ws.root.is_none() {
            "Add a folder (⇧⌘O) to search its Markdown files."
        } else {
            "No matching files."
        };
        let weak = cx.entity().downgrade();
        let cancel = weak.clone();
        let dismiss = weak.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .pt(px(80.))
            .bg(rgba(0x00000035))
            .on_mouse_down(MouseButton::Left, move |_, w, cx| {
                let _ = dismiss.update(cx, |v, cx| v.close_overlay(w, cx));
            })
            .child(
                div()
                    .w(px(570.))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .rounded_xl()
                    .overflow_hidden()
                    .shadow_md()
                    .child(
                        Command::new(&self.palette)
                            .placeholder(if outline {
                                "Jump to heading…"
                            } else {
                                "Find a Markdown file…"
                            })
                            .max_h(px(320.))
                            .empty(move |_, _, cx| {
                                div()
                                    .py_6()
                                    .text_center()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(empty_msg)
                            })
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
                            .footer(move |_, _, cx| {
                                div()
                                    .px_3()
                                    .py_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if outline {
                                        "↑ ↓ Navigate    ↵ Jump    Esc Close"
                                    } else {
                                        "↑ ↓ Navigate    ↵ Open    ⌘↵ New tab    ⌥↵ Split    Esc Close"
                                    })
                            }),
                    ),
            )
    }

    #[cfg(feature = "screenshot")]
    pub fn ready(&self, cx: &App) -> bool {
        !self.ws().indexing && self.tab().panes().all(|p| p.read(cx).ready(cx))
    }

    #[cfg(feature = "screenshot")]
    pub fn debug_actions(&mut self, actions: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Paths in actions are relative to the workspace folder, else to the
        // first document's folder; `~` stands for a space.
        let base = self.ws().root.clone().or_else(|| {
            self.focused_pane()
                .read(cx)
                .path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
        });
        let resolve = |base: &Option<PathBuf>, name: &str| {
            base.as_ref().map(|b| b.join(name.replace('~', " ")))
        };
        for action in actions.split(',') {
            let (name, arg) = action.split_once(':').unwrap_or((action, ""));
            match name {
                "dark" => Theme::change(ThemeMode::Dark, Some(window), cx),
                "light" => Theme::change(ThemeMode::Light, Some(window), cx),
                "nosidebar" => self.prefs.sidebar = false,
                "browse" => {
                    if let Some(parent) = base.clone() {
                        self.add_folder(parent, window, cx);
                    }
                }
                "workspace" => self.add_folder(PathBuf::from(arg), window, cx),
                "switch" => self.switch_to_nth(arg.parse().unwrap_or(0), window, cx),
                "collapse" => {
                    if let Some(&ws) = self.order().get(arg.parse().unwrap_or(0)) {
                        self.toggle_open(ws, cx);
                    }
                }
                "sidebar-focus" => self.tree_focus.focus(window, cx),
                "tab" => {
                    if let Some(path) = resolve(&self.ws().root.clone().or(base.clone()), arg) {
                        self.open_in(path, Target::NewTab, window, cx);
                    }
                }
                "split" => {
                    if let Some(path) = resolve(&self.ws().root.clone().or(base.clone()), arg) {
                        self.open_in(path, Target::Split, window, cx);
                    }
                }
                "properties" => self.with_pane(cx, |p, cx| {
                    p.properties = true;
                    cx.notify();
                }),
                "source" => self.with_pane(cx, |p, cx| p.toggle_source(cx)),
                "help" => self.with_pane(cx, |p, cx| p.toggle_help(cx)),
                "quick-open" => self.open_picker(false, window, cx),
                "outline" => self.open_picker(true, window, cx),
                "find" => {
                    let pane = self.focused_pane();
                    pane.update(cx, |p, cx| p.set_find_query(arg, window, cx));
                }
                _ => {}
            }
        }
        cx.notify();
    }
}

impl Render for Reader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_tabs = self.ws().tabs.len() > 1;
        div()
            .id("viewer")
            .key_context("Viewer")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_drop(cx.listener(|v, dragged: &ExternalPaths, w, cx| {
                v.drop_anywhere(dragged.paths(), w, cx)
            }))
            .on_action(cx.listener(|v, _: &OpenFile, w, cx| v.prompt(false, w, cx)))
            .on_action(cx.listener(|v, _: &OpenFolder, w, cx| v.prompt(true, w, cx)))
            .on_action(cx.listener(|v, _: &ToggleSidebar, w, cx| v.toggle_sidebar(w, cx)))
            .on_action(cx.listener(|v, _: &FocusSidebar, w, cx| {
                v.prefs.sidebar = true;
                v.tree_focus.focus(w, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &FocusReader, w, cx| v.focus_reader(w, cx)))
            .on_action(cx.listener(|v, _: &QuickOpen, w, cx| v.open_picker(false, w, cx)))
            .on_action(cx.listener(|v, _: &Outline, w, cx| v.open_picker(true, w, cx)))
            .on_action(cx.listener(|v, a: &OpenPath, w, cx| {
                v.picker = None;
                v.open_in(a.path.clone(), Target::Focused, w, cx);
            }))
            .on_action(
                cx.listener(|v, _: &OpenInNewTab, w, cx| v.open_selection(Target::NewTab, w, cx)),
            )
            .on_action(
                cx.listener(|v, _: &OpenInSplit, w, cx| v.open_selection(Target::Split, w, cx)),
            )
            .on_action(cx.listener(|v, a: &JumpHeading, _, cx| {
                v.with_pane(cx, |p, cx| p.jump_heading(a.index, cx))
            }))
            .on_action(cx.listener(|v, _: &Find, w, cx| {
                v.focused_pane().update(cx, |p, cx| p.open_find(w, cx))
            }))
            .on_action(cx.listener(|v, _: &NextMatch, _, cx| {
                v.with_pane(cx, |p, cx| p.next_match(false, cx))
            }))
            .on_action(cx.listener(|v, _: &PrevMatch, _, cx| {
                v.with_pane(cx, |p, cx| p.next_match(true, cx))
            }))
            .on_action(cx.listener(|v, _: &Escape, w, cx| v.close_overlay(w, cx)))
            .on_action(cx.listener(|v, _: &ToggleSource, _, cx| {
                v.with_pane(cx, |p, cx| p.toggle_source(cx))
            }))
            .on_action(cx.listener(|v, _: &ToggleProperties, _, cx| {
                v.with_pane(cx, |p, cx| {
                    p.properties = !p.properties;
                    cx.notify();
                })
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
            .on_action(
                cx.listener(|v, _: &Back, _, cx| v.with_pane(cx, |p, cx| p.navigate(true, cx))),
            )
            .on_action(
                cx.listener(|v, _: &Forward, _, cx| v.with_pane(cx, |p, cx| p.navigate(false, cx))),
            )
            .on_action(cx.listener(|v, _: &Reload, _, cx| {
                v.with_pane(cx, |p, cx| p.reload(cx));
                v.scan(v.active, cx);
            }))
            .on_action(cx.listener(|_, _: &CloseWindow, w, _| w.remove_window()))
            .on_action(cx.listener(|_, _: &FullScreen, w, _| w.toggle_fullscreen()))
            .on_action(cx.listener(|v, _: &Help, _, cx| v.with_pane(cx, |p, cx| p.toggle_help(cx))))
            .on_action(cx.listener(|v, _: &NewTab, w, cx| v.new_tab(w, cx)))
            .on_action(cx.listener(|v, _: &CloseTab, w, cx| v.close_focused(w, cx)))
            .on_action(cx.listener(|v, _: &ReopenClosedTab, w, cx| {
                if v.ws_mut().tabs.reopen() {
                    v.after_tab_change(w, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &NextTab, w, cx| {
                v.ws_mut().tabs.cycle(1);
                v.after_tab_change(w, cx);
            }))
            .on_action(cx.listener(|v, _: &PrevTab, w, cx| {
                v.ws_mut().tabs.cycle(-1);
                v.after_tab_change(w, cx);
            }))
            .on_action(cx.listener(|v, _: &ToggleSplit, w, cx| v.toggle_split(w, cx)))
            .on_action(cx.listener(|v, _: &FocusLeftPane, w, cx| v.focus_pane(false, w, cx)))
            .on_action(cx.listener(|v, _: &FocusRightPane, w, cx| v.focus_pane(true, w, cx)))
            .on_action(cx.listener(|v, a: &SwitchWorkspace, w, cx| v.switch_to_nth(a.index, w, cx)))
            .on_action(cx.listener(|v, _: &NextWorkspace, w, cx| v.cycle_workspace(1, w, cx)))
            .on_action(cx.listener(|v, _: &PrevWorkspace, w, cx| v.cycle_workspace(-1, w, cx)))
            .on_action(
                cx.listener(|v, _: &CloseWorkspace, w, cx| v.close_workspace(v.active, w, cx)),
            )
            .on_action(cx.listener(|v, _: &TreeUp, w, cx| v.tree_move(-1, w, cx)))
            .on_action(cx.listener(|v, _: &TreeDown, w, cx| v.tree_move(1, w, cx)))
            .on_action(cx.listener(|v, _: &TreeOpen, w, cx| v.tree_open(Target::Focused, w, cx)))
            .on_action(cx.listener(|v, _: &TreeLeft, _, cx| v.tree_left(cx)))
            .on_action(cx.listener(|v, _: &TreeRight, w, cx| v.tree_right(w, cx)))
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
                            .when(show_tabs, |d| d.child(self.tab_bar(cx)))
                            .child(self.panes(window, cx))
                            .child(self.footer(cx)),
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
    use markdown_core::Document;

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
    /// Run `f` on the reader with its window.
    fn with_reader<R>(
        w: WindowHandle<Root>,
        view: &Entity<Reader>,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut Reader, &mut Window, &mut Context<Reader>) -> R,
    ) -> R {
        let r = cx
            .update_window(w.into(), |_, w, cx| view.update(cx, |v, cx| f(v, w, cx)))
            .unwrap();
        cx.run_until_parked();
        r
    }
    fn pane<R>(view: &Entity<Reader>, cx: &mut TestAppContext, f: impl FnOnce(&DocPane) -> R) -> R {
        view.read_with(cx, |v, cx| f(v.focused_pane().read(cx)))
    }
    fn folder(names: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (name, body) in names {
            std::fs::write(tmp.path().join(name), body).unwrap();
        }
        tmp
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
            v.with_pane(cx, |p, cx| {
                p.document = Some(Document::parse("# Original\n- [ ] untouched".into()));
                p.present(cx);
            })
        });
        cx.run_until_parked();
        interact(w, cx, |w, cx| {
            w.click("source", cx);
            w.press("cmd-2", cx);
            w.input("must not edit", cx);
        });
        assert!(pane(&view, cx, |p| p.source));
        assert_eq!(
            pane(&view, cx, |p| p.document.as_ref().unwrap().source.clone()),
            "# Original\n- [ ] untouched"
        );
    }

    #[gpui_kit::test]
    fn find_matches_rendered_text_and_escape_dismisses(cx: &mut TestAppContext) {
        let (w, view) = open(cx);
        view.update(cx, |v, cx| {
            v.with_pane(cx, |p, cx| {
                p.document = Some(Document::parse(
                    "# Hello **world**\n\nAnother WORLD.".into(),
                ));
                p.present(cx);
            })
        });
        cx.run_until_parked();
        interact(w, cx, |w, cx| {
            w.press("cmd-f", cx);
            w.render_frame(cx);
            w.input("world", cx);
        });
        assert_eq!(pane(&view, cx, |p| p.matches.len()), 2);
        interact(w, cx, |w, cx| w.press("cmd-g", cx));
        assert_eq!(pane(&view, cx, |p| p.match_index), 1);
        interact(w, cx, |w, cx| w.press("escape", cx));
        assert!(!pane(&view, cx, |p| p.find));
    }

    #[gpui_kit::test]
    fn finder_drops_open_into_the_side_they_land_on(cx: &mut TestAppContext) {
        let tmp = folder(&[
            ("A.md", "# A"),
            ("B.md", "# B"),
            ("C.md", "# C"),
            ("D.md", "# D"),
            ("notes.txt", "not markdown"),
        ]);
        let p = |n: &str| tmp.path().join(n);
        let (w, view) = open(cx);
        let paths_of = |view: &Entity<Reader>, cx: &mut TestAppContext| {
            view.read_with(cx, |v, cx| {
                v.tab()
                    .panes()
                    .map(|p| p.read(cx).path.clone().unwrap())
                    .collect::<Vec<_>>()
            })
        };

        // Nothing droppable: no zone.
        let plan = view.read_with(cx, |v, cx| v.plan_drop(&[p("notes.txt")], true, false, cx));
        assert!(plan.is_none());

        // Two files on an empty tab open side by side.
        with_reader(w, &view, cx, |v, w, cx| {
            let paths = [p("A.md"), p("B.md"), p("notes.txt")];
            let (zone, hint) = v.plan_drop(&paths, true, false, cx).unwrap();
            assert_eq!(zone, DropZone::Whole);
            assert_eq!(hint.as_ref(), "Open side by side");
            v.drop_zone = Some(zone);
            v.drop_on_panes(&paths, w, cx);
        });
        assert_eq!(paths_of(&view, cx), [p("A.md"), p("B.md")]);

        // A file on the left half of a split replaces that pane; extras open
        // as background tabs and the split tab stays active.
        with_reader(w, &view, cx, |v, w, cx| {
            let paths = [p("C.md"), p("D.md")];
            let (zone, hint) = v.plan_drop(&paths, false, false, cx).unwrap();
            assert_eq!(zone, DropZone::Left);
            assert_eq!(hint.as_ref(), "Replace left pane (+1 tab)");
            v.drop_zone = Some(zone);
            v.drop_on_panes(&paths, w, cx);
        });
        assert_eq!(paths_of(&view, cx), [p("C.md"), p("B.md")]);
        assert!(!view.read_with(cx, |v, _| v.tab().focus_right));
        assert_eq!(view.read_with(cx, |v, _| v.ws().tabs.len()), 2);

        // On a single document, the right half splits it.
        with_reader(w, &view, cx, |v, w, cx| {
            v.ws_mut().tabs.activate(1);
            let (zone, _) = v.plan_drop(&[p("A.md")], true, false, cx).unwrap();
            assert_eq!(zone, DropZone::Right);
            v.drop_zone = Some(zone);
            v.drop_on_panes(&[p("A.md")], w, cx);
        });
        assert_eq!(paths_of(&view, cx), [p("D.md"), p("A.md")]);
        assert!(view.read_with(cx, |v, _| v.tab().focus_right));

        // Files on the tab strip open as new tabs.
        with_reader(w, &view, cx, |v, w, cx| {
            v.drop_on_tabs(&[p("B.md"), p("notes.txt")], w, cx)
        });
        assert_eq!(view.read_with(cx, |v, _| v.ws().tabs.len()), 3);
        assert_eq!(paths_of(&view, cx), [p("B.md")]);
    }

    #[gpui_kit::test]
    fn finder_folder_drops_add_workspaces(cx: &mut TestAppContext) {
        let one = folder(&[("A.md", "# A")]);
        let two = folder(&[("B.md", "# B")]);
        let (w, view) = open(cx);
        let before = view.read_with(cx, |v, _| v.workspaces.len());
        with_reader(w, &view, cx, |v, w, cx| {
            v.drop_anywhere(&[one.path().into(), two.path().into()], w, cx)
        });
        view.read_with(cx, |v, _| {
            assert!(v.workspaces.len() > before);
            assert_eq!(v.ws().root.as_deref(), Some(two.path()));
            assert!(
                v.workspaces
                    .iter()
                    .any(|ws| ws.root.as_deref() == Some(one.path()))
            );
            assert!(v.prefs.sidebar);
        });
    }

    #[gpui_kit::test]
    fn quick_open_filters_and_opens_with_keyboard(cx: &mut TestAppContext) {
        let tmp = folder(&[("Alpha.md", "# Alpha"), ("Beta.md", "# Beta")]);
        let second = tmp.path().join("Beta.md");
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(tmp.path().into(), w, cx)
        });
        interact(w, cx, |w, cx| {
            w.press("cmd-p", cx);
            w.render_frame(cx);
            w.input("Beta", cx);
        });
        interact(w, cx, |w, cx| w.press("enter", cx));
        assert_eq!(pane(&view, cx, |p| p.path.clone()), Some(second));
        assert!(view.read_with(cx, |v, _| v.picker.is_none()));
    }

    #[gpui_kit::test]
    fn quick_open_cmd_enter_opens_a_new_tab_and_alt_enter_splits(cx: &mut TestAppContext) {
        let tmp = folder(&[("Alpha.md", "# Alpha"), ("Beta.md", "# Beta")]);
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(tmp.path().into(), w, cx)
        });
        interact(w, cx, |w, cx| {
            w.press("cmd-p", cx);
            w.render_frame(cx);
            w.input("Alpha", cx);
        });
        interact(w, cx, |w, cx| w.press("cmd-enter", cx));
        assert_eq!(view.read_with(cx, |v, _| v.ws().tabs.len()), 2);
        assert_eq!(pane(&view, cx, |p| p.title()), "Alpha.md");

        interact(w, cx, |w, cx| {
            w.press("cmd-p", cx);
            w.render_frame(cx);
            w.input("Beta", cx);
        });
        interact(w, cx, |w, cx| w.press("alt-enter", cx));
        let (split, right) = view.read_with(cx, |v, _| (v.tab().is_split(), v.tab().focus_right));
        assert!(split && right);
        assert_eq!(pane(&view, cx, |p| p.title()), "Beta.md");
    }

    #[gpui_kit::test]
    fn tabs_split_close_and_reopen(cx: &mut TestAppContext) {
        let tmp = folder(&[("A.md", "# A"), ("B.md", "# B")]);
        let (a, b) = (tmp.path().join("A.md"), tmp.path().join("B.md"));
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_in(a.clone(), Target::Focused, w, cx);
            v.open_in(b.clone(), Target::NewTab, w, cx);
        });
        assert_eq!(view.read_with(cx, |v, _| v.ws().tabs.len()), 2);

        // ⌘\ shows B twice side by side; actions follow the focused pane.
        interact(w, cx, |w, cx| w.press("cmd-\\", cx));
        assert!(view.read_with(cx, |v, _| v.tab().is_split()));
        interact(w, cx, |w, cx| w.press("cmd-shift-m", cx));
        let sources = view.read_with(cx, |v, cx| {
            v.tab()
                .panes()
                .map(|p| p.read(cx).source)
                .collect::<Vec<_>>()
        });
        assert_eq!(sources, [false, true], "only the focused pane toggles");

        // ⌘W closes the focused pane first, then the tab.
        interact(w, cx, |w, cx| w.press("cmd-w", cx));
        assert!(!view.read_with(cx, |v, _| v.tab().is_split()));
        interact(w, cx, |w, cx| w.press("cmd-w", cx));
        assert_eq!(view.read_with(cx, |v, _| v.ws().tabs.len()), 1);
        assert_eq!(pane(&view, cx, |p| p.path.clone()), Some(a));
        interact(w, cx, |w, cx| w.press("cmd-shift-t", cx));
        assert_eq!(view.read_with(cx, |v, _| v.ws().tabs.len()), 2);
        assert_eq!(pane(&view, cx, |p| p.path.clone()), Some(b));
        interact(w, cx, |w, cx| w.press("ctrl-tab", cx));
        assert_eq!(view.read_with(cx, |v, _| v.ws().tabs.active), 0);
    }

    #[gpui_kit::test]
    fn workspaces_keep_their_own_tabs_and_files_route_home(cx: &mut TestAppContext) {
        let one = folder(&[("One.md", "# One")]);
        let two = folder(&[("Two.md", "# Two")]);
        let loose = folder(&[("Loose.md", "# Loose")]);
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(one.path().into(), w, cx);
            v.open_in(one.path().join("One.md"), Target::Focused, w, cx);
            v.add_folder(two.path().into(), w, cx);
        });
        // The empty Files workspace was adopted by the first folder.
        assert_eq!(
            view.read_with(cx, |v, _| (v.workspaces.len(), v.active)),
            (2, 1)
        );
        assert!(pane(&view, cx, |p| p.is_blank()));

        // A file under folder one goes back to its workspace and tab.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_path(one.path().join("One.md"), w, cx)
        });
        assert_eq!(
            view.read_with(cx, |v, _| (v.active, v.ws().tabs.len())),
            (0, 1)
        );

        // A file outside every folder gets a Files workspace.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_path(loose.path().join("Loose.md"), w, cx)
        });
        assert_eq!(
            view.read_with(cx, |v, _| (v.workspaces.len(), v.active)),
            (3, 2)
        );
        assert_eq!(pane(&view, cx, |p| p.title()), "Loose.md");

        interact(w, cx, |w, cx| w.press("ctrl-2", cx));
        assert_eq!(view.read_with(cx, |v, _| v.active), 1);
        assert!(
            view.read_with(cx, |v, _| v.ws().files.len() == 1),
            "rescanned on switch"
        );

        // Saved and restored.
        let saved = view.read_with(cx, |v, _| v.prefs.workspaces.clone());
        assert_eq!(saved.len(), 3);
        assert_eq!(saved[0].tabs[0].left.path, Some(one.path().join("One.md")));
    }

    #[gpui_kit::test]
    fn sidebar_folders_open_close_and_reorder_without_switching(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let (alpha, zeta) = (tmp.path().join("alpha"), tmp.path().join("zeta"));
        for (dir, names) in [(&zeta, &["z1.md", "z2.md"][..]), (&alpha, &["a.md"][..])] {
            std::fs::create_dir(dir).unwrap();
            for name in names {
                std::fs::write(dir.join(name), "# Doc").unwrap();
            }
        }
        let (w, view) = open(cx);
        // Added zeta first, so manual order is zeta, alpha; alpha is active.
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(zeta.clone(), w, cx);
            v.add_folder(alpha.clone(), w, cx);
        });
        let roots = |view: &Entity<Reader>, cx: &mut TestAppContext| {
            view.read_with(cx, |v, _| {
                v.order()
                    .into_iter()
                    .map(|i| v.workspaces[i].name())
                    .collect::<Vec<_>>()
            })
        };
        let active = |view: &Entity<Reader>, cx: &mut TestAppContext| {
            view.read_with(cx, |v, _| v.ws().name())
        };
        assert_eq!(roots(&view, cx), ["zeta", "alpha"]);
        assert_eq!(active(&view, cx), "alpha");
        assert_eq!(view.read_with(cx, |v, _| v.layout().len()), 5, "both open");

        // Closing and reopening a folder never switches workspaces.
        with_reader(w, &view, cx, |v, _, cx| v.toggle_open(0, cx));
        assert_eq!(view.read_with(cx, |v, _| v.layout().len()), 3);
        assert_eq!(active(&view, cx), "alpha");
        with_reader(w, &view, cx, |v, _, cx| v.toggle_open(0, cx));

        // The keyboard crosses workspaces; ↑ onto another workspace's file
        // only moves the cursor, ↵ opens it there.
        interact(w, cx, |w, cx| {
            w.press("cmd-1", cx);
            w.press("up", cx);
        });
        assert_eq!(active(&view, cx), "alpha");
        assert!(pane(&view, cx, |p| p.is_blank()));
        interact(w, cx, |w, cx| w.press("enter", cx));
        assert_eq!(active(&view, cx), "zeta");
        assert_eq!(pane(&view, cx, |p| p.title()), "z2.md");

        // ← on a file goes up to its folder row, and ← again closes it.
        interact(w, cx, |w, cx| {
            w.press("left", cx);
            w.press("left", cx);
        });
        assert!(view.read_with(cx, |v, _| !v.ws().open));
        assert_eq!(active(&view, cx), "zeta");

        // Dragging alpha onto zeta reorders, keeping the active workspace.
        with_reader(w, &view, cx, |v, _, cx| v.move_workspace(1, 0, cx));
        assert_eq!(roots(&view, cx), ["alpha", "zeta"]);
        assert_eq!(active(&view, cx), "zeta");

        // Sorting by recent use puts zeta first, and ⌃1 follows the sidebar.
        with_reader(w, &view, cx, |v, _, cx| v.set_sort(Sort::Recent, cx));
        assert_eq!(roots(&view, cx), ["zeta", "alpha"]);
        interact(w, cx, |w, cx| w.press("ctrl-2", cx));
        assert_eq!(active(&view, cx), "alpha");
        assert_eq!(roots(&view, cx), ["alpha", "zeta"]);

        // Collapse all keeps the active workspace open; again closes it too.
        with_reader(w, &view, cx, |v, _, cx| {
            for ws in &mut v.workspaces {
                ws.open = true;
            }
            v.collapse_all(cx);
        });
        let open = |view: &Entity<Reader>, cx: &mut TestAppContext| {
            view.read_with(cx, |v, _| {
                v.workspaces
                    .iter()
                    .filter(|w| w.open)
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(open(&view, cx), ["alpha"]);
        with_reader(w, &view, cx, |v, _, cx| v.collapse_all(cx));
        assert!(open(&view, cx).is_empty());

        let saved = view.read_with(cx, |v, _| v.prefs.clone());
        assert!(
            saved
                .workspaces
                .iter()
                .all(|w| w.collapsed && w.last_used > 0)
        );
        assert_eq!(saved.sidebar_sort, Sort::Recent);
    }

    #[gpui_kit::test]
    fn open_inactive_workspaces_are_watched_and_rescanned(cx: &mut TestAppContext) {
        let one = folder(&[("One.md", "# One")]);
        let two = folder(&[("Two.md", "# Two")]);
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(one.path().into(), w, cx);
            v.add_folder(two.path().into(), w, cx);
        });
        let watched = view.read_with(cx, |v, _| v.watched.len());
        assert_eq!(watched, 2, "both open folders");
        let added = one.path().join("Added.md");
        std::fs::write(&added, "# Added").unwrap();
        with_reader(w, &view, cx, |v, _, cx| v.files_changed(&[added], cx));
        assert_eq!(view.read_with(cx, |v, _| v.workspaces[0].files.len()), 2);
        assert_eq!(view.read_with(cx, |v, _| v.active), 1, "still on two");

        // A closed, inactive folder stops being watched.
        with_reader(w, &view, cx, |v, _, cx| v.toggle_open(0, cx));
        assert_eq!(view.read_with(cx, |v, _| v.watched.len()), 1);
    }

    #[gpui_kit::test]
    fn loose_files_list_their_open_documents(cx: &mut TestAppContext) {
        let tmp = folder(&[("A.md", "# A"), ("B.md", "# B")]);
        let (w, view) = open(cx);
        assert!(
            view.read_with(cx, |v, _| v.layout().is_empty()),
            "empty state"
        );
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_path(tmp.path().join("A.md"), w, cx);
            v.open_path(tmp.path().join("B.md"), w, cx);
        });
        let names = view.read_with(cx, |v, _| {
            v.layout()
                .into_iter()
                .map(|i| match i {
                    Item::Header(ws) => v.workspaces[ws].name(),
                    item => v
                        .row(item)
                        .unwrap()
                        .path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into(),
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(names, ["Open Files", "A.md", "B.md"]);
        // Clicking an open document goes to its tab.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_from_tree(v.active, tmp.path().join("A.md"), Target::Focused, w, cx)
        });
        assert_eq!(
            view.read_with(cx, |v, _| (v.ws().tabs.len(), v.ws().tabs.active)),
            (2, 0)
        );
    }

    #[gpui_kit::test]
    fn copy_source_writes_markdown_to_clipboard(cx: &mut TestAppContext) {
        let tmp = folder(&[("Doc.md", "# Doc\n\nbody")]);
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_in(tmp.path().join("Doc.md"), Target::Focused, w, cx)
        });
        view.update(cx, |v, cx| {
            v.with_pane(cx, |p, cx| {
                p.source = true;
                p.copy_source(cx);
            })
        });
        assert!(pane(&view, cx, |p| p.copied));
        let text = cx.read_from_clipboard().and_then(|c| c.text());
        assert!(text.is_some_and(|t| t.contains("# Doc")));
    }

    #[gpui_kit::test]
    fn cmd_k_opens_quick_open_and_escape_closes(cx: &mut TestAppContext) {
        let (w, view) = open(cx);
        interact(w, cx, |w, cx| w.press("cmd-k", cx));
        assert_eq!(view.read_with(cx, |v, _| v.picker), Some(false));
        interact(w, cx, |w, cx| w.press("escape", cx));
        assert!(view.read_with(cx, |v, _| v.picker.is_none()));
    }

    #[gpui_kit::test]
    fn rapid_navigation_keeps_latest_file_and_handles_deletion(cx: &mut TestAppContext) {
        let tmp = folder(&[
            ("First.md", "# First"),
            ("Second.md", "# Second\n- [ ] untouched"),
        ]);
        let (first, second) = (tmp.path().join("First.md"), tmp.path().join("Second.md"));
        let (_, view) = open(cx);
        view.update(cx, |v, cx| {
            v.with_pane(cx, |p, cx| {
                p.load_file(first.clone(), true, cx);
                p.load_file(second.clone(), true, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(
            pane(&view, cx, |p| p.document.as_ref().map(|d| d.body.clone())),
            Some("# Second\n- [ ] untouched".into())
        );
        view.update(cx, |v, cx| v.with_pane(cx, |p, cx| p.navigate(true, cx)));
        cx.run_until_parked();
        assert_eq!(pane(&view, cx, |p| p.path.clone()), Some(first.clone()));
        std::fs::remove_file(&first).unwrap();
        view.update(cx, |v, cx| {
            v.with_pane(cx, |p, cx| p.load_file(first, false, cx))
        });
        cx.run_until_parked();
        assert!(pane(&view, cx, |p| p.error.is_some()));
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            "# Second\n- [ ] untouched"
        );
    }
}
