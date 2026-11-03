//! Named themes for the window chrome: the toolbar, sidebar, tab strip and
//! status bar around the page. The page itself keeps the theme's own
//! background, so reading looks the same under every theme. Each theme has a
//! light and a dark variant.
//!
//! The editor themes tint the chrome and its accent. The fun ones add a
//! second and third color: folder and file icons in the sidebar, a colored
//! line on the active tab, and a colored status bar.

use gpui_kit::{
    component::{ActiveTheme, Theme, ThemeMode},
    *,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Palette {
    /// GPUI Kit's neutral grays.
    #[default]
    Classic,
    /// Warm paper and amber, like a sepia reading mode.
    Paper,
    /// Cool northern blue-gray.
    Fjord,
    /// Night-drive purple with hot pink and cyan.
    Synthwave,
    /// Charcoal and embers, with golden folders.
    Campfire,
    /// Tropical teal water with coral.
    Lagoon,
    /// Pink, grape and mint.
    Bubblegum,
}

/// The colors a theme sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    /// Toolbar, sidebar, tab strip and status bar.
    pub chrome: Hsla,
    /// Hovered and selected rows and tabs.
    pub selection: Hsla,
    /// Lines between the chrome and the page.
    pub border: Hsla,
    /// The active workspace's dot, the focused pane's bar, focus rings.
    pub primary: Hsla,
    pub accents: Accents,
}

/// The extra colors a theme paints the chrome with. `None` keeps the
/// default look.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Accents {
    /// Folder icons and chevrons in the sidebar.
    pub folder: Option<Hsla>,
    /// File icons in the sidebar.
    pub file: Option<Hsla>,
    /// The line across the top of the active tab.
    pub tab_line: Option<Hsla>,
    /// Status bar background and text.
    pub status: Option<(Hsla, Hsla)>,
}

impl Global for Accents {}

/// Hex colors for one mode of a theme.
struct Spec {
    chrome: u32,
    selection: u32,
    border: u32,
    primary: u32,
    folder: Option<u32>,
    file: Option<u32>,
    status: Option<(u32, u32)>,
}

/// An editor theme: tinted chrome, its accent on the active tab.
const fn plain(chrome: u32, selection: u32, border: u32, primary: u32) -> Spec {
    Spec {
        chrome,
        selection,
        border,
        primary,
        folder: None,
        file: None,
        status: None,
    }
}

/// A fun theme: `[chrome, selection, border, primary, folder, file,
/// status bar, status text]`.
const fn fun(c: [u32; 8]) -> Spec {
    Spec {
        chrome: c[0],
        selection: c[1],
        border: c[2],
        primary: c[3],
        folder: Some(c[4]),
        file: Some(c[5]),
        status: Some((c[6], c[7])),
    }
}

impl Palette {
    /// Quiet themes for editing and long reading.
    pub const EDITOR: [Palette; 3] = [Palette::Classic, Palette::Paper, Palette::Fjord];
    /// The colorful ones.
    pub const FUN: [Palette; 4] = [
        Palette::Synthwave,
        Palette::Campfire,
        Palette::Lagoon,
        Palette::Bubblegum,
    ];
    pub const ALL: [Palette; 7] = [
        Palette::Classic,
        Palette::Paper,
        Palette::Fjord,
        Palette::Synthwave,
        Palette::Campfire,
        Palette::Lagoon,
        Palette::Bubblegum,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Palette::Classic => "Classic",
            Palette::Paper => "Paper",
            Palette::Fjord => "Fjord",
            Palette::Synthwave => "Synthwave",
            Palette::Campfire => "Campfire",
            Palette::Lagoon => "Lagoon",
            Palette::Bubblegum => "Bubblegum",
        }
    }

    fn spec(self, dark: bool) -> Option<Spec> {
        let (light, night) = match self {
            Palette::Classic => return None,
            Palette::Paper => (
                plain(0xf4efe6, 0xe8dfcf, 0xe2d8c6, 0xa8702f),
                plain(0x24201b, 0x3a332a, 0x3a342b, 0xd9a35b),
            ),
            Palette::Fjord => (
                plain(0xebeff5, 0xd9e1ec, 0xd5dce7, 0x4c6fa5),
                plain(0x1f2530, 0x323c4c, 0x323b49, 0x88a6d6),
            ),
            Palette::Synthwave => (
                fun([
                    0xf3edfb, 0xe6d9f7, 0xdfd0f2, 0xd6338a, 0x0b9fae, 0x8a4fd8, 0x7b2cbf, 0xffffff,
                ]),
                fun([
                    0x1d1530, 0x34254f, 0x34274d, 0xff4fa3, 0x2de2e6, 0xb388ff, 0x5b2a86, 0xffd6ef,
                ]),
            ),
            Palette::Campfire => (
                fun([
                    0xf7efe4, 0xf0dcc4, 0xead6bd, 0xd9480f, 0xd99a00, 0xa0522d, 0x3b2a20, 0xffd8a8,
                ]),
                fun([
                    0x211a16, 0x3b2a20, 0x3a2b22, 0xff7a3d, 0xffc94d, 0xe8a17a, 0xc2410c, 0xfff4e6,
                ]),
            ),
            Palette::Lagoon => (
                fun([
                    0xe6f6f4, 0xcdece8, 0xc7e5e1, 0xf25f5c, 0x0f9d8a, 0x3a86ff, 0x0f9d8a, 0xffffff,
                ]),
                fun([
                    0x0f2226, 0x1d3a3f, 0x1f3a3e, 0xff8787, 0x38d9a9, 0x74c0fc, 0x0b7285, 0xe6fcf5,
                ]),
            ),
            Palette::Bubblegum => (
                fun([
                    0xfcedf5, 0xf7d9ea, 0xf2d0e3, 0xc2255c, 0x7950f2, 0x12b886, 0xf06595, 0xffffff,
                ]),
                fun([
                    0x26141f, 0x40213a, 0x3d2236, 0xf783ac, 0xb197fc, 0x63e6be, 0xa61e4d, 0xffdeeb,
                ]),
            ),
        };
        Some(if dark { night } else { light })
    }

    /// The colors for light or dark mode; `None` keeps the theme's own.
    pub fn colors(self, dark: bool) -> Option<Colors> {
        let s = self.spec(dark)?;
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        Some(Colors {
            chrome: c(s.chrome),
            selection: c(s.selection),
            border: c(s.border),
            primary: c(s.primary),
            accents: Accents {
                folder: s.folder.map(c),
                file: s.file.map(c),
                tab_line: Some(c(s.primary)),
                status: s.status.map(|(bg, fg)| (c(bg), c(fg))),
            },
        })
    }

    /// The colors shown beside its name in the picker, in the current mode:
    /// one for an editor theme, three for a fun one.
    pub fn swatches(self, cx: &App) -> Vec<Hsla> {
        let theme = cx.theme();
        match self.colors(theme.is_dark()) {
            None => vec![theme.muted_foreground],
            Some(c) => std::iter::once(c.primary)
                .chain(c.accents.folder)
                .chain(c.accents.file)
                .collect(),
        }
    }
}

/// The accents in effect, for code that paints the chrome.
pub fn accents(cx: &App) -> Accents {
    cx.try_global::<Accents>().copied().unwrap_or_default()
}

/// Load the light or dark theme, then lay the palette over its chrome. The
/// app paints its toolbar from `title_bar`, its tab strip from `tab_bar`,
/// its status bar from `status_bar` and the lines between them from
/// `sidebar_border`; under `Classic` those match how the window looked
/// before themes existed.
pub fn apply(mode: ThemeMode, palette: Palette, cx: &mut App) {
    Theme::change(mode, None, cx);
    let colors = palette.colors(mode.is_dark());
    cx.set_global(colors.map(|c| c.accents).unwrap_or_default());
    Theme::update(cx, |theme| {
        let Some(c) = colors else {
            theme.title_bar = theme.background;
            theme.tab_bar = theme.sidebar;
            theme.status_bar = theme.background;
            theme.sidebar_border = theme.border;
            return;
        };
        theme.sidebar = c.chrome;
        theme.title_bar = c.chrome;
        theme.tab_bar = c.chrome;
        theme.status_bar = c.chrome;
        // Not `accent`, `muted` or `border`: the page draws inline code,
        // code blocks and tables with those.
        theme.sidebar_accent = c.selection;
        theme.sidebar_border = c.border;
        theme.title_bar_border = c.border;
        theme.status_bar_border = c.border;
        theme.primary = c.primary;
        theme.ring = c.primary;
        theme.drag_border = c.primary;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn every_theme_has_both_modes_and_round_trips() {
        for palette in Palette::ALL {
            let tinted = palette != Palette::Classic;
            assert_eq!(palette.colors(false).is_some(), tinted);
            assert_eq!(palette.colors(true).is_some(), tinted);
            if let (Some(light), Some(dark)) = (palette.colors(false), palette.colors(true)) {
                assert!(light.chrome.l > 0.85, "{palette:?} light chrome is pale");
                assert!(dark.chrome.l < 0.2, "{palette:?} dark chrome is deep");
                let fun = Palette::FUN.contains(&palette);
                assert_eq!(light.accents.folder.is_some(), fun);
                assert_eq!(dark.accents.status.is_some(), fun);
            }
            let json = serde_json::to_string(&palette).unwrap();
            assert_eq!(serde_json::from_str::<Palette>(&json).unwrap(), palette);
        }
        let groups: Vec<_> = Palette::EDITOR
            .iter()
            .chain(&Palette::FUN)
            .copied()
            .collect();
        assert_eq!(groups, Palette::ALL);
        assert_eq!(
            serde_json::to_string(&Palette::Synthwave).unwrap(),
            r#""synthwave""#
        );
    }
}
