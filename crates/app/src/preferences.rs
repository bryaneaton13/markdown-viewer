use crate::sidebar::Sort;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub sidebar: bool,
    pub font_size: f32,
    pub dark: Option<bool>,
    /// Open workspaces, in sidebar order.
    pub workspaces: Vec<SavedWorkspace>,
    pub active_workspace: usize,
    /// How the sidebar orders folder workspaces.
    pub sidebar_sort: Sort,
    /// Pre-0.2 single folder / file. Read once and folded into `workspaces`.
    #[serde(skip_serializing)]
    pub folder: Option<PathBuf>,
    #[serde(skip_serializing)]
    pub file: Option<PathBuf>,
}

/// One workspace: a folder (or loose files when `root` is `None`) and its tabs.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct SavedWorkspace {
    pub root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expanded: Vec<PathBuf>,
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
    /// Closed in the sidebar.
    #[serde(skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// Unix milliseconds it was last active, for "Recently Used" order.
    #[serde(skip_serializing_if = "is_zero")]
    pub last_used: u64,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct SavedTab {
    pub left: SavedPane,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<SavedPane>,
    #[serde(skip_serializing_if = "is_false")]
    pub focus_right: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub pinned: bool,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct SavedPane {
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "is_false")]
    pub source: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sidebar: true,
            font_size: 16.,
            dark: None,
            workspaces: Vec::new(),
            active_workspace: 0,
            sidebar_sort: Sort::Manual,
            folder: None,
            file: None,
        }
    }
}
impl Preferences {
    fn path() -> PathBuf {
        std::env::var_os("MARKDOWN_VIEWER_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join("Library/Application Support/Markdown Viewer")
            })
            .join("settings.json")
    }
    pub fn load() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        let prefs: Self = std::fs::read(Self::path())
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default();
        prefs.normalized()
    }

    /// Clamp values and fold the pre-workspace `folder` / `file` keys into a
    /// single workspace.
    pub fn normalized(mut self) -> Self {
        self.font_size = self.font_size.clamp(12., 26.);
        let (folder, file) = (self.folder.take(), self.file.take());
        if self.workspaces.is_empty() && (folder.is_some() || file.is_some()) {
            let root = folder.filter(|f| file.as_ref().is_none_or(|p| p.starts_with(f)));
            self.workspaces.push(SavedWorkspace {
                root,
                tabs: vec![SavedTab {
                    left: SavedPane {
                        path: file,
                        source: false,
                    },
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        self.active_workspace = self
            .active_workspace
            .min(self.workspaces.len().saturating_sub(1));
        self
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let path = Self::path();
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap())?;
            let temp = path.with_extension("json.tmp");
            std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
            std::fs::rename(temp, path)
        })();
        if let Err(e) = result {
            eprintln!("Could not save viewer preferences: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_folder_and_file_become_one_workspace() {
        let prefs: Preferences = serde_json::from_str(
            r#"{"sidebar":true,"font_size":40,"folder":"/notes","file":"/notes/a.md"}"#,
        )
        .unwrap();
        let prefs = prefs.normalized();
        assert_eq!(prefs.font_size, 26.);
        assert_eq!(prefs.workspaces.len(), 1);
        let ws = &prefs.workspaces[0];
        assert_eq!(ws.root, Some("/notes".into()));
        assert_eq!(ws.tabs[0].left.path, Some("/notes/a.md".into()));
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(
            !json.contains("\"folder\""),
            "legacy keys aren't written back"
        );
    }

    #[test]
    fn workspaces_round_trip() {
        let mut prefs = Preferences::default();
        prefs.workspaces.push(SavedWorkspace {
            root: Some("/a".into()),
            expanded: vec!["/a/notes".into()],
            tabs: vec![
                SavedTab {
                    left: SavedPane {
                        path: Some("/a/x.md".into()),
                        source: true,
                    },
                    right: Some(SavedPane {
                        path: Some("/a/y.md".into()),
                        source: false,
                    }),
                    focus_right: true,
                    pinned: true,
                },
                SavedTab::default(),
            ],
            active_tab: 1,
            collapsed: true,
            last_used: 42,
        });
        prefs.active_workspace = 3;
        prefs.sidebar_sort = Sort::Recent;
        let back: Preferences =
            serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        let back = back.normalized();
        assert_eq!(back.workspaces, prefs.workspaces);
        assert_eq!(back.active_workspace, 0, "clamped");
        assert_eq!(back.sidebar_sort, Sort::Recent);
        let json = serde_json::to_string(&back).unwrap();
        assert!(json.contains(r#""sidebar_sort":"recent""#));
    }
}
