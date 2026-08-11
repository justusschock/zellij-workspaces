use std::{collections::BTreeSet, fs, io, path::Path};

use kdl::KdlDocument;

use crate::dir::Dir;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DefaultWorkspace {
    pub name: String,
    pub cwd: Dir,
    pub template: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Config {
    pub workspaces: Vec<DefaultWorkspace>,
}

impl Config {
    pub(crate) fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(source) => Self::parse(&source),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    fn parse(source: &str) -> io::Result<Self> {
        let document = source
            .parse::<KdlDocument>()
            .map_err(|error| invalid(format!("invalid workspace config KDL: {error}")))?;
        if document.nodes().len() != 1 || document.nodes()[0].name().value() != "workspaces" {
            return Err(invalid(
                "workspaces.kdl must contain exactly one top-level `workspaces` node",
            ));
        }
        let root = document
            .get("workspaces")
            .ok_or_else(|| invalid("workspaces.kdl must contain a `workspaces` node"))?;
        if !root.entries().is_empty() {
            return Err(invalid(
                "the `workspaces` node cannot have arguments or properties",
            ));
        }

        let mut workspaces = Vec::new();
        let mut names = BTreeSet::new();
        if let Some(nodes) = root.children() {
            for node in nodes.nodes() {
                if node.name().value() != "workspace" {
                    return Err(invalid(format!(
                        "unknown workspace config node `{}`",
                        node.name().value()
                    )));
                }
                if node.children().is_some() {
                    return Err(invalid("workspace entries cannot contain child nodes"));
                }
                let positional = node
                    .entries()
                    .iter()
                    .filter(|entry| entry.name().is_none())
                    .collect::<Vec<_>>();
                if positional.len() != 1 {
                    return Err(invalid(
                        "workspace entries require exactly one string name argument",
                    ));
                }
                let name = positional[0]
                    .value()
                    .as_string()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| invalid("workspace names must be non-empty strings"))?;
                if !names.insert(name.to_owned()) {
                    return Err(invalid(format!("duplicate workspace `{name}`")));
                }

                let mut cwd = None;
                let mut template = None;
                for entry in node.entries().iter().filter(|entry| entry.name().is_some()) {
                    let property = entry.name().expect("filtered named entries").value();
                    let value = entry.value().as_string().ok_or_else(|| {
                        invalid(format!(
                            "workspace `{name}` property `{property}` must be a string"
                        ))
                    })?;
                    match property {
                        "cwd" if cwd.replace(value).is_none() => {}
                        "template" if template.replace(value).is_none() => {}
                        "cwd" | "template" => {
                            return Err(invalid(format!(
                                "workspace `{name}` repeats property `{property}`"
                            )));
                        }
                        _ => {
                            return Err(invalid(format!(
                                "workspace `{name}` has unknown property `{property}`"
                            )));
                        }
                    }
                }
                let cwd =
                    cwd.ok_or_else(|| invalid(format!("workspace `{name}` requires `cwd`")))?;
                let template = template
                    .filter(|template| !template.is_empty())
                    .ok_or_else(|| invalid(format!("workspace `{name}` requires `template`")))?;
                workspaces.push(DefaultWorkspace {
                    name: name.to_owned(),
                    cwd: Dir::from_home_path(cwd),
                    template: template.to_owned(),
                });
            }
        }

        Ok(Self { workspaces })
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::Config;
    use crate::dir::Dir;

    #[test]
    fn parses_ordered_default_workspaces() {
        let config = Config::parse(
            r#"workspaces {
                workspace "grid" cwd="~/Developer/grid" template="codex"
                workspace "databases" cwd="~" template="databases"
            }"#,
        )
        .unwrap();

        assert_eq!(config.workspaces.len(), 2);
        assert_eq!(config.workspaces[0].name, "grid");
        assert_eq!(config.workspaces[0].cwd, Dir::home().join("Developer/grid"));
        assert_eq!(config.workspaces[0].template, "codex");
        assert_eq!(config.workspaces[1].cwd, Dir::home());
    }

    #[test]
    fn rejects_duplicates_and_unknown_properties() {
        assert!(
            Config::parse(
                r#"workspaces {
                workspace "grid" cwd="~/grid" template="codex"
                workspace "grid" cwd="~/other" template="codex"
            }"#
            )
            .is_err()
        );
        assert!(
            Config::parse(
                r#"workspaces { workspace "grid" cwd="~/grid" template="codex" command="codex" }"#
            )
            .is_err()
        );
    }
}
