//! Per-branch database lifecycle (M4) — supersedes reviq's `dbbranch`.
//!
//! Each git branch/worktree gets its own database; `grove db ensure` hands the
//! shell a `DATABASE_URL` (clean-stdout contract) and migrates to head. Grove
//! supports **multiple, independently-engined** databases per worktree behind
//! the [`DbEngine`] trait — Postgres ships first (the `dbbranch` template-clone
//! model, reflink/PG18 fast path); MySQL/SQLite/Redis/… are additive.
//!
//! Status: trait + types only; the Postgres engine and `grove db` commands land
//! in M4. See design spec §7.

/// Errors from per-branch database operations.
#[derive(thiserror::Error, Debug)]
pub enum DbError {
    #[error("database engine error: {0}")]
    Engine(String),

    #[error("template '{0}' does not exist — run `grove db sync` first")]
    MissingTemplate(String),
}

/// Convenience alias for fallible db operations.
pub type Result<T> = core::result::Result<T, DbError>;

/// A provisioned branch database and the env it should export.
#[derive(Debug, Clone)]
pub struct DbHandle {
    /// Logical database name (e.g. `cache`) from the `database` config block.
    pub name: String,
    /// `(env_var, value)` to export, e.g. `("DATABASE_URL", "postgres://…")`.
    pub env: Vec<(String, String)>,
}

/// Options for [`DbEngine::reset`].
#[derive(Debug, Clone, Default)]
pub struct ResetOpts {
    /// Rebuild from migrations instead of re-cloning the template.
    pub schema: bool,
    pub force: bool,
}

/// A database backend that supports per-branch clones.
pub trait DbEngine {
    /// Create-if-missing (and migrate to head), returning the env to export.
    fn ensure(&self, branch: &str) -> Result<DbHandle>;
    /// The env for an existing branch DB, without creating it.
    fn env(&self, branch: &str) -> Result<DbHandle>;
    fn reset(&self, branch: &str, opts: &ResetOpts) -> Result<()>;
    fn drop(&self, branch: &str) -> Result<()>;
    /// Refresh the template/baseline from `from_ref` (auto-sync, §7.4).
    fn sync_template(&self, from_ref: &str) -> Result<()>;
    /// Is this branch's DB missing or behind the template head?
    fn is_stale(&self, branch: &str) -> Result<bool>;
}
