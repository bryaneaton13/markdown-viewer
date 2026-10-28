use std::path::{Path, PathBuf};

pub const MAX_FILES: usize = 20_000;

pub fn discover(root: &Path) -> (Vec<PathBuf>, bool) {
    let mut files = Vec::new();
    let mut limited = false;
    for entry in ignore::WalkBuilder::new(root)
        .hidden(true)
        .follow_links(false)
        .max_depth(Some(32))
        .build()
        .flatten()
    {
        if entry.file_type().is_some_and(|t| t.is_file())
            && markdown_core::is_markdown(entry.path())
        {
            files.push(entry.into_path());
            if files.len() > MAX_FILES {
                files.pop();
                limited = true;
                break;
            }
        }
    }
    files.sort_by_cached_key(|p| p.to_string_lossy().to_lowercase());
    (files, limited)
}

#[derive(Clone)]
pub struct Row {
    pub path: PathBuf,
    pub depth: usize,
    pub directory: bool,
}

pub fn rows(
    root: &Path,
    files: &[PathBuf],
    expanded: &std::collections::HashSet<PathBuf>,
) -> Vec<Row> {
    // Build adjacency once instead of rescanning every file for every expanded
    // directory. Tree layout remains bounded even with thousands of folders.
    let mut children =
        std::collections::BTreeMap::<PathBuf, std::collections::BTreeSet<(bool, PathBuf)>>::new();
    for file in files {
        if !file.starts_with(root) {
            continue;
        }
        if let Some(parent) = file.parent() {
            children
                .entry(parent.to_owned())
                .or_default()
                .insert((true, file.clone()));
            let mut dir = parent;
            while dir != root {
                let Some(parent) = dir.parent() else {
                    break;
                };
                children
                    .entry(parent.to_owned())
                    .or_default()
                    .insert((false, dir.to_owned()));
                dir = parent;
            }
        }
    }
    fn append(
        dir: &Path,
        depth: usize,
        children: &std::collections::BTreeMap<PathBuf, std::collections::BTreeSet<(bool, PathBuf)>>,
        expanded: &std::collections::HashSet<PathBuf>,
        out: &mut Vec<Row>,
    ) {
        let Some(entries) = children.get(dir) else {
            return;
        };
        for (file, path) in entries {
            out.push(Row {
                path: path.clone(),
                depth,
                directory: !file,
            });
            if !file && expanded.contains(path) {
                append(path, depth + 1, children, expanded, out);
            }
        }
    }
    let mut out = Vec::new();
    append(root, 0, &children, expanded, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_is_markdown_only_and_tree_expands() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("notes")).unwrap();
        for name in ["readme.md", "notes/a.markdown", "ignore.txt", ".hidden.md"] {
            std::fs::write(tmp.path().join(name), "# Test").unwrap();
        }
        let (files, limited) = discover(tmp.path());
        assert!(!limited);
        assert_eq!(files.len(), 2);
        let mut expanded = std::collections::HashSet::new();
        assert_eq!(rows(tmp.path(), &files, &expanded).len(), 2);
        expanded.insert(tmp.path().join("notes"));
        assert_eq!(rows(tmp.path(), &files, &expanded).len(), 3);
    }
}
