use crate::{chrome::Palette, sidebar::Sort};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub sidebar: bool,
    pub font_size: f32,
    /// Light or dark; `None` matches the system.
    pub dark: Option<bool>,
    /// Color theme for the window chrome. A name this version doesn't know
    /// falls back to Classic instead of discarding every setting.
    #[serde(deserialize_with = "lenient")]
    pub palette: Palette,
    /// Open workspaces, in sidebar order.
    pub workspaces: Vec<SavedWorkspace>,
    pub active_workspace: usize,
    /// The window's tabs. They can mix files from any workspace.
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
    /// How the sidebar orders folder workspaces.
    pub sidebar_sort: Sort,
    /// Ask GitHub for a newer release now and then.
    pub check_updates: bool,
    /// The version whose "What's new" card was last shown. Missing in
    /// settings written before 0.2, which therefore count as an update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_version: Option<String>,
    /// No settings file existed: a first launch, which has nothing "new".
    #[serde(skip)]
    pub first_launch: bool,
    /// Pre-0.2 single folder / file. Read once and folded into `workspaces`.
    #[serde(skip_serializing)]
    pub folder: Option<PathBuf>,
    #[serde(skip_serializing)]
    pub file: Option<PathBuf>,
}

/// One workspace: a folder, or loose files when `root` is `None`.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct SavedWorkspace {
    pub root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expanded: Vec<PathBuf>,
    /// 0.2 previews kept tabs per workspace. Read once and folded into the
    /// window's tabs.
    #[serde(skip_serializing)]
    pub tabs: Vec<SavedTab>,
    #[serde(skip_serializing)]
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
    /// Still the preview tab, which the next sidebar click reuses.
    #[serde(skip_serializing_if = "is_false")]
    pub preview: bool,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct SavedPane {
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "is_false")]
    pub source: bool,
}

fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Palette, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
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
            palette: Palette::Classic,
            workspaces: Vec::new(),
            active_workspace: 0,
            tabs: Vec::new(),
            active_tab: 0,
            sidebar_sort: Sort::Manual,
            check_updates: true,
            last_seen_version: None,
            first_launch: false,
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
        let Ok(bytes) = std::fs::read(Self::path()) else {
            return Self {
                first_launch: true,
                ..Self::default()
            };
        };
        let prefs: Self = serde_json::from_slice(&bytes).unwrap_or_default();
        prefs.normalized()
    }

    /// Clamp values, fold the pre-workspace `folder` / `file` keys into a
    /// single workspace, and gather per-workspace tabs into the window's.
    pub fn normalized(mut self) -> Self {
        self.font_size = self.font_size.clamp(12., 26.);
        let (folder, file) = (self.folder.take(), self.file.take());
        if self.workspaces.is_empty() && (folder.is_some() || file.is_some()) {
            let root = folder.filter(|f| file.as_ref().is_none_or(|p| p.starts_with(f)));
            self.workspaces.push(SavedWorkspace {
                root,
                ..Default::default()
            });
            self.tabs = vec![SavedTab {
                left: SavedPane {
                    path: file,
                    source: false,
                },
                ..Default::default()
            }];
        }
        self.active_workspace = self
            .active_workspace
            .min(self.workspaces.len().saturating_sub(1));
        if self.tabs.is_empty() {
            // The active workspace's tabs first, so its active tab stays put.
            let mut order: Vec<usize> = (0..self.workspaces.len()).collect();
            if !order.is_empty() {
                order.remove(self.active_workspace);
                order.insert(0, self.active_workspace);
            }
            for (n, ix) in order.into_iter().enumerate() {
                let ws = &mut self.workspaces[ix];
                let active = ws.active_tab;
                for (i, tab) in std::mem::take(&mut ws.tabs).into_iter().enumerate() {
                    let blank = tab.left.path.is_none()
                        && tab.right.as_ref().is_none_or(|r| r.path.is_none());
                    if n == 0 && i == active {
                        self.active_tab = self.tabs.len();
                    } else if blank {
                        continue;
                    }
                    self.tabs.push(tab);
                }
            }
        }
        for ws in &mut self.workspaces {
            ws.tabs.clear();
        }
        self.active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));
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
            log::warn!("Could not save viewer preferences: {e}");
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
        assert_eq!(prefs.workspaces[0].root, Some("/notes".into()));
        assert_eq!(prefs.tabs[0].left.path, Some("/notes/a.md".into()));
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(
            !json.contains("\"folder\""),
            "legacy keys aren't written back"
        );
    }

    #[test]
    fn unknown_theme_names_keep_the_other_settings() {
        let prefs: Preferences =
            serde_json::from_str(r#"{"font_size":20,"palette":"sage"}"#).unwrap();
        assert_eq!((prefs.font_size, prefs.palette), (20., Palette::Classic));
    }

    #[test]
    fn workspaces_round_trip() {
        let mut prefs = Preferences::default();
        prefs.workspaces.push(SavedWorkspace {
            root: Some("/a".into()),
            expanded: vec!["/a/notes".into()],
            collapsed: true,
            last_used: 42,
            ..Default::default()
        });
        prefs.tabs = vec![
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
                preview: false,
            },
            SavedTab {
                preview: true,
                ..Default::default()
            },
        ];
        prefs.active_tab = 1;
        prefs.active_workspace = 3;
        prefs.sidebar_sort = Sort::Recent;
        prefs.palette = Palette::Synthwave;
        let back: Preferences =
            serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        let back = back.normalized();
        assert_eq!(back.workspaces, prefs.workspaces);
        assert_eq!((&back.tabs, back.active_tab), (&prefs.tabs, 1));
        assert_eq!(back.active_workspace, 0, "clamped");
        assert_eq!(back.sidebar_sort, Sort::Recent);
        assert_eq!(back.palette, Palette::Synthwave);
        let json = serde_json::to_string(&back).unwrap();
        assert!(json.contains(r#""sidebar_sort":"recent""#));
        assert!(json.contains(r#""palette":"synthwave""#));
    }

    #[test]
    fn per_workspace_tabs_fold_into_the_window() {
        let tab = |path: &str| SavedTab {
            left: SavedPane {
                path: Some(path.into()),
                source: false,
            },
            ..Default::default()
        };
        let prefs: Preferences = serde_json::from_value(serde_json::json!({
            "active_workspace": 1,
            "workspaces": [
                {"root": "/a", "tabs": [tab("/a/1.md"), SavedTab::default()], "active_tab": 0},
                {"root": "/b", "tabs": [tab("/b/1.md"), tab("/b/2.md")], "active_tab": 1},
            ],
        }))
        .unwrap();
        let prefs = prefs.normalized();
        let paths: Vec<_> = prefs
            .tabs
            .iter()
            .map(|t| t.left.path.clone().unwrap())
            .collect();
        assert_eq!(
            paths,
            [PathBuf::from("/b/1.md"), "/b/2.md".into(), "/a/1.md".into()]
        );
        assert_eq!(prefs.active_tab, 1, "blank tabs are dropped");
        assert!(prefs.workspaces.iter().all(|w| w.tabs.is_empty()));
    }
}
