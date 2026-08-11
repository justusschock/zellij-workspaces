use std::{fs, io, path::PathBuf, process::Command};

use crate::dir::Dir;

#[derive(Clone, Debug)]
pub(crate) struct Repository {
    root: Dir,
    source: Dir,
}

impl Repository {
    pub(crate) fn discover(cwd: &Dir) -> io::Result<Self> {
        let source_output = Command::new("git")
            .args([
                "-C",
                cwd.as_path().to_string_lossy().as_ref(),
                "rev-parse",
                "--show-toplevel",
            ])
            .output()?;
        if !source_output.status.success() {
            return Err(command_error(
                "find the current Git worktree",
                &source_output.stderr,
            ));
        }
        let source = String::from_utf8(source_output.stdout)
            .map_err(|_| invalid("Git worktree path is not valid UTF-8"))?;
        let source = source.trim();
        if source.is_empty() {
            return Err(invalid("Git returned an empty worktree path"));
        }

        let output = Command::new("git")
            .args([
                "-C",
                cwd.as_path().to_string_lossy().as_ref(),
                "worktree",
                "list",
                "--porcelain",
            ])
            .output()?;
        if !output.status.success() {
            return Err(command_error("find the Git repository", &output.stderr));
        }
        let stdout = String::from_utf8(output.stdout)
            .map_err(|_| invalid("Git worktree output is not valid UTF-8"))?;
        let root = parse_worktrees(&stdout)
            .into_iter()
            .next()
            .ok_or_else(|| invalid("Git returned no worktrees"))?;
        Ok(Self {
            root,
            source: Dir::from(source.to_owned()),
        })
    }

    pub(crate) fn worktrees(&self) -> io::Result<Vec<Dir>> {
        let output = Command::new("git")
            .args([
                "-C",
                self.source.as_path().to_string_lossy().as_ref(),
                "worktree",
                "list",
                "--porcelain",
            ])
            .output()?;
        if !output.status.success() {
            return Err(command_error("list Git worktrees", &output.stderr));
        }
        let stdout = String::from_utf8(output.stdout)
            .map_err(|_| invalid("Git worktree output is not valid UTF-8"))?;
        Ok(parse_worktrees(&stdout))
    }

    pub(crate) fn create_worktree(&self, workstream: &str) -> io::Result<Dir> {
        validate_workstream(workstream)?;
        let parent = self.root.join(".worktrees");
        fs::create_dir_all(&parent)?;
        let destination = parent.join(workstream);
        if destination.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("worktree path already exists: {}", destination.display()),
            ));
        }

        let output = Command::new("git")
            .args([
                "-C",
                self.source.as_path().to_string_lossy().as_ref(),
                "worktree",
                "add",
                "-b",
                workstream,
                destination.as_path().to_string_lossy().as_ref(),
            ])
            .output()?;
        if !output.status.success() {
            return Err(command_error("create the Git worktree", &output.stderr));
        }
        Ok(destination)
    }
}

fn parse_worktrees(output: &str) -> Vec<Dir> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .filter(|path| !path.is_empty())
        .map(|path| Dir::from(PathBuf::from(path)))
        .collect()
}

fn validate_workstream(workstream: &str) -> io::Result<()> {
    if workstream.is_empty()
        || !workstream
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid(
            "workstream names may contain only letters, numbers, `-`, `_`, and `.`",
        ));
    }
    Ok(())
}

fn command_error(action: &str, stderr: &[u8]) -> io::Error {
    let detail = String::from_utf8_lossy(stderr).trim().to_owned();
    if detail.is_empty() {
        io::Error::other(format!("Failed to {action}"))
    } else {
        io::Error::other(format!("Failed to {action}: {detail}"))
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf, process, process::Command};

    use super::{Repository, parse_worktrees, validate_workstream};
    use crate::dir::Dir;

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn parses_porcelain_worktree_paths_in_order() {
        let worktrees = parse_worktrees(
            "worktree /tmp/repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/repo/.worktrees/topic\nHEAD def\nbranch refs/heads/topic\n",
        );
        assert_eq!(worktrees[0].as_path().to_str(), Some("/tmp/repo"));
        assert_eq!(
            worktrees[1].as_path().to_str(),
            Some("/tmp/repo/.worktrees/topic")
        );
    }

    #[test]
    fn workstream_names_cannot_escape_the_managed_directory() {
        assert!(validate_workstream("fix-login_2").is_ok());
        assert!(validate_workstream("../escape").is_err());
        assert!(validate_workstream("feature/name").is_err());
        assert!(validate_workstream("").is_err());
    }

    #[test]
    fn new_worktrees_use_the_main_worktree_directory_and_current_head() {
        let root = env::temp_dir().join(format!("zellij-workspaces-git-test-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let _fixture = Fixture(root.clone());
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "Test User"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        git(&root, &["commit", "--allow-empty", "-m", "initial"]);

        let repository = Repository::discover(&Dir::from(root.clone())).unwrap();
        let topic = repository.create_worktree("topic").unwrap();
        git(&topic, &["commit", "--allow-empty", "-m", "topic"]);
        let topic_head = git(&topic, &["rev-parse", "HEAD"]);

        let from_topic = Repository::discover(&topic).unwrap();
        let followup = from_topic.create_worktree("followup").unwrap();

        assert_eq!(
            followup,
            Dir::from(fs::canonicalize(&root).unwrap().join(".worktrees/followup"))
        );
        assert_eq!(git(&followup, &["branch", "--show-current"]), "followup");
        assert_eq!(git(&followup, &["rev-parse", "HEAD"]), topic_head);
    }
}
