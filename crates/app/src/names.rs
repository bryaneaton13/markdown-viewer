//! Telling files with the same name apart, the way editors label tabs: two
//! `README.md` tabs read `README.md  notes` and `README.md  tools`. Each file
//! gets the fewest parent folders, nearest first, that no other file with
//! its name shares.

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// For each of `paths`, the parent folders that tell it apart from every
/// other path with the same file name, like `notes` or `docs/notes`. `None`
/// for a name nothing else shares, unless `always` asks for at least the
/// nearest folder. The same path listed twice counts once.
pub fn disambiguate(paths: &[&Path], always: bool) -> Vec<Option<String>> {
    let mut by_name: HashMap<OsString, Vec<PathBuf>> = HashMap::new();
    for path in paths {
        let group = by_name
            .entry(path.file_name().unwrap_or_default().to_owned())
            .or_default();
        if !group.iter().any(|p| p == path) {
            group.push(path.to_path_buf());
        }
    }
    paths
        .iter()
        .map(|path| {
            let group = &by_name[path.file_name().unwrap_or_default()];
            let others: Vec<&PathBuf> = group.iter().filter(|p| p != path).collect();
            if others.is_empty() && !always {
                return None;
            }
            let mine = parents(path);
            let theirs: Vec<Vec<String>> = others.iter().map(|p| parents(p)).collect();
            let depth = (1..=mine.len())
                .find(|&d| theirs.iter().all(|t| suffix(t, d) != suffix(&mine, d)))
                .unwrap_or(mine.len());
            (depth > 0).then(|| suffix(&mine, depth).join("/"))
        })
        .collect()
}

/// A path's folder names, nearest first.
fn parents(path: &Path) -> Vec<String> {
    path.parent()
        .map(|p| {
            p.components()
                .rev()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The nearest `depth` folders, outermost first.
fn suffix(parents: &[String], depth: usize) -> Vec<String> {
    let mut out: Vec<String> = parents.iter().take(depth).cloned().collect();
    out.reverse();
    out
}

/// A path for display, with the home folder as `~`.
pub fn tilde(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return Path::new("~").join(rest).to_string_lossy().into_owned();
    }
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(paths: &[&str], always: bool) -> Vec<Option<String>> {
        let paths: Vec<&Path> = paths.iter().map(Path::new).collect();
        disambiguate(&paths, always)
    }

    #[test]
    fn unique_names_need_nothing() {
        assert_eq!(run(&["/a/x.md", "/b/y.md"], false), [None, None]);
        assert_eq!(
            run(&["/a/x.md", "/a/x.md"], false),
            [None, None],
            "the same file twice"
        );
    }

    #[test]
    fn shared_names_get_the_nearest_folder_that_differs() {
        assert_eq!(
            run(
                &["/w/notes/README.md", "/w/tools/README.md", "/w/a.md"],
                false
            ),
            [Some("notes".into()), Some("tools".into()), None]
        );
        assert_eq!(
            run(&["/one/docs/README.md", "/two/docs/README.md"], false),
            [Some("one/docs".into()), Some("two/docs".into())]
        );
        // Each path takes only as many folders as it needs.
        assert_eq!(
            run(
                &["/a/x/README.md", "/b/x/README.md", "/c/y/README.md"],
                false
            ),
            [Some("a/x".into()), Some("b/x".into()), Some("y".into())]
        );
        // One path is the other's suffix: the shorter one runs out.
        assert_eq!(
            run(&["/x/README.md", "/a/x/README.md"], false),
            [Some("x".into()), Some("a/x".into())]
        );
    }

    #[test]
    fn always_names_at_least_the_folder() {
        assert_eq!(
            run(&["/w/notes/a.md", "/w/b.md"], true),
            [Some("notes".into()), Some("w".into())]
        );
        assert_eq!(run(&["/a.md"], true), [None], "no folder to name");
    }
}
