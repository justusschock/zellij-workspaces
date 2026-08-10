use std::{env, io, process};

use crate::{
    action::Action, dir::Dir, keymap::Keymaps, log, options::OPTIONS, template::TemplateEngine, ui,
    zellij,
};

#[derive(Debug, PartialEq)]
enum Input {
    Interactive,
    NewSession,
    PrintSidebarKeymaps,
    Render {
        template: String,
        session: String,
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
        Input::PrintSidebarKeymaps => print_sidebar_keymaps_and_exit(),
        Input::Invalid(error) => {
            return Action::Exit(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                error.clone(),
            )))
            .exec();
        }
        _ => {}
    }

    let action = match zellij::list_sessions() {
        Err(error) => Action::Exit(Err(error)),
        Ok(sessions) => match input {
            Input::NewSession => ui::new_session_prompt(sessions),
            Input::Render { .. } | Input::PrintSidebarKeymaps | Input::Invalid(_) => unreachable!(),
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
}
