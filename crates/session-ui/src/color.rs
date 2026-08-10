use zellij_tile::prelude::{InputMode, Palette, PaletteColor, Styling, ThemeHue};

pub const MOCHA_SURFACE_1: PaletteColor = PaletteColor::Rgb((69, 71, 90));
pub const MOCHA_SURFACE_0: PaletteColor = PaletteColor::Rgb((49, 50, 68));

pub fn palette_from_styling(styling: Styling) -> Palette {
    Palette {
        source: Default::default(),
        theme_hue: ThemeHue::Dark,
        fg: styling.ribbon_unselected.background,
        bg: styling.text_selected.background,
        black: styling.text_unselected.background,
        red: styling.exit_code_error.base,
        green: styling.exit_code_success.base,
        yellow: styling.exit_code_error.emphasis_0,
        blue: styling.ribbon_unselected.emphasis_2,
        magenta: styling.ribbon_unselected.emphasis_3,
        cyan: styling.text_unselected.emphasis_1,
        white: styling.text_unselected.base,
        orange: styling.text_unselected.emphasis_0,
        gray: styling.table_title.background,
        purple: styling.exit_code_error.emphasis_3,
        gold: styling.exit_code_error.emphasis_1,
        silver: styling.exit_code_error.emphasis_2,
        pink: styling.multiplayer_user_colors.player_9,
        brown: styling.frame_selected.emphasis_3,
    }
}

pub struct ModeColor {
    pub fg: PaletteColor,
    pub bg: PaletteColor,
}

impl ModeColor {
    pub fn new(mode: InputMode, palette: Palette) -> Self {
        let fg = match palette.theme_hue {
            ThemeHue::Dark => palette.black,
            ThemeHue::Light => palette.white,
        };

        let bg = match mode {
            InputMode::Locked => palette.cyan,
            InputMode::Normal => palette.green,
            _ => palette.orange,
        };

        Self { fg, bg }
    }
}

#[cfg(test)]
mod tests {
    use zellij_tile::prelude::{Palette, PaletteColor, Styling, ThemeHue};

    use super::palette_from_styling;

    #[test]
    fn zellij_styling_preserves_the_terminal_background_for_plugin_rendering() {
        let original = Palette {
            theme_hue: ThemeHue::Dark,
            fg: PaletteColor::Rgb((205, 214, 244)),
            bg: PaletteColor::Rgb((30, 30, 46)),
            black: PaletteColor::Rgb((69, 71, 90)),
            white: PaletteColor::Rgb((186, 194, 222)),
            ..Palette::default()
        };
        let styling = Styling::from(original);

        let palette = palette_from_styling(styling);

        assert_eq!(palette.bg, PaletteColor::Rgb((30, 30, 46)));
        assert_eq!(palette.fg, PaletteColor::Rgb((205, 214, 244)));
        assert_eq!(palette.black, PaletteColor::Rgb((69, 71, 90)));
    }
}
