use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub sidebar: bool,
    pub font_size: f32,
    pub dark: Option<bool>,
    pub folder: Option<PathBuf>,
    pub file: Option<PathBuf>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            sidebar: true,
            font_size: 16.,
            dark: None,
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
        let mut prefs: Self = std::fs::read(Self::path())
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default();
        prefs.font_size = prefs.font_size.clamp(12., 26.);
        prefs
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
