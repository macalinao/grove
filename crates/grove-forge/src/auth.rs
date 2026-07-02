//! Zero-login token discovery: reuse `gh` / `tea` credentials as a token
//! *source* (never by parsing their command output).
//!
//! The public [`github_token`] / [`gitea_token`] resolvers mirror how the
//! upstream CLIs locate a token, so a user who has run `gh auth login` or
//! `tea login` never has to log into Grove. Both honor the `GROVE_FORGE_TOKEN`
//! env var and an explicit `grove.forge.token` as the highest-precedence
//! overrides. The file-parsing helpers take an explicit path so they stay pure
//! and testable against fixture config trees.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::host_of;

/// A single `tea` login entry (`logins[]` in `config.yml`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TeaLogin {
    /// Login nickname (`name`).
    #[serde(default)]
    pub name: String,
    /// Base URL of the Gitea instance; its host is matched against `origin`.
    #[serde(default)]
    pub url: String,
    /// API token.
    #[serde(default)]
    pub token: String,
    /// Whether TLS verification is disabled for this instance.
    #[serde(default)]
    pub insecure: bool,
}

/// Highest-precedence token: `GROVE_FORGE_TOKEN`, then an explicit config value.
fn override_token(explicit: Option<&str>) -> Option<String> {
    non_empty_env("GROVE_FORGE_TOKEN")
        .or_else(|| explicit.map(str::to_string).filter(|s| !s.is_empty()))
}

/// A `$VAR` value, treating unset and empty as absent.
fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// The user's home directory (`$HOME`), or an empty path when unset.
fn home_dir() -> PathBuf {
    non_empty_env("HOME").map_or_else(PathBuf::new, PathBuf::from)
}

// ---------------------------------------------------------------------------
// GitHub (go-gh semantics)
// ---------------------------------------------------------------------------

/// Resolve a GitHub token for `host`, mirroring `gh`'s `TokenForHost`.
///
/// Order: `GROVE_FORGE_TOKEN` / `explicit` → the `gh` env chain (public vs GHE)
/// → `hosts.yml` (honoring `GH_CONFIG_DIR` / `XDG_CONFIG_HOME`) → the
/// `gh auth token` subprocess (which also covers the OS-keyring case that
/// file-parsing cannot).
#[must_use]
pub fn github_token(host: &str, explicit: Option<&str>) -> Option<String> {
    if let Some(t) = override_token(explicit) {
        return Some(t);
    }
    let host = host_of(host);
    gh_env_token(&host)
        .or_else(|| gh_hosts_token(&gh_config_dir(), &host))
        .or_else(|| gh_cli_token(&host))
}

/// Whether `host` is treated as public GitHub (vs an enterprise instance).
fn is_public_github(host: &str) -> bool {
    host == "github.com" || host == "localhost" || host.starts_with("127.0.0.1")
}

/// The `gh` env-var chain for `host` (public vs enterprise variants).
fn gh_env_token(host: &str) -> Option<String> {
    let vars: [&str; 2] = if is_public_github(host) {
        ["GH_TOKEN", "GITHUB_TOKEN"]
    } else {
        ["GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"]
    };
    vars.iter().find_map(|v| non_empty_env(v))
}

/// The `gh` config directory: `GH_CONFIG_DIR` → `XDG_CONFIG_HOME/gh` →
/// `~/.config/gh`.
fn gh_config_dir() -> PathBuf {
    if let Some(d) = non_empty_env("GH_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    if let Some(x) = non_empty_env("XDG_CONFIG_HOME") {
        return Path::new(&x).join("gh");
    }
    home_dir().join(".config").join("gh")
}

/// The `oauth_token` for `host` from `<config_dir>/hosts.yml`, if present.
fn gh_hosts_token(config_dir: &Path, host: &str) -> Option<String> {
    let text = std::fs::read_to_string(config_dir.join("hosts.yml")).ok()?;
    gh_hosts_token_from_str(&text, host)
}

/// A `gh` host entry; unknown fields (`user`, `git_protocol`, …) are ignored.
#[derive(Deserialize)]
struct GhHost {
    #[serde(default)]
    oauth_token: Option<String>,
}

/// Parse `hosts.yml` text and return the `oauth_token` for `host`.
fn gh_hosts_token_from_str(yaml: &str, host: &str) -> Option<String> {
    let hosts: std::collections::HashMap<String, GhHost> = serde_yaml::from_str(yaml).ok()?;
    hosts
        .get(host)?
        .oauth_token
        .clone()
        .filter(|t| !t.is_empty())
}

/// The keyring/token fallback: `gh auth token --hostname <host>`.
fn gh_cli_token(host: &str) -> Option<String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token", "--hostname", host])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

// ---------------------------------------------------------------------------
// Gitea (tea semantics)
// ---------------------------------------------------------------------------

/// Resolve a Gitea token for `host`, mirroring `tea`'s config lookup.
///
/// Order: `GROVE_FORGE_TOKEN` / `explicit` → `GITEA_TOKEN` → the matching
/// `logins[]` entry in `tea`'s `config.yml` (XDG path, then the legacy
/// `~/.tea/tea.yml`), matched on `origin` host. There is no subprocess fallback
/// (`tea` stores plaintext and is often absent).
#[must_use]
pub fn gitea_token(host: &str, explicit: Option<&str>) -> Option<String> {
    if let Some(t) = override_token(explicit) {
        return Some(t);
    }
    if let Some(t) = non_empty_env("GITEA_TOKEN") {
        return Some(t);
    }
    let host = host_of(host);
    tea_login_for_host(&tea_logins(), &host).map(|l| l.token.clone())
}

/// The `tea` login whose URL host matches `host`, if any.
///
/// Detection and the native Gitea client use this to recover the instance's
/// base URL and its `insecure` TLS flag straight from a `tea login`, so a
/// Gitea repo often needs no `grove.forge.host` config at all.
#[must_use]
pub fn tea_login(host: &str) -> Option<TeaLogin> {
    let host = host_of(host);
    tea_login_for_host(&tea_logins(), &host).cloned()
}

/// The `tea` config search path: `XDG_CONFIG_HOME/tea/config.yml` (default
/// `~/.config/tea/config.yml`) then the legacy `~/.tea/tea.yml`.
fn tea_config_paths() -> Vec<PathBuf> {
    let primary = non_empty_env("XDG_CONFIG_HOME")
        .map_or_else(|| home_dir().join(".config"), PathBuf::from)
        .join("tea")
        .join("config.yml");
    vec![primary, home_dir().join(".tea").join("tea.yml")]
}

/// The `logins[]` from the first readable `tea` config file, if any.
fn tea_logins() -> Vec<TeaLogin> {
    for path in tea_config_paths() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            return tea_logins_from_str(&text);
        }
    }
    Vec::new()
}

/// The `tea` config document (only `logins` is consumed).
#[derive(Deserialize)]
struct TeaConfig {
    #[serde(default)]
    logins: Vec<TeaLogin>,
}

/// Parse `tea` `config.yml` text into its `logins`.
fn tea_logins_from_str(yaml: &str) -> Vec<TeaLogin> {
    serde_yaml::from_str::<TeaConfig>(yaml)
        .map(|c| c.logins)
        .unwrap_or_default()
}

/// The first login whose URL host matches `host` and carries a token.
fn tea_login_for_host<'a>(logins: &'a [TeaLogin], host: &str) -> Option<&'a TeaLogin> {
    logins
        .iter()
        .find(|l| !l.token.is_empty() && host_of(&l.url) == host)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("grove-auth-")
            .tempdir()
            .unwrap()
    }

    #[test]
    fn reads_github_token_from_hosts_yml_tree() {
        // A fixture `gh` config dir: <dir>/hosts.yml with two hosts.
        let dir = temp_dir();
        let yaml = "\
github.com:
    oauth_token: gho_public
    user: octocat
    git_protocol: https
ghe.myhost.com:
    oauth_token: ghe_enterprise
    user: admin
";
        std::fs::write(dir.path().join("hosts.yml"), yaml).unwrap();

        assert_eq!(
            gh_hosts_token(dir.path(), "github.com").as_deref(),
            Some("gho_public")
        );
        assert_eq!(
            gh_hosts_token(dir.path(), "ghe.myhost.com").as_deref(),
            Some("ghe_enterprise")
        );
        // An unknown host has no token.
        assert!(gh_hosts_token(dir.path(), "gitlab.com").is_none());
        // A missing file is a no-op (not an error).
        assert!(gh_hosts_token(temp_dir().path(), "github.com").is_none());
    }

    #[test]
    fn github_env_chain_is_host_aware() {
        assert!(is_public_github("github.com"));
        assert!(is_public_github("localhost"));
        assert!(!is_public_github("ghe.myhost.com"));
    }

    #[test]
    fn reads_gitea_token_from_tea_config_tree() {
        // A fixture `tea` config tree: <xdg>/tea/config.yml with two logins.
        let dir = temp_dir();
        let tea = dir.path().join("tea");
        std::fs::create_dir_all(&tea).unwrap();
        let yaml = "\
logins:
    - name: myhost
      url: https://gitea.myhost.com
      token: gitea_secret
      user: dev
      insecure: true
    - name: public
      url: https://github.com
      token: gh_secret
      insecure: false
";
        let mut f = std::fs::File::create(tea.join("config.yml")).unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let logins = tea_logins_from_str(yaml);
        assert_eq!(logins.len(), 2);

        let matched = tea_login_for_host(&logins, "gitea.myhost.com").unwrap();
        assert_eq!(matched.token, "gitea_secret");
        assert!(matched.insecure, "the insecure flag must round-trip");

        // Host matching ignores the scheme in the login URL.
        assert_eq!(
            tea_login_for_host(&logins, "github.com").map(|l| l.token.as_str()),
            Some("gh_secret")
        );
        // No login for an unknown host.
        assert!(tea_login_for_host(&logins, "gitlab.com").is_none());
    }

    #[test]
    fn tea_config_with_no_logins_is_empty() {
        assert!(tea_logins_from_str("logins: []").is_empty());
        assert!(tea_logins_from_str("# empty\n").is_empty());
        // Malformed YAML degrades to no logins rather than panicking.
        assert!(tea_logins_from_str("::: not yaml :::").is_empty());
    }
}
