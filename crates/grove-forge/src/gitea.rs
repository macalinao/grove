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

use crate::{Forge, ForgeError, PrInfo, PrState, Provider, Result};

/// How many recent PRs to scan when matching a head branch.
const PULL_LIMIT: u32 = 50;

/// A [`Forge`] talking to a Gitea instance's REST API (`/api/v1`).
pub struct GiteaForge {
    client: Client,
    api_base: String,
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
            owner: owner.to_string(),
            repo: repo.to_string(),
            token: token.filter(|t| !t.is_empty()),
            runtime,
        })
    }

    /// Fetch the most recent PR whose head branch is `branch`.
    async fn fetch_pull(&self, branch: &str) -> Result<Option<GtPull>> {
        let url = format!("{}/repos/{}/{}/pulls", self.api_base, self.owner, self.repo);
        let limit = PULL_LIMIT.to_string();
        let request = self.client.get(&url).query(&[
            ("state", "all"),
            ("sort", "recentupdate"),
            ("limit", limit.as_str()),
        ]);
        let request = match &self.token {
            Some(t) => request.header("Authorization", format!("token {t}")),
            None => request,
        };
        let response = request
            .send()
            .await
            .map_err(|e| ForgeError::Request(e.to_string()))?
            .error_for_status()
            .map_err(|e| ForgeError::Request(e.to_string()))?;
        let pulls: Vec<GtPull> = response
            .json()
            .await
            .map_err(|e| ForgeError::Request(e.to_string()))?;
        Ok(pulls.into_iter().find(|p| p.head.ref_ == branch))
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
}

/// Build the `reqwest` client, disabling TLS verification when `insecure`.
fn build_client(insecure: bool) -> Result<Client> {
    Client::builder()
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(|e| ForgeError::Request(e.to_string()))
}

/// The subset of a Gitea pull-request object Grove needs from the list endpoint.
#[derive(Deserialize)]
struct GtPull {
    #[serde(default)]
    state: String,
    #[serde(default)]
    merged: bool,
    #[serde(default)]
    base: GtRef,
    #[serde(default)]
    head: GtRef,
}

/// A pull request's `base`/`head` ref (only the branch name is consumed).
#[derive(Deserialize, Default)]
struct GtRef {
    #[serde(rename = "ref", default)]
    ref_: String,
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
    PrInfo {
        state,
        base: pull.base.ref_,
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
        let merged = pr_info(GtPull {
            state: "closed".into(),
            merged: true,
            base: GtRef {
                ref_: "main".into(),
            },
            head: GtRef::default(),
        });
        assert_eq!(merged.state, PrState::Merged);
        assert_eq!(merged.base, "main");

        let closed = pr_info(GtPull {
            state: "closed".into(),
            merged: false,
            base: GtRef::default(),
            head: GtRef::default(),
        });
        assert_eq!(closed.state, PrState::Closed);

        let open = pr_info(GtPull {
            state: "open".into(),
            merged: false,
            base: GtRef::default(),
            head: GtRef::default(),
        });
        assert_eq!(open.state, PrState::Open);
    }
}
