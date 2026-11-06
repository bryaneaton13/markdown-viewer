//! The window: title-bar toolbar, workspace sidebar, tab strip, and one or
//! two document panes side by side. Document state lives in `DocPane`; this
//! view routes actions to the focused pane and owns workspaces and tabs.
//! The sidebar lists every workspace as a folder that opens and closes
//! (layout in `sidebar.rs`). Tabs belong to the window and can mix files
//! from any workspace; the active workspace follows the focused document
//! and scopes Quick Open.

use crate::{
    actions::*,
    chrome::{self, Palette},
    files, info, names,
    pane::{DocPane, FontSize, PaneEvent},
    pet::Nib,
    preferences::{Preferences, SavedPane, SavedTab, SavedWorkspace},
    sidebar::{self, Item, Sort},
    tabs::{Tab, TabSet},
    workspace::{Workspace, home_for, name_of, now_millis},
};
use gpui_kit::{
    assets::IconName as Lucide,
    base::TestSupportExt,
    component::{
        button::*,
        command::{Command, CommandGroup, CommandItem, CommandState},
        menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
        popover::Popover,
        resizable::{ResizableState, h_resizable, resizable_panel},
        tooltip::Tooltip,
        *,
    },
    prelude::FluentBuilder,
    *,
};
use notify::Watcher;
use std::{
    collections::{HashMap, HashSet},
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
/// Dropping within this share of the panes' width from the left or right
/// edge splits; the middle opens in place. Clamped so the edge stays easy to
/// hit in narrow windows and the middle stays wide in large ones.
const EDGE_SHARE: f32 = 0.25;
const EDGE_MIN: f32 = 72.;
const EDGE_MAX: f32 = 240.;

/// Where a file opens.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    /// A single click in the sidebar: the tab already showing it, else the
    /// preview tab (or an empty one), else a new preview tab.
    Preview,
    /// Like `Preview`, then keep that tab open: a double-click, ↵, Quick
    /// Open, or a file opened from Finder.
    Keep,
    /// Replace the focused pane's document, keeping its tab open.
    Focused,
    /// A new tab beside the active one.
    NewTab,
    /// The other pane of the active tab, splitting it first if needed.
    Split,
}

/// Where a drag over the panes would land.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DropZone {
    /// An empty tab: one file opens, two open side by side.
    Whole,
    /// Replace the visible pane's document: the middle of a single pane, or
    /// a narrow split.
    Here,
    /// Split to this side of a single pane, or replace this pane of a split.
    Left,
    Right,
}

/// Where the pointer sits over the panes.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Spot {
    /// Close to the left (`Some(false)`) or right (`Some(true)`) edge.
    edge: Option<bool>,
    right_half: bool,
}

impl Spot {
    fn at(x: f32, left: f32, width: f32) -> Self {
        let edge = (width * EDGE_SHARE)
            .clamp(EDGE_MIN, EDGE_MAX)
            .min(width / 2.);
        let off = x - left;
        Self {
            edge: if off < edge {
                Some(false)
            } else if width - off < edge {
                Some(true)
            } else {
                None
            },
            right_half: off > width / 2.,
        }
    }

    fn of<T>(event: &DragMoveEvent<T>) -> Self {
        let (b, x) = (event.bounds, event.event.position.x);
        Self::at(f32::from(x), f32::from(b.left()), f32::from(b.size.width))
    }
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

/// Payload while a file is dragged out of the sidebar's tree.
#[derive(Clone)]
struct DraggedFile {
    path: PathBuf,
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
    /// The workspace Quick Open searches and the sidebar marks: the one the
    /// focused document lives in, or the one last switched to.
    active: usize,
    tabs: TabSet<Entity<DocPane>>,
    /// The focused document `active` last followed, so a pane finishing a
    /// load doesn't undo switching workspaces.
    followed: Option<PathBuf>,
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
    _appearance: Option<Subscription>,
    /// The toolbar's appearance panel is open.
    appearance_open: bool,
    /// The footer's document info panel is open.
    info_open: bool,
    /// Nib, alive only while that panel is open.
    nib: Option<Entity<Nib>>,
}

impl Reader {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = Preferences::load();
        chrome::apply(Self::mode(prefs.dark, window), prefs.palette, cx);
        // "Match System" follows the Mac's appearance as it changes.
        let appearance = cx.observe_window_appearance(window, |this, window, cx| {
            if this.prefs.dark.is_none() {
                this.apply_theme(window, cx);
            }
        });
        cx.set_global(FontSize(prefs.font_size));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let tabs = prefs
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
                tab.preview = t.preview && !t.pinned && tab.right.is_none();
                tab
            })
            .collect();
        let tabs = TabSet::restore(tabs, prefs.active_tab, || Self::make_pane(None, window, cx));
        let mut this = Self {
            focus,
            tree_focus: cx.focus_handle(),
            palette: cx.new(|cx| CommandState::new(window, cx)),
            workspaces: Vec::new(),
            active: 0,
            tabs,
            followed: None,
            picker: None,
            cursor: None,
            tree_scroll: UniformListScrollHandle::new(),
            split_state: cx.new(|_| ResizableState::default()),
            drop_zone: None,
            drop_hint: SharedString::default(),
            watched: Vec::new(),
            _watch: None,
            _watch_task: None,
            _appearance: Some(appearance),
            appearance_open: false,
            info_open: false,
            nib: None,
            prefs,
        };
        let saved = std::mem::take(&mut this.prefs.workspaces);
        for ws in &saved {
            if ws.root.as_ref().is_some_and(|r| !r.is_dir())
                || this.workspaces.iter().any(|w| w.root == ws.root)
            {
                continue;
            }
            let mut workspace = Workspace::new(ws.root.clone());
            workspace.expanded = ws.expanded.iter().cloned().collect();
            workspace.open = !ws.collapsed;
            workspace.last_used = ws.last_used;
            this.workspaces.push(workspace);
        }
        if this.workspaces.is_empty() {
            this.workspaces.push(Workspace::new(None));
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
        if self.focused_pane().read(cx).path != self.followed {
            self.sync_active(cx);
        }
        self.rewatch(cx);
        self.save(cx);
        cx.notify();
    }

    // MARK: Accessors

    fn ws(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    fn tab(&self) -> &Tab<Entity<DocPane>> {
        self.tabs.active()
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

    /// The workspace a file belongs to: the deepest folder containing it,
    /// else "Open Files".
    fn home_of(&self, path: &Path) -> Option<usize> {
        let roots: Vec<_> = self.workspaces.iter().map(|w| w.root.as_deref()).collect();
        home_for(&roots, path)
    }

    /// Whether `path` is listed under workspace `ix`: inside its folder, or
    /// for "Open Files", outside every folder.
    fn belongs(&self, ix: usize, path: &Path) -> bool {
        match &self.workspaces[ix].root {
            Some(root) => path.starts_with(root),
            None => !self.workspaces.iter().any(|w| w.contains(path)),
        }
    }

    fn pane_in(&self, ix: usize, pane: &Entity<DocPane>, cx: &App) -> bool {
        pane.read(cx)
            .path
            .as_deref()
            .is_some_and(|p| self.belongs(ix, p))
    }

    // MARK: Opening

    /// Open from Finder, the command line, or the Open panel: a folder
    /// becomes a workspace; a file opens in a tab already showing it, an
    /// empty focused pane, or a new tab.
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
        let adopt = self
            .workspaces
            .iter()
            .position(|w| w.root.is_none() && w.rows.iter().all(|r| r.path.starts_with(&path)));
        if let Some(ix) = adopt {
            let ws = &mut self.workspaces[ix];
            ws.root = Some(path);
            ws.expanded.clear();
            ws.scanned = false;
            self.active = ix;
        } else {
            self.workspaces.push(Workspace::new(Some(path)));
            self.active = self.workspaces.len() - 1;
        }
        self.after_workspace_change(window, cx);
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_in(path, Target::Keep, window, cx);
    }

    /// Open `path` in the preview tab, the way a single click in the
    /// sidebar does, and return the tab it landed in. A tab already showing
    /// it comes forward instead; an empty tab is used before a new one.
    fn open_preview(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        if self.show_existing(&path, window, cx) {
            return self.tabs.active;
        }
        let blank = (!self.tab().is_split() && self.focused_pane().read(cx).is_blank())
            .then_some(self.tabs.active);
        match self.tabs.preview_index().or(blank) {
            Some(ix) => {
                self.tabs.activate(ix);
                let tab = self.tabs.active_mut();
                tab.preview = !tab.pinned;
                let pane = tab.left.clone();
                pane.update(cx, |p, cx| p.load_file(path, true, cx));
            }
            None => {
                let pane = Self::make_pane(None, window, cx);
                pane.update(cx, |p, cx| p.load_file(path, true, cx));
                self.tabs.insert_after_active(Tab::preview(pane));
            }
        }
        self.after_tab_change(window, cx);
        self.tabs.active
    }

    /// Keep the preview tab showing `path` open, as a double-click does.
    fn keep_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let showing = self
            .tabs
            .tabs
            .iter()
            .position(|t| t.preview && t.left.read(cx).path.as_deref() == Some(path));
        if let Some(ix) = showing {
            self.keep_tab(ix, cx);
        }
    }

    fn keep_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.tabs.keep(ix) {
            self.save(cx);
            cx.notify();
        }
    }

    /// Bring forward the tab and pane already showing `path`, if any.
    fn show_existing(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let showing = self.tabs.tabs.iter().enumerate().find_map(|(i, t)| {
            let right = t
                .panes()
                .position(|p| p.read(cx).path.as_deref() == Some(path))?;
            Some((i, right == 1))
        });
        let Some((tab, right)) = showing else {
            return false;
        };
        self.tabs.activate(tab);
        self.tabs.active_mut().focus_right = right;
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
        self.set_active(ws, cx);
        self.open_in(path, target, window, cx);
    }

    /// Open a file from the tree, Quick Open, Finder or a drop.
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
            Target::Preview => {
                self.open_preview(path, window, cx);
            }
            Target::Keep => {
                let ix = self.open_preview(path, window, cx);
                self.keep_tab(ix, cx);
            }
            Target::Focused => {
                load(&self.focused_pane(), cx);
                self.tabs.active_mut().preview = false;
                self.after_tab_change(window, cx);
            }
            Target::NewTab => {
                let pane = Self::make_pane(None, window, cx);
                load(&pane, cx);
                self.tabs.insert_after_active(Tab::new(pane));
                self.after_tab_change(window, cx);
            }
            Target::Split => {
                if self.tab().is_split() {
                    let tab = self.tabs.active_mut();
                    tab.focus_right = !tab.focus_right;
                    load(&self.focused_pane(), cx);
                } else {
                    let pane = Self::make_pane(None, window, cx);
                    load(&pane, cx);
                    self.tabs.active_mut().split_with(pane, true);
                }
                self.after_tab_change(window, cx);
            }
        }
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

    // MARK: Dropping files

    /// Files and folders dropped outside the panes and tab strip: the same
    /// as opening them from Finder.
    fn drop_anywhere(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        self.drop_zone = None;
        let (files, dirs) = droppable(paths);
        for path in dirs.into_iter().chain(files) {
            self.open_path(path, window, cx);
        }
    }

    /// Files dropped on the tab strip open as new tabs.
    fn drop_on_tabs(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let (files, dirs) = droppable(paths);
        if !dirs.is_empty() {
            return self.drop_anywhere(paths, window, cx);
        }
        for path in files {
            self.open_in(path, Target::NewTab, window, cx);
        }
    }

    /// What dropping `paths` (from Finder or the sidebar) at `spot` would
    /// do, or `None` when there is nothing to open. Near an outer edge a
    /// single pane splits; anywhere else the file opens in place.
    fn plan_drop(
        &self,
        paths: &[PathBuf],
        spot: Spot,
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
        let side = |right: bool| {
            if right {
                (DropZone::Right, "right")
            } else {
                (DropZone::Left, "left")
            }
        };
        if tab.is_split() {
            if narrow {
                return Some((DropZone::Here, format!("Open here{}", more(n - 1)).into()));
            }
            let (zone, side) = side(spot.right_half);
            return Some((zone, format!("Replace {side} pane{}", more(n - 1)).into()));
        }
        if self.focused_pane().read(cx).is_blank() {
            let hint = if n == 1 {
                "Open".to_owned()
            } else {
                format!("Open side by side{}", more(n - 2))
            };
            return Some((DropZone::Whole, hint.into()));
        }
        Some(match spot.edge {
            Some(right) => {
                let (zone, side) = side(right);
                (zone, format!("Open on the {side}{}", more(n - 1)).into())
            }
            None => (DropZone::Here, format!("Open here{}", more(n - 1)).into()),
        })
    }

    /// Track a file drag over the panes, from Finder or the sidebar.
    fn drag_files_over(
        &mut self,
        paths: &[PathBuf],
        inside: bool,
        spot: Spot,
        narrow: bool,
        cx: &mut Context<Self>,
    ) {
        let plan = inside
            .then(|| self.plan_drop(paths, spot, narrow, cx))
            .flatten();
        let zone = plan.as_ref().map(|p| p.0);
        let hint = plan.map(|p| p.1).unwrap_or_default();
        if self.drop_zone != zone || self.drop_hint != hint {
            self.drop_zone = zone;
            self.drop_hint = hint;
            cx.notify();
        }
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
        let keep = self.tabs.active;
        match zone {
            DropZone::Whole => {
                let fill_split = !self.tab().is_split();
                self.open_in(first, Target::Focused, window, cx);
                if fill_split && let Some(second) = files.next() {
                    self.open_in(second, Target::Split, window, cx);
                }
            }
            DropZone::Here => self.open_in(first, Target::Focused, window, cx),
            DropZone::Left | DropZone::Right => {
                let right = zone == DropZone::Right;
                if self.tab().is_split() {
                    self.tabs.active_mut().focus_right = right;
                    self.open_in(first, Target::Focused, window, cx);
                } else {
                    let pane = Self::make_pane(None, window, cx);
                    pane.update(cx, |p, cx| p.load_file(first, true, cx));
                    self.tabs.active_mut().split_with(pane, right);
                }
            }
        }
        for path in files {
            self.open_in(path, Target::NewTab, window, cx);
        }
        self.tabs.activate(keep);
        self.after_tab_change(window, cx);
    }

    // MARK: Workspaces

    /// Make a workspace active without touching the tabs.
    fn set_active(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.workspaces.len() {
            return;
        }
        let changed = ix != self.active;
        self.active = ix;
        let ws = &mut self.workspaces[ix];
        ws.last_used = now_millis();
        // Unwatched folders may have changed since they were last scanned.
        let root = ws.root.clone();
        if !ws.scanned || (changed && !self.watched.iter().any(|(p, _)| Some(p) == root.as_ref())) {
            self.scan(ix, cx);
        }
    }

    /// Switch to a workspace from the keyboard or the sidebar: it opens in
    /// the sidebar, and a tab showing one of its files comes forward.
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

    /// Remove a workspace from the sidebar and close the panes showing its
    /// files (⇧⌘T brings them back). Tabs from other workspaces stay.
    fn close_workspace(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.workspaces.len() {
            return;
        }
        let close: HashSet<_> = self
            .tabs
            .panes()
            .filter(|p| self.pane_in(ix, p, cx))
            .map(|p| p.entity_id())
            .collect();
        let blank = Self::make_pane(None, window, cx);
        self.tabs
            .close_panes(|p| close.contains(&p.entity_id()), || blank);
        self.workspaces.remove(ix);
        if self.workspaces.is_empty() {
            self.workspaces.push(Workspace::new(None));
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
        self.refresh_loose(cx);
        self.sync_active(cx);
        self.settle(window, cx);
    }

    fn after_workspace_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        self.drop_zone = None;
        self.workspaces[self.active].open = true;
        self.set_active(self.active, cx);
        // Bring forward a tab from this workspace unless one already shows.
        let ix = self.active;
        if !self.pane_in(ix, &self.focused_pane(), cx) {
            let found = self.tabs.tabs.iter().enumerate().find_map(|(i, t)| {
                if self.pane_in(ix, t.focused(), cx) {
                    Some((i, t.focus_right))
                } else {
                    let right = t.panes().position(|p| self.pane_in(ix, p, cx))?;
                    Some((i, right == 1))
                }
            });
            if let Some((tab, right)) = found {
                self.tabs.activate(tab);
                self.tabs.active_mut().focus_right = right;
            }
        }
        self.followed = self.focused_pane().read(cx).path.clone();
        self.refresh_loose(cx);
        self.settle(window, cx);
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

    /// "Open Files" lists the open documents outside every folder, and is
    /// added back when one opens.
    fn refresh_loose(&mut self, cx: &App) {
        let paths: Vec<PathBuf> = self
            .tabs
            .panes()
            .filter_map(|p| p.read(cx).path.clone())
            .filter(|p| !self.workspaces.iter().any(|w| w.contains(p)))
            .collect();
        match self.workspaces.iter_mut().find(|w| w.root.is_none()) {
            Some(ws) => ws.set_loose_rows(paths),
            None if !paths.is_empty() => {
                let mut ws = Workspace::new(None);
                ws.scanned = true;
                ws.set_loose_rows(paths);
                self.workspaces.push(ws);
            }
            None => {}
        }
    }

    /// The active workspace follows the focused document to its folder.
    fn sync_active(&mut self, cx: &mut Context<Self>) {
        let path = self.focused_pane().read(cx).path.clone();
        self.followed = path.clone();
        let Some(path) = path else {
            return;
        };
        if !self.belongs(self.active, &path)
            && let Some(ix) = self.home_of(&path)
        {
            self.set_active(ix, cx);
        }
    }

    // MARK: Tabs and panes

    /// After tabs, panes or their documents change: the active workspace
    /// follows the focused document.
    fn after_tab_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_loose(cx);
        self.sync_active(cx);
        self.settle(window, cx);
    }

    /// Keep keyboard focus inside the visible content, so window actions
    /// keep reaching this view, and save.
    fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tree_focus.is_focused(window) {
            self.focus_reader(window, cx);
        }
        self.rewatch(cx);
        self.save(cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = Self::make_pane(None, window, cx);
        self.tabs.insert_after_active(Tab::new(pane));
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
        } else if self.tabs.len() > 1 {
            let ix = self.tabs.active;
            self.close_tab(ix, window, cx);
        } else if !self.focused_pane().read(cx).is_blank() {
            let pane = Self::make_pane(None, window, cx);
            let old = std::mem::replace(&mut self.tabs.active_mut().left, pane);
            self.tabs.remember_pane(old);
            self.after_tab_change(window, cx);
        } else {
            window.remove_window();
        }
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() == 1 {
            self.close_focused(window, cx);
        } else if self.tabs.close(ix) {
            self.after_tab_change(window, cx);
        }
    }

    fn close_pane(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.tabs.active_mut().take_pane(right) {
            self.tabs.remember_pane(pane);
            self.after_tab_change(window, cx);
        }
    }

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(ix) {
            self.after_tab_change(window, cx);
        }
    }

    /// ⌘\: show the focused document again beside itself, or split the two
    /// panes back into separate tabs.
    fn toggle_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().is_split() {
            self.tabs.pane_to_tab(true, false);
        } else {
            let pane = Self::make_pane(None, window, cx);
            if let Some(path) = self.focused_pane().read(cx).path.clone() {
                pane.update(cx, |p, cx| p.load_file(path, true, cx));
            }
            self.tabs.active_mut().split_with(pane, true);
        }
        self.after_tab_change(window, cx);
    }

    fn focus_pane(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tabs.active_mut();
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
    /// open in the sidebar, plus the folders of open files outside those.
    /// Closed workspaces rescan when reopened.
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
        for pane in self.tabs.panes() {
            if let Some(parent) = pane.read(cx).path.as_ref().and_then(|p| p.parent())
                && !wanted
                    .iter()
                    .any(|(p, recursive)| p == parent || (*recursive && parent.starts_with(p)))
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
        let panes: Vec<_> = self.tabs.panes().cloned().collect();
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
                    collapsed: !ws.open,
                    last_used: ws.last_used,
                    ..Default::default()
                }
            })
            .collect();
        self.prefs.tabs = self
            .tabs
            .tabs
            .iter()
            .map(|t| SavedTab {
                left: t.left.read(cx).to_saved(),
                right: t.right.as_ref().map(|p| p.read(cx).to_saved()),
                focus_right: t.focus_right,
                pinned: t.pinned,
                preview: t.preview,
            })
            .collect();
        self.prefs.active_tab = self.tabs.active;
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
    /// workspace previews it in the preview tab; other workspaces wait for
    /// ↵ or a click.
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
            self.open_from_tree(item.ws(), row.path, Target::Preview, window, cx);
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

    /// ↵ or a click: open or close a folder, or open a file (a click
    /// previews it; ↵ keeps it open).
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

    /// Light or dark: the saved choice, else the system's.
    fn mode(dark: Option<bool>, window: &Window) -> ThemeMode {
        match dark {
            Some(true) => ThemeMode::Dark,
            Some(false) => ThemeMode::Light,
            None => window.appearance().into(),
        }
    }

    fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        chrome::apply(Self::mode(self.prefs.dark, window), self.prefs.palette, cx);
        cx.notify();
    }

    /// Light (`Some(false)`), dark (`Some(true)`) or match the system (`None`).
    fn set_appearance(&mut self, dark: Option<bool>, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.dark = dark;
        self.apply_theme(window, cx);
        self.prefs.save();
    }

    fn set_palette(&mut self, palette: Palette, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.palette = palette;
        self.apply_theme(window, cx);
        self.prefs.save();
    }

    /// The toolbar's appearance panel: light, dark or system, and the
    /// themes, each with its colors. It stays open while you try them;
    /// a click outside or Escape closes it.
    fn appearance_panel(weak: &WeakEntity<Self>, cx: &App) -> AnyElement {
        let Some(view) = weak.upgrade() else {
            return div().into_any_element();
        };
        let (dark, palette) = {
            let prefs = &view.read(cx).prefs;
            (prefs.dark, prefs.palette)
        };
        let theme = cx.theme();
        let (hover, muted) = (theme.accent, theme.muted_foreground);
        let heading = |label: &'static str| {
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_sm()
                .text_color(muted)
                .child(label)
        };
        let row = |id: SharedString, checked: bool, content: AnyElement| {
            div()
                .id(id)
                .h(px(30.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(
                    div()
                        .w(px(14.))
                        .flex_shrink_0()
                        .when(checked, |d| d.child(Icon::new(IconName::Check).small())),
                )
                .child(content)
        };
        let mut panel = div()
            .w(px(220.))
            .flex()
            .flex_col()
            .child(heading("Appearance"));
        for (label, value) in [
            ("Light", Some(false)),
            ("Dark", Some(true)),
            ("Match System", None),
        ] {
            let weak = weak.clone();
            panel = panel.child(
                row(
                    format!("appearance-{label}").into(),
                    dark == value,
                    div().child(label).into_any_element(),
                )
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |v, cx| v.set_appearance(value, window, cx));
                })
                .test_support(),
            );
        }
        for (group, options) in [
            ("Editor Themes", &Palette::EDITOR[..]),
            ("Fun Themes", &Palette::FUN[..]),
        ] {
            panel = panel
                .child(div().my_1().h(px(1.)).bg(theme.border))
                .child(heading(group));
            for &option in options {
                // Overlapping dots, one per color the theme adds.
                let mut dots = div().flex().items_center().w(px(26.)).flex_shrink_0();
                for (i, color) in option.swatches(cx).into_iter().enumerate() {
                    dots = dots.child(
                        div()
                            .size(px(10.))
                            .rounded_full()
                            .bg(color)
                            .when(i > 0, |d| d.ml(px(-3.))),
                    );
                }
                let weak = weak.clone();
                panel = panel.child(
                    row(
                        format!("palette-{}", option.label()).into(),
                        option == palette,
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(dots)
                            .child(option.label())
                            .into_any_element(),
                    )
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |v, cx| v.set_palette(option, window, cx));
                    })
                    .test_support(),
                );
            }
        }
        panel.into_any_element()
    }

    // MARK: Rendering

    fn toolbar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let pane = self.focused_pane();
        let pane = pane.read(cx);
        let (source, help, has_doc, copied) =
            (pane.source, pane.help, pane.document.is_some(), pane.copied);
        // The folder the focused document lives in.
        let home = match &pane.path {
            Some(path) => self.home_of(path),
            None => Some(self.active),
        };
        let root = home.and_then(|ix| self.workspaces[ix].root.as_deref());
        let workspace = root.map(|root| name_of(Some(root)));
        // The folders between the workspace and the file: a breadcrumb.
        let crumbs: Vec<String> = match (&pane.path, root) {
            (Some(path), Some(root)) if !help => path
                .parent()
                .and_then(|p| p.strip_prefix(root).ok())
                .map(|rel| {
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default(),
            (Some(path), None) if !help => path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| vec![n.to_string_lossy().into_owned()])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let full_path = pane.path.as_deref().filter(|_| !help).map(names::tilde);
        let weak = cx.entity().downgrade();
        // 44px toolbar doubles as the title bar; traffic lights are centered in it (main.rs).
        TitleBar::new()
            .h(px(TOOLBAR_HEIGHT))
            .bg(theme.title_bar)
            .border_color(theme.sidebar_border)
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
                            .id("breadcrumb")
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when_some(full_path, |d, path| {
                                d.tooltip(move |w, cx| Tooltip::new(path.clone()).build(w, cx))
                            })
                            // Which workspace the document belongs to, and
                            // the folders down to it.
                            .when_some(workspace, |d, name| {
                                d.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{name}  /")),
                                )
                            })
                            .when(!crumbs.is_empty(), |d| {
                                d.child(
                                    div()
                                        .min_w(px(24.))
                                        .truncate()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{}  /", crumbs.join("  /  "))),
                                )
                            })
                            .child(div().flex_shrink_0().max_w(px(360.)).truncate().child(
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
                        Popover::new("appearance")
                            .anchor(Anchor::TopRight)
                            .open(self.appearance_open)
                            .on_open_change({
                                let weak = weak.clone();
                                move |open, _, cx| {
                                    let _ = weak.update(cx, |v, cx| {
                                        v.appearance_open = *open;
                                        cx.notify();
                                    });
                                }
                            })
                            .trigger(
                                Button::new("theme")
                                    .ghost()
                                    .icon(Icon::new(Lucide::Palette))
                                    .tooltip("Appearance & themes · ⇧⌘D toggles dark"),
                            )
                            .content(move |_, _, cx| Self::appearance_panel(&weak, cx)),
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

    /// Actions on one file, for the sidebar's and the tab strip's context
    /// menus. `open` adds the ways to open it.
    fn file_menu_items(
        menu: PopupMenu,
        path: PathBuf,
        open: bool,
        weak: WeakEntity<Self>,
    ) -> PopupMenu {
        let item =
            |label: &'static str, f: fn(&mut Reader, &Path, &mut Window, &mut Context<Reader>)| {
                let (weak, path) = (weak.clone(), path.clone());
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |v, cx| f(v, &path, window, cx));
                })
            };
        let menu = if open {
            menu.item(item("Open", |v, p, w, cx| {
                v.open_in(p.to_owned(), Target::Keep, w, cx)
            }))
            .item(item("Open in New Tab", |v, p, w, cx| {
                v.open_in(p.to_owned(), Target::NewTab, w, cx)
            }))
            .item(item("Open to the Side", |v, p, w, cx| {
                v.open_in(p.to_owned(), Target::Split, w, cx)
            }))
            .separator()
        } else {
            menu
        };
        menu.item(item("Reveal in Finder", |_, p, _, cx| cx.reveal_path(p)))
            .item(item("Copy Path", |_, p, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(p.to_string_lossy().into_owned()))
            }))
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
        let accents = chrome::accents(cx);
        let folder_color = accents.folder.unwrap_or(muted);
        let file_color = accents.file.unwrap_or(muted);
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
                let docs = self
                    .tabs
                    .tabs
                    .iter()
                    .filter(|t| t.panes().any(|p| self.pane_in(w, p, cx)))
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
                        } else if folder {
                            folder_color
                        } else {
                            muted
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
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
                    .on_click(cx.listener(move |v, e: &ClickEvent, _, cx| {
                        // A double-click opens and stays open.
                        if e.click_count() > 1 {
                            return;
                        }
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
            Item::Entry { ws, row: row_ix } => {
                let Some(entry) = self.row(item) else {
                    return div().id(("entry", ix)).into_any_element();
                };
                // Loose files have no tree around them: name their folder.
                let hint = (self.workspaces[ws].root.is_none())
                    .then(|| {
                        let rows = &self.workspaces[ws].rows;
                        let paths: Vec<&Path> = rows.iter().map(|r| r.path.as_path()).collect();
                        names::disambiguate(&paths, true).swap_remove(row_ix)
                    })
                    .flatten();
                let (menu_weak, menu_path) = (cx.entity().downgrade(), entry.path.clone());
                let selected = current.as_ref() == Some(&entry.path);
                let directory = entry.directory;
                let icon = if !directory {
                    IconName::FileText
                } else if self.workspaces[ws].expanded.contains(&entry.path) {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                };
                let path = entry.path.clone();
                let drag = (!directory).then(|| {
                    let name: SharedString = entry
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                        .into();
                    (DraggedFile { path: path.clone() }, name)
                });
                row.id(("entry", ix))
                    .pl(px(22. + entry.depth as f32 * 14.))
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(accent).text_color(theme.sidebar_accent_foreground)
                    })
                    .when(!selected, |d| d.hover(move |s| s.bg(hover)))
                    .child(
                        Icon::new(icon)
                            .size_4()
                            .flex_shrink_0()
                            .text_color(if directory { folder_color } else { file_color }),
                    )
                    .child(
                        div().flex_shrink_0().max_w(px(170.)).truncate().child(
                            entry
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    )
                    .when_some(hint, |d, hint| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(muted)
                                .child(hint),
                        )
                    })
                    .on_click(cx.listener(move |v, e: &ClickEvent, w, cx| {
                        let m = e.modifiers();
                        let target = if m.platform {
                            Target::NewTab
                        } else if m.alt {
                            Target::Split
                        } else {
                            Target::Preview
                        };
                        // The first click of a double-click already opened
                        // the file (or folder); the second keeps it open.
                        if e.click_count() > 1 {
                            if target == Target::Preview && !directory {
                                v.keep_path(&path, cx);
                            }
                            return;
                        }
                        v.cursor = Some(Cursor {
                            ws,
                            path: Some(path.clone()),
                        });
                        v.tree_focus.focus(w, cx);
                        v.tree_open(target, w, cx);
                    }))
                    // Drag a file onto the page to open or split it there.
                    .when_some(drag, |d, (payload, name)| {
                        d.on_drag(payload, move |_, _, _, cx| cx.new(|_| Ghost(name.clone())))
                    })
                    .context_menu(move |menu, _, _| {
                        Self::file_menu_items(
                            menu,
                            menu_path.clone(),
                            !directory,
                            menu_weak.clone(),
                        )
                    })
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
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar)
            // Folders dropped here become workspaces; files open in theirs.
            .drag_over::<ExternalPaths>({
                let accent = cx.theme().sidebar_accent;
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
                        .child("Add a folder to browse its Markdown files. Tabs can show files from any folder.")
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
                    .h(px(28.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().sidebar_border)
                    // A fun theme's status bar runs under the sidebar too.
                    .bg(chrome::accents(cx)
                        .status
                        .map_or(cx.theme().sidebar, |(bg, _)| bg))
                    .text_xs()
                    .text_color(Self::status_colors(cx).1)
                    .child(div().min_w_0().truncate().child(if ws.indexing {
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
                    })),
            )
    }

    /// Folder hints for open documents that share a file name, so two
    /// `README.md` tabs read `README.md  notes` and `README.md  tools`.
    fn hints(&self, cx: &App) -> HashMap<PathBuf, String> {
        let paths: Vec<PathBuf> = self
            .tabs
            .panes()
            .filter_map(|p| p.read(cx).path.clone())
            .collect();
        let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        paths
            .iter()
            .cloned()
            .zip(names::disambiguate(&refs, false))
            .filter_map(|(path, hint)| Some((path, hint?)))
            .collect()
    }

    /// A pane's title, and the hint that tells it apart from another open
    /// document with the same name.
    fn pane_label(
        pane: &Entity<DocPane>,
        hints: &HashMap<PathBuf, String>,
        cx: &App,
    ) -> (String, Option<String>) {
        let pane = pane.read(cx);
        let hint = (!pane.help)
            .then(|| hints.get(pane.path.as_ref()?).cloned())
            .flatten();
        (pane.title(), hint)
    }

    /// One pane's title and hint, laid out in a tab or header.
    fn label_parts(
        title: String,
        hint: Option<String>,
        max: f32,
        italic: bool,
        muted: Hsla,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .min_w_0()
            .child(
                div()
                    .max_w(px(max))
                    .truncate()
                    .when(italic, |d| d.italic())
                    .child(title),
            )
            .when_some(hint, |d, hint| {
                d.child(
                    div()
                        .max_w(px(140.))
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .child(hint),
                )
            })
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (accent, hover) = (theme.sidebar_accent, theme.sidebar_accent);
        let tabs = &self.tabs;
        let hints = self.hints(cx);
        let last = tabs.len() - 1;
        let weak = cx.entity().downgrade();
        // Once tabs come from more than one folder, each names its folder.
        let folders: Vec<Vec<usize>> = tabs
            .tabs
            .iter()
            .map(|t| {
                let mut homes: Vec<usize> = t
                    .panes()
                    .filter_map(|p| self.home_of(p.read(cx).path.as_deref()?))
                    .filter(|&ix| self.workspaces[ix].root.is_some())
                    .collect();
                homes.dedup();
                homes
            })
            .collect();
        let mixed = folders.iter().flatten().collect::<HashSet<_>>().len() > 1;
        let count = tabs.len();
        let tab_line = chrome::accents(cx).tab_line;
        let mut strip = div().id("tab-strip").flex().min_w_0().overflow_x_scroll();
        for (ix, tab) in tabs.tabs.iter().enumerate() {
            let selected = ix == tabs.active;
            let (pinned, split, preview) = (tab.pinned, tab.is_split(), tab.preview);
            let parts: Vec<(String, Option<String>)> = tab
                .panes()
                .map(|p| Self::pane_label(p, &hints, cx))
                .collect();
            let ghost: SharedString = parts
                .iter()
                .map(|(t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join("  |  ")
                .into();
            // Once tabs come from more than one folder, each names its folder
            // unless a hint already tells it apart.
            let folder = (mixed && parts.iter().all(|(_, h)| h.is_none())).then(|| {
                folders[ix]
                    .iter()
                    .map(|&w| self.workspaces[w].name())
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            let paths: Vec<PathBuf> = tab
                .panes()
                .filter_map(|p| p.read(cx).path.clone())
                .collect();
            let tooltip: Option<SharedString> = (!paths.is_empty()).then(|| {
                let mut text = paths
                    .iter()
                    .map(|p| names::tilde(p))
                    .collect::<Vec<_>>()
                    .join("\n");
                if preview {
                    text.push_str("\nPreview · double-click to keep open");
                }
                text.into()
            });
            let first_path = paths.first().cloned();
            let weak = weak.clone();
            let max = if split {
                170.
            } else if pinned {
                120.
            } else {
                200.
            };
            let muted_fg = theme.muted_foreground;
            let mut label = div().flex().items_center().gap_1p5().min_w_0();
            for (i, (title, hint)) in parts.into_iter().enumerate() {
                if i > 0 {
                    label = label.child(div().text_color(muted_fg).child("|"));
                }
                label = label.child(Self::label_parts(title, hint, max, preview, muted_fg));
            }
            strip = strip.child(
                div()
                    .id(("tab", ix))
                    .relative()
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
                    .border_color(theme.sidebar_border)
                    .when(selected, |d| {
                        d.bg(theme.background).text_color(theme.foreground)
                    })
                    .when(!selected, |d| {
                        d.text_color(theme.muted_foreground)
                            .hover(move |d| d.bg(hover))
                    })
                    .when_some(tab_line.filter(|_| selected), |d, line| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(2.))
                                .bg(line),
                        )
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
                    .child(label)
                    .when_some(folder.filter(|f| !f.is_empty()), |d, folder| {
                        d.child(
                            div()
                                .max_w(px(120.))
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(folder),
                        )
                    })
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
                    .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                        this.activate_tab(ix, window, cx);
                        if e.click_count() > 1 {
                            this.keep_tab(ix, cx);
                        }
                    }))
                    // Middle-click closes, as in browsers and editors.
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            if !pinned {
                                this.close_tab(ix, window, cx);
                            }
                        }),
                    )
                    .when_some(tooltip, |d, text| {
                        d.tooltip(move |w, cx| Tooltip::new(text.clone()).build(w, cx))
                    })
                    .on_drag(DraggedTab { from: ix }, move |_, _, _, cx| {
                        cx.new(|_| Ghost(ghost.clone()))
                    })
                    .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(move |this, dragged: &DraggedTab, window, cx| {
                        this.tabs.move_tab(dragged.from, ix);
                        this.after_tab_change(window, cx);
                    }))
                    .drag_over::<DraggedPane>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedPane, window, cx| {
                        this.tabs.pane_to_tab(dragged.right, true);
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
                        let menu = if preview {
                            menu.item(item("Keep Open", |v, ix, _, cx| v.keep_tab(ix, cx)))
                        } else {
                            menu
                        };
                        let menu = menu
                            .item(item(
                                if pinned { "Unpin Tab" } else { "Pin Tab" },
                                |v, ix, w, cx| {
                                    v.tabs.toggle_pin(ix);
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
                                    v.tabs.activate(ix);
                                    v.toggle_split(w, cx);
                                },
                            ))
                            .separator()
                            .item(item("Close Tab", |v, ix, w, cx| v.close_tab(ix, w, cx)))
                            .item(item("Close Other Tabs", |v, ix, w, cx| {
                                v.tabs.close_others(ix);
                                v.after_tab_change(w, cx);
                            }))
                            .item(
                                item("Close Tabs to the Right", |v, ix, w, cx| {
                                    v.tabs.close_to_right(ix);
                                    v.after_tab_change(w, cx);
                                })
                                .disabled(ix + 1 >= count),
                            );
                        match first_path.clone() {
                            Some(path) => {
                                Self::file_menu_items(menu.separator(), path, false, weak.clone())
                            }
                            None => menu,
                        }
                    }),
            );
        }
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .h(px(32.))
            .border_b_1()
            .border_color(theme.sidebar_border)
            .bg(theme.tab_bar)
            .child(strip)
            // Dropping on the empty space moves the tab to the end.
            .child(
                div()
                    .id("tab-bar-end")
                    .flex_1()
                    .h_full()
                    .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(move |this, dragged: &DraggedTab, window, cx| {
                        this.tabs.move_tab(dragged.from, last);
                        this.after_tab_change(window, cx);
                    }))
                    .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &ExternalPaths, window, cx| {
                        this.drop_on_tabs(dragged.paths(), window, cx)
                    }))
                    .drag_over::<DraggedFile>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedFile, window, cx| {
                        this.open_in(dragged.path.clone(), Target::NewTab, window, cx)
                    }))
                    .drag_over::<DraggedPane>(move |style, _, _, _| style.bg(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedPane, window, cx| {
                        this.tabs.pane_to_tab(dragged.right, true);
                        this.after_tab_change(window, cx);
                    })),
            )
            .child(div().w_2().flex_shrink_0())
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
        let hints = self.hints(cx);
        let (title, hint) = tab
            .pane(right)
            .map(|p| Self::pane_label(p, &hints, cx))
            .unwrap_or_default();
        let ghost: SharedString = title.clone().into();
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
            .border_color(theme.sidebar_border)
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
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .when(!focused, |d| d.text_color(theme.muted_foreground))
                    .child(Self::label_parts(
                        title,
                        hint,
                        400.,
                        false,
                        theme.muted_foreground,
                    )),
            )
            .child(
                Button::new(Self::pane_id("swap-panes", right))
                    .icon(Icon::new(gpui_kit::assets::IconName::ArrowLeftRight))
                    .ghost()
                    .xsmall()
                    .tooltip("Swap panes")
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.tabs.active_mut().swap();
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
        let (border, primary) = (theme.sidebar_border, theme.primary);
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
            let hints = self.hints(cx);
            for right in [false, true] {
                let title = tab
                    .pane(right)
                    .map(|p| match Self::pane_label(p, &hints, cx) {
                        (title, Some(hint)) => format!("{title} · {hint}"),
                        (title, None) => title,
                    })
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
                    // Only near an edge: a tab has nothing to do in the middle.
                    let from = event.drag(cx).from;
                    let inside = event.bounds.contains(&event.event.position);
                    let zone = Spot::of(event)
                        .edge
                        .filter(|_| inside && this.tabs.can_split_with(from))
                        .map(|right| {
                            if right {
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
                    this.tabs
                        .merge_into_active(dragged.from, zone == DropZone::Right);
                }
                this.after_tab_change(window, cx);
            }))
            .on_drag_move::<ExternalPaths>(cx.listener(
                move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let inside = event.bounds.contains(&event.event.position);
                    let paths = event.drag(cx).paths().to_vec();
                    this.drag_files_over(&paths, inside, Spot::of(event), narrow, cx);
                },
            ))
            .on_drop(cx.listener(|this, dragged: &ExternalPaths, window, cx| {
                this.drop_on_panes(dragged.paths(), window, cx)
            }))
            .on_drag_move::<DraggedFile>(cx.listener(
                move |this, event: &DragMoveEvent<DraggedFile>, _, cx| {
                    let inside = event.bounds.contains(&event.event.position);
                    let paths = [event.drag(cx).path.clone()];
                    this.drag_files_over(&paths, inside, Spot::of(event), narrow, cx);
                },
            ))
            .on_drop(cx.listener(|this, dragged: &DraggedFile, window, cx| {
                this.drop_on_panes(std::slice::from_ref(&dragged.path), window, cx)
            }))
            .child(body)
            .when_some(zone, |d, zone| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .map(|d| match zone {
                            DropZone::Whole | DropZone::Here => d.left_0().right_0(),
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

    /// Status bar colors: a fun theme's own, else the chrome's.
    fn status_colors(cx: &App) -> (Hsla, Hsla) {
        let theme = cx.theme();
        chrome::accents(cx)
            .status
            .unwrap_or((theme.status_bar, theme.muted_foreground))
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = self.focused_pane();
        let pane = pane.read(cx);
        let (status_bg, status_fg) = Self::status_colors(cx);
        div()
            .px_5()
            .h(px(28.))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().sidebar_border)
            .bg(status_bg)
            .text_xs()
            .text_color(status_fg)
            .child(if pane.source {
                "SOURCE · READ ONLY"
            } else {
                "MARKDOWN · READ ONLY"
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(match pane.stats.clone().filter(|_| !pane.help) {
                        Some(stats) => self.info_button(stats, cx).into_any_element(),
                        None => div().child("Local files").into_any_element(),
                    })
                    .child(format!("·   {} pt", self.prefs.font_size)),
            )
    }

    /// The footer's word count, which opens what the document is made of.
    fn set_info_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.info_open = open;
        self.nib = open.then(|| {
            let mood = self
                .focused_pane()
                .read(cx)
                .stats
                .as_ref()
                .map(info::mood)
                .unwrap_or_default();
            cx.new(|cx| Nib::new(mood, cx))
        });
        cx.notify();
    }

    fn info_button(&self, stats: markdown_core::Stats, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let (_, fg) = Self::status_colors(cx);
        let label = format!(
            "{} words · {} min",
            info::thousands(stats.words),
            stats.reading_minutes()
        );
        Popover::new("doc-info")
            .anchor(Anchor::BottomRight)
            .open(self.info_open)
            .on_open_change({
                let weak = weak.clone();
                move |open, _, cx| {
                    let _ = weak.update(cx, |v, cx| v.set_info_open(*open, cx));
                }
            })
            .trigger(
                // Drawn in the status bar's own colors: a fun theme's bar
                // can be dark under a light chrome.
                Button::new("doc-info-button")
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .foreground(fg)
                            .hover(fg.opacity(0.12))
                            .active(fg.opacity(0.18)),
                    )
                    .xsmall()
                    .label(label)
                    .tooltip("What's in this document"),
            )
            .content(move |_, _, cx| {
                let Some(view) = weak.upgrade() else {
                    return div().into_any_element();
                };
                let pane = view.read(cx).focused_pane();
                let pane = pane.read(cx);
                let (Some(stats), Some(doc)) = (pane.stats.as_ref(), pane.document.as_ref()) else {
                    return div().into_any_element();
                };
                let nib = view
                    .read(cx)
                    .nib
                    .clone()
                    .map(|n| n.into_any_element())
                    .unwrap_or_else(|| div().into_any_element());
                let weak = weak.clone();
                let jump = Rc::new(move |index: usize, window: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |v, cx| {
                        v.set_info_open(false, cx);
                        v.with_pane(cx, |p, cx| p.jump_heading(index, cx));
                        v.focus_reader(window, cx);
                    });
                });
                info::panel(stats, &doc.headings, jump, nib, cx)
            })
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
                    // Indent from the shallowest level used, not from `#`.
                    let top = d.headings.iter().map(|h| h.level).min().unwrap_or(1);
                    d.headings
                        .iter()
                        .enumerate()
                        .map(|(index, h)| {
                            let heading = h.clone();
                            CommandItem::new()
                                .label(h.text.clone())
                                .keywords([format!("h{}", h.level)])
                                .action(Box::new(JumpHeading { index }))
                                .child(move |_, cx| info::heading_row(&heading, top, cx))
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
            // Top-aligned, so the panel is only as tall as its rows.
            .items_start()
            .pt(px(64.))
            .bg(rgba(0x00000035))
            .on_mouse_down(MouseButton::Left, move |_, w, cx| {
                let _ = dismiss.update(cx, |v, cx| v.close_overlay(w, cx));
            })
            .child(
                div()
                    .w(px(570.))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .rounded_lg()
                    .shadow_lg()
                    .child(
                        Command::new(&self.palette)
                            .placeholder(if outline {
                                "Jump to heading…"
                            } else {
                                "Find a Markdown file…"
                            })
                            .bordered(true)
                            .max_h(px(380.))
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
                                if outline {
                                    info::key_hints(
                                        &[("↑ ↓", "Navigate"), ("↵", "Jump"), ("Esc", "Close")],
                                        cx,
                                    )
                                } else {
                                    info::key_hints(
                                        &[
                                            ("↑ ↓", "Navigate"),
                                            ("↵", "Open"),
                                            ("⌘ ↵", "New tab"),
                                            ("⌥ ↵", "Split"),
                                            ("Esc", "Close"),
                                        ],
                                        cx,
                                    )
                                }
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
                "dark" => self.set_appearance(Some(true), window, cx),
                "light" => self.set_appearance(Some(false), window, cx),
                "palette" => {
                    if let Some(p) = Palette::EDITOR
                        .into_iter()
                        .chain(Palette::FUN)
                        .find(|p| p.label().eq_ignore_ascii_case(arg))
                    {
                        self.set_palette(p, window, cx);
                    }
                }
                "preview" => {
                    if let Some(path) = resolve(&self.ws().root.clone().or(base.clone()), arg) {
                        self.open_in(path, Target::Preview, window, cx);
                    }
                }
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
                "appearance" => self.appearance_open = true,
                "info" => self.set_info_open(true, cx),
                "nib" => {
                    if let Some(nib) = &self.nib {
                        nib.update(cx, |n, _| n.pose(arg));
                    }
                }
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
        // A lone preview tab shows too, so its italic title says the next
        // click replaces it.
        let show_tabs = self.tabs.len() > 1 || self.tab().preview;
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
                v.open_in(a.path.clone(), Target::Keep, w, cx);
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
                v.set_appearance(Some(dark), w, cx);
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
                if v.tabs.reopen() {
                    v.after_tab_change(w, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &NextTab, w, cx| {
                v.tabs.cycle(1);
                v.after_tab_change(w, cx);
            }))
            .on_action(cx.listener(|v, _: &PrevTab, w, cx| {
                v.tabs.cycle(-1);
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
            .on_action(cx.listener(|v, _: &TreeOpen, w, cx| v.tree_open(Target::Keep, w, cx)))
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

    #[test]
    fn splitting_starts_near_the_edges() {
        let at = |x: f32, width: f32| Spot::at(x, 100., width);
        // Wide: a quarter of the width, capped.
        assert_eq!(at(100. + 230., 1000.).edge, Some(false));
        assert_eq!(at(100. + 250., 1000.).edge, None);
        assert_eq!(at(100. + 770., 1000.).edge, Some(true));
        assert_eq!(at(100. + 600., 1000.).edge, None);
        assert!(at(100. + 600., 1000.).right_half);
        // Narrow: at least 72px, never past the middle.
        assert_eq!(at(100. + 70., 200.).edge, Some(false));
        assert_eq!(at(100. + 100., 200.).edge, None);
        assert_eq!(at(100. + 49., 100.).edge, Some(false));
        assert_eq!(at(100. + 51., 100.).edge, Some(true));
    }

    #[gpui_kit::test]
    fn drops_split_near_the_edges_and_open_in_the_middle(cx: &mut TestAppContext) {
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
        let middle = Spot::at(600., 0., 1000.);
        let left_half = Spot::at(400., 0., 1000.);
        let (left_edge, right_edge) = (Spot::at(40., 0., 1000.), Spot::at(960., 0., 1000.));

        // Nothing droppable: no zone.
        let plan = view.read_with(cx, |v, cx| {
            v.plan_drop(&[p("notes.txt")], middle, false, cx)
        });
        assert!(plan.is_none());

        // Two files on an empty tab open side by side.
        with_reader(w, &view, cx, |v, w, cx| {
            let paths = [p("A.md"), p("B.md"), p("notes.txt")];
            let (zone, hint) = v.plan_drop(&paths, middle, false, cx).unwrap();
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
            let (zone, hint) = v.plan_drop(&paths, left_half, false, cx).unwrap();
            assert_eq!(zone, DropZone::Left);
            assert_eq!(hint.as_ref(), "Replace left pane (+1 tab)");
            v.drop_zone = Some(zone);
            v.drop_on_panes(&paths, w, cx);
        });
        assert_eq!(paths_of(&view, cx), [p("C.md"), p("B.md")]);
        assert!(!view.read_with(cx, |v, _| v.tab().focus_right));
        assert_eq!(view.read_with(cx, |v, _| v.tabs.len()), 2);

        // On a single document, the middle opens in place (Back returns)...
        with_reader(w, &view, cx, |v, w, cx| {
            v.tabs.activate(1);
            let (zone, hint) = v.plan_drop(&[p("A.md")], middle, false, cx).unwrap();
            assert_eq!((zone, hint.as_ref()), (DropZone::Here, "Open here"));
            v.drop_zone = Some(zone);
            v.drop_on_panes(&[p("A.md")], w, cx);
        });
        assert_eq!(paths_of(&view, cx), [p("A.md")]);
        assert!(pane(&view, cx, |p| p.can_go_back()));

        // ...and only the edges split it.
        with_reader(w, &view, cx, |v, w, cx| {
            let (zone, hint) = v.plan_drop(&[p("B.md")], left_edge, false, cx).unwrap();
            assert_eq!((zone, hint.as_ref()), (DropZone::Left, "Open on the left"));
            let (zone, _) = v.plan_drop(&[p("B.md")], right_edge, false, cx).unwrap();
            assert_eq!(zone, DropZone::Right);
            v.drop_zone = Some(zone);
            v.drop_on_panes(&[p("B.md")], w, cx);
        });
        assert_eq!(paths_of(&view, cx), [p("A.md"), p("B.md")]);
        assert!(view.read_with(cx, |v, _| v.tab().focus_right));

        // Files on the tab strip open as new tabs.
        with_reader(w, &view, cx, |v, w, cx| {
            v.drop_on_tabs(&[p("B.md"), p("notes.txt")], w, cx)
        });
        assert_eq!(view.read_with(cx, |v, _| v.tabs.len()), 3);
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
        assert_eq!(view.read_with(cx, |v, _| v.tabs.len()), 2);
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
        assert_eq!(view.read_with(cx, |v, _| v.tabs.len()), 2);

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
        assert_eq!(view.read_with(cx, |v, _| v.tabs.len()), 1);
        assert_eq!(pane(&view, cx, |p| p.path.clone()), Some(a));
        interact(w, cx, |w, cx| w.press("cmd-shift-t", cx));
        assert_eq!(view.read_with(cx, |v, _| v.tabs.len()), 2);
        assert_eq!(pane(&view, cx, |p| p.path.clone()), Some(b));
        interact(w, cx, |w, cx| w.press("ctrl-tab", cx));
        assert_eq!(view.read_with(cx, |v, _| v.tabs.active), 0);
    }

    #[gpui_kit::test]
    fn tabs_mix_workspaces_and_the_active_one_follows_focus(cx: &mut TestAppContext) {
        let one = folder(&[("One.md", "# One")]);
        let two = folder(&[("Two.md", "# Two")]);
        let loose = folder(&[("Loose.md", "# Loose")]);
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(one.path().into(), w, cx);
            v.open_in(one.path().join("One.md"), Target::Focused, w, cx);
            v.add_folder(two.path().into(), w, cx);
        });
        // The empty Files workspace was adopted by the first folder, and
        // switching folders keeps the tabs.
        assert_eq!(
            view.read_with(cx, |v, _| (v.workspaces.len(), v.active)),
            (2, 1)
        );
        assert_eq!(pane(&view, cx, |p| p.title()), "One.md");

        // A file from the other folder opens beside it in the same strip.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_from_tree(1, two.path().join("Two.md"), Target::NewTab, w, cx)
        });
        assert_eq!(view.read_with(cx, |v, _| (v.tabs.len(), v.active)), (2, 1));
        // Each tab names its folder once they're mixed.
        let folders = view.read_with(cx, |v, cx| {
            v.tabs
                .tabs
                .iter()
                .map(|t| v.home_of(t.left.read(cx).path.as_deref().unwrap()))
                .collect::<Vec<_>>()
        });
        assert_eq!(folders, [Some(0), Some(1)]);

        // The active workspace follows the focused tab.
        with_reader(w, &view, cx, |v, w, cx| v.activate_tab(0, w, cx));
        assert_eq!(view.read_with(cx, |v, _| v.active), 0);
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_path(two.path().join("Two.md"), w, cx)
        });
        assert_eq!(
            view.read_with(cx, |v, _| (v.tabs.len(), v.tabs.active, v.active)),
            (2, 1, 1),
            "an open file comes forward"
        );

        // A file outside every folder gets a Files workspace.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_path(loose.path().join("Loose.md"), w, cx)
        });
        assert_eq!(
            view.read_with(cx, |v, _| (v.workspaces.len(), v.active, v.tabs.len())),
            (3, 2, 3)
        );
        assert_eq!(pane(&view, cx, |p| p.title()), "Loose.md");

        // Switching workspaces brings forward one of its tabs.
        interact(w, cx, |w, cx| w.press("ctrl-1", cx));
        assert_eq!(view.read_with(cx, |v, _| v.active), 0);
        assert_eq!(pane(&view, cx, |p| p.title()), "One.md");

        // Closing a workspace closes only its tabs.
        with_reader(w, &view, cx, |v, w, cx| v.close_workspace(0, w, cx));
        let titles = view.read_with(cx, |v, cx| {
            v.tabs
                .tabs
                .iter()
                .map(|t| t.left.read(cx).title())
                .collect::<Vec<_>>()
        });
        assert_eq!(titles, ["Two.md", "Loose.md"]);

        // Saved and restored.
        let saved = view.read_with(cx, |v, _| v.prefs.clone());
        assert_eq!(saved.workspaces.len(), 2);
        assert_eq!(saved.tabs.len(), 2);
        assert_eq!(saved.tabs[0].left.path, Some(two.path().join("Two.md")));
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
            v.open_from_tree(v.active, tmp.path().join("A.md"), Target::Preview, w, cx)
        });
        assert_eq!(
            view.read_with(cx, |v, _| (v.tabs.len(), v.tabs.active)),
            (2, 0)
        );
    }

    #[gpui_kit::test]
    fn sidebar_clicks_preview_and_double_clicks_keep(cx: &mut TestAppContext) {
        let tmp = folder(&[("A.md", "# A"), ("B.md", "# B"), ("C.md", "# C")]);
        let p = |n: &str| tmp.path().join(n);
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(tmp.path().into(), w, cx)
        });
        let state = |view: &Entity<Reader>, cx: &mut TestAppContext| {
            view.read_with(cx, |v, cx| {
                (
                    v.tabs.len(),
                    v.tabs.preview_index(),
                    v.focused_pane().read(cx).title(),
                )
            })
        };
        let ws = view.read_with(cx, |v, _| v.active);

        // The empty tab becomes the preview, and the next click reuses it.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_from_tree(ws, p("A.md"), Target::Preview, w, cx)
        });
        assert_eq!(state(&view, cx), (1, Some(0), "A.md".into()));
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_from_tree(ws, p("B.md"), Target::Preview, w, cx)
        });
        assert_eq!(state(&view, cx), (1, Some(0), "B.md".into()));
        assert!(pane(&view, cx, |p| p.can_go_back()), "Back returns to A");

        // A double-click keeps it; the next click opens a new preview tab.
        with_reader(w, &view, cx, |v, _, cx| v.keep_path(&p("B.md"), cx));
        assert_eq!(state(&view, cx), (1, None, "B.md".into()));
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_from_tree(ws, p("C.md"), Target::Preview, w, cx)
        });
        assert_eq!(state(&view, cx), (2, Some(1), "C.md".into()));
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_from_tree(ws, p("A.md"), Target::Preview, w, cx)
        });
        assert_eq!(state(&view, cx), (2, Some(1), "A.md".into()));
        assert!(view.read_with(cx, |v, _| v.prefs.tabs[1].preview), "saved");

        // A file already open comes forward instead.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_in(p("B.md"), Target::Preview, w, cx)
        });
        assert_eq!(state(&view, cx), (2, Some(1), "B.md".into()));

        // ↵ / Quick Open / Finder open into the preview tab and keep it.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_in(p("C.md"), Target::Keep, w, cx)
        });
        assert_eq!(state(&view, cx), (2, None, "C.md".into()));

        // ⌘-click always opens a kept tab.
        with_reader(w, &view, cx, |v, w, cx| {
            v.open_from_tree(ws, p("A.md"), Target::NewTab, w, cx)
        });
        assert_eq!(state(&view, cx), (3, None, "A.md".into()));
    }

    #[gpui_kit::test]
    fn tabs_with_the_same_name_name_their_folders(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        for dir in ["notes", "tools"] {
            std::fs::create_dir(tmp.path().join(dir)).unwrap();
            std::fs::write(tmp.path().join(dir).join("README.md"), "# Read me").unwrap();
        }
        std::fs::write(tmp.path().join("Other.md"), "# Other").unwrap();
        let (w, view) = open(cx);
        with_reader(w, &view, cx, |v, w, cx| {
            v.add_folder(tmp.path().into(), w, cx);
            v.open_in(tmp.path().join("notes/README.md"), Target::Keep, w, cx);
            v.open_in(tmp.path().join("tools/README.md"), Target::NewTab, w, cx);
            v.open_in(tmp.path().join("Other.md"), Target::NewTab, w, cx);
        });
        let hints = view.read_with(cx, |v, cx| {
            let hints = v.hints(cx);
            v.tabs
                .tabs
                .iter()
                .map(|t| Reader::pane_label(&t.left, &hints, cx).1)
                .collect::<Vec<_>>()
        });
        assert_eq!(hints, [Some("notes".into()), Some("tools".into()), None]);
    }

    #[gpui_kit::test]
    fn color_presets_tint_the_chrome_but_not_the_page(cx: &mut TestAppContext) {
        let (w, view) = open(cx);
        let colors = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                let t = cx.theme();
                (
                    t.background,
                    t.sidebar,
                    t.title_bar,
                    t.tab_bar,
                    t.status_bar,
                    t.muted,
                )
            })
        };
        let (page, sidebar, toolbar, tabs, status, code) = colors(cx);
        assert_eq!((toolbar, status, tabs), (page, page, sidebar), "neutral");
        for dark in [false, true] {
            with_reader(w, &view, cx, |v, w, cx| {
                v.set_appearance(Some(dark), w, cx);
                v.set_palette(Palette::Synthwave, w, cx);
            });
            let tint = Palette::Synthwave.colors(dark).unwrap();
            let (page2, sidebar2, toolbar2, tabs2, status2, _) = colors(cx);
            assert_eq!(
                (sidebar2, toolbar2, tabs2, status2),
                (tint.chrome, tint.chrome, tint.chrome, tint.chrome)
            );
            assert_ne!(page2, tint.chrome);
            assert_eq!(cx.update(|cx| cx.theme().is_dark()), dark);
        }
        // Back to neutral light: exactly as before.
        with_reader(w, &view, cx, |v, w, cx| {
            v.set_palette(Palette::Classic, w, cx);
            v.set_appearance(Some(false), w, cx);
        });
        assert_eq!(colors(cx), (page, sidebar, toolbar, tabs, status, code));
    }

    #[gpui_kit::test]
    fn appearance_panel_stays_open_while_trying_themes(cx: &mut TestAppContext) {
        let (w, view) = open(cx);
        interact(w, cx, |w, cx| w.click("theme", cx));
        interact(w, cx, |_, _| {});
        interact(w, cx, |_, _| {});
        interact(w, cx, |w, cx| w.click("palette-Synthwave", cx));
        assert_eq!(
            view.read_with(cx, |v, _| v.prefs.palette),
            Palette::Synthwave
        );
        // Still open: the next row is there to click without reopening.
        interact(w, cx, |w, cx| {
            w.click("palette-Lagoon", cx);
            w.click("appearance-Dark", cx);
        });
        let prefs = view.read_with(cx, |v, _| (v.prefs.palette, v.prefs.dark));
        assert_eq!(prefs, (Palette::Lagoon, Some(true)));
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
