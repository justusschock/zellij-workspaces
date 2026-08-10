use zellij_tile::prelude::{ClientId, PaneId, SessionListSnapshot, TabInfo};

pub(crate) const SIDEBAR_BREAKPOINT: usize = 100;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FocusTarget {
    pub(crate) tab_position: Option<usize>,
    pub(crate) pane_id: Option<(u32, bool)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionRow {
    pub(crate) name: String,
    pub(crate) current: bool,
    pub(crate) unread_tabs: usize,
    pub(crate) focus: FocusTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SidebarRow {
    Live(SessionRow),
    NewWorkspace,
}

pub(crate) fn dashboard_rows(
    snapshot: &SessionListSnapshot,
    client_id: ClientId,
) -> Vec<SidebarRow> {
    let mut sessions = snapshot.live_sessions.iter().collect::<Vec<_>>();
    sessions.sort_by(|left, right| {
        left.creation_time
            .cmp(&right.creation_time)
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut rows = sessions
        .into_iter()
        .map(|session| {
            let tab_position = session
                .tab_history
                .get(&client_id)
                .and_then(|history| history.last())
                .copied();
            let pane_id = session
                .pane_history
                .get(&client_id)
                .and_then(|history| history.last())
                .copied()
                .map(pane_focus);

            SidebarRow::Live(SessionRow {
                name: session.name.clone(),
                current: session.is_current_session,
                unread_tabs: unread_count(&session.tabs),
                focus: FocusTarget {
                    tab_position,
                    pane_id,
                },
            })
        })
        .collect::<Vec<_>>();

    rows.push(SidebarRow::NewWorkspace);
    rows
}

pub(crate) fn unread_count(tabs: &[TabInfo]) -> usize {
    tabs.iter().filter(|tab| tab.has_bell_notification).count()
}

pub(crate) fn is_wide(display_area_columns: usize) -> bool {
    display_area_columns >= SIDEBAR_BREAKPOINT
}

fn pane_focus(pane_id: PaneId) -> (u32, bool) {
    match pane_id {
        PaneId::Terminal(id) => (id, false),
        PaneId::Plugin(id) => (id, true),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};

    use zellij_tile::prelude::{PaneId, SessionInfo, SessionListSnapshot, TabInfo};

    use super::{
        FocusTarget, SIDEBAR_BREAKPOINT, SessionRow, SidebarRow, dashboard_rows, is_wide,
        unread_count,
    };

    const CLIENT_ID: u16 = 7;

    fn tab(position: usize, active: bool, unread: bool) -> TabInfo {
        TabInfo {
            position,
            name: format!("tab-{position}"),
            active,
            has_bell_notification: unread,
            ..TabInfo::default()
        }
    }

    fn session(name: &str, created_at: u64, current: bool, tabs: Vec<TabInfo>) -> SessionInfo {
        SessionInfo {
            name: name.into(),
            tabs,
            is_current_session: current,
            creation_time: Duration::from_secs(created_at),
            ..SessionInfo::default()
        }
    }

    #[test]
    fn breakpoint_matches_the_approved_geometry() {
        assert_eq!(SIDEBAR_BREAKPOINT, 100);
    }

    #[test]
    fn orders_live_sessions_by_creation_time_then_name() {
        let snapshot = SessionListSnapshot {
            live_sessions: vec![
                session("charlie", 20, false, vec![]),
                session("bravo", 10, false, vec![]),
                session("alpha", 10, true, vec![]),
            ],
            resurrectable_sessions: vec![],
        };

        let names = dashboard_rows(&snapshot, CLIENT_ID)
            .into_iter()
            .filter_map(|row| match row {
                SidebarRow::Live(row) => Some(row.name),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(names, ["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn derives_current_unread_and_last_focus_for_the_client() {
        let mut target = session(
            "project",
            10,
            true,
            vec![tab(0, false, true), tab(1, true, false)],
        );
        target.tab_history = BTreeMap::from([(CLIENT_ID, vec![0, 1])]);
        target.pane_history =
            BTreeMap::from([(CLIENT_ID, vec![PaneId::Terminal(4), PaneId::Plugin(9)])]);
        let snapshot = SessionListSnapshot {
            live_sessions: vec![target],
            resurrectable_sessions: vec![],
        };

        assert_eq!(
            dashboard_rows(&snapshot, CLIENT_ID)[0],
            SidebarRow::Live(SessionRow {
                name: "project".into(),
                current: true,
                unread_tabs: 1,
                focus: FocusTarget {
                    tab_position: Some(1),
                    pane_id: Some((9, true)),
                },
            })
        );
    }

    #[test]
    fn focus_falls_back_when_the_client_has_no_history() {
        let snapshot = SessionListSnapshot {
            live_sessions: vec![session("project", 10, false, vec![])],
            resurrectable_sessions: vec![],
        };

        let SidebarRow::Live(row) = &dashboard_rows(&snapshot, CLIENT_ID)[0] else {
            panic!("expected a live row");
        };
        assert_eq!(row.focus, FocusTarget::default());
    }

    #[test]
    fn resurrectable_sessions_do_not_create_dashboard_rows() {
        let snapshot = SessionListSnapshot {
            live_sessions: vec![],
            resurrectable_sessions: vec![
                ("old-a".into(), Duration::from_secs(1)),
                ("old-b".into(), Duration::from_secs(2)),
            ],
        };

        assert_eq!(
            dashboard_rows(&snapshot, CLIENT_ID),
            vec![SidebarRow::NewWorkspace]
        );
    }

    #[test]
    fn empty_session_list_keeps_creation() {
        let snapshot = SessionListSnapshot::default();
        assert_eq!(
            dashboard_rows(&snapshot, CLIENT_ID),
            vec![SidebarRow::NewWorkspace]
        );
    }

    #[test]
    fn counts_only_persistent_native_bells() {
        let tabs = vec![
            tab(0, true, false),
            tab(1, false, true),
            tab(2, false, true),
        ];
        assert_eq!(unread_count(&tabs), 2);
    }

    #[test]
    fn switches_to_wide_mode_at_one_hundred_columns() {
        assert!(!is_wide(99));
        assert!(is_wide(100));
    }
}
