use std::{
    env,
    ffi::OsString,
    io,
    process::{Command, ExitStatus},
    slice, vec,
};

use crate::dir::Dir;

const BIN: &str = "zellij";

pub(crate) fn inside_session() -> bool {
    env::var_os("ZELLIJ").is_some()
}

#[derive(Eq, PartialEq, Clone, Ord, PartialOrd)]
pub struct Session {
    pub name: String,
}

#[derive(Clone)]
pub struct Sessions(Vec<Session>);

impl IntoIterator for Sessions {
    type Item = Session;
    type IntoIter = vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<Session> for Sessions {
    fn from_iter<I: IntoIterator<Item = Session>>(iter: I) -> Self {
        Sessions(iter.into_iter().collect())
    }
}

impl Sessions {
    pub fn empty() -> Self {
        Self(vec![])
    }

    pub fn from_output(output: Vec<&str>) -> Self {
        let mut sessions = Vec::with_capacity(output.len());

        for line in &output {
            if line.contains("EXITED") {
                continue;
            }
            let Some((name, _metadata)) = line.split_once(" [") else {
                continue;
            };
            if !name.is_empty() {
                sessions.push(Session {
                    name: name.to_owned(),
                });
            }
        }

        sessions.sort();

        Self(sessions)
    }

    pub fn iter(&self) -> slice::Iter<'_, Session> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, session: &str) -> bool {
        self.0.iter().any(|s| s.name == session)
    }
}

pub(crate) fn list_sessions() -> io::Result<Sessions> {
    let output = Command::new(BIN)
        .args(["list-sessions", "--no-formatting"])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8(output.stdout).unwrap();

        let lines: Vec<&str> = stdout.lines().collect();

        Ok(Sessions::from_output(lines))
    } else {
        let exit_code = match output.status.code() {
            Some(code) => code.to_string(),
            None => "-".to_string(),
        };

        let stderr = String::from_utf8(output.stderr);

        match stderr {
            Ok(err) => {
                if err.contains("No") && err.contains("sessions found") {
                    Ok(Sessions::empty())
                } else {
                    Err(io::Error::other(format!(
                        "Failed to get Zellij sessions. Exit code: {}. {}",
                        exit_code, err
                    )))
                }
            }
            Err(_) => Err(io::Error::other(format!(
                "Failed to get Zellij sessions. Exit code: {}",
                exit_code
            ))),
        }
    }
}

pub(crate) fn create(
    session: &str,
    layout: &Option<String>,
    dir: &Option<Dir>,
) -> Result<ExitStatus, io::Error> {
    let inside = inside_session();
    let args = entry_args(inside, session, layout, dir);

    let mut cmd = Command::new(BIN);

    if !inside {
        if let Some(dir) = dir {
            cmd.current_dir(dir);
        }
    }

    cmd.args(args).status()
}

pub(crate) fn ensure_background(
    session: &str,
    layout: &str,
    dir: &Dir,
) -> Result<ExitStatus, io::Error> {
    Command::new(BIN)
        .args(background_args(session, layout, dir))
        .status()
}

fn background_args(session: &str, layout: &str, dir: &Dir) -> Vec<OsString> {
    vec![
        "attach".into(),
        "--create-background".into(),
        session.into(),
        "options".into(),
        "--default-cwd".into(),
        dir.as_os_str().to_owned(),
        "--default-layout".into(),
        layout.into(),
    ]
}

pub(crate) fn new_tab(name: &str, layout: &str, dir: &Dir) -> Result<ExitStatus, io::Error> {
    Command::new(BIN)
        .args(new_tab_args(name, layout, dir))
        .status()
}

fn new_tab_args(name: &str, layout: &str, dir: &Dir) -> Vec<OsString> {
    vec![
        "action".into(),
        "new-tab".into(),
        "--name".into(),
        name.into(),
        "--cwd".into(),
        dir.as_os_str().to_owned(),
        "--layout".into(),
        layout.into(),
    ]
}

fn entry_args(
    inside: bool,
    session: &str,
    layout: &Option<String>,
    dir: &Option<Dir>,
) -> Vec<OsString> {
    let mut args = if inside {
        vec!["action".into(), "switch-session".into(), session.into()]
    } else {
        vec!["--session".into(), session.into()]
    };

    if inside {
        if let Some(dir) = dir {
            args.push("--cwd".into());
            args.push(dir.as_os_str().to_owned());
        }
        if let Some(layout) = layout {
            args.push("--layout".into());
            args.push(layout.into());
        }
    } else if let Some(layout) = layout {
        args.push("--new-session-with-layout".into());
        args.push(layout.into());
    }

    args
}

fn attach_args(inside: bool, session: &str) -> Vec<OsString> {
    if inside {
        vec!["action".into(), "switch-session".into(), session.into()]
    } else {
        vec!["attach".into(), session.into()]
    }
}

pub(crate) fn attach(session: &str) -> Result<ExitStatus, io::Error> {
    Command::new(BIN)
        .args(attach_args(inside_session(), session))
        .status()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{Sessions, attach_args, background_args, entry_args, new_tab_args};
    use crate::dir::Dir;

    fn strings(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn inside_session_uses_switch_session_with_layout_and_cwd() {
        let args = entry_args(
            true,
            "work",
            &Some("/tmp/services.kdl".into()),
            &Some(Dir::from("/tmp/work".to_owned())),
        );

        assert_eq!(
            args,
            strings(&[
                "action",
                "switch-session",
                "work",
                "--cwd",
                "/tmp/work",
                "--layout",
                "/tmp/services.kdl",
            ])
        );
    }

    #[test]
    fn outside_session_keeps_new_session_with_layout() {
        let args = entry_args(false, "work", &Some("/tmp/development.kdl".into()), &None);

        assert_eq!(
            args,
            strings(&[
                "--session",
                "work",
                "--new-session-with-layout",
                "/tmp/development.kdl",
            ])
        );
    }

    #[test]
    fn default_workspaces_are_created_or_restored_in_the_background() {
        assert_eq!(
            background_args("work", "/tmp/codex.kdl", &Dir::from("/tmp/work".to_owned())),
            strings(&[
                "attach",
                "--create-background",
                "work",
                "options",
                "--default-cwd",
                "/tmp/work",
                "--default-layout",
                "/tmp/codex.kdl",
            ])
        );
    }

    #[test]
    fn worktree_tabs_use_the_rendered_layout_name_and_directory() {
        assert_eq!(
            new_tab_args(
                "fix-login",
                "/tmp/tab.kdl",
                &Dir::from("/tmp/tree".to_owned())
            ),
            strings(&[
                "action",
                "new-tab",
                "--name",
                "fix-login",
                "--cwd",
                "/tmp/tree",
                "--layout",
                "/tmp/tab.kdl",
            ])
        );
    }

    #[test]
    fn inside_session_switches_to_existing_session() {
        assert_eq!(
            attach_args(true, "work"),
            strings(&["action", "switch-session", "work"])
        );
    }

    #[test]
    fn outside_session_attaches_to_existing_session() {
        assert_eq!(attach_args(false, "work"), strings(&["attach", "work"]));
    }

    #[test]
    fn exited_sessions_are_discarded_at_the_parse_boundary() {
        let sessions = Sessions::from_output(vec![
            "live [Created 1m ago]",
            "old [Created 2m ago] (EXITED - attach to resurrect)",
        ]);

        let names = sessions
            .iter()
            .map(|session| session.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, ["live"]);
        assert!(sessions.contains("live"));
        assert!(!sessions.contains("old"));
    }

    #[test]
    fn malformed_session_lines_are_ignored() {
        let sessions = Sessions::from_output(vec!["", "missing metadata", "live [Created now]"]);

        let names = sessions
            .iter()
            .map(|session| session.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, ["live"]);
    }
}
