mod color;
mod datetime;
mod keymap;
mod model;
mod session;
mod sidebar;
mod statusbar;
mod tabs;
mod view;

use std::{
    cmp::{max, min},
    collections::BTreeMap,
    path::PathBuf,
};

use zellij_tile::prelude::*;
use zellij_tile_utils::style;

use crate::{
    datetime::DateTime,
    keymap::{SidebarAction, SidebarKeymaps},
    model::{SidebarRow, dashboard_rows, is_wide, unread_count},
    session::Session,
    sidebar::{move_selection, row_for_screen_line},
    statusbar::{select_visible_tabs, truncate_to_width},
    tabs::Tabs,
    view::Bg,
};

const SIDEBAR_REFRESH_SECONDS: f64 = 1.0;
const KEYMAP_CONTEXT: &str = "zellij-workspaces-sidebar-keymaps";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PluginView {
    #[default]
    Statusbar,
    Sidebar,
}

#[derive(Default)]
struct State {
    view: PluginView,
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    mode_info: ModeInfo,
    mouse_click_pos: usize,
    should_change_tab: bool,
    now: DateTime,
    sessions: SessionListSnapshot,
    client_id: ClientId,
    plugin_id: u32,
    initial_cwd: PathBuf,
    pane_manifest: PaneManifest,
    sidebar_selected: usize,
    sidebar_first_row: usize,
    sidebar_visible_rows: usize,
    sidebar_rendered_rows: Vec<SidebarRow>,
    sidebar_visible: bool,
    sidebar_wide: Option<bool>,
    permissions_granted: bool,
    permission_error: Option<String>,
    sidebar_keymaps: SidebarKeymaps,
}

register_plugin!(State);

fn host_hide_self() {
    #[cfg(target_family = "wasm")]
    hide_self();
}

fn host_show_self(should_float_if_hidden: bool) {
    #[cfg(target_family = "wasm")]
    show_self(should_float_if_hidden);
    #[cfg(not(target_family = "wasm"))]
    let _ = should_float_if_hidden;
}

fn host_move_focus_right() {
    #[cfg(target_family = "wasm")]
    move_focus(Direction::Right);
}

fn host_switch_session(name: &str, tab_position: Option<usize>, pane_id: Option<(u32, bool)>) {
    #[cfg(target_family = "wasm")]
    switch_session_with_focus(name, tab_position, pane_id);
    #[cfg(not(target_family = "wasm"))]
    let _ = (name, tab_position, pane_id);
}

fn host_get_session_list() -> Result<SessionListSnapshot, String> {
    #[cfg(target_family = "wasm")]
    return get_session_list();
    #[cfg(not(target_family = "wasm"))]
    Err("session list is only available in the Zellij host".to_owned())
}

#[cfg(any(target_family = "wasm", test))]
fn open_workspace_picker_with<Open, Show>(
    initial_cwd: &std::path::Path,
    open_pane: Open,
    show_pane: Show,
) where
    Open: FnOnce(
        CommandToRun,
        Option<FloatingPaneCoordinates>,
        BTreeMap<String, String>,
    ) -> Option<PaneId>,
    Show: FnOnce(PaneId, bool, bool),
{
    let mut command = CommandToRun::new("zellij-workspaces");
    command.args.push("--new".into());
    command.cwd = Some(initial_cwd.to_path_buf());
    let coordinates = FloatingPaneCoordinates::new(
        Some("10%".into()),
        Some("9%".into()),
        Some("80%".into()),
        Some("82%".into()),
        Some(false),
        Some(false),
    );
    if let Some(pane_id) = open_pane(command, coordinates, BTreeMap::new()) {
        show_pane(pane_id, true, true);
    }
}

fn host_open_workspace_picker(initial_cwd: &std::path::Path) {
    #[cfg(target_family = "wasm")]
    open_workspace_picker_with(
        initial_cwd,
        open_command_pane_floating_near_plugin,
        show_pane_with_id,
    );
    #[cfg(not(target_family = "wasm"))]
    let _ = initial_cwd;
}

fn host_load_sidebar_keymaps() {
    #[cfg(target_family = "wasm")]
    run_command(
        &["zellij-workspaces", "--print-sidebar-keymaps"],
        BTreeMap::from([("zellij-workspaces".to_owned(), KEYMAP_CONTEXT.to_owned())]),
    );
}

fn plugin_permissions() -> Vec<PermissionType> {
    vec![
        PermissionType::ReadApplicationState,
        PermissionType::ChangeApplicationState,
        PermissionType::RunCommands,
    ]
}

fn statusbar_events() -> Vec<EventType> {
    vec![
        EventType::PermissionRequestResult,
        EventType::TabUpdate,
        EventType::ModeUpdate,
        EventType::Mouse,
        EventType::Timer,
    ]
}

fn sidebar_events() -> Vec<EventType> {
    vec![
        EventType::PermissionRequestResult,
        EventType::TabUpdate,
        EventType::ModeUpdate,
        EventType::PaneUpdate,
        EventType::SessionUpdate,
        EventType::Key,
        EventType::Mouse,
        EventType::Timer,
        EventType::Visible,
        EventType::RunCommandResult,
    ]
}

fn same_rendered_rows(left: &[SidebarRow], right: &[SidebarRow]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (SidebarRow::Live(left), SidebarRow::Live(right)) => {
                    left.name == right.name
                        && left.current == right.current
                        && left.unread_tabs == right.unread_tabs
                }
                (SidebarRow::NewWorkspace, SidebarRow::NewWorkspace) => true,
                _ => false,
            })
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.view = match configuration.get("view").map(String::as_str) {
            Some("sidebar") => PluginView::Sidebar,
            _ => PluginView::Statusbar,
        };
        let plugin_ids = get_plugin_ids();
        self.client_id = plugin_ids.client_id;
        self.plugin_id = plugin_ids.plugin_id;
        self.initial_cwd = plugin_ids.initial_cwd;
        self.sidebar_visible = true;

        let permissions = plugin_permissions();
        request_permission(&permissions);

        set_selectable(true);
        match self.view {
            PluginView::Statusbar => {
                set_timeout(1.0);
                subscribe(&statusbar_events());
            }
            PluginView::Sidebar => {
                subscribe(&sidebar_events());
            }
        }
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = false;

        match event {
            Event::PermissionRequestResult(status) => match status {
                PermissionStatus::Granted => {
                    self.permissions_granted = true;
                    self.permission_error = None;
                    if self.view == PluginView::Statusbar {
                        set_selectable(false);
                    } else {
                        if let Ok(snapshot) = host_get_session_list() {
                            let _ = self.replace_sessions(snapshot);
                        }
                        set_timeout(SIDEBAR_REFRESH_SECONDS);
                        host_load_sidebar_keymaps();
                        if let Some(columns) = self
                            .tabs
                            .iter()
                            .find(|tab| tab.active)
                            .map(|tab| tab.display_area_columns)
                        {
                            let _ = self.update_sidebar_width(columns);
                        }
                        should_render = true;
                    }
                }
                PermissionStatus::Denied => {
                    self.permissions_granted = false;
                    self.permission_error = Some(" Zellij permissions required".to_owned());
                    should_render = true;
                }
            },
            Event::ModeUpdate(mode_info) => {
                should_render = self.mode_info != mode_info;
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                if let Some(active_tab_index) = tabs.iter().position(|t| t.active) {
                    // tabs are indexed starting from 1 so we need to add 1
                    let active_tab_idx = active_tab_index + 1;

                    let tabs_changed = self.active_tab_idx != active_tab_idx || self.tabs != tabs;
                    self.active_tab_idx = active_tab_idx;
                    if self.view == PluginView::Sidebar && self.permissions_granted {
                        let width_changed =
                            self.update_sidebar_width(tabs[active_tab_index].display_area_columns);
                        let rows_changed = self.update_current_session_tabs(&tabs);
                        should_render = width_changed || rows_changed;
                    } else {
                        should_render = tabs_changed;
                    }
                    self.tabs = tabs;
                } else {
                    eprintln!("Could not find active tab.");
                    should_render = self.view == PluginView::Statusbar && self.tabs != tabs;
                    self.tabs = tabs;
                }
            }
            Event::PaneUpdate(pane_manifest) if self.view == PluginView::Sidebar => {
                self.pane_manifest = pane_manifest;
            }
            Event::SessionUpdate(live_sessions, resurrectable_sessions)
                if self.view == PluginView::Sidebar =>
            {
                let snapshot = SessionListSnapshot {
                    live_sessions,
                    resurrectable_sessions,
                };
                should_render = self.replace_sessions(snapshot);
            }
            Event::Visible(visible) if self.view == PluginView::Sidebar => {
                should_render = self.sidebar_visible != visible;
                self.sidebar_visible = visible;
                if visible && self.permissions_granted {
                    should_render |= self.refresh_sessions();
                }
            }
            Event::Key(key) if self.view == PluginView::Sidebar => {
                should_render = self.handle_sidebar_key(key);
            }
            Event::Mouse(event) => {
                if self.view == PluginView::Sidebar {
                    should_render = self.handle_sidebar_mouse(event);
                } else {
                    match event {
                        Mouse::LeftClick(_, col) => {
                            if self.mouse_click_pos != col {
                                should_render = true;
                                self.should_change_tab = true;
                            }
                            self.mouse_click_pos = col;
                        }
                        Mouse::ScrollUp(_) => {
                            should_render = true;
                            switch_tab_to(min(self.active_tab_idx + 1, self.tabs.len()) as u32);
                        }
                        Mouse::ScrollDown(_) => {
                            should_render = true;
                            switch_tab_to(max(self.active_tab_idx.saturating_sub(1), 1) as u32);
                        }
                        _ => {}
                    }
                }
            }
            Event::Timer(_) => {
                if self.view == PluginView::Statusbar {
                    let now = DateTime::now();
                    should_render = now != self.now;
                    self.now = now;
                    set_timeout(1.0);
                } else if self.permissions_granted {
                    if self.sidebar_visible {
                        should_render = self.refresh_sessions();
                    }
                    set_timeout(SIDEBAR_REFRESH_SECONDS);
                }
            }
            Event::RunCommandResult(exit_code, stdout, stderr, context)
                if self.view == PluginView::Sidebar
                    && context.get("zellij-workspaces").map(String::as_str)
                        == Some(KEYMAP_CONTEXT) =>
            {
                if exit_code == Some(0) {
                    match String::from_utf8(stdout)
                        .map_err(|error| error.to_string())
                        .and_then(|source| SidebarKeymaps::from_protocol(&source))
                    {
                        Ok(keymaps) => self.sidebar_keymaps = keymaps,
                        Err(error) => eprintln!("Failed to load sidebar keymaps: {error}"),
                    }
                } else {
                    eprintln!(
                        "Failed to load sidebar keymaps: {}",
                        String::from_utf8_lossy(&stderr).trim()
                    );
                }
                should_render = false;
            }
            _ => {
                eprintln!("Unexpected event: {:?}", event);
            }
        };

        should_render
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        if self.view != PluginView::Sidebar
            || message.name != "toggle_sessions"
            || !self.sidebar_is_on_active_tab()
        {
            return false;
        }

        if self.sidebar_wide == Some(true) {
            if self.sidebar_is_focused() {
                host_move_focus_right();
            } else {
                if self.permissions_granted {
                    self.refresh_sessions();
                }
                host_show_self(false);
                self.sidebar_visible = true;
            }
        } else if self.sidebar_visible {
            host_hide_self();
            self.sidebar_visible = false;
        } else {
            if self.permissions_granted {
                self.refresh_sessions();
            }
            host_show_self(true);
            self.sidebar_visible = true;
        }
        true
    }

    fn render(&mut self, rows: usize, cols: usize) {
        if self.view == PluginView::Sidebar {
            if let Some(message) = &self.permission_error {
                self.sidebar_first_row = 0;
                self.sidebar_visible_rows = 0;
                self.sidebar_rendered_rows.clear();
                let width = unicode_width::UnicodeWidthStr::width(message.as_str());
                let line = format!("{}{}", message, " ".repeat(cols.saturating_sub(width)));
                let palette = color::palette_from_styling(self.mode_info.style.colors);
                print!("{}", style!(palette.red, palette.bg).bold().paint(line));
                return;
            }
            let dashboard_rows = dashboard_rows(&self.sessions, self.client_id);
            self.sidebar_selected = move_selection(self.sidebar_selected, 0, dashboard_rows.len());
            let palette = color::palette_from_styling(self.mode_info.style.colors);
            let (rendered, first_row) =
                sidebar::render(&dashboard_rows, self.sidebar_selected, rows, cols, palette);
            self.sidebar_first_row = first_row;
            self.sidebar_visible_rows = rows.saturating_sub(2).min(dashboard_rows.len());
            self.sidebar_rendered_rows = dashboard_rows;
            print!("{rendered}");
            return;
        }

        self.render_statusbar(cols);
    }
}

impl State {
    fn sidebar_rows(&self) -> Vec<SidebarRow> {
        dashboard_rows(&self.sessions, self.client_id)
    }

    fn clamp_sidebar_selection(&mut self) {
        self.sidebar_selected = move_selection(self.sidebar_selected, 0, self.sidebar_rows().len());
    }

    fn replace_sessions(&mut self, mut sessions: SessionListSnapshot) -> bool {
        let previous_rows = self.sidebar_rows();
        let previous_selected = self.sidebar_selected;
        let selected_session = previous_rows
            .get(self.sidebar_selected)
            .and_then(|row| match row {
                SidebarRow::Live(session) => Some(session.name.clone()),
                SidebarRow::NewWorkspace => None,
            });
        let new_workspace_selected = selected_session.is_none()
            && matches!(
                previous_rows.get(self.sidebar_selected),
                Some(SidebarRow::NewWorkspace)
            );
        let previous_current = self
            .sessions
            .live_sessions
            .iter()
            .find(|session| session.is_current_session)
            .map(|session| session.name.clone());
        if !self.tabs.is_empty() {
            if let Some(current_session) = sessions
                .live_sessions
                .iter_mut()
                .find(|session| session.is_current_session)
            {
                current_session.tabs = self.tabs.clone();
            }
        }
        let next_current = sessions
            .live_sessions
            .iter()
            .find(|session| session.is_current_session)
            .map(|session| session.name.clone());

        self.sessions = sessions;
        let rows = self.sidebar_rows();
        if previous_current != next_current {
            self.sidebar_selected = rows
                .iter()
                .position(|row| matches!(row, SidebarRow::Live(session) if session.current))
                .unwrap_or_else(|| move_selection(self.sidebar_selected, 0, rows.len()));
        } else if let Some(selected_session) = selected_session {
            self.sidebar_selected = rows
                .iter()
                .position(
                    |row| matches!(row, SidebarRow::Live(session) if session.name == selected_session),
                )
                .unwrap_or_else(|| {
                    move_selection(self.sidebar_selected, 0, rows.len())
                });
        } else if new_workspace_selected {
            self.sidebar_selected = rows.len().saturating_sub(1);
        } else {
            self.clamp_sidebar_selection();
        }
        !same_rendered_rows(&previous_rows, &rows) || previous_selected != self.sidebar_selected
    }

    fn refresh_sessions(&mut self) -> bool {
        let Ok(snapshot) = host_get_session_list() else {
            return false;
        };
        if self.sessions == snapshot {
            return false;
        }
        self.replace_sessions(snapshot)
    }

    fn update_current_session_tabs(&mut self, tabs: &[TabInfo]) -> bool {
        let previous_rows = self.sidebar_rows();
        let Some(current_session) = self
            .sessions
            .live_sessions
            .iter_mut()
            .find(|session| session.is_current_session)
        else {
            return false;
        };
        if current_session.tabs == tabs {
            return false;
        }
        current_session.tabs = tabs.to_vec();
        !same_rendered_rows(&previous_rows, &self.sidebar_rows())
    }

    fn sidebar_is_focused(&self) -> bool {
        self.pane_manifest
            .panes
            .values()
            .flatten()
            .any(|pane| pane.is_plugin && pane.id == self.plugin_id && pane.is_focused)
    }

    fn sidebar_is_on_active_tab(&self) -> bool {
        let Some(active_tab_position) = self
            .tabs
            .iter()
            .find(|tab| tab.active)
            .map(|tab| tab.position)
        else {
            return false;
        };
        self.pane_manifest
            .panes
            .get(&active_tab_position)
            .is_some_and(|panes| {
                panes
                    .iter()
                    .any(|pane| pane.is_plugin && pane.id == self.plugin_id)
            })
    }

    fn update_sidebar_width(&mut self, display_area_columns: usize) -> bool {
        let wide = is_wide(display_area_columns);
        if self.sidebar_wide == Some(wide) {
            return false;
        }
        self.sidebar_wide = Some(wide);

        if wide {
            if !self.sidebar_visible {
                host_show_self(false);
                self.sidebar_visible = true;
            }
        } else if self.sidebar_visible {
            host_hide_self();
            self.sidebar_visible = false;
        }
        true
    }

    fn handle_sidebar_key(&mut self, key: KeyWithModifier) -> bool {
        match self.sidebar_keymaps.action(&key) {
            Some(SidebarAction::Down) => {
                let selected = move_selection(self.sidebar_selected, 1, self.sidebar_rows().len());
                let changed = self.sidebar_selected != selected;
                self.sidebar_selected = selected;
                changed
            }
            Some(SidebarAction::Up) => {
                let selected = move_selection(self.sidebar_selected, -1, self.sidebar_rows().len());
                let changed = self.sidebar_selected != selected;
                self.sidebar_selected = selected;
                changed
            }
            Some(SidebarAction::Open) => {
                self.activate_sidebar_selection();
                false
            }
            Some(SidebarAction::NewWorkspace) => {
                self.open_workspace_picker();
                false
            }
            Some(SidebarAction::Cancel) => {
                self.leave_sidebar();
                false
            }
            None => false,
        }
    }

    fn handle_sidebar_mouse(&mut self, event: Mouse) -> bool {
        match event {
            Mouse::LeftClick(screen_line, _) => {
                let Some((selected, row)) = self.rendered_sidebar_row(screen_line) else {
                    return false;
                };
                let changed = self.sidebar_selected != selected;
                self.sidebar_selected = selected;
                self.activate_sidebar_row(row);
                changed
            }
            Mouse::ScrollUp(_) => {
                let selected = move_selection(self.sidebar_selected, -1, self.sidebar_rows().len());
                let changed = self.sidebar_selected != selected;
                self.sidebar_selected = selected;
                changed
            }
            Mouse::ScrollDown(_) => {
                let selected = move_selection(self.sidebar_selected, 1, self.sidebar_rows().len());
                let changed = self.sidebar_selected != selected;
                self.sidebar_selected = selected;
                changed
            }
            _ => false,
        }
    }

    fn activate_sidebar_selection(&mut self) {
        let Some(row) = self.sidebar_rows().get(self.sidebar_selected).cloned() else {
            return;
        };

        self.activate_sidebar_row(row);
    }

    fn rendered_sidebar_row(&self, screen_line: isize) -> Option<(usize, SidebarRow)> {
        let visible_index = row_for_screen_line(screen_line, self.sidebar_visible_rows)?;
        let selected = self.sidebar_first_row + visible_index;
        self.sidebar_rendered_rows
            .get(selected)
            .cloned()
            .map(|row| (selected, row))
    }

    fn activate_sidebar_row(&self, row: SidebarRow) {
        match row {
            SidebarRow::Live(session) => {
                host_switch_session(
                    &session.name,
                    session.focus.tab_position,
                    session.focus.pane_id,
                );
            }
            SidebarRow::NewWorkspace => self.open_workspace_picker(),
        }
    }

    fn open_workspace_picker(&self) {
        host_open_workspace_picker(&self.initial_cwd);
    }

    fn leave_sidebar(&mut self) {
        if self.sidebar_wide == Some(true) {
            host_move_focus_right();
        } else {
            host_hide_self();
            self.sidebar_visible = false;
        }
    }

    fn render_statusbar(&mut self, cols: usize) {
        if self.tabs.is_empty() || cols == 0 {
            return;
        }

        let session_name = &self.mode_info.session_name;
        let mode = self.mode_info.mode;
        let palette = color::palette_from_styling(self.mode_info.style.colors);
        let unread = unread_count(&self.tabs);
        let session_unread = if is_wide(cols) { 0 } else { unread };

        let mut session = Session::render(session_name.as_deref(), mode, palette, session_unread);
        let mut datetime = self.now.render(mode, palette);
        let pad = Bg::render(2, palette);
        let fixed_width = session.len + datetime.len + (pad.len * 2);

        if fixed_width >= cols {
            let unread_label = if unread == 0 {
                String::new()
            } else {
                format!(" ●{unread}")
            };
            let label = format!(
                " {}{} ",
                session_name.as_deref().unwrap_or("Zellij"),
                unread_label
            );
            let label = truncate_to_width(&label, cols);
            let label_width = unicode_width::UnicodeWidthStr::width(label.as_str());
            let text = format!("{}{}", label, " ".repeat(cols.saturating_sub(label_width)));
            print!("{}", style!(palette.fg, palette.bg).bold().paint(text));
            return;
        }

        let tab_widths = Tabs::widths(&self.tabs, mode);
        let active_index = self.tabs.iter().position(|tab| tab.active).unwrap_or(0);
        let visible = select_visible_tabs(&tab_widths, active_index, cols - fixed_width);
        let mut tabs = Tabs::render_indices(&self.tabs, &visible.indices, mode, palette);
        let omitted = visible.omitted.then(|| Tabs::omission(palette));
        let omitted_width = usize::from(omitted.is_some());
        let occupied = fixed_width + tabs.len + omitted_width;

        let mut blocks = Vec::with_capacity(cols);
        blocks.append(&mut session.blocks);
        blocks.push(pad.clone());
        blocks.append(&mut tabs.blocks);
        if let Some(omitted) = omitted {
            blocks.push(omitted);
        }
        if occupied < cols {
            blocks.push(Bg::render(cols - occupied, palette));
        }
        blocks.push(pad);
        blocks.append(&mut datetime.blocks);

        let mut bar = String::new();
        let mut cursor = 0;

        for block in blocks {
            bar = format!("{}{}", bar, block.body);

            if let Some(idx) = block.tab_index {
                if self.should_change_tab
                    && self.mouse_click_pos >= cursor
                    && self.mouse_click_pos < cursor + block.len
                {
                    // Tabs are indexed starting from 1, therefore we need add 1 to idx
                    let tab_index = idx + 1;
                    switch_tab_to(tab_index as u32);
                }
            }

            cursor += block.len;
        }

        let bg = match palette.theme_hue {
            ThemeHue::Dark => palette.black,
            ThemeHue::Light => palette.white,
        };

        match bg {
            PaletteColor::Rgb((r, g, b)) => {
                print!("{}\u{1b}[48;2;{};{};{}m\u{1b}[0K", bar, r, g, b);
            }
            PaletteColor::EightBit(color) => {
                print!("{}\u{1b}[48;5;{}m\u{1b}[0K", bar, color);
            }
        }

        self.should_change_tab = false;
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::Path, str::FromStr, time::Duration};

    use zellij_tile::prelude::{
        ClientId, EventType, KeyWithModifier, PaneId, PaneInfo, PaneManifest, PermissionType,
        SessionInfo, SessionListSnapshot, TabInfo,
    };

    use super::{
        PluginView, SidebarRow, State, dashboard_rows, open_workspace_picker_with,
        plugin_permissions, sidebar_events, statusbar_events,
    };

    fn plugin(id: u32) -> PaneInfo {
        PaneInfo {
            id,
            is_plugin: true,
            ..PaneInfo::default()
        }
    }

    fn session(name: &str, created_at: u64, current: bool) -> SessionInfo {
        SessionInfo {
            name: name.into(),
            creation_time: Duration::from_secs(created_at),
            is_current_session: current,
            ..SessionInfo::default()
        }
    }

    #[test]
    fn only_the_sidebar_in_the_active_tab_handles_global_messages() {
        let mut state = State {
            plugin_id: 9,
            tabs: vec![
                TabInfo {
                    position: 0,
                    active: false,
                    ..TabInfo::default()
                },
                TabInfo {
                    position: 1,
                    active: true,
                    ..TabInfo::default()
                },
            ],
            pane_manifest: PaneManifest {
                panes: HashMap::from([(0, vec![plugin(9)]), (1, vec![plugin(12)])]),
            },
            ..State::default()
        };

        assert!(!state.sidebar_is_on_active_tab());
        state.plugin_id = 12;
        assert!(state.sidebar_is_on_active_tab());
    }

    #[test]
    fn both_views_receive_permission_results() {
        assert!(statusbar_events().contains(&EventType::PermissionRequestResult));
        assert!(sidebar_events().contains(&EventType::PermissionRequestResult));
        assert!(sidebar_events().contains(&EventType::RunCommandResult));
        assert!(!statusbar_events().contains(&EventType::RunCommandResult));
    }

    #[test]
    fn every_view_requests_the_complete_sidebar_permission_set() {
        assert_eq!(
            plugin_permissions(),
            vec![
                PermissionType::ReadApplicationState,
                PermissionType::ChangeApplicationState,
                PermissionType::RunCommands,
            ]
        );
    }

    #[test]
    fn workspace_picker_reveals_and_focuses_the_opened_floating_pane() {
        let mut opened_command = None;
        let mut shown_pane = None;

        open_workspace_picker_with(
            Path::new("/work/project"),
            |command, _coordinates, _context| {
                opened_command = Some(command);
                Some(PaneId::Terminal(42))
            },
            |pane_id, should_float_if_hidden, should_focus_pane| {
                shown_pane = Some((pane_id, should_float_if_hidden, should_focus_pane));
            },
        );

        let command = opened_command.expect("workspace picker command was not opened");
        assert_eq!(command.path, Path::new("zellij-workspaces"));
        assert_eq!(command.args, ["--new"]);
        assert_eq!(command.cwd.as_deref(), Some(Path::new("/work/project")));
        assert_eq!(shown_pane, Some((PaneId::Terminal(42), true, true)));
    }

    #[test]
    fn session_switch_selects_the_new_current_workspace() {
        let mut state = State::default();
        let _ = state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![session("first", 1, true), session("second", 2, false)],
            ..SessionListSnapshot::default()
        });
        assert_eq!(state.sidebar_selected, 0);

        let _ = state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![session("first", 1, false), session("second", 2, true)],
            ..SessionListSnapshot::default()
        });
        assert_eq!(state.sidebar_selected, 1);
    }

    #[test]
    fn ordinary_refresh_preserves_manual_sidebar_selection() {
        let snapshot = SessionListSnapshot {
            live_sessions: vec![session("first", 1, true), session("second", 2, false)],
            ..SessionListSnapshot::default()
        };
        let mut state = State::default();
        let _ = state.replace_sessions(snapshot.clone());
        state.sidebar_selected = 1;

        let _ = state.replace_sessions(snapshot);

        assert_eq!(state.sidebar_selected, 1);
    }

    #[test]
    fn refresh_preserves_the_selected_workspace_when_rows_shift() {
        let mut state = State::default();
        let _ = state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![
                session("first", 1, true),
                session("second", 2, false),
                session("third", 3, false),
            ],
            ..SessionListSnapshot::default()
        });
        state.sidebar_selected = 2;

        let _ = state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![session("first", 1, true), session("third", 3, false)],
            ..SessionListSnapshot::default()
        });

        assert_eq!(state.sidebar_selected, 1);
    }

    #[test]
    fn refresh_keeps_new_workspace_selected_at_the_end() {
        let mut state = State::default();
        let _ = state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![session("first", 1, true)],
            ..SessionListSnapshot::default()
        });
        state.sidebar_selected = 1;

        let _ = state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![session("first", 1, true), session("second", 2, false)],
            ..SessionListSnapshot::default()
        });

        assert_eq!(state.sidebar_selected, 2);
    }

    #[test]
    fn unchanged_and_nonvisual_session_updates_do_not_redraw_the_sidebar() {
        let snapshot = SessionListSnapshot {
            live_sessions: vec![session("first", 1, true)],
            ..SessionListSnapshot::default()
        };
        let mut state = State {
            view: PluginView::Sidebar,
            sessions: snapshot.clone(),
            ..State::default()
        };

        assert!(!state.replace_sessions(snapshot));

        let mut nonvisual_update = session("first", 1, true);
        nonvisual_update.connected_clients = 2;
        assert!(!state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![nonvisual_update],
            ..SessionListSnapshot::default()
        }));
        assert_eq!(state.sessions.live_sessions[0].connected_clients, 2);
    }

    #[test]
    fn navigation_at_a_boundary_does_not_redraw_the_sidebar() {
        let mut state = State::default();

        assert!(!state.handle_sidebar_key(KeyWithModifier::from_str("k").unwrap()));
    }

    #[test]
    fn sidebar_redraws_only_when_responsive_width_changes() {
        let tabs = [TabInfo {
            name: "editor".into(),
            active: true,
            display_area_columns: 120,
            ..TabInfo::default()
        }];
        let mut state = State {
            sidebar_wide: Some(true),
            sidebar_visible: true,
            ..State::default()
        };

        assert!(!state.update_sidebar_width(tabs[0].display_area_columns));
        assert!(state.update_sidebar_width(80));
    }

    #[test]
    fn current_session_tab_events_update_sidebar_attention_immediately() {
        let tabs = vec![TabInfo {
            active: true,
            display_area_columns: 120,
            ..TabInfo::default()
        }];
        let mut current = session("first", 1, true);
        current.tabs = tabs.clone();
        let mut state = State {
            view: PluginView::Sidebar,
            sessions: SessionListSnapshot {
                live_sessions: vec![current],
                ..SessionListSnapshot::default()
            },
            tabs: tabs.clone(),
            active_tab_idx: 1,
            sidebar_wide: Some(true),
            sidebar_visible: true,
            permissions_granted: true,
            ..State::default()
        };
        let attention_tabs = vec![TabInfo {
            has_bell_notification: true,
            ..tabs[0].clone()
        }];

        assert!(state.update_current_session_tabs(&attention_tabs));
        assert!(state.sessions.live_sessions[0].tabs[0].has_bell_notification);
    }

    #[test]
    fn stale_snapshot_does_not_erase_current_session_attention() {
        let current_tabs = vec![TabInfo {
            active: true,
            has_bell_notification: true,
            ..TabInfo::default()
        }];
        let mut current = session("first", 1, true);
        current.tabs = current_tabs.clone();
        let mut state = State {
            sessions: SessionListSnapshot {
                live_sessions: vec![current],
                ..SessionListSnapshot::default()
            },
            tabs: current_tabs,
            ..State::default()
        };
        let mut stale_current = session("first", 1, true);
        stale_current.tabs = vec![TabInfo {
            active: true,
            has_bell_notification: false,
            ..TabInfo::default()
        }];

        assert!(!state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![stale_current],
            ..SessionListSnapshot::default()
        }));
        assert!(state.sessions.live_sessions[0].tabs[0].has_bell_notification);
    }

    #[test]
    fn peer_session_attention_redraws_the_sidebar_notification() {
        let mut peer = session("second", 2, false);
        peer.tabs = vec![TabInfo {
            active: true,
            ..TabInfo::default()
        }];
        let mut state = State {
            sessions: SessionListSnapshot {
                live_sessions: vec![session("first", 1, true), peer.clone()],
                ..SessionListSnapshot::default()
            },
            ..State::default()
        };
        peer.tabs[0].has_bell_notification = true;

        assert!(state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![session("first", 1, true), peer],
            ..SessionListSnapshot::default()
        }));
        let SidebarRow::Live(peer_row) = &state.sidebar_rows()[1] else {
            panic!("expected peer session row");
        };
        assert_eq!(peer_row.unread_tabs, 1);
    }

    #[test]
    fn mouse_click_uses_the_row_identity_from_the_last_render() {
        let rendered_sessions = SessionListSnapshot {
            live_sessions: vec![session("first", 1, true), session("second", 2, false)],
            ..SessionListSnapshot::default()
        };
        let mut state = State {
            sessions: rendered_sessions.clone(),
            sidebar_rendered_rows: dashboard_rows(&rendered_sessions, ClientId::default()),
            sidebar_visible_rows: 3,
            ..State::default()
        };

        let _ = state.replace_sessions(SessionListSnapshot {
            live_sessions: vec![session("first", 1, true)],
            ..SessionListSnapshot::default()
        });

        assert!(matches!(state.sidebar_rows()[1], SidebarRow::NewWorkspace));
        assert!(matches!(
            state.rendered_sidebar_row(2),
            Some((1, SidebarRow::Live(ref session))) if session.name == "second"
        ));
    }
}
