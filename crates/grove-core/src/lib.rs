//! Core worktree lifecycle for Grove: discovery, naming, create/remove/rename.
//!
//! This crate ties [`grove_git`] (the git process layer) to [`grove_config`]
//! (resolved settings) and exposes the operations the CLI drives. Forge, db,
//! adapters and the task graph plug in around these primitives in later
//! milestones.

use std::path::{Path, PathBuf};

pub use grove_config::{Config, ConfigError};
pub use grove_git::{GitError, Repo, Worktree};

/// Errors from worktree lifecycle operations.
#[derive(thiserror::Error, Debug)]
pub enum CoreError {
    #[error(transparent)]
    Git(#[from] GitError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("no worktree matching '{0}'")]
    NotFound(String),

    #[error("destination already exists: {0}")]
    DestExists(PathBuf),

    #[error("creating {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Convenience alias for fallible core operations.
pub type Result<T> = core::result::Result<T, CoreError>;

/// A repository plus its resolved configuration — the entry point for the CLI.
pub struct Grove {
    pub repo: Repo,
    pub config: Config,
    root: PathBuf,
}

/// Options for [`Grove::create`].
#[derive(Debug, Clone, Default)]
pub struct CreateOpts {
    /// Branch to create/check out. Defaults to the worktree `name`.
    pub branch: Option<String>,
    /// Start point when creating a new branch (`--from`).
    pub base: Option<String>,
    /// Override the folder name (`--folder`). Defaults to the sanitized branch.
    pub folder: Option<String>,
    /// Allow checking out a branch already used by another worktree.
    pub force: bool,
}

/// Options for [`Grove::remove`].
#[derive(Debug, Clone, Default)]
pub struct RemoveOpts {
    pub delete_branch: bool,
    pub force: bool,
}

impl Grove {
    /// Discover the repo from the current directory and load configuration.
    pub fn open() -> Result<Grove> {
        let repo = Repo::discover()?;
        Grove::with_repo(repo)
    }

    pub fn with_repo(repo: Repo) -> Result<Grove> {
        let config = Config::load(&repo)?;
        let root = repo
            .main_worktree()
            .unwrap_or_else(|_| repo.cwd().to_path_buf());
        Ok(Grove { repo, config, root })
    }

    /// The main worktree root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All worktrees in the repository.
    pub fn list(&self) -> Result<Vec<Worktree>> {
        Ok(self.repo.worktrees()?)
    }

    /// The base directory new worktrees are created under.
    #[must_use]
    pub fn worktrees_dir(&self) -> PathBuf {
        self.config.resolve_worktrees_dir(&self.root)
    }

    /// The on-disk path a worktree for `branch` would occupy.
    #[must_use]
    pub fn folder_for(&self, branch: &str) -> PathBuf {
        let folder = format!("{}{}", self.config.worktrees_prefix, sanitize(branch));
        self.worktrees_dir().join(folder)
    }

    /// Find an existing worktree by branch name or folder name.
    pub fn find(&self, name: &str) -> Result<Option<Worktree>> {
        let prefixed = format!("{}{}", self.config.worktrees_prefix, sanitize(name));
        Ok(self.list()?.into_iter().find(|w| {
            w.branch.as_deref() == Some(name)
                || w.folder_name() == Some(name)
                || w.folder_name() == Some(prefixed.as_str())
        }))
    }

    /// Resolve a worktree path by branch/folder name, erroring if not found.
    pub fn path_for(&self, name: &str) -> Result<PathBuf> {
        self.find(name)?
            .map(|w| w.path)
            .ok_or_else(|| CoreError::NotFound(name.to_string()))
    }

    /// Create a new worktree. Returns its path.
    pub fn create(&self, name: &str, opts: &CreateOpts) -> Result<PathBuf> {
        let branch = opts.branch.clone().unwrap_or_else(|| name.to_string());
        let folder = opts.folder.as_ref().map_or_else(
            || self.folder_for(&branch),
            |f| self.worktrees_dir().join(f),
        );

        if folder.exists() {
            return Err(CoreError::DestExists(folder));
        }
        if let Some(parent) = folder.parent() {
            std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let create_branch = !self.repo.branch_exists(&branch)?;
        self.repo.add_worktree(
            &folder,
            &branch,
            create_branch,
            opts.base.as_deref(),
            opts.force,
        )?;
        Ok(folder)
    }

    /// Remove a worktree (and optionally its branch).
    pub fn remove(&self, name: &str, opts: &RemoveOpts) -> Result<()> {
        let wt = self
            .find(name)?
            .ok_or_else(|| CoreError::NotFound(name.to_string()))?;
        self.repo.remove_worktree(&wt.path, opts.force)?;
        if opts.delete_branch {
            if let Some(branch) = wt.branch {
                self.repo.delete_branch(&branch)?;
            }
        }
        Ok(())
    }

    /// Rename a worktree and its branch. Returns the new path.
    pub fn rename(&self, old: &str, new: &str, force: bool) -> Result<PathBuf> {
        let wt = self
            .find(old)?
            .ok_or_else(|| CoreError::NotFound(old.to_string()))?;
        let dest = self.folder_for(new);
        self.repo.move_worktree(&wt.path, &dest, force)?;
        if let Some(old_branch) = wt.branch {
            if old_branch != new {
                self.repo.rename_branch(&old_branch, new)?;
            }
        }
        Ok(dest)
    }
}

/// Make a branch name safe to use as a directory component.
#[must_use]
pub fn sanitize(branch: &str) -> String {
    branch.replace('/', "-")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_slashes() {
        assert_eq!(sanitize("feat/x"), "feat-x");
        assert_eq!(sanitize("plain"), "plain");
    }
}
