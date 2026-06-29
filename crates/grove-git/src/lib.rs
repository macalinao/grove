//! Thin wrapper over the `git` CLI, focused on worktree operations.
//!
//! Per the Grove design, mutating worktree operations shell out to
//! `git worktree` (git owns that machinery and is the contract we rely on).
//! Reads are parsed from `--porcelain` output. A future iteration may move
//! read paths onto `gix`; the API here is deliberately small so that swap
//! stays local.

use std::path::{Path, PathBuf};
use std::process::Command;

mod error;

pub use error::GitError;

/// Convenience alias for fallible git operations.
pub type Result<T> = core::result::Result<T, GitError>;

/// A single git worktree as reported by `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// Short branch name (`refs/heads/foo` → `foo`), if not detached.
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: bool,
}

impl Worktree {
    /// The directory name of the worktree (final path component).
    #[must_use]
    pub fn folder_name(&self) -> Option<&str> {
        self.path.file_name().and_then(|s| s.to_str())
    }
}

/// A handle to a git repository, rooted at one of its worktrees.
#[derive(Debug, Clone)]
pub struct Repo {
    /// Directory we invoke `git` from (a valid worktree toplevel).
    cwd: PathBuf,
}

impl Repo {
    /// Discover the repository containing the current directory.
    pub fn discover() -> Result<Repo> {
        let cwd = std::env::current_dir().map_err(GitError::Cwd)?;
        Repo::discover_from(&cwd)
    }

    /// Discover the repository containing `dir`.
    pub fn discover_from(dir: &Path) -> Result<Repo> {
        let toplevel =
            run_git(dir, &["rev-parse", "--show-toplevel"]).map_err(|_| GitError::NotARepo)?;
        Ok(Repo {
            cwd: PathBuf::from(toplevel.trim()),
        })
    }

    /// The worktree we were opened from.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Run a git subcommand in this repo, returning trimmed stdout.
    pub fn git(&self, args: &[&str]) -> Result<String> {
        run_git(&self.cwd, args)
    }

    /// The git common directory (shared across worktrees), as an absolute path.
    ///
    /// This is where repo-wide state lives (the real `.git` for the main
    /// worktree). Use it to store data that should be shared by every worktree.
    ///
    /// # Errors
    /// Returns an error if `git rev-parse` fails or produces no output.
    pub fn git_common_dir(&self) -> Result<PathBuf> {
        let out = run_git(&self.cwd, &["rev-parse", "--git-common-dir"])?;
        let raw = PathBuf::from(out.trim());
        if raw.is_absolute() {
            Ok(raw)
        } else {
            Ok(self.cwd.join(raw))
        }
    }

    /// The primary (first) worktree of the repository.
    pub fn main_worktree(&self) -> Result<PathBuf> {
        self.worktrees()?
            .into_iter()
            .next()
            .map(|w| w.path)
            .ok_or(GitError::NoWorktrees)
    }

    /// All worktrees, in git's listing order (primary first).
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let out = self.git(&["worktree", "list", "--porcelain"])?;
        Ok(parse_worktrees(&out))
    }

    /// Read a git config value, returning `None` if unset.
    pub fn config_get(&self, key: &str) -> Result<Option<String>> {
        let output = Command::new("git")
            .current_dir(&self.cwd)
            .args(["config", "--get", key])
            .output()
            .map_err(|source| GitError::Spawn {
                cmd: format!("config --get {key}"),
                source,
            })?;

        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else if output.status.code() == Some(1) {
            // exit code 1 == key not present; anything else is a real error.
            Ok(None)
        } else {
            Err(GitError::Command {
                cmd: format!("config --get {key}"),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    /// Set a git config value (`git config [--global] <key> <value>`).
    ///
    /// # Errors
    /// Returns an error if `git` cannot be spawned or the command fails.
    pub fn config_set(&self, key: &str, value: &str, global: bool) -> Result<()> {
        let mut args: Vec<&str> = vec!["config"];
        if global {
            args.push("--global");
        }
        args.extend_from_slice(&[key, value]);
        self.git(&args)?;
        Ok(())
    }

    /// Unset a git config value (`git config [--global] --unset <key>`).
    ///
    /// A missing key (git exit code 5) is treated as a successful no-op.
    ///
    /// # Errors
    /// Returns an error if `git` cannot be spawned or the command fails for a
    /// reason other than the key being absent.
    pub fn config_unset(&self, key: &str, global: bool) -> Result<()> {
        let mut args: Vec<&str> = vec!["config"];
        if global {
            args.push("--global");
        }
        args.extend_from_slice(&["--unset", key]);

        let output = Command::new("git")
            .current_dir(&self.cwd)
            .args(&args)
            .output()
            .map_err(|source| GitError::Spawn {
                cmd: args.join(" "),
                source,
            })?;

        if output.status.success() || output.status.code() == Some(5) {
            // exit code 5 == key not present; nothing to unset.
            Ok(())
        } else {
            Err(GitError::Command {
                cmd: args.join(" "),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    /// List all `grove.*` config entries as `(key, value)` pairs.
    ///
    /// Runs `git config --get-regexp '^grove\.'`. Returns an empty vector when
    /// no matching keys are set (git exit code 1).
    ///
    /// # Errors
    /// Returns an error if `git` cannot be spawned or the command fails for a
    /// reason other than there being no matches.
    pub fn config_list_grove(&self) -> Result<Vec<(String, String)>> {
        let args = ["config", "--get-regexp", r"^grove\."];
        let output = Command::new("git")
            .current_dir(&self.cwd)
            .args(args)
            .output()
            .map_err(|source| GitError::Spawn {
                cmd: args.join(" "),
                source,
            })?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(parse_config_pairs(&stdout))
        } else if output.status.code() == Some(1) {
            // exit code 1 == no matching keys.
            Ok(Vec::new())
        } else {
            Err(GitError::Command {
                cmd: args.join(" "),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    /// Read all values for a (possibly multi-valued) git config key.
    ///
    /// Returns an empty vector if the key is unset.
    ///
    /// # Errors
    /// Returns [`GitError`] if `git config` cannot be spawned or fails for a
    /// reason other than the key being absent.
    pub fn config_get_all(&self, key: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .current_dir(&self.cwd)
            .args(["config", "--get-all", key])
            .output()
            .map_err(|source| GitError::Spawn {
                cmd: format!("config --get-all {key}"),
                source,
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect())
        } else if output.status.code() == Some(1) {
            // exit code 1 == key not present; anything else is a real error.
            Ok(Vec::new())
        } else {
            Err(GitError::Command {
                cmd: format!("config --get-all {key}"),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    /// Does a local branch named `branch` exist?
    pub fn branch_exists(&self, branch: &str) -> Result<bool> {
        self.ref_exists(&format!("refs/heads/{branch}"))
    }

    /// Does the remote-tracking branch `<remote>/<branch>` exist?
    pub fn remote_branch_exists(&self, remote: &str, branch: &str) -> Result<bool> {
        self.ref_exists(&format!("refs/remotes/{remote}/{branch}"))
    }

    /// Does `spec` resolve to an existing ref (quietly)?
    fn ref_exists(&self, spec: &str) -> Result<bool> {
        let output = Command::new("git")
            .current_dir(&self.cwd)
            .args(["rev-parse", "--verify", "--quiet", spec])
            .output()
            .map_err(|source| GitError::Spawn {
                cmd: format!("rev-parse --verify {spec}"),
                source,
            })?;
        Ok(output.status.success())
    }

    /// The short name of the currently checked-out branch, if not detached.
    pub fn current_branch(&self) -> Result<Option<String>> {
        let output = Command::new("git")
            .current_dir(&self.cwd)
            .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
            .output()
            .map_err(|source| GitError::Spawn {
                cmd: "symbolic-ref --short HEAD".to_string(),
                source,
            })?;
        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
    }

    /// The default branch of `remote` (its HEAD), e.g. `main`, if known.
    pub fn remote_head_branch(&self, remote: &str) -> Result<Option<String>> {
        let spec = format!("refs/remotes/{remote}/HEAD");
        let output = Command::new("git")
            .current_dir(&self.cwd)
            .args(["symbolic-ref", "--quiet", &spec])
            .output()
            .map_err(|source| GitError::Spawn {
                cmd: format!("symbolic-ref {spec}"),
                source,
            })?;
        if output.status.success() {
            let full = String::from_utf8_lossy(&output.stdout);
            let prefix = format!("refs/remotes/{remote}/");
            Ok(full.trim().strip_prefix(&prefix).map(str::to_string))
        } else {
            Ok(None)
        }
    }

    /// Fetch `remote` (`git fetch <remote>`).
    ///
    /// # Errors
    /// Returns [`GitError`] if `git` cannot be spawned or the fetch fails
    /// (e.g. the network is unavailable).
    pub fn fetch(&self, remote: &str) -> Result<()> {
        self.git(&["fetch", remote])?;
        Ok(())
    }

    /// Set the upstream of local `branch` to `upstream` (`<remote>/<name>`).
    pub fn set_upstream(&self, branch: &str, upstream: &str) -> Result<()> {
        self.git(&["branch", &format!("--set-upstream-to={upstream}"), branch])?;
        Ok(())
    }

    /// Create a worktree at `path`.
    ///
    /// If `create_branch`, runs `git worktree add -b <branch> <path> [base]`;
    /// otherwise checks out the existing `branch` into the new worktree.
    pub fn add_worktree(
        &self,
        path: &Path,
        branch: &str,
        create_branch: bool,
        base: Option<&str>,
        force: bool,
    ) -> Result<()> {
        let path = path.to_string_lossy().into_owned();
        let mut args: Vec<String> = vec!["worktree".into(), "add".into()];
        if force {
            args.push("--force".into());
        }
        if create_branch {
            args.push("-b".into());
            args.push(branch.into());
            args.push(path);
            if let Some(base) = base {
                args.push(base.into());
            }
        } else {
            args.push(path);
            args.push(branch.into());
        }
        self.git_owned(&args)?;
        Ok(())
    }

    /// Remove the worktree at `path`.
    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<()> {
        let path = path.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path);
        self.git(&args)?;
        Ok(())
    }

    /// Move the worktree at `from` to `to`.
    pub fn move_worktree(&self, from: &Path, to: &Path, force: bool) -> Result<()> {
        let from = from.to_string_lossy().into_owned();
        let to = to.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "move"];
        if force {
            args.push("--force");
        }
        args.push(&from);
        args.push(&to);
        self.git(&args)?;
        Ok(())
    }

    /// Rename a branch (`git branch -m old new`).
    pub fn rename_branch(&self, old: &str, new: &str) -> Result<()> {
        self.git(&["branch", "-m", old, new])?;
        Ok(())
    }

    /// Delete a branch (`git branch -D name`).
    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        self.git(&["branch", "-D", branch])?;
        Ok(())
    }

    fn git_owned(&self, args: &[String]) -> Result<String> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.git(&refs)
    }
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|source| GitError::Spawn {
            cmd: args.join(" "),
            source,
        })?;
    if !output.status.success() {
        return Err(GitError::Command {
            cmd: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| GitError::NotUtf8)
}

/// Read a single value from an arbitrary git-config INI file (`git config -f`).
///
/// Returns `None` when the key is absent (git exit code 1). Works without a
/// repository, so it can read `.gtrconfig` / `.groveconfig` directly.
///
/// # Errors
///
/// Returns an error if `git` cannot be spawned or fails for any reason other
/// than the key being missing.
pub fn config_file_get(file: &Path, key: &str) -> Result<Option<String>> {
    let file = file.to_string_lossy().into_owned();
    let output = Command::new("git")
        .args(["config", "-f", &file, "--get", key])
        .output()
        .map_err(|source| GitError::Spawn {
            cmd: format!("config -f {file} --get {key}"),
            source,
        })?;
    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    } else if output.status.code() == Some(1) {
        Ok(None)
    } else {
        Err(GitError::Command {
            cmd: format!("config -f {file} --get {key}"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// Read all values for a (possibly multi-valued) key from an arbitrary
/// git-config INI file (`git config -f <file> --get-all <key>`).
///
/// Returns an empty vector when the key is absent (git exit code 1). Works
/// without a repository, so it can read `.gtrconfig` / `.groveconfig` directly.
///
/// # Errors
///
/// Returns an error if `git` cannot be spawned or fails for any reason other
/// than the key being missing.
pub fn config_file_get_all(file: &Path, key: &str) -> Result<Vec<String>> {
    let file = file.to_string_lossy().into_owned();
    let output = Command::new("git")
        .args(["config", "-f", &file, "--get-all", key])
        .output()
        .map_err(|source| GitError::Spawn {
            cmd: format!("config -f {file} --get-all {key}"),
            source,
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    } else if output.status.code() == Some(1) {
        Ok(Vec::new())
    } else {
        Err(GitError::Command {
            cmd: format!("config -f {file} --get-all {key}"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// Parse the output of `git worktree list --porcelain`.
fn parse_worktrees(s: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut cur: Option<Worktree> = None;

    for line in s.lines() {
        if line.is_empty() {
            out.extend(cur.take());
            continue;
        }
        let (key, val) = match line.split_once(' ') {
            Some((k, v)) => (k, Some(v)),
            None => (line, None),
        };
        match key {
            "worktree" => {
                out.extend(cur.take());
                cur = Some(Worktree {
                    path: PathBuf::from(val.unwrap_or_default()),
                    head: None,
                    branch: None,
                    bare: false,
                    detached: false,
                    locked: false,
                });
            }
            "HEAD" => set(&mut cur, |w| w.head = val.map(str::to_string)),
            "branch" => set(&mut cur, |w| {
                w.branch = val.map(|v| v.trim_start_matches("refs/heads/").to_string());
            }),
            "bare" => set(&mut cur, |w| w.bare = true),
            "detached" => set(&mut cur, |w| w.detached = true),
            "locked" => set(&mut cur, |w| w.locked = true),
            _ => {}
        }
    }
    out.extend(cur.take());
    out
}

fn set(cur: &mut Option<Worktree>, f: impl FnOnce(&mut Worktree)) {
    if let Some(w) = cur.as_mut() {
        f(w);
    }
}

/// Parse `git config --get-regexp` output ("key value" per line) into pairs.
fn parse_config_pairs(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_listing() {
        let sample = "\
worktree /home/u/repo
HEAD abc123
branch refs/heads/main

worktree /home/u/repo-worktrees/feat-x
HEAD def456
branch refs/heads/feat/x

worktree /home/u/repo-worktrees/detached
HEAD 999aaa
detached
";
        let wts = parse_worktrees(sample);
        assert_eq!(wts.len(), 3);
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[0].folder_name(), Some("repo"));
        assert_eq!(wts[1].branch.as_deref(), Some("feat/x"));
        assert!(!wts[1].detached);
        assert!(wts[2].detached);
        assert_eq!(wts[2].branch, None);
    }

    #[test]
    fn handles_trailing_block_without_blank_line() {
        let sample = "worktree /a\nHEAD aaa\nbranch refs/heads/a";
        let wts = parse_worktrees(sample);
        assert_eq!(wts.len(), 1);
        assert_eq!(wts[0].branch.as_deref(), Some("a"));
    }

    #[test]
    fn parses_config_pairs_lines() {
        let out = "grove.worktrees.dir ../wt\ngrove.editor.default cursor\n";
        let pairs = parse_config_pairs(out);
        assert_eq!(
            pairs,
            vec![
                ("grove.worktrees.dir".to_string(), "../wt".to_string()),
                ("grove.editor.default".to_string(), "cursor".to_string()),
            ]
        );
    }

    #[test]
    fn config_set_get_list_unset_round_trip() {
        let dir = std::env::temp_dir().join(format!("grove-cfg-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .unwrap()
        };
        run(&["init"]);
        let repo = Repo::discover_from(&dir).unwrap();

        assert_eq!(repo.config_get("grove.test.key").unwrap(), None);
        repo.config_set("grove.test.key", "hello", false).unwrap();
        assert_eq!(
            repo.config_get("grove.test.key").unwrap().as_deref(),
            Some("hello")
        );
        let listed = repo.config_list_grove().unwrap();
        assert!(listed.contains(&("grove.test.key".to_string(), "hello".to_string())));
        repo.config_unset("grove.test.key", false).unwrap();
        assert_eq!(repo.config_get("grove.test.key").unwrap(), None);
        // Unsetting a missing key is a no-op.
        repo.config_unset("grove.test.key", false).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }
}
