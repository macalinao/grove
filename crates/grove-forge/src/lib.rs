//! Native forge integration for Grove.
//!
//! M2 implements HTTP clients for **GitHub** and **Gitea** (plus GitLab) behind
//! the [`Forge`] trait — no `gh`/`glab` shell-out required. This powers
//! `clean --merged/--closed`, `new --pr/--issue`, and `grove pr`.
//!
//! Status: trait + types only; HTTP clients land in M2.

/// Errors from forge queries.
#[derive(thiserror::Error, Debug)]
pub enum ForgeError {
    #[error("no forge configured for this remote")]
    NotConfigured,

    #[error("forge request failed: {0}")]
    Request(String),
}

/// Convenience alias for fallible forge operations.
pub type Result<T> = core::result::Result<T, ForgeError>;

/// State of a pull/merge request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub number: u64,
    pub head_branch: String,
    pub state: PrState,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub number: u64,
    pub title: String,
}

/// Which hosting provider a remote points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    Gitea,
    GitLab,
}

/// A hosting provider Grove can query for PR/issue state.
pub trait Forge {
    fn provider(&self) -> Provider;
    /// The open/merged/closed PR whose head is `branch`, if any.
    fn pr_for_branch(&self, branch: &str) -> Result<Option<PullRequest>>;
    /// Fetch a PR by number (for `new --pr`).
    fn pr_by_number(&self, number: u64) -> Result<PullRequest>;
    /// Fetch an issue by number (for `new --issue`).
    fn issue(&self, number: u64) -> Result<Issue>;
    /// The browser URL to open a "create PR" page for `branch` against `base`.
    fn open_pr_url(&self, branch: &str, base: &str) -> String;
}
