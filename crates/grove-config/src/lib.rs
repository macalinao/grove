//! Layered configuration for Grove.
//!
//! Sources, highest → lowest precedence (see the design spec §3.9):
//!   1. local `grove.kdl` (the native format)
//!   2. local git config `grove.*`
//!   3. `.gtrconfig` (gtr compat — read only, mapped `gtr.* → grove.*`)  *(TODO)*
//!   4. global git config `grove.*`
//!
//! M1 implements the git-config layer and the native `grove.kdl` layer; the
//! `.gtrconfig`/`.groveconfig` compat readers are stubbed for a follow-up.

use std::path::{Path, PathBuf};

use grove_git::{GitError, Repo};

mod gtr_compat;
mod kdl_source;

/// Errors from loading configuration.
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error(transparent)]
    Git(#[from] GitError),

    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("parsing {path}: {message}")]
    Kdl { path: PathBuf, message: String },
}

/// Convenience alias for fallible config operations.
pub type Result<T> = core::result::Result<T, ConfigError>;

/// Resolved Grove configuration for a repository.
#[derive(Debug, Clone)]
pub struct Config {
    /// Base directory for new worktrees (relative paths resolve against the
    /// main worktree). Default: `<repo>-worktrees` beside the repo.
    pub worktrees_dir: Option<String>,
    /// Folder-name prefix for new worktrees (NOT a branch prefix — see the
    /// gtr gotcha in the design notes).
    pub worktrees_prefix: String,
    /// Default editor adapter name.
    pub editor_default: Option<String>,
    /// Default AI tool adapter name.
    pub ai_default: Option<String>,
    /// Color policy: `auto` | `always` | `never`.
    pub color: ColorChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    pub(crate) fn parse(s: &str) -> ColorChoice {
        match s {
            "always" => ColorChoice::Always,
            "never" => ColorChoice::Never,
            _ => ColorChoice::Auto,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            worktrees_dir: None,
            worktrees_prefix: String::new(),
            editor_default: None,
            ai_default: None,
            color: ColorChoice::Auto,
        }
    }
}

impl Config {
    /// Load and merge all configuration sources for `repo`.
    pub fn load(repo: &Repo) -> Result<Config> {
        let mut cfg = Config::default();
        let root = repo
            .main_worktree()
            .unwrap_or_else(|_| repo.cwd().to_path_buf());

        // Lowest precedence: gtr-compat files (.gtrconfig then .groveconfig).
        gtr_compat::apply(&mut cfg, &root);

        // Then git config grove.* (covers global + local).
        cfg.apply_git_config(repo)?;

        // Highest precedence: a `grove.kdl` at the main worktree root.
        let kdl_path = root.join("grove.kdl");
        if kdl_path.exists() {
            let src = std::fs::read_to_string(&kdl_path).map_err(|source| ConfigError::Read {
                path: kdl_path.clone(),
                source,
            })?;
            kdl_source::apply(&mut cfg, &src).map_err(|message| ConfigError::Kdl {
                path: kdl_path,
                message,
            })?;
        }

        Ok(cfg)
    }

    fn apply_git_config(&mut self, repo: &Repo) -> Result<()> {
        if let Some(v) = repo.config_get("grove.worktrees.dir")? {
            self.worktrees_dir = Some(v);
        }
        if let Some(v) = repo.config_get("grove.worktrees.prefix")? {
            self.worktrees_prefix = v;
        }
        if let Some(v) = repo.config_get("grove.editor.default")? {
            self.editor_default = Some(v);
        }
        if let Some(v) = repo.config_get("grove.ai.default")? {
            self.ai_default = Some(v);
        }
        if let Some(v) = repo.config_get("grove.ui.color")? {
            self.color = ColorChoice::parse(&v);
        }
        Ok(())
    }

    /// Resolve the worktree base directory against the repository `root`.
    #[must_use]
    pub fn resolve_worktrees_dir(&self, root: &Path) -> PathBuf {
        match &self.worktrees_dir {
            Some(dir) => {
                let p = Path::new(dir);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    root.join(p)
                }
            }
            None => default_worktrees_dir(root),
        }
    }
}

/// Default base dir: a sibling directory named `<repo>-worktrees`.
#[must_use]
pub fn default_worktrees_dir(root: &Path) -> PathBuf {
    let name = root.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
    let parent = root.parent().unwrap_or(root);
    parent.join(format!("{name}-worktrees"))
}
