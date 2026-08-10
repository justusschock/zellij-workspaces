use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct VisibleTabs {
    pub(crate) indices: Vec<usize>,
    pub(crate) omitted: bool,
}

pub(crate) fn select_visible_tabs(
    widths: &[usize],
    active_index: usize,
    available_width: usize,
) -> VisibleTabs {
    if widths.is_empty() {
        return VisibleTabs::default();
    }
    if widths.iter().sum::<usize>() <= available_width {
        return VisibleTabs {
            indices: (0..widths.len()).collect(),
            omitted: false,
        };
    }

    let active_index = active_index.min(widths.len() - 1);
    let tab_budget = available_width.saturating_sub(1);
    if widths[active_index] > tab_budget {
        return VisibleTabs {
            indices: vec![],
            omitted: true,
        };
    }

    let mut start = active_index;
    let mut end = active_index;
    let mut used = widths[active_index];
    loop {
        let mut changed = false;
        if start > 0 && used + widths[start - 1] <= tab_budget {
            start -= 1;
            used += widths[start];
            changed = true;
        }
        if end + 1 < widths.len() && used + widths[end + 1] <= tab_budget {
            end += 1;
            used += widths[end];
            changed = true;
        }
        if !changed {
            break;
        }
    }

    VisibleTabs {
        indices: (start..=end).collect(),
        omitted: true,
    }
}

pub(crate) fn truncate_to_width(value: &str, max_width: usize) -> String {
    let mut width = 0;
    value
        .chars()
        .take_while(|character| {
            let character_width = character.width().unwrap_or(0);
            if width + character_width > max_width {
                false
            } else {
                width += character_width;
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{VisibleTabs, select_visible_tabs, truncate_to_width};

    #[test]
    fn empty_tabs_need_no_omission_marker() {
        assert_eq!(
            select_visible_tabs(&[], 0, 40),
            VisibleTabs {
                indices: vec![],
                omitted: false,
            }
        );
    }

    #[test]
    fn keeps_every_tab_when_they_fit() {
        assert_eq!(
            select_visible_tabs(&[8, 8, 8], 1, 24),
            VisibleTabs {
                indices: vec![0, 1, 2],
                omitted: false,
            }
        );
    }

    #[test]
    fn keeps_the_active_tab_and_a_contiguous_window_when_truncated() {
        assert_eq!(
            select_visible_tabs(&[8, 8, 8, 8], 2, 18),
            VisibleTabs {
                indices: vec![1, 2],
                omitted: true,
            }
        );
    }

    #[test]
    fn returns_only_an_omission_marker_when_the_active_tab_cannot_fit() {
        assert_eq!(
            select_visible_tabs(&[16], 0, 8),
            VisibleTabs {
                indices: vec![],
                omitted: true,
            }
        );
    }

    #[test]
    fn unicode_truncation_never_exceeds_the_available_width() {
        let rendered = truncate_to_width(" session ● ", 8);
        assert!(rendered.width() <= 8);
        assert_eq!(truncate_to_width("abc", 0), "");
    }
}
