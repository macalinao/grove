//! Forge refs: address a worktree by a PR/branch/issue URL, a `#n` / `pr/n`
//! shorthand, or a plain branch name.
//!
//! [`ForgeRef::parse`] is pure and infallible — anything it doesn't recognize
//! as a URL or a `#n`/`pr/n` shorthand falls through to [`ForgeRef::Branch`],
//! preserving today's "plain branch" behavior. A full URL is *self-describing*:
//! its path shape distinguishes GitHub from Gitea (`pull` vs `pulls`, `tree` vs
//! `src/branch`), so URL forms need no `grove.provider` / `grove.forge.host`
//! configuration. The resolution (fetching PR heads, creating branches, finding
//! existing worktrees) lives in `grove-core`; this module only parses.

use crate::Provider;

/// A parsed worktree address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeRef {
    /// A PR/MR number on the current repo's forge (`#42`, `pr/42`).
    Pr(u64),
    /// An issue number on the current repo's forge.
    Issue(u64),
    /// A plain branch name (today's behavior).
    Branch(String),
    /// A self-describing forge URL.
    Url(ForgeUrl),
}

/// A parsed forge URL: its host, `owner/repo`, the provider implied by the URL
/// shape, and what it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeUrl {
    /// Bare host (e.g. `github.com`, `gitea.myhost.com`, `127.0.0.1:8080`).
    pub host: String,
    /// Repository owner.
    pub owner: String,
    /// Repository name (without a trailing `.git`).
    pub repo: String,
    /// Provider implied by the URL shape, when the shape is provider-specific
    /// (`pull`/`tree` → GitHub, `pulls`/`src/branch` → Gitea). `issues` is
    /// shared, so it leaves this `None` and falls back to host detection.
    pub provider: Option<Provider>,
    /// What the URL addresses within the repo.
    pub kind: ForgeUrlKind,
}

/// What a [`ForgeUrl`] points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeUrlKind {
    /// A pull/merge request by number.
    Pr(u64),
    /// An issue by number.
    Issue(u64),
    /// A branch by name.
    Branch(String),
}

impl ForgeRef {
    /// Parse a worktree address. Never fails: an unrecognized string is a branch.
    #[must_use]
    pub fn parse(input: &str) -> ForgeRef {
        let s = input.trim();
        if let Some(rest) = s.strip_prefix('#') {
            if let Some(n) = leading_num(rest) {
                return ForgeRef::Pr(n);
            }
        }
        if let Some(n) = strip_ci_prefix(s, "pr/").and_then(leading_num) {
            return ForgeRef::Pr(n);
        }
        if is_url(s) {
            if let Some(url) = parse_url(s) {
                return ForgeRef::Url(url);
            }
        }
        ForgeRef::Branch(s.to_string())
    }
}

impl core::fmt::Display for ForgeRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ForgeRef::Pr(n) => write!(f, "#{n}"),
            ForgeRef::Issue(n) => write!(f, "issue #{n}"),
            ForgeRef::Branch(b) => f.write_str(b),
            ForgeRef::Url(u) => match &u.kind {
                ForgeUrlKind::Pr(n) => write!(f, "#{n}"),
                ForgeUrlKind::Issue(n) => write!(f, "issue #{n}"),
                ForgeUrlKind::Branch(b) => f.write_str(b),
            },
        }
    }
}

/// Whether `s` begins with an `http://` or `https://` scheme.
fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// The leading run of ASCII digits parsed as a `u64` (empty → `None`).
fn leading_num(s: &str) -> Option<u64> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Strip a case-insensitive `prefix`, returning the remainder.
fn strip_ci_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix))
        .then(|| &s[prefix.len()..])
}

/// Parse a forge URL into its host, `owner/repo`, provider hint, and target.
fn parse_url(s: &str) -> Option<ForgeUrl> {
    let (_scheme, rest) = s.split_once("://")?;
    // Drop any query string / fragment before splitting path segments.
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let mut segs = rest.split('/');
    let host = non_empty(segs.next())?.to_string();
    let owner = non_empty(segs.next())?.to_string();
    let repo_seg = non_empty(segs.next())?;
    let repo = repo_seg
        .strip_suffix(".git")
        .unwrap_or(repo_seg)
        .to_string();
    let (provider, kind) = parse_url_kind(&mut segs)?;
    Some(ForgeUrl {
        host,
        owner,
        repo,
        provider,
        kind,
    })
}

/// Interpret the path segments after `owner/repo` into a provider hint + kind.
fn parse_url_kind<'a>(
    segs: &mut impl Iterator<Item = &'a str>,
) -> Option<(Option<Provider>, ForgeUrlKind)> {
    match segs.next()? {
        "pull" => Some((
            Some(Provider::GitHub),
            ForgeUrlKind::Pr(leading_num(segs.next()?)?),
        )),
        "pulls" => Some((
            Some(Provider::Gitea),
            ForgeUrlKind::Pr(leading_num(segs.next()?)?),
        )),
        "issues" => Some((None, ForgeUrlKind::Issue(leading_num(segs.next()?)?))),
        "tree" => branch_kind(segs, Provider::GitHub),
        // Gitea branch URLs are `/src/branch/<branch>`.
        "src" if segs.next() == Some("branch") => branch_kind(segs, Provider::Gitea),
        _ => None,
    }
}

/// Collect the remaining segments as a (possibly slash-bearing) branch name.
fn branch_kind<'a>(
    segs: &mut impl Iterator<Item = &'a str>,
    provider: Provider,
) -> Option<(Option<Provider>, ForgeUrlKind)> {
    let branch = segs.collect::<Vec<_>>().join("/");
    (!branch.is_empty()).then_some((Some(provider), ForgeUrlKind::Branch(branch)))
}

/// A trimmed, non-empty view of an optional segment.
fn non_empty(seg: Option<&str>) -> Option<&str> {
    seg.filter(|s| !s.is_empty())
}

impl Provider {
    /// The canonical `grove.provider` override string for this provider.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::GitHub => "github",
            Provider::Gitea => "gitea",
            Provider::GitLab => "gitlab",
        }
    }

    /// The remote ref carrying a PR/MR head, e.g. `refs/pull/42/head`
    /// (GitHub/Gitea) or `refs/merge-requests/42/head` (GitLab). Fetching it
    /// directly makes fork/cross-repo checkout work without adding a remote.
    #[must_use]
    pub fn pull_ref(self, number: u64) -> String {
        match self {
            Provider::GitLab => format!("refs/merge-requests/{number}/head"),
            Provider::GitHub | Provider::Gitea => format!("refs/pull/{number}/head"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hash_and_pr_shorthand() {
        assert_eq!(ForgeRef::parse("#42"), ForgeRef::Pr(42));
        assert_eq!(ForgeRef::parse("pr/17"), ForgeRef::Pr(17));
        assert_eq!(ForgeRef::parse("PR/17"), ForgeRef::Pr(17));
        assert_eq!(ForgeRef::parse("  #7  "), ForgeRef::Pr(7));
    }

    #[test]
    fn plain_names_are_branches() {
        assert_eq!(
            ForgeRef::parse("feat/foo"),
            ForgeRef::Branch("feat/foo".to_string())
        );
        // A `#` not followed by digits is just a branch name.
        assert_eq!(
            ForgeRef::parse("#wip"),
            ForgeRef::Branch("#wip".to_string())
        );
        assert_eq!(
            ForgeRef::parse("release-1.2"),
            ForgeRef::Branch("release-1.2".to_string())
        );
    }

    #[test]
    fn parses_github_pr_url() {
        let ForgeRef::Url(u) = ForgeRef::parse("https://github.com/o/r/pull/42") else {
            panic!("expected a URL ref");
        };
        assert_eq!(u.host, "github.com");
        assert_eq!(u.owner, "o");
        assert_eq!(u.repo, "r");
        assert_eq!(u.provider, Some(Provider::GitHub));
        assert_eq!(u.kind, ForgeUrlKind::Pr(42));
    }

    #[test]
    fn parses_gitea_pr_url_plural() {
        let ForgeRef::Url(u) = ForgeRef::parse("https://gitea.myhost.com/o/r/pulls/17") else {
            panic!("expected a URL ref");
        };
        assert_eq!(u.host, "gitea.myhost.com");
        assert_eq!(u.provider, Some(Provider::Gitea));
        assert_eq!(u.kind, ForgeUrlKind::Pr(17));
    }

    #[test]
    fn parses_branch_urls_for_both_forges() {
        let ForgeRef::Url(gh) = ForgeRef::parse("https://github.com/o/r/tree/feat/foo") else {
            panic!("expected a URL ref");
        };
        assert_eq!(gh.provider, Some(Provider::GitHub));
        assert_eq!(gh.kind, ForgeUrlKind::Branch("feat/foo".to_string()));

        let ForgeRef::Url(gt) = ForgeRef::parse("https://gitea.myhost.com/o/r/src/branch/feat/foo")
        else {
            panic!("expected a URL ref");
        };
        assert_eq!(gt.provider, Some(Provider::Gitea));
        assert_eq!(gt.kind, ForgeUrlKind::Branch("feat/foo".to_string()));
    }

    #[test]
    fn parses_issue_url_without_provider_hint() {
        let ForgeRef::Url(u) = ForgeRef::parse("https://github.com/o/r/issues/88") else {
            panic!("expected a URL ref");
        };
        assert_eq!(u.kind, ForgeUrlKind::Issue(88));
        // `issues` is shared by both forges, so no provider is implied.
        assert_eq!(u.provider, None);
    }

    #[test]
    fn strips_git_suffix_query_and_trailing_path() {
        let ForgeRef::Url(u) = ForgeRef::parse("https://github.com/o/r.git/pull/42/files?w=1")
        else {
            panic!("expected a URL ref");
        };
        assert_eq!(u.repo, "r");
        assert_eq!(u.kind, ForgeUrlKind::Pr(42));
    }

    #[test]
    fn unrecognized_url_shapes_fall_back_to_branch() {
        // A URL we can't classify is kept verbatim as a branch name rather than
        // being silently dropped.
        assert_eq!(
            ForgeRef::parse("https://github.com/o/r/wiki"),
            ForgeRef::Branch("https://github.com/o/r/wiki".to_string())
        );
    }

    #[test]
    fn pull_ref_is_provider_specific() {
        assert_eq!(Provider::GitHub.pull_ref(42), "refs/pull/42/head");
        assert_eq!(Provider::Gitea.pull_ref(17), "refs/pull/17/head");
        assert_eq!(Provider::GitLab.pull_ref(5), "refs/merge-requests/5/head");
    }
}
