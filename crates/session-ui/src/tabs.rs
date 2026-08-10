use unicode_width::UnicodeWidthStr;

use zellij_tile::prelude::*;
use zellij_tile_utils::style;

use crate::{
    color,
    view::{Block, View},
};

pub struct Tabs;

impl Tabs {
    pub fn render_indices(
        tabs: &[TabInfo],
        indices: &[usize],
        mode: InputMode,
        palette: Palette,
    ) -> View {
        let mut blocks: Vec<Block> = Vec::with_capacity(indices.len());
        let mut total_len = 0;

        for index in indices {
            let block = Tab::render(&tabs[*index], mode, palette);

            total_len += block.len;
            blocks.push(block);
        }

        View {
            blocks,
            len: total_len,
        }
    }

    pub fn widths(tabs: &[TabInfo], mode: InputMode) -> Vec<usize> {
        tabs.iter()
            .map(|tab| Tab::display_width(tab, mode))
            .collect()
    }

    pub fn omission(palette: Palette) -> Block {
        let body = style!(palette.yellow, color::MOCHA_SURFACE_0)
            .bold()
            .paint("…");
        Block {
            body: body.to_string(),
            len: 1,
            tab_index: None,
        }
    }
}

pub struct Tab;

impl Tab {
    fn content(tab: &TabInfo, mode: InputMode) -> String {
        let mut text = tab.name.clone();

        if tab.active && mode == InputMode::RenameTab && text.is_empty() {
            text = String::from("Enter name...");
        }

        if tab.is_sync_panes_active {
            text.push_str(" [sync]");
        }
        text
    }

    pub fn display_width(tab: &TabInfo, mode: InputMode) -> usize {
        let badge_width = usize::from(tab.has_bell_notification) * 2;
        let content_width = Self::content(tab, mode).width() + badge_width;
        if content_width < 14 {
            16
        } else {
            content_width + 2
        }
    }

    pub fn render(tab: &TabInfo, mode: InputMode, palette: Palette) -> Block {
        let text = Self::content(tab, mode);
        let badge = if tab.has_bell_notification {
            " ●"
        } else {
            ""
        };
        let content_width = text.width() + badge.width();
        let len = Self::display_width(tab, mode);
        let padding = len.saturating_sub(content_width);
        let left_padding = padding / 2;
        let right_padding = padding - left_padding;

        let fg = match palette.theme_hue {
            ThemeHue::Dark => palette.white,
            ThemeHue::Light => palette.black,
        };

        let bg = if tab.active {
            color::MOCHA_SURFACE_1
        } else {
            color::MOCHA_SURFACE_0
        };

        let regular = style!(fg, bg).bold();
        let attention = style!(palette.yellow, bg).bold();
        let body = format!(
            "{}{}{}{}",
            regular.paint(" ".repeat(left_padding)),
            regular.paint(text),
            attention.paint(badge),
            regular.paint(" ".repeat(right_padding)),
        );

        Block {
            body,
            len,
            tab_index: Some(tab.position),
        }
    }
}

#[cfg(test)]
mod tests {
    use zellij_tile::prelude::{InputMode, Palette, PaletteColor, TabInfo};

    use super::Tab;

    fn tab(active: bool) -> TabInfo {
        TabInfo {
            name: "editor".into(),
            active,
            ..TabInfo::default()
        }
    }

    #[test]
    fn tab_backgrounds_use_catppuccin_mocha_surfaces() {
        let palette = Palette {
            fg: PaletteColor::Rgb((205, 214, 244)),
            bg: PaletteColor::Rgb((30, 30, 46)),
            ..Palette::default()
        };

        let active = Tab::render(&tab(true), InputMode::Normal, palette);
        let inactive = Tab::render(&tab(false), InputMode::Normal, palette);

        assert!(active.body.contains("48;2;69;71;90"));
        assert!(inactive.body.contains("48;2;49;50;68"));
    }
}
