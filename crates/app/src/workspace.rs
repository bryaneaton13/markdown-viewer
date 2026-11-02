//! A workspace: one folder's file tree plus the tabs opened from it. The
//! window keeps several and shows one at a time. A workspace without a root
//! ("Files") collects documents opened from outside every folder.

use crate::{files, pane::DocPane, tabs::TabSet};
use gpui_kit::{Entity, Task, UniformListScrollHandle};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub struct Workspace {
    pub root: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub rows: Vec<files::Row>,
    pub expanded: HashSet<PathBuf>,
    pub selected: usize,
    pub tree_scroll: UniformListScrollHandle,
    pub indexing: bool,
    pub limited: bool,
    /// Whether `files` reflects the folder (inactive workspaces scan lazily).
    pub scanned: bool,
    pub tabs: TabSet<Entity<DocPane>>,
    pub scan_generation: usize,
    pub _scan: Option<Task<()>>,
}

impl Workspace {
    pub fn new(root: Option<PathBuf>, tabs: TabSet<Entity<DocPane>>) -> Self {
        Self {
            root,
            files: Vec::new(),
            rows: Vec::new(),
            expanded: HashSet::new(),
            selected: 0,
            tree_scroll: UniformListScrollHandle::new(),
            indexing: false,
            limited: false,
            scanned: false,
            tabs,
            scan_generation: 0,
            _scan: None,
        }
    }

    pub fn name(&self) -> String {
        name_of(self.root.as_deref())
    }

    /// Whether a file belongs under this workspace's folder.
    pub fn contains(&self, path: &Path) -> bool {
        self.root.as_ref().is_some_and(|r| path.starts_with(r))
    }

    pub fn rebuild_rows(&mut self) {
        self.rows = self
            .root
            .as_ref()
            .map(|r| files::rows(r, &self.files, &self.expanded))
            .unwrap_or_default();
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }
}

pub fn name_of(root: Option<&Path>) -> String {
    match root {
        Some(root) => root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned()),
        None => "Files".into(),
    }
}

/// The workspace a file opened from Finder belongs to: the one with the
/// deepest folder containing it, else the loose "Files" workspace.
pub fn home_for(roots: &[Option<&Path>], path: &Path) -> Option<usize> {
    roots
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.filter(|r| path.starts_with(r)).map(|r| (i, r)))
        .max_by_key(|(_, r)| r.components().count())
        .map(|(i, _)| i)
        .or_else(|| roots.iter().position(|r| r.is_none()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_go_to_the_deepest_containing_folder() {
        let a = PathBuf::from("/work");
        let b = PathBuf::from("/work/docs");
        let roots = [Some(a.as_path()), None, Some(b.as_path())];
        assert_eq!(home_for(&roots, Path::new("/work/docs/x.md")), Some(2));
        assert_eq!(home_for(&roots, Path::new("/work/y.md")), Some(0));
        assert_eq!(home_for(&roots, Path::new("/elsewhere/z.md")), Some(1));
        assert_eq!(home_for(&roots[..1], Path::new("/elsewhere/z.md")), None);
        assert_eq!(name_of(Some(&b)), "docs");
        assert_eq!(name_of(None), "Files");
    }
}
