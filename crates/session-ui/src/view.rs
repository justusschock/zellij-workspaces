use unicode_width::UnicodeWidthStr;

use zellij_tile::prelude::*;
use zellij_tile_utils::style;

pub struct View {
    pub blocks: Vec<Block>,
    pub len: usize,
}

#[derive(Default, Clone, Debug)]
pub struct Block {
    pub body: String,
    pub len: usize,
    pub tab_index: Option<usize>,
}

pub struct Bg;

impl Bg {
    pub fn render(cols: usize, palette: Palette) -> Block {
        let text = format!("{: <1$}", "", cols);
        let body = style!(palette.fg, palette.bg).paint(text);

        Block {
            body: body.to_string(),
            len: cols,
            tab_index: None,
        }
    }
}

pub struct Separator;

impl Separator {
    const CHAR: &'static str = "";

    pub fn render(fg: &PaletteColor, bg: &PaletteColor) -> Block {
        let text = Self::CHAR;
        let len = text.width();
        let body = style!(*fg, *bg).paint(text);

        Block {
            body: body.to_string(),
            len,
            tab_index: None,
        }
    }
}
