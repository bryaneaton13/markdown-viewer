//! The window's tabs, which can mix files from any workspace. A tab shows one document, or two side by side.
//! The split is capped at two panes: a tab that is already split can't take
//! another pane, and a split tab can't be dropped into another tab.
//!
//! The model is generic over the pane type so its rules can be tested without
//! a window; the app uses `Entity<DocPane>`.

use std::sync::atomic::{AtomicU64, Ordering};

/// Recently closed tabs kept for ⇧⌘T.
const CLOSED_KEPT: usize = 20;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug)]
pub struct Tab<P> {
    /// Stable identity, so the active tab survives reordering.
    pub id: u64,
    pub left: P,
    /// Second pane when split side by side.
    pub right: Option<P>,
    /// Navigation targets the right pane.
    pub focus_right: bool,
    /// Pinned tabs sit first and have no close button.
    pub pinned: bool,
    /// A preview tab (italic title) shows whatever was last single-clicked
    /// in the sidebar and is reused by the next click. Double-clicking,
    /// pinning, splitting or moving it keeps it open.
    pub preview: bool,
}

impl<P> Tab<P> {
    pub fn new(pane: P) -> Self {
        Self {
            id: next_id(),
            left: pane,
            right: None,
            focus_right: false,
            pinned: false,
            preview: false,
        }
    }

    /// A tab that the next sidebar click may replace.
    pub fn preview(pane: P) -> Self {
        Self {
            preview: true,
            ..Self::new(pane)
        }
    }

    pub fn is_split(&self) -> bool {
        self.right.is_some()
    }

    /// The pane navigation applies to.
    pub fn focused(&self) -> &P {
        match &self.right {
            Some(right) if self.focus_right => right,
            _ => &self.left,
        }
    }

    /// One side: left (`false`) or right (`true`).
    pub fn pane(&self, right: bool) -> Option<&P> {
        if right {
            self.right.as_ref()
        } else {
            Some(&self.left)
        }
    }

    pub fn panes(&self) -> impl Iterator<Item = &P> {
        std::iter::once(&self.left).chain(self.right.as_ref())
    }

    /// Show `pane` beside this tab's: on the right, or on the left when
    /// `right` is false. The new pane takes focus. False when already split.
    pub fn split_with(&mut self, pane: P, right: bool) -> bool {
        if self.is_split() {
            return false;
        }
        self.preview = false;
        if right {
            self.right = Some(pane);
        } else {
            let old = std::mem::replace(&mut self.left, pane);
            self.right = Some(old);
        }
        self.focus_right = right;
        true
    }

    /// Remove one pane (the right one when `right`) and return it. The other
    /// pane stays and the tab is no longer split.
    pub fn take_pane(&mut self, right: bool) -> Option<P> {
        let other = self.right.take()?;
        self.focus_right = false;
        if right {
            Some(other)
        } else {
            Some(std::mem::replace(&mut self.left, other))
        }
    }

    /// Swap the two panes, keeping focus on the same pane.
    pub fn swap(&mut self) {
        if let Some(right) = self.right.as_mut() {
            std::mem::swap(&mut self.left, right);
            self.focus_right = !self.focus_right;
        }
    }
}

#[derive(Debug)]
pub struct TabSet<P> {
    pub tabs: Vec<Tab<P>>,
    pub active: usize,
    closed: Vec<Tab<P>>,
}

impl<P> TabSet<P> {
    pub fn new(first: Tab<P>) -> Self {
        Self {
            tabs: vec![first],
            active: 0,
            closed: Vec::new(),
        }
    }

    /// Restore from saved tabs; an empty list falls back to `blank`.
    pub fn restore(tabs: Vec<Tab<P>>, active: usize, blank: impl FnOnce() -> P) -> Self {
        let mut set = if tabs.is_empty() {
            Self::new(Tab::new(blank()))
        } else {
            Self {
                active: active.min(tabs.len() - 1),
                tabs,
                closed: Vec::new(),
            }
        };
        set.keep_pinned_first();
        set
    }

    pub fn active(&self) -> &Tab<P> {
        &self.tabs[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Tab<P> {
        &mut self.tabs[self.active]
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn panes(&self) -> impl Iterator<Item = &P> {
        self.tabs.iter().flat_map(|t| t.panes())
    }

    /// Insert beside the active tab and make it active. An unpinned tab never
    /// lands among pinned ones.
    pub fn insert_after_active(&mut self, tab: Tab<P>) {
        let pinned = self.tabs.iter().filter(|t| t.pinned).count();
        let at = (self.active + 1).max(if tab.pinned { 0 } else { pinned });
        let at = at.min(self.tabs.len());
        self.tabs.insert(at, tab);
        self.active = at;
    }

    pub fn activate(&mut self, index: usize) -> bool {
        if index < self.tabs.len() && index != self.active {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Next (`delta` 1) or previous (-1) tab, wrapping around.
    pub fn cycle(&mut self, delta: isize) {
        let n = self.tabs.len() as isize;
        self.active = ((self.active as isize + delta).rem_euclid(n)) as usize;
    }

    /// Close a tab, remembering it for `reopen`. The last tab can't be closed
    /// (the caller replaces its contents instead). The tab to the right takes
    /// its place when the active tab closes.
    pub fn close(&mut self, index: usize) -> bool {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return false;
        }
        let tab = self.tabs.remove(index);
        self.remember(tab);
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        true
    }

    /// Close every tab except `index` and pinned ones.
    pub fn close_others(&mut self, index: usize) {
        let Some(keep) = self.tabs.get(index).map(|t| t.id) else {
            return;
        };
        let mut kept = Vec::new();
        for tab in std::mem::take(&mut self.tabs) {
            if tab.id == keep || tab.pinned {
                kept.push(tab);
            } else {
                self.remember(tab);
            }
        }
        self.tabs = kept;
        self.active = self.tabs.iter().position(|t| t.id == keep).unwrap_or(0);
    }

    /// Close the unpinned tabs after `index`.
    pub fn close_to_right(&mut self, index: usize) {
        let Some(keep) = self.tabs.get(index).map(|t| t.id) else {
            return;
        };
        let active = self.tabs[self.active].id;
        let mut kept = Vec::new();
        for (i, tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            if i <= index || tab.pinned {
                kept.push(tab);
            } else {
                self.remember(tab);
            }
        }
        self.tabs = kept;
        self.active = self
            .tabs
            .iter()
            .position(|t| t.id == active)
            .or_else(|| self.tabs.iter().position(|t| t.id == keep))
            .unwrap_or(0);
    }

    /// The preview tab, if there is one.
    pub fn preview_index(&self) -> Option<usize> {
        self.tabs.iter().position(|t| t.preview)
    }

    /// Keep a preview tab open: the next sidebar click opens a new one.
    pub fn keep(&mut self, index: usize) -> bool {
        match self.tabs.get_mut(index) {
            Some(tab) if tab.preview => {
                tab.preview = false;
                true
            }
            _ => false,
        }
    }

    fn remember(&mut self, mut tab: Tab<P>) {
        tab.pinned = false;
        tab.preview = false;
        self.closed.push(tab);
        if self.closed.len() > CLOSED_KEPT {
            self.closed.remove(0);
        }
    }

    /// Close every pane `close` matches, remembering them for `reopen`. A
    /// split keeps its other pane; a tab left with none closes. If no tab is
    /// left, one with a `blank` pane takes their place.
    pub fn close_panes(&mut self, mut close: impl FnMut(&P) -> bool, blank: impl FnOnce() -> P) {
        let (active_id, was) = (self.tabs[self.active].id, self.active);
        let mut kept = Vec::new();
        for mut tab in std::mem::take(&mut self.tabs) {
            if tab.right.as_ref().is_some_and(&mut close)
                && let Some(pane) = tab.take_pane(true)
            {
                self.remember_pane(pane);
            }
            if !close(&tab.left) {
                kept.push(tab);
            } else if let Some(pane) = tab.take_pane(false) {
                self.remember_pane(pane);
                kept.push(tab);
            } else {
                self.remember(tab);
            }
        }
        if kept.is_empty() {
            kept.push(Tab::new(blank()));
        }
        self.tabs = kept;
        self.active = self
            .tabs
            .iter()
            .position(|t| t.id == active_id)
            .unwrap_or(was.min(self.tabs.len() - 1));
    }

    /// Remember a single pane closed out of a split.
    pub fn remember_pane(&mut self, pane: P) {
        self.remember(Tab::new(pane));
    }

    /// ⇧⌘T: bring back the most recently closed tab beside the active one.
    pub fn reopen(&mut self) -> bool {
        match self.closed.pop() {
            Some(tab) => {
                self.insert_after_active(tab);
                true
            }
            None => false,
        }
    }

    /// Drag a tab to a new position, following the active tab by identity.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || from == to {
            return;
        }
        let id = self.tabs[self.active].id;
        let mut tab = self.tabs.remove(from);
        tab.preview = false;
        self.tabs.insert(to.min(self.tabs.len()), tab);
        self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
        self.keep_pinned_first();
    }

    pub fn toggle_pin(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.pinned = !tab.pinned;
            tab.preview = false;
            self.keep_pinned_first();
        }
    }

    /// Keep pinned tabs first (stable), following the active tab by identity.
    fn keep_pinned_first(&mut self) {
        let id = self.tabs[self.active].id;
        self.tabs.sort_by_key(|t| !t.pinned);
        self.active = self.tabs.iter().position(|t| t.id == id).unwrap_or(0);
    }

    /// Whether tab `from` could be dropped beside the active tab.
    pub fn can_split_with(&self, from: usize) -> bool {
        from != self.active
            && from < self.tabs.len()
            && !self.tabs[self.active].is_split()
            && !self.tabs[from].is_split()
    }

    /// Move tab `from` into the active tab as its left or right pane.
    pub fn merge_into_active(&mut self, from: usize, right: bool) -> bool {
        if !self.can_split_with(from) {
            return false;
        }
        let keep = self.tabs[self.active].id;
        let tab = self.tabs.remove(from);
        self.active = self.tabs.iter().position(|t| t.id == keep).unwrap_or(0);
        self.tabs[self.active].split_with(tab.left, right)
    }

    /// Move one pane of the active split tab out into its own tab, placed
    /// right after it. `activate` switches to the new tab.
    pub fn pane_to_tab(&mut self, right: bool, activate: bool) -> bool {
        let Some(pane) = self.tabs[self.active].take_pane(right) else {
            return false;
        };
        let keep = self.active;
        self.insert_after_active(Tab::new(pane));
        if !activate {
            self.active = keep;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(set: &TabSet<&'static str>) -> Vec<String> {
        set.tabs
            .iter()
            .map(|t| match t.right {
                Some(r) => format!("{}|{}", t.left, r),
                None => t.left.to_string(),
            })
            .collect()
    }

    #[test]
    fn insert_close_and_reopen() {
        let mut set = TabSet::new(Tab::new("a"));
        set.insert_after_active(Tab::new("b"));
        set.activate(0);
        set.insert_after_active(Tab::new("c"));
        assert_eq!(labels(&set), ["a", "c", "b"]);
        assert_eq!(*set.active().focused(), "c");

        assert!(set.close(1));
        assert_eq!(labels(&set), ["a", "b"]);
        assert_eq!(
            *set.active().focused(),
            "b",
            "the right neighbour takes over"
        );
        assert!(set.reopen());
        assert_eq!(labels(&set), ["a", "b", "c"]);
        assert_eq!(set.active, 2);

        set.close_others(2);
        assert_eq!(labels(&set), ["c"]);
        assert!(!set.close(0), "the last tab stays");
    }

    #[test]
    fn cycle_wraps() {
        let mut set = TabSet::new(Tab::new("a"));
        set.insert_after_active(Tab::new("b"));
        set.cycle(1);
        assert_eq!(set.active, 0);
        set.cycle(-1);
        assert_eq!(set.active, 1);
    }

    #[test]
    fn split_is_capped_at_two_panes() {
        let mut tab = Tab::new("a");
        assert!(tab.split_with("b", true));
        assert_eq!(*tab.focused(), "b");
        assert!(!tab.split_with("c", true), "no third pane");
        assert_eq!(tab.take_pane(false), Some("a"));
        assert_eq!(tab.left, "b");
        assert!(!tab.is_split());

        let mut tab = Tab::new("a");
        tab.split_with("b", false);
        assert_eq!((tab.left, tab.right), ("b", Some("a")));
        assert_eq!(*tab.focused(), "b");
        tab.swap();
        assert_eq!((tab.left, tab.right), ("a", Some("b")));
        assert_eq!(*tab.focused(), "b", "focus follows the pane");
    }

    #[test]
    fn dragging_tabs_into_and_out_of_a_split() {
        let mut set = TabSet::new(Tab::new("a"));
        set.insert_after_active(Tab::new("b"));
        set.insert_after_active(Tab::new("c"));
        set.activate(0);
        assert!(set.merge_into_active(2, true));
        assert_eq!(labels(&set), ["a|c", "b"]);
        assert!(!set.can_split_with(1), "active tab is already split");
        set.activate(1);
        assert!(!set.can_split_with(0), "split tabs can't be dropped in");

        set.activate(0);
        assert!(set.pane_to_tab(true, false));
        assert_eq!(labels(&set), ["a", "c", "b"]);
        assert_eq!(set.active, 0);
    }

    #[test]
    fn closing_panes_keeps_the_other_side_of_a_split() {
        let mut set = TabSet::new(Tab::new("a1"));
        set.insert_after_active(Tab::new("b1"));
        set.active_mut().split_with("a2", true);
        set.insert_after_active(Tab::new("a3"));
        set.activate(1);
        assert_eq!(labels(&set), ["a1", "b1|a2", "a3"]);
        set.close_panes(|p| p.starts_with('a'), || "blank");
        assert_eq!(labels(&set), ["b1"]);
        assert_eq!(set.active, 0);
        assert!(set.reopen(), "closed panes can come back");
        set.close_panes(|_| true, || "blank");
        assert_eq!(labels(&set), ["blank"]);
    }

    #[test]
    fn pinned_tabs_stay_first() {
        let mut set = TabSet::new(Tab::new("a"));
        set.insert_after_active(Tab::new("b"));
        set.insert_after_active(Tab::new("c"));
        set.toggle_pin(2);
        assert_eq!(labels(&set), ["c", "a", "b"]);
        assert_eq!(*set.active().focused(), "c");
        set.activate(0);
        set.insert_after_active(Tab::new("d"));
        assert_eq!(labels(&set), ["c", "d", "a", "b"]);
        set.move_tab(3, 0);
        assert_eq!(
            labels(&set),
            ["c", "b", "d", "a"],
            "unpinned can't pass pinned"
        );
    }

    #[test]
    fn preview_tabs_are_kept_by_pinning_splitting_or_moving() {
        let mut set = TabSet::new(Tab::new("a"));
        set.insert_after_active(Tab::preview("b"));
        assert_eq!(set.preview_index(), Some(1));
        assert!(set.keep(1));
        assert!(!set.keep(1), "already kept");
        assert_eq!(set.preview_index(), None);

        set.insert_after_active(Tab::preview("c"));
        set.toggle_pin(2);
        assert_eq!(set.preview_index(), None, "pinning keeps it");

        set.insert_after_active(Tab::preview("d"));
        set.active_mut().split_with("e", true);
        assert_eq!(set.preview_index(), None, "splitting keeps it");

        set.insert_after_active(Tab::preview("f"));
        let at = set.preview_index().unwrap();
        set.move_tab(at, 1);
        assert_eq!(set.preview_index(), None, "moving keeps it");

        set.insert_after_active(Tab::preview("g"));
        let at = set.preview_index().unwrap();
        set.close(at);
        set.reopen();
        assert_eq!(set.preview_index(), None, "reopened tabs are kept");
    }

    #[test]
    fn close_to_the_right_spares_pinned_tabs() {
        let mut set = TabSet::new(Tab::new("a"));
        set.insert_after_active(Tab::new("b"));
        set.insert_after_active(Tab::new("c"));
        set.insert_after_active(Tab::new("d"));
        set.toggle_pin(3);
        assert_eq!(labels(&set), ["d", "a", "b", "c"]);
        set.activate(3);
        set.close_to_right(1);
        assert_eq!(labels(&set), ["d", "a"]);
        assert_eq!(*set.active().focused(), "a", "the clicked tab takes over");
        assert!(set.reopen());
        assert_eq!(labels(&set), ["d", "a", "c"]);
    }
}
