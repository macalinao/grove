//! Native Gitea forge backed by a thin `reqwest` client (Gitea `/api/v1`).
//!
//! Gitea has no ubiquitous CLI (`tea` is rarely installed), so — unlike the
//! GitLab path — it is served natively over HTTP. The [`Forge`] trait is
//! synchronous, so [`GiteaForge`] owns a small current-thread `tokio` runtime
//! and `block_on`s the async `reqwest` calls internally; callers stay sync.
//!
//! Gitea's list-pulls endpoint has no `head` filter (unlike GitHub), so
//! `pr_for_branch` lists recent PRs and matches the head ref client-side.
//! PR state derives from the `merged` bool plus the `state` string.

use reqwest::Client;
use serde::Deserialize;
use tokio::runtime::Runtime;

use crate::{Forge, ForgeError, Issue, PrInfo, PrState, Provider, Result, Url, web_base};

/// How many recent PRs to scan when matching a head branch.
const PULL_LIMIT: u32 = 50;

/// A [`Forge`] talking to a Gitea instance's REST API (`/api/v1`).
pub struct GiteaForge {
    client: Client,
    api_base: String,
    web_base: String,
    owner: String,
    repo: String,
    token: Option<String>,
    runtime: Runtime,
}

impl GiteaForge {
    /// Build a client for `owner/repo` on `host`, authenticating with `token`
    /// when one was resolved. `host` may be a bare hostname
    /// (`gitea.myhost.com`) or a full base URL (`https://gitea.myhost.com`);
    /// it is normalized to a `/api/v1` base. `insecure` disables TLS
    /// verification (mirroring a `tea` login's `insecure` flag).
    ///
    /// # Errors
    /// Returns [`ForgeError::Request`] if the HTTP client or the runtime cannot
    /// be constructed.
    pub fn new(
        owner: &str,
        repo: &str,
        host: &str,
        token: Option<String>,
        insecure: bool,
    ) -> Result<GiteaForge> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ForgeError::Request(e.to_string()))?;
        let client = build_client(insecure)?;
        Ok(GiteaForge {
            client,
            api_base: api_base(host),
            web_base: web_base(host),
            owner: owner.to_string(),
            repo: repo.to_string(),
            token: token.filter(|t| !t.is_empty()),
            runtime,
        })
    }

    /// Build an authenticated GET request for `url` (adds the `token` header).
    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        let request = self.client.get(url);
        match &self.token {
            Some(t) => request.header("Authorization", format!("token {t}")),
            None => request,
        }
    }

    /// Deserialize a JSON response, mapping transport/HTTP errors uniformly.
    async fn fetch<T: serde::de::DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<T> {
        request
            .send()
            .await
            .map_err(|e| ForgeError::Request(e.to_string()))?
            .error_for_status()
            .map_err(|e| ForgeError::Request(e.to_string()))?
            .json()
            .await
            .map_err(|e| ForgeError::Request(e.to_string()))
    }

    /// Fetch the most recent PR whose head branch is `branch`.
    async fn fetch_pull(&self, branch: &str) -> Result<Option<GtPull>> {
        let url = format!("{}/repos/{}/{}/pulls", self.api_base, self.owner, self.repo);
        let limit = PULL_LIMIT.to_string();
        let request = self.get(&url).query(&[
            ("state", "all"),
            ("sort", "recentupdate"),
            ("limit", limit.as_str()),
        ]);
        let pulls: Vec<GtPull> = Self::fetch(request).await?;
        Ok(pulls.into_iter().find(|p| p.head.ref_ == branch))
    }

    /// Fetch a single PR by number.
    async fn fetch_pull_by_number(&self, number: u64) -> Result<GtPull> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{number}",
            self.api_base, self.owner, self.repo
        );
        Self::fetch(self.get(&url)).await
    }

    /// Fetch a single issue by number.
    async fn fetch_issue(&self, number: u64) -> Result<GtIssue> {
        let url = format!(
            "{}/repos/{}/{}/issues/{number}",
            self.api_base, self.owner, self.repo
        );
        Self::fetch(self.get(&url)).await
    }
}

impl Forge for GiteaForge {
    fn provider(&self) -> Provider {
        Provider::Gitea
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
            "{}/{}/{}/compare/{base}...{head}",
            self.web_base, self.owner, self.repo
        );
        Url::parse(&raw).map_err(|e| ForgeError::Request(e.to_string()))
    }
}

/// Build the `reqwest` client, disabling TLS verification when `insecure`.
fn build_client(insecure: bool) -> Result<Client> {
    Client::builder()
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(|e| ForgeError::Request(e.to_string()))
}

/// The subset of a Gitea pull-request object Grove needs.
#[derive(Deserialize)]
struct GtPull {
    #[serde(default)]
    number: u64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    merged: bool,
    #[serde(default)]
    base: GtRef,
    #[serde(default)]
    head: GtHead,
}

/// A pull request's `base` ref (only the branch name is consumed).
#[derive(Deserialize, Default)]
struct GtRef {
    #[serde(rename = "ref", default)]
    ref_: String,
}

/// A pull request's `head` ref plus its repository (for fork detection).
#[derive(Deserialize, Default)]
struct GtHead {
    #[serde(rename = "ref", default)]
    ref_: String,
    #[serde(default)]
    repo: Option<GtRepo>,
}

/// A Gitea repository object (`owner.login` + `name`).
#[derive(Deserialize, Default)]
struct GtRepo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    owner: GtOwner,
}

/// A repository owner (`login`).
#[derive(Deserialize, Default)]
struct GtOwner {
    #[serde(default)]
    login: String,
}

/// The subset of a Gitea issue object Grove needs.
#[derive(Deserialize)]
struct GtIssue {
    #[serde(default)]
    number: u64,
    #[serde(default)]
    title: String,
}

/// Map a Gitea pull object to a [`PrInfo`]. Gitea reports `state` as
/// `open`/`closed` and a separate `merged` bool; a merged PR is `Merged` even
/// though its `state` is `closed`.
fn pr_info(pull: GtPull) -> PrInfo {
    let state = if pull.merged {
        PrState::Merged
    } else if pull.state.eq_ignore_ascii_case("closed") {
        PrState::Closed
    } else {
        PrState::Open
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

/// Normalize a host or base URL to a Gitea REST API base
/// (`https://<host>/api/v1`). A full URL keeps its scheme (`http` for local
/// test servers) and its authority.
pub(crate) fn api_base(host: &str) -> String {
    let host = host.trim_end_matches('/');
    if let Some((scheme, rest)) = host.split_once("://") {
        let authority = rest.split('/').next().unwrap_or(rest);
        return format!("{scheme}://{authority}/api/v1");
    }
    format!("https://{host}/api/v1")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn api_base_normalizes_hosts_and_urls() {
        assert_eq!(
            api_base("gitea.myhost.com"),
            "https://gitea.myhost.com/api/v1"
        );
        assert_eq!(
            api_base("https://gitea.myhost.com"),
            "https://gitea.myhost.com/api/v1"
        );
        assert_eq!(
            api_base("https://gitea.myhost.com/"),
            "https://gitea.myhost.com/api/v1"
        );
        // A local test server keeps http and its port.
        assert_eq!(
            api_base("http://127.0.0.1:3000"),
            "http://127.0.0.1:3000/api/v1"
        );
    }

    #[test]
    fn pr_info_uses_merged_bool_over_state() {
        let merged: GtPull =
            serde_json::from_str(r#"{"state":"closed","merged":true,"base":{"ref":"main"}}"#)
                .unwrap();
        let merged = pr_info(merged);
        assert_eq!(merged.state, PrState::Merged);
        assert_eq!(merged.base, "main");

        let closed: GtPull = serde_json::from_str(r#"{"state":"closed","merged":false}"#).unwrap();
        assert_eq!(pr_info(closed).state, PrState::Closed);

        let open: GtPull = serde_json::from_str(r#"{"state":"open","merged":false}"#).unwrap();
        assert_eq!(pr_info(open).state, PrState::Open);
    }

    /// A recorded `GET /repos/{owner}/{repo}/pulls/{n}` body for a fork PR.
    const FORK_PULL: &str = r#"{
        "number": 17,
        "state": "open",
        "title": "Improve docs",
        "merged": false,
        "base": { "ref": "main" },
        "head": {
            "ref": "docs/improve",
            "repo": { "name": "demo", "owner": { "login": "contributor" } }
        }
    }"#;

    /// A recorded `GET /repos/{owner}/{repo}/issues/{n}` body.
    const ISSUE: &str = r#"{ "number": 7, "title": "Typo in README" }"#;

    #[test]
    fn pr_by_number_reports_fork_head_and_title() {
        let server = crate::test_server::serve(&[("/pulls/", FORK_PULL), ("/issues/", ISSUE)]);
        let forge = GiteaForge::new("octo", "demo", &server.base, None, false).unwrap();

        let pr = forge.pr_by_number(17).unwrap();
        assert_eq!(pr.number, 17);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(pr.title, "Improve docs");
        assert_eq!(pr.base, "main");
        assert_eq!(pr.head, "docs/improve");
        assert_eq!(pr.head_owner, "contributor");
        assert_eq!(pr.head_repo, "demo");
    }

    #[test]
    fn issue_reports_number_and_title() {
        let server = crate::test_server::serve(&[("/pulls/", FORK_PULL), ("/issues/", ISSUE)]);
        let forge = GiteaForge::new("octo", "demo", &server.base, None, false).unwrap();

        let issue = forge.issue(7).unwrap();
        assert_eq!(issue.number, 7);
        assert_eq!(issue.title, "Typo in README");
    }

    #[test]
    fn compare_url_has_no_expand_query() {
        let forge = GiteaForge::new("octo", "demo", "gitea.myhost.com", None, false).unwrap();
        let url = forge.compare_url("main", "docs/improve").unwrap();
        assert_eq!(
            url.as_str(),
            "https://gitea.myhost.com/octo/demo/compare/main...docs/improve"
        );
    }
}
