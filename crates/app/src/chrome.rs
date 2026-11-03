//! Color presets for the window chrome: the toolbar, sidebar, tab strip and
//! status bar around the page. The page itself keeps the theme's own
//! background, so reading looks the same under every preset. Each preset has
//! a light and a dark variant, in the spirit of sepia, Nord-like slate,
//! Everforest-like sage and Rosé Pine Dawn-like rose editor themes.

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
    Neutral,
    /// Warm paper and amber.
    Paper,
    /// Cool blue-gray.
    Slate,
    /// Soft green.
    Sage,
    /// Muted pink.
    Rose,
}

/// The chrome colors a preset sets.
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
}

impl Palette {
    pub const ALL: [Palette; 5] = [
        Palette::Neutral,
        Palette::Paper,
        Palette::Slate,
        Palette::Sage,
        Palette::Rose,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Palette::Neutral => "Neutral",
            Palette::Paper => "Paper",
            Palette::Slate => "Slate",
            Palette::Sage => "Sage",
            Palette::Rose => "Rose",
        }
    }

    /// The colors for light or dark mode; `None` keeps the theme's own.
    pub fn colors(self, dark: bool) -> Option<Colors> {
        // (chrome, selection, border, primary), light then dark.
        let [light, night] = match self {
            Palette::Neutral => return None,
            Palette::Paper => [
                [0xf4efe6, 0xe8dfcf, 0xe2d8c6, 0xa8702f],
                [0x24201b, 0x3a332a, 0x3a342b, 0xd9a35b],
            ],
            Palette::Slate => [
                [0xebeff5, 0xd9e1ec, 0xd5dce7, 0x4c6fa5],
                [0x1f2530, 0x323c4c, 0x323b49, 0x88a6d6],
            ],
            Palette::Sage => [
                [0xedf1e8, 0xdce5d2, 0xd6dfcd, 0x55804a],
                [0x1f2620, 0x324033, 0x313c32, 0x93be84],
            ],
            Palette::Rose => [
                [0xf8efef, 0xefdee0, 0xe9d6d8, 0xb05a73],
                [0x272024, 0x3f3238, 0x3c2f35, 0xe0a3b4],
            ],
        };
        let [chrome, selection, border, primary] = if dark { night } else { light };
        Some(Colors {
            chrome: rgb(chrome).into(),
            selection: rgb(selection).into(),
            border: rgb(border).into(),
            primary: rgb(primary).into(),
        })
    }

    /// The colors as shown in the picker's swatch, in the current mode.
    pub fn swatch(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        self.colors(theme.is_dark())
            .map_or(theme.muted_foreground, |c| c.primary)
    }
}

/// Load the light or dark theme, then lay the preset over its chrome. The
/// app paints its toolbar from `title_bar`, its tab strip from `tab_bar`
/// its status bar from `status_bar` and the lines between them from
/// `sidebar_border`; under `Neutral` those match how the window looked
/// before presets existed.
pub fn apply(mode: ThemeMode, palette: Palette, cx: &mut App) {
    Theme::change(mode, None, cx);
    Theme::update(cx, |theme| {
        let Some(c) = palette.colors(mode.is_dark()) else {
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
    fn every_tinted_preset_has_both_modes_and_round_trips() {
        for palette in Palette::ALL {
            let tinted = palette != Palette::Neutral;
            assert_eq!(palette.colors(false).is_some(), tinted);
            assert_eq!(palette.colors(true).is_some(), tinted);
            if let (Some(light), Some(dark)) = (palette.colors(false), palette.colors(true)) {
                assert!(light.chrome.l > 0.85, "{palette:?} light chrome is pale");
                assert!(dark.chrome.l < 0.2, "{palette:?} dark chrome is deep");
            }
            let json = serde_json::to_string(&palette).unwrap();
            assert_eq!(serde_json::from_str::<Palette>(&json).unwrap(), palette);
        }
        assert_eq!(
            serde_json::to_string(&Palette::Slate).unwrap(),
            r#""slate""#
        );
    }
}
