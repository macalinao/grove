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
pub mod trust;

pub use trust::{TrustStatus, is_trusted, record_trust, trust_status};

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
    /// Glob patterns of files to copy from the source worktree (`grove copy`).
    pub copy_include: Vec<String>,
    /// Glob patterns of files to exclude from `grove copy`.
    pub copy_exclude: Vec<String>,
    /// Directories to copy wholesale (e.g. `node_modules`), copy-on-write
    /// where the filesystem supports it.
    pub copy_include_dirs: Vec<String>,
    /// Glob patterns of subdirectories to skip while copying `copy_include_dirs`.
    pub copy_exclude_dirs: Vec<String>,
    /// Commands run in a new worktree after creation (alongside the task graph).
    pub hook_post_create: Vec<String>,
    /// Commands run in a worktree before it is removed.
    pub hook_pre_remove: Vec<String>,
    /// Commands run (in the main repo) after a worktree is removed.
    pub hook_post_remove: Vec<String>,
    /// Commands sourced in the current shell after `grove cd` / `grove new --cd`.
    pub hook_post_cd: Vec<String>,
    /// Default remote for base refs and tracking (default: `origin`).
    pub default_remote: Option<String>,
    /// Default base branch (default: the remote's HEAD).
    pub default_branch: Option<String>,
    /// Upstream tracking mode for new branches.
    pub track: TrackMode,
    /// Workspace file passed to VS Code-style editors (or `none` to disable).
    pub editor_workspace: Option<String>,
    /// Forge provider override (`github` | `gitlab` | `gitea`); auto-detected otherwise.
    pub provider: Option<String>,
}

/// Upstream tracking mode for branches created by `grove new`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackMode {
    /// Track a remote branch when one exists, otherwise don't (default).
    #[default]
    Auto,
    /// Always set an upstream on the configured remote.
    Remote,
    /// Branch locally without setting any upstream.
    Local,
    /// Never set an upstream.
    None,
}

impl TrackMode {
    /// Parse a tracking mode, falling back to [`TrackMode::Auto`].
    #[must_use]
    pub fn parse(s: &str) -> TrackMode {
        match s {
            "remote" => TrackMode::Remote,
            "local" => TrackMode::Local,
            "none" => TrackMode::None,
            _ => TrackMode::Auto,
        }
    }
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
            copy_include: Vec::new(),
            copy_exclude: Vec::new(),
            copy_include_dirs: Vec::new(),
            copy_exclude_dirs: Vec::new(),
            hook_post_create: Vec::new(),
            hook_pre_remove: Vec::new(),
            hook_post_remove: Vec::new(),
            hook_post_cd: Vec::new(),
            default_remote: None,
            default_branch: None,
            track: TrackMode::Auto,
            editor_workspace: None,
            provider: None,
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

        // Lowest precedence: `gtr.*` keys in git config (migration aid).
        gtr_compat::apply_gtr_gitconfig(&mut cfg, repo);

        // Then gtr-compat files (.gtrconfig then .groveconfig).
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
        self.apply_git_scalars(repo)?;
        self.apply_git_copy(repo)?;
        self.apply_git_hooks(repo)?;
        Ok(())
    }

    /// Single-valued `grove.*` git-config keys.
    fn apply_git_scalars(&mut self, repo: &Repo) -> Result<()> {
        if let Some(v) = repo.config_get("grove.worktrees.dir")? {
            self.worktrees_dir = Some(v);
        }
        if let Some(v) = repo.config_get("grove.worktrees.prefix")? {
            self.worktrees_prefix = v;
        }
        if let Some(v) = repo.config_get("grove.editor.default")? {
            self.editor_default = Some(v);
        }
        if let Some(v) = repo.config_get("grove.editor.workspace")? {
            self.editor_workspace = Some(v);
        }
        if let Some(v) = repo.config_get("grove.ai.default")? {
            self.ai_default = Some(v);
        }
        if let Some(v) = repo.config_get("grove.ui.color")? {
            self.color = ColorChoice::parse(&v);
        }
        if let Some(v) = repo.config_get("grove.defaultRemote")? {
            self.default_remote = Some(v);
        }
        if let Some(v) = repo.config_get("grove.defaultBranch")? {
            self.default_branch = Some(v);
        }
        if let Some(v) = repo.config_get("grove.track")? {
            self.track = TrackMode::parse(&v);
        }
        if let Some(v) = repo.config_get("grove.provider")? {
            self.provider = Some(v);
        }
        Ok(())
    }

    /// Multi-valued `grove.copy.*` git-config keys.
    fn apply_git_copy(&mut self, repo: &Repo) -> Result<()> {
        set_if_present(
            &mut self.copy_include,
            repo.config_get_all("grove.copy.include")?,
        );
        set_if_present(
            &mut self.copy_exclude,
            repo.config_get_all("grove.copy.exclude")?,
        );
        set_if_present(
            &mut self.copy_include_dirs,
            repo.config_get_all("grove.copy.includeDirs")?,
        );
        set_if_present(
            &mut self.copy_exclude_dirs,
            repo.config_get_all("grove.copy.excludeDirs")?,
        );
        Ok(())
    }

    /// Multi-valued `grove.hooks.*` git-config keys.
    fn apply_git_hooks(&mut self, repo: &Repo) -> Result<()> {
        set_if_present(
            &mut self.hook_post_create,
            repo.config_get_all("grove.hooks.postCreate")?,
        );
        set_if_present(
            &mut self.hook_pre_remove,
            repo.config_get_all("grove.hooks.preRemove")?,
        );
        set_if_present(
            &mut self.hook_post_remove,
            repo.config_get_all("grove.hooks.postRemove")?,
        );
        set_if_present(
            &mut self.hook_post_cd,
            repo.config_get_all("grove.hooks.postCd")?,
        );
        Ok(())
    }

    /// The configured default remote, or `origin`.
    #[must_use]
    pub fn remote(&self) -> &str {
        self.default_remote.as_deref().unwrap_or("origin")
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

/// Overwrite `slot` with `values` only when the latter is non-empty, so a
/// lower-precedence source isn't clobbered by an unset multi-valued key.
fn set_if_present(slot: &mut Vec<String>, values: Vec<String>) {
    if !values.is_empty() {
        *slot = values;
    }
}

/// Default base dir: a sibling directory named `<repo>-worktrees`.
#[must_use]
pub fn default_worktrees_dir(root: &Path) -> PathBuf {
    let name = root.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
    let parent = root.parent().unwrap_or(root);
    parent.join(format!("{name}-worktrees"))
}
