use std::{env, path::PathBuf, sync::LazyLock};

use crate::dir::Dir;

pub(crate) static OPTIONS: LazyLock<Options> = LazyLock::new(Options::new);

#[derive(Debug)]
pub(crate) struct Options {
    pub root: Dir,
    pub ignore: Vec<String>,
    pub depth: Option<usize>,
    pub templates: PathBuf,
    pub keymaps: PathBuf,
    pub cache: PathBuf,
    pub banners: Option<Dir>,
}

impl Options {
    pub fn new() -> Self {
        Self {
            root: path_from_home("ZELLIJ_WORKSPACES_ROOT_DIR").unwrap_or_else(Dir::home),
            ignore: variable("ZELLIJ_WORKSPACES_IGNORE_DIRS")
                .map(|dirs| dirs.split(',').map(|dir| dir.trim().to_owned()).collect())
                .unwrap_or_default(),
            depth: parse_depth(),
            templates: path_from_home("ZELLIJ_WORKSPACES_TEMPLATES_DIR")
                .unwrap_or_else(|| Dir::home().join(".config/zellij-workspaces/templates"))
                .as_path()
                .to_path_buf(),
            keymaps: path_from_home("ZELLIJ_WORKSPACES_KEYMAPS_FILE")
                .unwrap_or_else(|| Dir::home().join(".config/zellij-workspaces/keymaps.kdl"))
                .as_path()
                .to_path_buf(),
            cache: path_from_home("ZELLIJ_WORKSPACES_CACHE_DIR")
                .map_or_else(default_cache_dir, |dir| dir.as_path().to_path_buf()),
            banners: path_from_home("ZELLIJ_WORKSPACES_BANNERS_DIR"),
        }
    }
}

fn parse_depth() -> Option<usize> {
    variable("ZELLIJ_WORKSPACES_MAX_DIRS_DEPTH").map(|value| {
        let depth = value.parse().unwrap_or_else(|error| {
            panic!(
                "ZELLIJ_WORKSPACES_MAX_DIRS_DEPTH must be a positive integer; found `{value}`: {error}"
            )
        });
        assert!(depth > 0, "ZELLIJ_WORKSPACES_MAX_DIRS_DEPTH must be positive");
        depth
    })
}

fn variable(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => panic!("{name} is not valid Unicode"),
    }
}

fn path_from_home(name: &str) -> Option<Dir> {
    variable(name).map(|value| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            Dir::from(path)
        } else {
            Dir::home().join(path)
        }
    })
}

fn default_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| Dir::home().join(".cache").as_path().to_path_buf())
        .join("zellij-workspaces/layouts")
}

#[cfg(test)]
mod tests {
    use super::Options;

    #[test]
    fn defaults_include_runtime_template_and_cache_directories() {
        let options = Options::new();
        assert!(options.templates.ends_with("zellij-workspaces/templates"));
        assert!(options.keymaps.ends_with("zellij-workspaces/keymaps.kdl"));
        assert!(options.cache.ends_with("zellij-workspaces/layouts"));
    }
}
