//! Native GitHub forge backed by `octocrab` (HTTP, no `gh` dependency).
//!
//! The [`Forge`] trait is synchronous, so [`GitHubForge`] owns a small
//! current-thread `tokio` runtime and `block_on`s the async `octocrab` calls
//! internally — callers stay sync. Requests use fully-qualified URLs
//! (`<api_base>/repos/...`) so a single client works against public GitHub,
//! GitHub Enterprise Server, and a local test server alike.

use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

use crate::{
    Forge, ForgeError, Issue, PrInfo, PrState, Provider, Result, Url, parse_web_url, web_base,
};

/// A [`Forge`] talking to GitHub's REST API via `octocrab`.
pub struct GitHubForge {
    client: Octocrab,
    api_base: String,
    web_base: String,
    owner: String,
    repo: String,
    runtime: Runtime,
}

impl GitHubForge {
    /// Build a client for `owner/repo` on `host`, authenticating with `token`
    /// when one was resolved.
    ///
    /// `host` may be a bare hostname (`github.com`, `ghe.myhost.com`) or a full
    /// base URL (`https://ghe.myhost.com`); it is normalized to a REST API base.
    ///
    /// # Errors
    /// Returns [`ForgeError::Request`] if the HTTP client or the runtime cannot
    /// be constructed.
    pub fn new(owner: &str, repo: &str, host: &str, token: Option<String>) -> Result<GitHubForge> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ForgeError::Request(e.to_string()))?;
        // `octocrab`'s tower `Buffer` spawns a worker task on construction, so
        // the client must be built inside the runtime context.
        let client = {
            let _guard = runtime.enter();
            let mut builder = Octocrab::builder();
            if let Some(t) = token.filter(|t| !t.is_empty()) {
                builder = builder.personal_token(t);
            }
            builder
                .build()
                .map_err(|e| ForgeError::Request(e.to_string()))?
        };
        Ok(GitHubForge {
            client,
            api_base: api_base(host),
            web_base: web_base(host),
            owner: owner.to_string(),
            repo: repo.to_string(),
            runtime,
        })
    }

    /// Fetch the most recent PR whose head is `branch`.
    async fn fetch_pull(&self, branch: &str) -> Result<Option<GhPull>> {
        let url = format!("{}/repos/{}/{}/pulls", self.api_base, self.owner, self.repo);
        let query = PullQuery {
            head: format!("{}:{}", self.owner, branch),
            state: "all",
            per_page: 1,
        };
        let pulls: Vec<GhPull> = self
            .client
            .get(&url, Some(&query))
            .await
            .map_err(|e| ForgeError::Request(e.to_string()))?;
        Ok(pulls.into_iter().next())
    }

    /// Fetch a single PR by number.
    async fn fetch_pull_by_number(&self, number: u64) -> Result<GhPull> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{number}",
            self.api_base, self.owner, self.repo
        );
        self.client
            .get(&url, None::<&()>)
            .await
            .map_err(|e| ForgeError::Request(e.to_string()))
    }

    /// Fetch a single issue by number.
    async fn fetch_issue(&self, number: u64) -> Result<GhIssue> {
        let url = format!(
            "{}/repos/{}/{}/issues/{number}",
            self.api_base, self.owner, self.repo
        );
        self.client
            .get(&url, None::<&()>)
            .await
            .map_err(|e| ForgeError::Request(e.to_string()))
    }
}

impl Forge for GitHubForge {
    fn provider(&self) -> Provider {
        Provider::GitHub
    }

    fn pr_for_branch(&self, branch: &str) -> Result<Option<PrInfo>> {
        let pull = self.runtime.block_on(self.fetch_pull(branch))?;
        Ok(pull.map(pr_info))
    }

    fn pr_by_number(&self, number: u64) -> Result<PrInfo> {
        let pull = self.runtime.block_on(self.fetch_pull_by_number(number))?;
        Ok(pr_info(pull))
    }

    fn issue(&self, number: u64) -> Result<Issue> {
        let issue = self.runtime.block_on(self.fetch_issue(number))?;
        Ok(Issue {
            number: issue.number,
            title: issue.title,
        })
    }

    fn compare_url(&self, base: &str, head: &str) -> Result<Url> {
        let raw = format!(
            "{}/{}/{}/compare/{base}...{head}?expand=1",
            self.web_base, self.owner, self.repo
        );
        parse_web_url(&raw)
    }

    fn branch_url(&self, branch: &str) -> Result<Url> {
        let raw = format!(
            "{}/{}/{}/tree/{branch}",
            self.web_base, self.owner, self.repo
        );
        parse_web_url(&raw)
    }

    fn pr_url(&self, number: u64) -> Result<Url> {
        let raw = format!(
            "{}/{}/{}/pull/{number}",
            self.web_base, self.owner, self.repo
        );
        parse_web_url(&raw)
    }
}

/// Query parameters for `GET /repos/{owner}/{repo}/pulls`.
#[derive(Serialize)]
struct PullQuery<'a> {
    head: String,
    state: &'a str,
    per_page: u8,
}

/// The subset of a pull-request object Grove needs.
#[derive(Deserialize)]
struct GhPull {
    #[serde(default)]
    number: u64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    base: GhRef,
    #[serde(default)]
    head: GhHead,
}

/// A pull request's `base` (target) ref — only the branch name is consumed.
#[derive(Deserialize, Default)]
struct GhRef {
    #[serde(rename = "ref", default)]
    ref_: String,
}

/// A pull request's `head` (source) ref plus its repository (for fork detection).
#[derive(Deserialize, Default)]
struct GhHead {
    #[serde(rename = "ref", default)]
    ref_: String,
    #[serde(default)]
    repo: Option<GhRepo>,
}

/// A repository object (`owner.login` + `name`).
#[derive(Deserialize, Default)]
struct GhRepo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    owner: GhOwner,
}

/// A repository owner (`login`).
#[derive(Deserialize, Default)]
struct GhOwner {
    #[serde(default)]
    login: String,
}

/// The subset of an issue object Grove needs.
#[derive(Deserialize)]
struct GhIssue {
    #[serde(default)]
    number: u64,
    #[serde(default)]
    title: String,
}

/// Map a GitHub pull object to a [`PrInfo`]. The endpoint reports `state` as
/// `open`/`closed`; a closed PR with a `merged_at` timestamp is merged.
fn pr_info(pull: GhPull) -> PrInfo {
    let state = if pull.state.eq_ignore_ascii_case("open") {
        PrState::Open
    } else if pull.merged_at.is_some() {
        PrState::Merged
    } else {
        PrState::Closed
    };
    let (head_owner, head_repo) = pull
        .head
        .repo
        .map(|r| (r.owner.login, r.name))
        .unwrap_or_default();
    PrInfo {
        number: pull.number,
        state,
        base: pull.base.ref_,
        head: pull.head.ref_,
        head_owner,
        head_repo,
        title: pull.title,
    }
}

/// Normalize a host or base URL to a REST API base:
/// `github.com` → `https://api.github.com`; an enterprise/self-hosted host →
/// `https://<host>/api/v3`; a full URL keeps its scheme (`http` for local test
/// servers) and gains `/api/v3` unless it already targets `api.github.com`.
fn api_base(host: &str) -> String {
    let host = host.trim_end_matches('/');
    if host == "github.com" {
        return "https://api.github.com".to_string();
    }
    if let Some((scheme, rest)) = split_scheme(host) {
        let authority = rest.split('/').next().unwrap_or(rest);
        if authority == "github.com" {
            return "https://api.github.com".to_string();
        }
        if authority == "api.github.com" {
            return format!("{scheme}://{authority}");
        }
        return format!("{scheme}://{authority}/api/v3");
    }
    format!("https://{host}/api/v3")
}

/// Split a URL into its `(scheme, remainder)` when it has one.
fn split_scheme(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    Some((scheme, rest))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn api_base_for_public_github() {
        assert_eq!(api_base("github.com"), "https://api.github.com");
        assert_eq!(api_base("https://github.com"), "https://api.github.com");
        assert_eq!(api_base("https://api.github.com"), "https://api.github.com");
    }

    #[test]
    fn api_base_for_enterprise_and_local() {
        assert_eq!(api_base("ghe.myhost.com"), "https://ghe.myhost.com/api/v3");
        assert_eq!(
            api_base("https://ghe.myhost.com"),
            "https://ghe.myhost.com/api/v3"
        );
        // A local test server keeps http and its port.
        assert_eq!(
            api_base("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080/api/v3"
        );
    }

    #[test]
    fn pr_info_distinguishes_merged_from_closed() {
        let merged: GhPull = serde_json::from_str(
            r#"{"state":"closed","merged_at":"2026-01-01T00:00:00Z","base":{"ref":"main"}}"#,
        )
        .unwrap();
        let merged = pr_info(merged);
        assert_eq!(merged.state, PrState::Merged);
        assert_eq!(merged.base, "main");

        let closed: GhPull = serde_json::from_str(r#"{"state":"closed"}"#).unwrap();
        assert_eq!(pr_info(closed).state, PrState::Closed);

        let open: GhPull = serde_json::from_str(r#"{"state":"open"}"#).unwrap();
        assert_eq!(pr_info(open).state, PrState::Open);
    }

    /// A recorded `GET /repos/{owner}/{repo}/pulls/{n}` body for a fork PR.
    const FORK_PULL: &str = r#"{
        "number": 42,
        "state": "open",
        "title": "Add widget support",
        "merged_at": null,
        "base": { "ref": "main" },
        "head": {
            "ref": "feat/widget",
            "repo": { "name": "demo", "owner": { "login": "contributor" } }
        }
    }"#;

    /// A recorded `GET /repos/{owner}/{repo}/issues/{n}` body.
    const ISSUE: &str = r#"{ "number": 88, "title": "Widgets should wobble" }"#;

    #[test]
    fn pr_by_number_reports_fork_head_and_title() {
        let server = crate::test_server::serve(&[("/pulls/", FORK_PULL), ("/issues/", ISSUE)]);
        let forge = GitHubForge::new("octo", "demo", &server.base, None).unwrap();

        let pr = forge.pr_by_number(42).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(pr.title, "Add widget support");
        assert_eq!(pr.base, "main");
        assert_eq!(pr.head, "feat/widget");
        // Fork detection: head owner differs from the repo owner.
        assert_eq!(pr.head_owner, "contributor");
        assert_eq!(pr.head_repo, "demo");
    }

    #[test]
    fn issue_reports_number_and_title() {
        let server = crate::test_server::serve(&[("/pulls/", FORK_PULL), ("/issues/", ISSUE)]);
        let forge = GitHubForge::new("octo", "demo", &server.base, None).unwrap();

        let issue = forge.issue(88).unwrap();
        assert_eq!(issue.number, 88);
        assert_eq!(issue.title, "Widgets should wobble");
    }

    #[test]
    fn compare_url_uses_web_base_with_expand() {
        let forge = GitHubForge::new("octo", "demo", "github.com", None).unwrap();
        let url = forge.compare_url("main", "feat/widget").unwrap();
        assert_eq!(
            url.as_str(),
            "https://github.com/octo/demo/compare/main...feat/widget?expand=1"
        );

        let ghe = GitHubForge::new("octo", "demo", "ghe.myhost.com", None).unwrap();
        assert_eq!(
            ghe.compare_url("main", "topic").unwrap().as_str(),
            "https://ghe.myhost.com/octo/demo/compare/main...topic?expand=1"
        );
    }

    #[test]
    fn branch_and_pr_urls_use_the_web_base() {
        let forge = GitHubForge::new("octo", "demo", "github.com", None).unwrap();
        assert_eq!(
            forge.branch_url("feat/widget").unwrap().as_str(),
            "https://github.com/octo/demo/tree/feat/widget"
        );
        assert_eq!(
            forge.pr_url(42).unwrap().as_str(),
            "https://github.com/octo/demo/pull/42"
        );

        let ghe = GitHubForge::new("octo", "demo", "ghe.myhost.com", None).unwrap();
        assert_eq!(
            ghe.pr_url(7).unwrap().as_str(),
            "https://ghe.myhost.com/octo/demo/pull/7"
        );
    }
}
