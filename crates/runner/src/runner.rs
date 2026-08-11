use std::{env, io, process};

use crate::{
    action::Action, config::Config, dir::Dir, keymap::Keymaps, log, options::OPTIONS,
    template::TemplateEngine, ui, zellij,
};

#[derive(Debug, PartialEq)]
enum Input {
    Interactive,
    NewSession,
    PrintSidebarKeymaps,
    NewTab {
        template: String,
    },
    Render {
        template: String,
        session: String,
        dir: Dir,
    },
    RenderTab {
        template: String,
        tab: String,
        dir: Dir,
    },
    Invalid(String),
    Session {
        session: String,
        template: Option<String>,
    },
}

impl Input {
    fn from_args() -> Self {
        Self::from_iter(env::args())
    }

    fn from_iter<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();

        match args.next() {
            None => Self::Interactive,
            Some(flag) if flag == "--new" => Self::NewSession,
            Some(flag) if flag == "--print-sidebar-keymaps" => Self::PrintSidebarKeymaps,
            Some(flag) if flag == "--new-tab" => {
                let Some(template) = args.next() else {
                    return Self::Invalid("--new-tab requires TEMPLATE".into());
                };
                if args.next().is_some() {
                    return Self::Invalid("--new-tab accepts exactly one template".into());
                }
                Self::NewTab { template }
            }
            Some(flag) if flag == "--render" => {
                let Some(template) = args.next() else {
                    return Self::Invalid("--render requires TEMPLATE SESSION DIRECTORY".into());
                };
                let Some(session) = args.next() else {
                    return Self::Invalid("--render requires TEMPLATE SESSION DIRECTORY".into());
                };
                let Some(dir) = args.next() else {
                    return Self::Invalid("--render requires TEMPLATE SESSION DIRECTORY".into());
                };
                if args.next().is_some() {
                    return Self::Invalid("--render accepts exactly three arguments".into());
                }
                Self::Render {
                    template,
                    session,
                    dir: Dir::from(dir),
                }
            }
            Some(flag) if flag == "--render-tab" => {
                let Some(template) = args.next() else {
                    return Self::Invalid("--render-tab requires TEMPLATE TAB DIRECTORY".into());
                };
                let Some(tab) = args.next() else {
                    return Self::Invalid("--render-tab requires TEMPLATE TAB DIRECTORY".into());
                };
                let Some(dir) = args.next() else {
                    return Self::Invalid("--render-tab requires TEMPLATE TAB DIRECTORY".into());
                };
                if args.next().is_some() {
                    return Self::Invalid("--render-tab accepts exactly three arguments".into());
                }
                Self::RenderTab {
                    template,
                    tab,
                    dir: Dir::from(dir),
                }
            }
            Some(session) => Self::Session {
                session,
                template: args.next(),
            },
        }
    }
}

pub(crate) fn init() {
    let input = Input::from_args();

    match &input {
        Input::Render {
            template,
            session,
            dir,
        } => render_and_exit(template, session, dir),
        Input::RenderTab { template, tab, dir } => render_tab_and_exit(template, tab, dir),
        Input::PrintSidebarKeymaps => print_sidebar_keymaps_and_exit(),
        Input::NewTab { template } => {
            if !zellij::inside_session() {
                return Action::Exit(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--new-tab must be run inside a Zellij session",
                )))
                .exec();
            }
            return ui::new_worktree_tab_prompt(template.clone()).exec();
        }
        Input::Invalid(error) => {
            return Action::Exit(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                error.clone(),
            )))
            .exec();
        }
        _ => {}
    }

    if let Err(error) = ensure_default_workspaces() {
        return Action::Exit(Err(error)).exec();
    }

    let action = match zellij::list_sessions() {
        Err(error) => Action::Exit(Err(error)),
        Ok(sessions) => match input {
            Input::NewSession => ui::new_session_prompt(sessions),
            Input::Render { .. }
            | Input::RenderTab { .. }
            | Input::PrintSidebarKeymaps
            | Input::NewTab { .. }
            | Input::Invalid(_) => unreachable!(),
            Input::Interactive if sessions.is_empty() => ui::new_session_prompt(sessions),
            Input::Interactive => ui::action_selector(sessions),
            Input::Session {
                session,
                template: _,
            } if sessions.contains(&session) => Action::AttachToSession(session),
            Input::Session { session, template } => Action::CreateNewSession {
                session,
                template,
                dir: None,
            },
        },
    };

    action.exec()
}

fn render_tab_and_exit(template: &str, tab: &str, dir: &Dir) -> ! {
    let engine = TemplateEngine::new(&OPTIONS.tab_templates, &OPTIONS.cache);
    match engine.render_tab(template, tab, dir) {
        Ok(path) => {
            println!("{}", path.display());
            process::exit(0);
        }
        Err(error) => {
            log::error(error);
            process::exit(1);
        }
    }
}

fn ensure_default_workspaces() -> io::Result<()> {
    let config = Config::load(&OPTIONS.config).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Failed to load {}: {error}", OPTIONS.config.display()),
        )
    })?;
    if config.workspaces.is_empty() {
        return Ok(());
    }

    let sessions = zellij::list_sessions()?;
    let engine = TemplateEngine::new(&OPTIONS.templates, &OPTIONS.cache);
    for workspace in config.workspaces {
        if sessions.contains(&workspace.name) {
            continue;
        }
        if !workspace.cwd.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "default workspace `{}` directory does not exist: {}",
                    workspace.name,
                    workspace.cwd.display()
                ),
            ));
        }
        let layout = engine.render(&workspace.template, &workspace.name, &workspace.cwd)?;
        let status =
            zellij::ensure_background(&workspace.name, &layout.to_string_lossy(), &workspace.cwd)?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "Failed to create or restore default workspace `{}`",
                workspace.name
            )));
        }
    }
    Ok(())
}

fn print_sidebar_keymaps_and_exit() -> ! {
    match Keymaps::load(&OPTIONS.keymaps) {
        Ok(keymaps) => {
            print!("{}", keymaps.sidebar_protocol());
            process::exit(0);
        }
        Err(error) => {
            log::error(format!(
                "Failed to load {}: {error}",
                OPTIONS.keymaps.display()
            ));
            process::exit(1);
        }
    }
}

fn render_and_exit(template: &str, session: &str, dir: &Dir) -> ! {
    let engine = TemplateEngine::new(&OPTIONS.templates, &OPTIONS.cache);
    match engine.render(template, session, dir) {
        Ok(path) => {
            println!("{}", path.display());
            process::exit(0);
        }
        Err(error) => {
            log::error(error);
            process::exit(1);
        }
    }
}

pub(crate) fn switch() {
    let action = match zellij::list_sessions() {
        Err(error) => Action::Exit(Err(error)),
        Ok(sessions) => {
            if sessions.is_empty() {
                ui::new_session_prompt(sessions)
            } else {
                ui::action_selector(sessions)
            }
        }
    };

    action.exec()
}

#[cfg(test)]
mod tests {
    use super::{Dir, Input};

    #[test]
    fn new_flag_selects_new_session_flow() {
        assert_eq!(
            Input::from_iter(["zellij-workspaces", "--new"]),
            Input::NewSession
        );
    }

    #[test]
    fn sidebar_keymap_flag_selects_noninteractive_output() {
        assert_eq!(
            Input::from_iter(["zellij-workspaces", "--print-sidebar-keymaps"]),
            Input::PrintSidebarKeymaps
        );
    }

    #[test]
    fn new_tab_flag_requires_exactly_one_template() {
        assert_eq!(
            Input::from_iter(["zellij-workspaces", "--new-tab", "codex"]),
            Input::NewTab {
                template: "codex".into()
            }
        );
        assert!(matches!(
            Input::from_iter(["zellij-workspaces", "--new-tab"]),
            Input::Invalid(_)
        ));
        assert!(matches!(
            Input::from_iter(["zellij-workspaces", "--new-tab", "codex", "extra"]),
            Input::Invalid(_)
        ));
    }

    #[test]
    fn positional_session_and_template_stay_supported() {
        assert_eq!(
            Input::from_iter(["zellij-workspaces", "work", "development"]),
            Input::Session {
                session: "work".into(),
                template: Some("development".into()),
            }
        );
    }

    #[test]
    fn render_mode_requires_template_session_and_directory() {
        assert!(matches!(
            Input::from_iter(["zellij-workspaces", "--render", "development"]),
            Input::Invalid(_)
        ));
        assert_eq!(
            Input::from_iter([
                "zellij-workspaces",
                "--render",
                "development",
                "work",
                "/tmp/work",
            ]),
            Input::Render {
                template: "development".into(),
                session: "work".into(),
                dir: Dir::from("/tmp/work".to_owned()),
            }
        );
    }

    #[test]
    fn render_tab_mode_requires_template_tab_and_directory() {
        assert!(matches!(
            Input::from_iter(["zellij-workspaces", "--render-tab", "agent"]),
            Input::Invalid(_)
        ));
        assert_eq!(
            Input::from_iter([
                "zellij-workspaces",
                "--render-tab",
                "agent",
                "fix-login",
                "/tmp/worktree",
            ]),
            Input::RenderTab {
                template: "agent".into(),
                tab: "fix-login".into(),
                dir: Dir::from("/tmp/worktree".to_owned()),
            }
        );
    }
}
