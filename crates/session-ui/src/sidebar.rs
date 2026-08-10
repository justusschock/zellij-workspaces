use unicode_width::UnicodeWidthStr;

use zellij_tile::prelude::{Palette, PaletteColor};
use zellij_tile_utils::style;

use crate::{color, model::SidebarRow, statusbar::truncate_to_width};

pub(crate) fn move_selection(current: usize, delta: isize, row_count: usize) -> usize {
    if row_count == 0 {
        return 0;
    }
    current
        .saturating_add_signed(delta)
        .min(row_count.saturating_sub(1))
}

pub(crate) fn row_for_screen_line(screen_line: isize, row_count: usize) -> Option<usize> {
    usize::try_from(screen_line)
        .ok()?
        .checked_sub(1)
        .filter(|row_index| *row_index < row_count)
}

fn row_parts(row: &SidebarRow) -> (&'static str, String, &'static str) {
    match row {
        SidebarRow::Live(session) => (
            if session.current { "› " } else { "  " },
            session.name.clone(),
            if session.unread_tabs > 0 { " ●" } else { "" },
        ),
        SidebarRow::NewWorkspace => ("+ ", "New workspace".into(), ""),
    }
}

pub(crate) fn fit_row(row: &SidebarRow, width: usize) -> String {
    let (prefix, label, suffix) = row_parts(row);
    let decoration_width = prefix.width() + suffix.width();
    let label = truncate_to_width(&label, width.saturating_sub(decoration_width));
    let content = format!("{prefix}{label}{suffix}");
    let content_width = content.width();
    format!(
        "{content}{}",
        " ".repeat(width.saturating_sub(content_width))
    )
}

pub(crate) fn visible_row_range(
    selected: usize,
    row_count: usize,
    available_rows: usize,
) -> std::ops::Range<usize> {
    if row_count == 0 || available_rows == 0 {
        return 0..0;
    }
    let selected = selected.min(row_count - 1);
    let start = selected
        .saturating_sub(available_rows / 2)
        .min(row_count.saturating_sub(available_rows));
    start..(start + available_rows.min(row_count))
}

fn paint_row(line: &str, selected: bool, unread: bool, palette: Palette) -> String {
    let (fg, bg) = if selected {
        (palette.fg, color::MOCHA_SURFACE_1)
    } else {
        (palette.fg, palette.bg)
    };
    let regular = style!(fg, bg);
    if unread {
        if let Some(badge_start) = line.rfind('●') {
            let badge_end = badge_start + '●'.len_utf8();
            return format!(
                "{}{}{}",
                regular.paint(&line[..badge_start]),
                style!(palette.yellow, bg).bold().paint("●"),
                regular.paint(&line[badge_end..])
            );
        }
    }
    if selected {
        regular.bold().paint(line).to_string()
    } else {
        regular.paint(line).to_string()
    }
}

pub(crate) fn render(
    rows: &[SidebarRow],
    selected: usize,
    height: usize,
    width: usize,
    palette: Palette,
) -> (String, usize) {
    if height == 0 || width == 0 {
        return (String::new(), 0);
    }

    let fg = palette.fg;
    let heading = truncate_to_width(" Sessions", width);
    let mut lines = vec![format!(
        "{}{}",
        style!(palette.yellow, palette.bg).bold().paint(&heading),
        " ".repeat(width.saturating_sub(heading.width()))
    )];

    let body_height = height.saturating_sub(2);
    let visible = visible_row_range(selected, rows.len(), body_height);
    let first_row = visible.start;
    for (row_index, row) in rows
        .iter()
        .enumerate()
        .take(visible.end)
        .skip(visible.start)
    {
        let line = fit_row(row, width);
        let unread = matches!(row, SidebarRow::Live(session) if session.unread_tabs > 0);
        lines.push(paint_row(&line, row_index == selected, unread, palette));
    }

    while lines.len() + usize::from(height > 1) < height {
        lines.push(style!(fg, palette.bg).paint(" ".repeat(width)).to_string());
    }
    if height > 1 {
        let help = truncate_to_width(" j/k move  ↵ open", width);
        lines.push(format!(
            "{}{}",
            style!(fg, palette.bg).paint(&help),
            style!(fg, palette.bg).paint(" ".repeat(width.saturating_sub(help.width())))
        ));
    }

    (
        lines
            .into_iter()
            .take(height)
            .collect::<Vec<_>>()
            .join("\n"),
        first_row,
    )
}

#[cfg(test)]
mod tests {
    use crate::model::{SessionRow, SidebarRow};
    use zellij_tile::prelude::{Palette, PaletteColor};

    use super::{fit_row, move_selection, paint_row, row_for_screen_line, visible_row_range};

    fn live(name: &str, current: bool, unread_tabs: usize) -> SidebarRow {
        SidebarRow::Live(SessionRow {
            name: name.into(),
            current,
            unread_tabs,
            focus: Default::default(),
        })
    }

    #[test]
    fn selection_is_clamped_to_available_rows() {
        assert_eq!(move_selection(0, -1, 3), 0);
        assert_eq!(move_selection(0, 1, 3), 1);
        assert_eq!(move_selection(2, 1, 3), 2);
        assert_eq!(move_selection(9, 0, 3), 2);
        assert_eq!(move_selection(0, 1, 0), 0);
    }

    #[test]
    fn rendered_rows_fit_and_keep_native_bell_attention() {
        let current = fit_row(&live("project-with-a-long-name", true, 2), 18);
        assert_eq!(unicode_width::UnicodeWidthStr::width(current.as_str()), 18);
        assert!(current.starts_with("› "));
        assert!(current.ends_with(" ●"));

        let quiet = fit_row(&live("quiet", false, 0), 18);
        assert_eq!(unicode_width::UnicodeWidthStr::width(quiet.as_str()), 18);
        assert!(!quiet.contains('●'));
    }

    #[test]
    fn selected_rows_use_catppuccin_surface_instead_of_inverted_text() {
        let palette = Palette {
            fg: PaletteColor::Rgb((205, 214, 244)),
            bg: PaletteColor::Rgb((30, 30, 46)),
            ..Palette::default()
        };

        let rendered = paint_row("  project       ", true, false, palette);

        assert!(rendered.contains("48;2;69;71;90"));
        assert!(!rendered.contains("48;2;205;214;244"));
    }

    #[test]
    fn mouse_rows_skip_the_heading() {
        assert_eq!(row_for_screen_line(0, 3), None);
        assert_eq!(row_for_screen_line(1, 3), Some(0));
        assert_eq!(row_for_screen_line(3, 3), Some(2));
        assert_eq!(row_for_screen_line(4, 3), None);
    }

    #[test]
    fn scrolling_window_keeps_the_selection_visible() {
        assert_eq!(visible_row_range(0, 10, 4), 0..4);
        assert_eq!(visible_row_range(5, 10, 4), 3..7);
        assert_eq!(visible_row_range(9, 10, 4), 6..10);
    }
}
