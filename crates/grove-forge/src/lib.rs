//! Forge integration for Grove.
//!
//! The provider is auto-detected from the `origin` URL (overridable via
//! `grove.provider`). GitHub is served natively over HTTP by [`GitHubForge`]
//! (via `octocrab`, no `gh` dependency), while GitLab still shells out to
//! `glab` through [`CliForge`]. Tokens are discovered with zero login by
//! reusing the credentials `gh` / `tea` already store (see [`auth`]). This
//! powers `grove clean --merged/--closed`.
//!
//! Callers build a forge through [`build_forge`], which detects the provider
//! and returns a boxed [`Forge`]; the trait itself is synchronous.

use std::path::{Path, PathBuf};
use std::process::Command;

pub mod auth;
mod gitea;
mod github;

pub use auth::{TeaLogin, gitea_token, github_token, tea_login};
pub use gitea::GiteaForge;
pub use github::GitHubForge;

/// Errors from forge queries.
#[derive(thiserror::Error, Debug)]
pub enum ForgeError {
    #[error("no forge detected for this remote (set grove.provider to github, gitea, or gitlab)")]
    NotConfigured,

    #[error("the {0} CLI is required but was not found on PATH")]
    CliMissing(&'static str),

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

/// A pull/merge request's state and target (base) branch.
#[derive(Debug, Clone)]
pub struct PrInfo {
    pub state: PrState,
    pub base: String,
}

/// Which hosting provider a remote points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    Gitea,
    GitLab,
}

impl Provider {
    /// Parse a `grove.provider` override value.
    #[must_use]
    pub fn parse(s: &str) -> Option<Provider> {
        match s.to_ascii_lowercase().as_str() {
            "github" => Some(Provider::GitHub),
            "gitea" | "forgejo" | "codeberg" => Some(Provider::Gitea),
            "gitlab" => Some(Provider::GitLab),
            _ => None,
        }
    }

    /// The CLI associated with this provider. GitHub and Gitea are served
    /// natively over HTTP, so this only steers the [`CliForge`] (`glab`) path.
    #[must_use]
    pub fn cli(self) -> &'static str {
        match self {
            Provider::GitHub => "gh",
            Provider::Gitea => "tea",
            Provider::GitLab => "glab",
        }
    }
}

/// A hosting provider Grove can query for PR/MR state.
pub trait Forge {
    fn provider(&self) -> Provider;
    /// The most recent PR/MR whose head/source is `branch`, if any.
    ///
    /// # Errors
    /// Returns [`ForgeError`] if the provider CLI is missing or the query fails.
    fn pr_for_branch(&self, branch: &str) -> Result<Option<PrInfo>>;
}

/// Detect the provider for `remote_url`, honoring an explicit `override_` first,
/// then falling back to host-name heuristics on the remote.
///
/// This is pure (no network, no config reads); [`build_forge`] layers the
/// `tea`-login match and the `/api/v1/version` probe on top.
#[must_use]
pub fn detect(remote_url: &str, override_: Option<&str>) -> Option<Provider> {
    if let Some(p) = override_.and_then(Provider::parse) {
        return Some(p);
    }
    provider_from_host(&host_of(remote_url))
}

/// Guess a provider from a host name (`github`/`gitlab`/`gitea`-family).
fn provider_from_host(host: &str) -> Option<Provider> {
    if host.contains("github") {
        Some(Provider::GitHub)
    } else if host.contains("gitlab") {
        Some(Provider::GitLab)
    } else if host.contains("gitea") || host.contains("forgejo") || host.contains("codeberg") {
        Some(Provider::Gitea)
    } else {
        None
    }
}

/// Options that steer [`build_forge`], sourced from `grove.*` config.
#[derive(Debug, Default, Clone, Copy)]
pub struct ForgeOptions<'a> {
    /// Explicit provider override (`grove.provider`).
    pub provider: Option<&'a str>,
    /// Self-hosted forge base URL or host (`grove.forge.host`).
    pub host: Option<&'a str>,
    /// Explicit API token (`grove.forge.token`).
    pub token: Option<&'a str>,
}

/// Build a [`Forge`] for `remote_url`, or `None` when no provider is detected.
///
/// GitHub is served natively (`octocrab`); GitLab shells out to `glab` from
/// `dir`. GitHub tokens are resolved with zero login via [`auth::github_token`].
///
/// # Errors
/// Returns [`ForgeError`] if the GitHub client cannot be constructed (e.g. the
/// remote URL has no parseable `owner/repo`).
pub fn build_forge(
    remote_url: &str,
    dir: &Path,
    opts: &ForgeOptions,
) -> Result<Option<Box<dyn Forge>>> {
    let Some(provider) = resolve_provider(remote_url, opts) else {
        return Ok(None);
    };
    match provider {
        Provider::GitHub => Ok(Some(Box::new(build_github(remote_url, opts)?))),
        Provider::Gitea => Ok(Some(Box::new(build_gitea(remote_url, opts)?))),
        Provider::GitLab => Ok(Some(Box::new(CliForge::new(provider, dir)))),
    }
}

/// Resolve the provider for `remote_url` using every available signal, in
/// order: explicit override / host heuristic ([`detect`]), then a matching
/// `tea` login (which also supplies the host), then a `/api/v1/version` probe
/// of the configured/remote host (Gitea's version endpoint).
fn resolve_provider(remote_url: &str, opts: &ForgeOptions) -> Option<Provider> {
    if let Some(p) = detect(remote_url, opts.provider) {
        return Some(p);
    }
    let host = forge_host(remote_url, opts);
    if tea_login(&host).is_some() {
        return Some(Provider::Gitea);
    }
    probe_gitea(&host).then_some(Provider::Gitea)
}

/// The host to reach the forge at: `grove.forge.host` if set, else the remote.
fn forge_host(remote_url: &str, opts: &ForgeOptions) -> String {
    opts.host
        .filter(|h| !h.is_empty())
        .map_or_else(|| host_of(remote_url), host_of)
}

/// Probe `GET {host}/api/v1/version` — a Gitea-specific endpoint — returning
/// `true` when it answers successfully. Best-effort with a short timeout; any
/// error (unreachable host, non-Gitea server, timeout) yields `false`.
fn probe_gitea(host: &str) -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    runtime.block_on(async {
        let Ok(client) = reqwest::Client::builder()
            .timeout(core::time::Duration::from_secs(3))
            .build()
        else {
            return false;
        };
        let url = format!("{}/version", gitea::api_base(host));
        client
            .get(&url)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    })
}

/// Construct the native Gitea forge, resolving the base URL, token, and
/// `insecure` flag from `grove.*` config plus any matching `tea` login.
fn build_gitea(remote_url: &str, opts: &ForgeOptions) -> Result<GiteaForge> {
    let (owner, repo) = owner_repo(remote_url).ok_or(ForgeError::NotConfigured)?;
    let origin_host = host_of(remote_url);
    let login = tea_login(&origin_host);
    // Base host precedence: `grove.forge.host` → the `tea` login URL → origin.
    let host = opts
        .host
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .or_else(|| {
            login
                .as_ref()
                .map(|l| l.url.clone())
                .filter(|u| !u.is_empty())
        })
        .unwrap_or_else(|| origin_host.clone());
    let token = gitea_token(&origin_host, opts.token);
    let insecure = login.is_some_and(|l| l.insecure);
    GiteaForge::new(&owner, &repo, &host, token, insecure)
}

/// Construct the native GitHub forge, resolving auth from config + `gh` creds.
fn build_github(remote_url: &str, opts: &ForgeOptions) -> Result<GitHubForge> {
    let (owner, repo) = owner_repo(remote_url).ok_or(ForgeError::NotConfigured)?;
    let host = opts
        .host
        .map_or_else(|| host_of(remote_url), str::to_string);
    let token = github_token(&host, opts.token);
    GitHubForge::new(&owner, &repo, &host, token)
}

/// Extract `(owner, repo)` from a git remote URL (ssh or https form).
fn owner_repo(url: &str) -> Option<(String, String)> {
    let path = if let Some((_, rest)) = url.split_once("://") {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
        rest.split_once('/').map(|(_, p)| p.to_string())?
    } else {
        // scp-style: git@host:owner/repo.git
        url.split_once(':').map(|(_, p)| p.to_string())?
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.rsplit('/');
    let repo = parts.next().filter(|s| !s.is_empty())?;
    let owner = parts.next().filter(|s| !s.is_empty())?;
    Some((owner.to_string(), repo.to_string()))
}

/// Extract the host portion of a git remote URL (ssh or https form).
pub(crate) fn host_of(url: &str) -> String {
    // scp-style: git@github.com:owner/repo.git
    if let Some(rest) = url.split_once('@').map(|(_, r)| r) {
        if let Some((host, _)) = rest.split_once(':') {
            if !host.contains('/') {
                return host.to_ascii_lowercase();
            }
        }
    }
    // url-style: https://github.com/owner/repo.git
    if let Some((_, rest)) = url.split_once("://") {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
        return rest
            .split(['/', ':'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
    }
    url.to_ascii_lowercase()
}

/// A [`Forge`] backed by the provider's CLI (`gh` or `glab`), run in a repo dir.
pub struct CliForge {
    provider: Provider,
    dir: PathBuf,
}

impl CliForge {
    #[must_use]
    pub fn new(provider: Provider, dir: &Path) -> CliForge {
        CliForge {
            provider,
            dir: dir.to_path_buf(),
        }
    }

    /// Run the provider CLI, returning trimmed stdout.
    fn run(&self, args: &[&str]) -> Result<String> {
        let cli = self.provider.cli();
        let output = Command::new(cli)
            .args(args)
            .current_dir(&self.dir)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ForgeError::CliMissing(cli)
                } else {
                    ForgeError::Request(e.to_string())
                }
            })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(ForgeError::Request(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }
}

impl Forge for CliForge {
    fn provider(&self) -> Provider {
        self.provider
    }

    fn pr_for_branch(&self, branch: &str) -> Result<Option<PrInfo>> {
        match self.provider {
            Provider::GitHub => {
                let out = self.run(&[
                    "pr",
                    "list",
                    "--head",
                    branch,
                    "--state",
                    "all",
                    "-L",
                    "1",
                    "--json",
                    "state,baseRefName",
                    "--jq",
                    r#".[] | "\(.state)\t\(.baseRefName)""#,
                ])?;
                Ok(parse_gh(&out))
            }
            Provider::GitLab => {
                let out = self.run(&[
                    "mr",
                    "list",
                    "--source-branch",
                    branch,
                    "--all",
                    "-P",
                    "1",
                    "-F",
                    "tsv",
                ])?;
                Ok(parse_glab(&out))
            }
            // GitHub and Gitea are served natively; a `CliForge` is only ever
            // built for GitLab, so this arm is unreachable in practice.
            Provider::Gitea => Err(ForgeError::CliMissing("tea")),
        }
    }
}

/// Parse `gh`'s `STATE\tBASE` line (empty when there is no PR).
#[must_use]
pub fn parse_gh(out: &str) -> Option<PrInfo> {
    let line = out.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let (state, base) = line.split_once('\t').unwrap_or((line, ""));
    Some(PrInfo {
        state: parse_state(state),
        base: base.to_string(),
    })
}

/// Parse `glab mr list -F tsv` output (best-effort; columns vary by version).
#[must_use]
pub fn parse_glab(out: &str) -> Option<PrInfo> {
    let line = out.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let lower = line.to_ascii_lowercase();
    let state = if lower.contains("merged") {
        PrState::Merged
    } else if lower.contains("closed") {
        PrState::Closed
    } else {
        PrState::Open
    };
    // The target branch isn't reliably available in tsv output.
    Some(PrInfo {
        state,
        base: String::new(),
    })
}

/// Map a provider's state string to [`PrState`].
fn parse_state(s: &str) -> PrState {
    match s.to_ascii_uppercase().as_str() {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ => PrState::Open,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn detects_provider_from_urls() {
        assert_eq!(
            detect("git@github.com:o/r.git", None),
            Some(Provider::GitHub)
        );
        assert_eq!(
            detect("https://gitlab.com/o/r.git", None),
            Some(Provider::GitLab)
        );
        assert_eq!(
            detect("https://gitea.myhost.com/o/r.git", None),
            Some(Provider::Gitea)
        );
        assert_eq!(
            detect("git@codeberg.org:o/r.git", None),
            Some(Provider::Gitea)
        );
        assert_eq!(detect("https://example.com/o/r", None), None);
        // Explicit override wins over the URL.
        assert_eq!(
            detect("https://example.com/o/r", Some("github")),
            Some(Provider::GitHub)
        );
        assert_eq!(
            detect("https://example.com/o/r", Some("gitea")),
            Some(Provider::Gitea)
        );
    }

    #[test]
    fn parses_provider_override_values() {
        assert_eq!(Provider::parse("gitea"), Some(Provider::Gitea));
        assert_eq!(Provider::parse("Forgejo"), Some(Provider::Gitea));
        assert_eq!(Provider::parse("codeberg"), Some(Provider::Gitea));
        assert_eq!(Provider::parse("GitHub"), Some(Provider::GitHub));
        assert_eq!(Provider::parse("gitlab"), Some(Provider::GitLab));
        assert_eq!(Provider::parse("bitbucket"), None);
    }

    #[test]
    fn parses_owner_repo_from_urls() {
        assert_eq!(
            owner_repo("git@github.com:owner/repo.git"),
            Some(("owner".into(), "repo".into()))
        );
        assert_eq!(
            owner_repo("https://github.com/owner/repo.git"),
            Some(("owner".into(), "repo".into()))
        );
        assert_eq!(
            owner_repo("https://ghe.myhost.com/o/r"),
            Some(("o".into(), "r".into()))
        );
        assert_eq!(
            owner_repo("https://user@github.com/owner/repo.git"),
            Some(("owner".into(), "repo".into()))
        );
        assert_eq!(owner_repo("not-a-url"), None);
    }

    #[test]
    fn parses_gh_states() {
        assert!(parse_gh("").is_none());
        let merged = parse_gh("MERGED\tmain").unwrap();
        assert_eq!(merged.state, PrState::Merged);
        assert_eq!(merged.base, "main");
        assert_eq!(parse_gh("OPEN\tdevelop").unwrap().state, PrState::Open);
        assert_eq!(parse_gh("CLOSED\tmain").unwrap().state, PrState::Closed);
    }

    #[test]
    fn parses_glab_states() {
        assert!(parse_glab("").is_none());
        assert_eq!(
            parse_glab("!5\tfix\tmerged").unwrap().state,
            PrState::Merged
        );
        assert_eq!(parse_glab("!5\tfix\topen").unwrap().state, PrState::Open);
    }
}
