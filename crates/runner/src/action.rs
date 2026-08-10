use std::{io, process};

use crate::{dir::Dir, log, options::OPTIONS, template::TemplateEngine, zellij};

pub(crate) enum Action {
    AttachToSession(String),
    CreateNewSession {
        session: String,
        template: Option<String>,
        dir: Option<Dir>,
    },
    Exit(Result<(), io::Error>),
}

impl Action {
    pub(crate) fn exec(self) {
        let action = self;
        let exit_after_entry = zellij::inside_session() && !matches!(action, Action::Exit(_));

        let status = match action {
            Action::CreateNewSession {
                session,
                template,
                dir: wd,
            } => {
                let workspace_dir = wd.clone().unwrap_or_else(Dir::cwd);
                let layout = match template {
                    Some(template) => {
                        let engine = TemplateEngine::new(&OPTIONS.templates, &OPTIONS.cache);
                        match engine.render(&template, &session, &workspace_dir) {
                            Ok(path) => Some(path.to_string_lossy().into_owned()),
                            Err(error) => Self::exit_with_error(error),
                        }
                    }
                    None => None,
                };
                zellij::create(&session, &layout, &wd)
            }
            Action::AttachToSession(session) => zellij::attach(&session),
            Action::Exit(Ok(())) => process::exit(0),
            Action::Exit(Err(error)) => Self::exit_with_error(error),
        };

        match status {
            Ok(status) => {
                if !status.success() {
                    process::exit(status.code().unwrap_or(1));
                }
                if exit_after_entry {
                    process::exit(0);
                }
            }
            Err(err) => Self::exit_with_error(err),
        }
    }

    fn exit_with_error(error: io::Error) -> ! {
        log::error(error);
        process::exit(1);
    }
}
