//! Forge integration for Grove, backed by the `gh` / `glab` CLIs.
//!
//! Mirrors gtr: the provider is auto-detected from the `origin` URL (overridable
//! via `grove.provider`), and PR/MR state is queried through the official CLI
//! (`gh` for GitHub, `glab` for GitLab) — which the user has already
//! authenticated. This powers `grove clean --merged/--closed`.
//!
//! A native HTTP client (no CLI dependency) can replace [`CliForge`] later
//! behind the same [`Forge`] trait without touching callers.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Errors from forge queries.
#[derive(thiserror::Error, Debug)]
pub enum ForgeError {
    #[error("no forge detected for this remote (set grove.provider to github or gitlab)")]
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
    GitLab,
}

impl Provider {
    /// Parse a `grove.provider` override value.
    #[must_use]
    pub fn parse(s: &str) -> Option<Provider> {
        match s.to_ascii_lowercase().as_str() {
            "github" => Some(Provider::GitHub),
            "gitlab" => Some(Provider::GitLab),
            _ => None,
        }
    }

    /// The CLI this provider is queried through.
    #[must_use]
    pub fn cli(self) -> &'static str {
        match self {
            Provider::GitHub => "gh",
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

/// Detect the provider for `remote_url`, honoring an explicit `override_` first.
#[must_use]
pub fn detect(remote_url: &str, override_: Option<&str>) -> Option<Provider> {
    if let Some(p) = override_.and_then(Provider::parse) {
        return Some(p);
    }
    let host = host_of(remote_url);
    if host.contains("github") {
        Some(Provider::GitHub)
    } else if host.contains("gitlab") {
        Some(Provider::GitLab)
    } else {
        None
    }
}

/// Extract the host portion of a git remote URL (ssh or https form).
fn host_of(url: &str) -> String {
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
        let out = match self.provider {
            Provider::GitHub => self.run(&[
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
            ])?,
            Provider::GitLab => self.run(&[
                "mr",
                "list",
                "--source-branch",
                branch,
                "--all",
                "-P",
                "1",
                "-F",
                "tsv",
            ])?,
        };
        Ok(match self.provider {
            Provider::GitHub => parse_gh(&out),
            Provider::GitLab => parse_glab(&out),
        })
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
        assert_eq!(detect("https://example.com/o/r", None), None);
        // Explicit override wins over the URL.
        assert_eq!(
            detect("https://example.com/o/r", Some("github")),
            Some(Provider::GitHub)
        );
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
