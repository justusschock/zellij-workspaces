use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
    process,
};

use minijinja::{Environment, UndefinedBehavior, context};
use serde::Serialize;

use crate::dir::Dir;

const TEMPLATE_SUFFIX: &str = ".kdl.tmpl";
const VARIABLE_PREFIX: &str = "ZELLIJ_WORKSPACES_VAR_";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceTemplate {
    pub name: String,
    path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct TemplateEngine {
    templates_dir: PathBuf,
    cache_dir: PathBuf,
}

#[derive(Serialize)]
struct Workspace<'a> {
    name: &'a str,
    cwd: &'a str,
}

#[derive(Serialize)]
struct Tab<'a> {
    name: &'a str,
}

#[derive(Serialize)]
struct Worktree<'a> {
    cwd: &'a str,
}

#[derive(Serialize)]
struct Tools {
    shell: String,
    editor: String,
}

impl TemplateEngine {
    pub(crate) fn new(templates_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            templates_dir: templates_dir.into(),
            cache_dir: cache_dir.into(),
        }
    }

    pub(crate) fn discover(&self) -> io::Result<Vec<WorkspaceTemplate>> {
        let entries = match fs::read_dir(&self.templates_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };

        let mut templates = entries
            .filter_map(Result::ok)
            .filter_map(|entry| Self::from_path(entry.path()))
            .collect::<Vec<_>>();
        templates.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(templates)
    }

    pub(crate) fn render(
        &self,
        template_name: &str,
        workspace_name: &str,
        workspace_dir: &Dir,
    ) -> io::Result<PathBuf> {
        let template = self
            .discover()?
            .into_iter()
            .find(|template| template.name == template_name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "workspace template `{template_name}` was not found in {}",
                        self.templates_dir.display()
                    ),
                )
            })?;
        let source = fs::read_to_string(&template.path)?;
        let cwd = workspace_dir.as_path().to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace path is not valid UTF-8",
            )
        })?;

        let mut environment = Environment::new();
        environment.set_undefined_behavior(UndefinedBehavior::Strict);
        environment.add_filter("kdl", kdl_escape);
        environment.add_filter("shell", shell_escape_for_kdl);

        let rendered = environment
            .template_from_str(&source)
            .and_then(|template| {
                template.render(context! {
                    workspace => Workspace { name: workspace_name, cwd },
                    tools => Tools::from_environment(),
                    vars => explicit_variables(),
                })
            })
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        self.write_cache(&template.name, &rendered)
    }

    pub(crate) fn render_tab(
        &self,
        template_name: &str,
        tab_name: &str,
        worktree_dir: &Dir,
    ) -> io::Result<PathBuf> {
        let template = self
            .discover()?
            .into_iter()
            .find(|template| template.name == template_name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "tab template `{template_name}` was not found in {}",
                        self.templates_dir.display()
                    ),
                )
            })?;
        let source = fs::read_to_string(&template.path)?;
        let cwd = worktree_dir.as_path().to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "worktree path is not valid UTF-8",
            )
        })?;

        let mut environment = Environment::new();
        environment.set_undefined_behavior(UndefinedBehavior::Strict);
        environment.add_filter("kdl", kdl_escape);
        environment.add_filter("shell", shell_escape_for_kdl);

        let rendered = environment
            .template_from_str(&source)
            .and_then(|template| {
                template.render(context! {
                    tab => Tab { name: tab_name },
                    worktree => Worktree { cwd },
                    tools => Tools::from_environment(),
                    vars => explicit_variables(),
                })
            })
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        self.write_cache(&format!("tab-{template_name}"), &rendered)
    }

    fn from_path(path: PathBuf) -> Option<WorkspaceTemplate> {
        let filename = path.file_name()?.to_str()?;
        let name = filename.strip_suffix(TEMPLATE_SUFFIX)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return None;
        }
        Some(WorkspaceTemplate {
            name: name.to_owned(),
            path,
        })
    }

    fn write_cache(&self, template_name: &str, rendered: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.cache_dir)?;
        let digest = content_digest(rendered.as_bytes());
        let destination = self
            .cache_dir
            .join(format!("{template_name}-{digest:016x}.kdl"));
        if fs::read_to_string(&destination).is_ok_and(|existing| existing == rendered) {
            return Ok(destination);
        }

        let temporary = self.cache_dir.join(format!(
            ".{template_name}-{digest:016x}.{}.tmp",
            process::id()
        ));
        write_private(&temporary, rendered)?;
        fs::rename(&temporary, &destination)?;
        Ok(destination)
    }
}

fn content_digest(contents: &[u8]) -> u64 {
    contents.iter().fold(0xcbf29ce484222325, |digest, byte| {
        (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

impl Tools {
    fn from_environment() -> Self {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        let editor = env::var("VISUAL")
            .or_else(|_| env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_owned());
        Self { shell, editor }
    }
}

fn explicit_variables() -> BTreeMap<String, String> {
    env::vars_os()
        .filter_map(|(name, value)| {
            let name = name.into_string().ok()?;
            let value = value.into_string().ok()?;
            name.strip_prefix(VARIABLE_PREFIX)
                .filter(|name| !name.is_empty())
                .map(|name| (name.to_owned(), value))
        })
        .collect()
}

fn kdl_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn shell_escape_for_kdl(value: &str) -> String {
    let shell_quoted = if value.is_empty() {
        "''".to_owned()
    } else if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    };
    kdl_escape(&shell_quoted)
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::{TemplateEngine, kdl_escape, shell_escape_for_kdl};
    use crate::dir::Dir;

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (Fixture, TemplateEngine) {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "zellij-workspaces-template-test-{}-{sequence}",
            process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let root = Fixture(root);
        let templates = root.path().join("templates");
        fs::create_dir(&templates).unwrap();
        let engine = TemplateEngine::new(templates, root.path().join("cache"));
        (root, engine)
    }

    #[test]
    fn discovers_only_valid_template_names_in_order() {
        let (root, engine) = fixture();
        let templates = root.path().join("templates");
        fs::write(templates.join("services.kdl.tmpl"), "layout {}").unwrap();
        fs::write(templates.join("development.kdl.tmpl"), "layout {}").unwrap();
        fs::write(templates.join("not-a-template.kdl"), "layout {}").unwrap();
        fs::write(root.path().join("ignored"), "ignored").unwrap();

        let names = engine
            .discover()
            .unwrap()
            .into_iter()
            .map(|template| template.name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["development", "services"]);
    }

    #[test]
    fn renders_workspace_context_and_reuses_identical_cache_file() {
        let (root, engine) = fixture();
        fs::write(
            root.path().join("templates/development.kdl.tmpl"),
            "layout { tab name=\"{{ workspace.name | kdl }}\" cwd=\"{{ workspace.cwd | kdl }}\" }",
        )
        .unwrap();
        let workspace = root.path().join("work tree");
        fs::create_dir(&workspace).unwrap();

        let first = engine
            .render(
                "development",
                "feature/quotes",
                &Dir::from(workspace.clone()),
            )
            .unwrap();
        let first_metadata = fs::metadata(&first).unwrap().modified().unwrap();
        let second = engine
            .render("development", "feature/quotes", &Dir::from(workspace))
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first_metadata,
            fs::metadata(second).unwrap().modified().unwrap()
        );
        assert!(fs::read_to_string(first).unwrap().contains("work tree"));
    }

    #[test]
    fn distinct_workspace_contexts_get_distinct_cache_files() {
        let (root, engine) = fixture();
        fs::write(
            root.path().join("templates/development.kdl.tmpl"),
            "layout { tab name=\"{{ workspace.name | kdl }}\" cwd=\"{{ workspace.cwd | kdl }}\" }",
        )
        .unwrap();

        let first = engine
            .render("development", "first", &Dir::from(root.path().join("one")))
            .unwrap();
        let second = engine
            .render("development", "second", &Dir::from(root.path().join("two")))
            .unwrap();

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
    }

    #[test]
    fn renders_tab_and_worktree_context_from_a_separate_template_directory() {
        let (root, _engine) = fixture();
        let tab_templates = root.path().join("tab-templates");
        fs::create_dir(&tab_templates).unwrap();
        fs::write(
            tab_templates.join("agent.kdl.tmpl"),
            "layout { tab name=\"{{ tab.name | kdl }}\" { pane cwd=\"{{ worktree.cwd | kdl }}\" command=\"agent\" } }",
        )
        .unwrap();
        let engine = TemplateEngine::new(tab_templates, root.path().join("cache"));

        let rendered = engine
            .render_tab(
                "agent",
                "fix-login",
                &Dir::from(root.path().join("worktree")),
            )
            .unwrap();
        let contents = fs::read_to_string(rendered).unwrap();

        assert!(contents.contains("fix-login"));
        assert!(contents.contains("worktree"));
        assert!(contents.contains("command=\"agent\""));
    }

    #[test]
    fn strict_templates_reject_missing_variables() {
        let (root, engine) = fixture();
        fs::write(
            root.path().join("templates/missing.kdl.tmpl"),
            "layout { tab name=\"{{ vars.DOES_NOT_EXIST | kdl }}\" }",
        )
        .unwrap();

        let error = engine
            .render("missing", "work", &Dir::from(root.path().to_path_buf()))
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn filters_escape_kdl_and_shell_boundaries() {
        assert_eq!(kdl_escape("a\\b\"c\n"), "a\\\\b\\\"c\\n");
        assert_eq!(shell_escape_for_kdl("two words"), "'two words'");
        assert_eq!(shell_escape_for_kdl("it's"), "'it'\\\\''s'");
    }

    #[test]
    fn explicit_variables_do_not_expose_unprefixed_environment() {
        let variables = super::explicit_variables();
        assert!(!variables.contains_key("PATH"));
        assert!(env::var("PATH").is_ok());
    }

    #[test]
    fn generic_examples_render_without_project_specific_variables() {
        let (root, engine) = fixture();
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        for name in ["development", "services"] {
            fs::copy(
                examples.join(format!("{name}.kdl.tmpl")),
                root.path().join(format!("templates/{name}.kdl.tmpl")),
            )
            .unwrap();
            let rendered = engine
                .render(name, "example", &Dir::from(root.path().to_path_buf()))
                .unwrap();
            let contents = fs::read_to_string(rendered).unwrap();
            assert!(contents.starts_with("layout {"));
            assert!(!contents.contains("{{"));
            contents.parse::<kdl::KdlDocument>().unwrap();
        }
    }
}
