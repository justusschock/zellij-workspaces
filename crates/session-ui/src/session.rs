use unicode_width::UnicodeWidthStr;

use zellij_tile::prelude::*;
use zellij_tile_utils::style;

use crate::{
    color::{self, ModeColor},
    view::{Block, View},
};

pub struct Session;

impl Session {
    pub fn render(
        name: Option<&str>,
        mode: InputMode,
        palette: Palette,
        unread_tabs: usize,
    ) -> View {
        let mut blocks = vec![];
        let mut total_len = 0;

        // name
        if let Some(name) = name {
            let ModeColor { fg, bg } = ModeColor::new(mode, palette);

            let unread = if unread_tabs == 0 {
                String::new()
            } else {
                format!(" ●{unread_tabs}")
            };
            let text = format!(" {}{} ", name.to_uppercase(), unread);
            let len = text.width();
            let body = style!(fg, bg).bold().paint(text);

            total_len += len;
            blocks.push(Block {
                body: body.to_string(),
                len,
                tab_index: None,
            })
        }

        // mode
        {
            let text = {
                let sym = match mode {
                    InputMode::Locked => "".to_string(),
                    InputMode::Normal => "".to_string(),
                    InputMode::Pane => "".to_string(),
                    _ => format!("{:?}", mode).to_uppercase(),
                };

                format!(" {} ", sym)
            };
            let len = text.width();
            let body = style!(palette.white, color::MOCHA_SURFACE_1).paint(text);

            total_len += len;
            blocks.push(Block {
                body: body.to_string(),
                len,
                tab_index: None,
            })
        }

        View {
            blocks,
            len: total_len,
        }
    }
}

#[cfg(test)]
mod tests {
    use zellij_tile::prelude::{InputMode, Palette};

    use super::Session;

    #[test]
    fn narrow_session_label_keeps_the_aggregate_unread_count() {
        let rendered = Session::render(Some("project"), InputMode::Normal, Palette::default(), 3);

        assert_eq!(rendered.len, 15);
        assert!(rendered.blocks[0].body.contains("PROJECT ●3"));
    }
}
