//! Layered configuration for Grove.
//!
//! Sources, highest → lowest precedence (see the design spec §3.9):
//!   1. local `grove.kdl` (the native format)
//!   2. local + global git config `grove.*`
//!   3. `.groveconfig` (grove's own keys, INI format)
//!   4. `.gtrconfig` (gtr compat — gtr's on-disk schema)
//!   5. `gtr.*` git config (migration aid)
//!
//! All layers are live (see [`Config::load`]). `grove config migrate` converts
//! a `.gtrconfig` into a native `grove.kdl` via [`Config::from_gtrconfig`] +
//! [`Config::to_kdl`].

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
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// The canonical string form, the inverse of [`TrackMode::parse`].
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TrackMode::Auto => "auto",
            TrackMode::Remote => "remote",
            TrackMode::Local => "local",
            TrackMode::None => "none",
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

    /// The canonical string form, the inverse of [`ColorChoice::parse`].
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ColorChoice::Auto => "auto",
            ColorChoice::Always => "always",
            ColorChoice::Never => "never",
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

    /// Build a config from *only* a `.gtrconfig` file (gtr's on-disk schema),
    /// ignoring every other source. Used by `grove config migrate` to translate
    /// a gtr setup into a native `grove.kdl`.
    #[must_use]
    pub fn from_gtrconfig(path: &Path) -> Config {
        let mut cfg = Config::default();
        gtr_compat::apply_gtrconfig(&mut cfg, path);
        cfg
    }

    /// Serialize the config to a `grove.kdl` document, emitting only the blocks
    /// that carry non-default values. The output round-trips through the native
    /// `grove.kdl` reader.
    #[must_use]
    pub fn to_kdl(&self) -> String {
        let mut out = String::new();

        let mut worktrees = Vec::new();
        push_scalar(&mut worktrees, "dir", self.worktrees_dir.as_deref());
        if !self.worktrees_prefix.is_empty() {
            push_scalar(&mut worktrees, "prefix", Some(&self.worktrees_prefix));
        }
        push_block(&mut out, "worktrees", &worktrees);

        let mut editor = Vec::new();
        push_scalar(&mut editor, "default", self.editor_default.as_deref());
        push_scalar(&mut editor, "workspace", self.editor_workspace.as_deref());
        push_block(&mut out, "editor", &editor);

        let mut ai = Vec::new();
        push_scalar(&mut ai, "default", self.ai_default.as_deref());
        push_block(&mut out, "ai", &ai);

        let mut defaults = Vec::new();
        push_scalar(&mut defaults, "remote", self.default_remote.as_deref());
        push_scalar(&mut defaults, "branch", self.default_branch.as_deref());
        if self.track != TrackMode::Auto {
            push_scalar(&mut defaults, "track", Some(self.track.as_str()));
        }
        push_scalar(&mut defaults, "provider", self.provider.as_deref());
        push_block(&mut out, "defaults", &defaults);

        let mut ui = Vec::new();
        if self.color != ColorChoice::Auto {
            push_scalar(&mut ui, "color", Some(self.color.as_str()));
        }
        push_block(&mut out, "ui", &ui);

        let mut copy = Vec::new();
        push_list(&mut copy, "include", &self.copy_include);
        push_list(&mut copy, "exclude", &self.copy_exclude);
        push_list(&mut copy, "includeDirs", &self.copy_include_dirs);
        push_list(&mut copy, "excludeDirs", &self.copy_exclude_dirs);
        push_block(&mut out, "copy", &copy);

        let mut hooks = Vec::new();
        push_hook(&mut hooks, "postCreate", &self.hook_post_create);
        push_hook(&mut hooks, "preRemove", &self.hook_pre_remove);
        push_hook(&mut hooks, "postRemove", &self.hook_post_remove);
        push_hook(&mut hooks, "postCd", &self.hook_post_cd);
        push_block(&mut out, "hooks", &hooks);

        out
    }
}

/// Escape a string for use inside a KDL quoted string.
fn kdl_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Push a single-valued `name "value"` child line when `value` is set.
fn push_scalar(lines: &mut Vec<String>, name: &str, value: Option<&str>) {
    if let Some(v) = value {
        lines.push(format!("    {name} {}", kdl_quote(v)));
    }
}

/// Push a `name "a" "b" "c"` child line when `values` is non-empty.
fn push_list(lines: &mut Vec<String>, name: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    let args = values
        .iter()
        .map(|v| kdl_quote(v))
        .collect::<Vec<_>>()
        .join(" ");
    lines.push(format!("    {name} {args}"));
}

/// Push one `name "value"` child line per hook command, keeping each on its own
/// node so commands with spaces stay readable.
fn push_hook(lines: &mut Vec<String>, name: &str, values: &[String]) {
    for v in values {
        lines.push(format!("    {name} {}", kdl_quote(v)));
    }
}

/// Append a `name { ... }` block to `out` when it has any child `lines`.
fn push_block(out: &mut String, name: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(name);
    out.push_str(" {\n");
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("}\n");
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod kdl_roundtrip_tests {
    use super::*;

    /// Every field set to a non-default value, including values that need KDL
    /// escaping, must survive a `to_kdl` → reader round-trip unchanged.
    #[test]
    fn to_kdl_round_trips_through_reader() {
        let original = Config {
            worktrees_dir: Some("../wt".into()),
            worktrees_prefix: "wt-".into(),
            editor_default: Some("cursor".into()),
            ai_default: Some("claude".into()),
            color: ColorChoice::Never,
            copy_include: vec!["*.env".into(), ".env.local".into()],
            copy_exclude: vec!["secret.env".into()],
            copy_include_dirs: vec!["config".into()],
            copy_exclude_dirs: vec!["node_modules".into()],
            hook_post_create: vec!["pnpm install".into(), "echo \"done\\n\"".into()],
            hook_pre_remove: vec!["echo bye".into()],
            hook_post_remove: vec!["echo gone".into()],
            hook_post_cd: vec!["echo hi".into()],
            default_remote: Some("upstream".into()),
            default_branch: Some("main".into()),
            track: TrackMode::Remote,
            editor_workspace: Some("code".into()),
            provider: Some("github".into()),
        };

        let kdl = original.to_kdl();
        let mut parsed = Config::default();
        kdl_source::apply(&mut parsed, &kdl).expect("generated kdl must parse");

        assert_eq!(parsed, original);
    }

    /// A config with only defaults serializes to an empty document.
    #[test]
    fn default_config_serializes_empty() {
        assert!(Config::default().to_kdl().is_empty());
    }
}
