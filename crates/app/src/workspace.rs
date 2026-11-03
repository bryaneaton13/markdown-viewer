//! A workspace: one folder's file tree. The window keeps several and lists
//! them all in the sidebar; its tabs can show files from any of them. A
//! workspace without a root ("Open Files") lists the open documents that
//! live outside every folder.

use crate::{files, sidebar};
use gpui_kit::Task;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub struct Workspace {
    pub root: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    /// The visible tree: a folder's expanded entries, or for loose files,
    /// the documents open in its tabs.
    pub rows: Vec<files::Row>,
    pub expanded: HashSet<PathBuf>,
    /// Open (expanded) in the sidebar.
    pub open: bool,
    /// When it was last the active workspace, in Unix milliseconds.
    pub last_used: u64,
    pub indexing: bool,
    pub limited: bool,
    /// Whether `files` reflects the folder (collapsed workspaces scan lazily).
    pub scanned: bool,
    pub scan_generation: usize,
    pub _scan: Option<Task<()>>,
}

impl Workspace {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            files: Vec::new(),
            rows: Vec::new(),
            expanded: HashSet::new(),
            open: true,
            last_used: 0,
            indexing: false,
            limited: false,
            scanned: false,
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

    /// Rebuild a folder's rows from its files. Loose files get theirs from
    /// `set_loose_rows`.
    pub fn rebuild_rows(&mut self) {
        if let Some(root) = &self.root {
            self.rows = files::rows(root, &self.files, &self.expanded);
        }
    }

    /// The loose workspace lists the open documents outside every folder,
    /// in tab order.
    pub fn set_loose_rows(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut seen = HashSet::new();
        self.rows = paths
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .map(|path| files::Row {
                path,
                depth: 0,
                directory: false,
            })
            .collect();
    }

    pub fn summary(&self) -> sidebar::Summary {
        sidebar::Summary {
            name: self.name(),
            folder: self.root.is_some(),
            open: self.open,
            rows: self.rows.len(),
            last_used: self.last_used,
        }
    }
}

pub fn name_of(root: Option<&Path>) -> String {
    match root {
        Some(root) => root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned()),
        None => "Open Files".into(),
    }
}

/// The workspace a file opened from Finder belongs to: the one with the
/// deepest folder containing it, else the loose "Open Files" workspace.
pub fn home_for(roots: &[Option<&Path>], path: &Path) -> Option<usize> {
    roots
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.filter(|r| path.starts_with(r)).map(|r| (i, r)))
        .max_by_key(|(_, r)| r.components().count())
        .map(|(i, _)| i)
        .or_else(|| roots.iter().position(|r| r.is_none()))
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
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
        assert_eq!(name_of(None), "Open Files");
    }
}
