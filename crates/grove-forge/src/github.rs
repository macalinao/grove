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

use crate::{Forge, ForgeError, PrInfo, PrState, Provider, Result};

/// A [`Forge`] talking to GitHub's REST API via `octocrab`.
pub struct GitHubForge {
    client: Octocrab,
    api_base: String,
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
}

impl Forge for GitHubForge {
    fn provider(&self) -> Provider {
        Provider::GitHub
    }

    fn pr_for_branch(&self, branch: &str) -> Result<Option<PrInfo>> {
        let pull = self.runtime.block_on(self.fetch_pull(branch))?;
        Ok(pull.map(pr_info))
    }
}

/// Query parameters for `GET /repos/{owner}/{repo}/pulls`.
#[derive(Serialize)]
struct PullQuery<'a> {
    head: String,
    state: &'a str,
    per_page: u8,
}

/// The subset of a pull-request object Grove needs from the list endpoint.
#[derive(Deserialize)]
struct GhPull {
    #[serde(default)]
    state: String,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    base: GhBase,
}

/// A pull request's `base` (target) ref.
#[derive(Deserialize, Default)]
struct GhBase {
    #[serde(rename = "ref", default)]
    ref_: String,
}

/// Map a GitHub pull object to a [`PrInfo`]. The list endpoint reports `state`
/// as `open`/`closed`; a closed PR with a `merged_at` timestamp is merged.
fn pr_info(pull: GhPull) -> PrInfo {
    let state = if pull.state.eq_ignore_ascii_case("open") {
        PrState::Open
    } else if pull.merged_at.is_some() {
        PrState::Merged
    } else {
        PrState::Closed
    };
    PrInfo {
        state,
        base: pull.base.ref_,
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
        let merged = pr_info(GhPull {
            state: "closed".into(),
            merged_at: Some("2026-01-01T00:00:00Z".into()),
            base: GhBase {
                ref_: "main".into(),
            },
        });
        assert_eq!(merged.state, PrState::Merged);
        assert_eq!(merged.base, "main");

        let closed = pr_info(GhPull {
            state: "closed".into(),
            merged_at: None,
            base: GhBase::default(),
        });
        assert_eq!(closed.state, PrState::Closed);

        let open = pr_info(GhPull {
            state: "open".into(),
            merged_at: None,
            base: GhBase::default(),
        });
        assert_eq!(open.state, PrState::Open);
    }
}
