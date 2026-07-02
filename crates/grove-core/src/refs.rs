//! Resolve a [`ForgeRef`] into a worktree: create one (for `grove new`) or find
//! the existing one (for `grove go` / `grove cd`).
//!
//! A PR ref becomes a worktree on the PR's head branch (named after the real
//! head, falling back to `pr-<n>`); the head is fetched via the provider's
//! `refs/pull/<n>/head` so fork and cross-repo PRs check out without adding a
//! remote, and the forge API is consulted only for nicer naming — everything
//! degrades gracefully when no token is available. An issue ref becomes a new
//! branch `<n>-<slug>` off the default base. A branch ref (or a self-describing
//! branch URL) behaves like today's plain-branch flow.

use std::path::PathBuf;

use grove_forge::{Forge, ForgeOptions, ForgeRef, ForgeUrl, ForgeUrlKind, Provider};

use crate::{CoreError, CreateOpts, Grove, Result, Worktree, slugify};

/// A boxed forge, when one could be built for the target.
type BoxForge = Option<Box<dyn Forge>>;

impl Grove {
    /// Create (or reuse) a worktree addressed by `r`, returning its path.
    ///
    /// # Errors
    /// Returns an error if forge lookup, fetching the PR head, or the underlying
    /// `git worktree add` fails.
    pub fn create_ref(&self, r: &ForgeRef, opts: &CreateOpts) -> Result<PathBuf> {
        match r {
            ForgeRef::Branch(b) => self.create(b, &branch_opts(opts, b, opts.fetch)),
            ForgeRef::Pr(n) => {
                let remote = self.config.remote().to_string();
                self.create_pr(self.forge()?, &remote, *n, opts)
            }
            ForgeRef::Issue(n) => self.create_issue(self.forge()?, *n, opts),
            ForgeRef::Url(u) => self.create_url(u, opts),
        }
    }

    /// Resolve `r` to an existing worktree path (for `go` / `cd`).
    ///
    /// # Errors
    /// Returns [`CoreError::NoWorktreeForRef`] when nothing matches, or a forge
    /// / git error while resolving.
    pub fn path_for_ref(&self, r: &ForgeRef) -> Result<PathBuf> {
        match r {
            ForgeRef::Branch(b) => self.path_for(b),
            ForgeRef::Pr(n) => self.find_pr_worktree(self.forge()?, *n),
            ForgeRef::Issue(n) => self.find_issue_worktree(self.forge()?, *n),
            ForgeRef::Url(u) => self.path_for_url(u),
        }
    }

    /// Create a worktree from a forge URL, dispatching on what it addresses.
    fn create_url(&self, u: &ForgeUrl, opts: &CreateOpts) -> Result<PathBuf> {
        match &u.kind {
            ForgeUrlKind::Branch(b) => self.create(b, &branch_opts(opts, b, opts.fetch)),
            ForgeUrlKind::Pr(n) => {
                let forge = self.forge_for_url(u)?;
                self.create_pr(forge, &clone_url(u), *n, opts)
            }
            ForgeUrlKind::Issue(n) => self.create_issue(self.forge_for_url(u)?, *n, opts),
        }
    }

    /// Create (or reuse) a worktree for PR `n`, fetching its head from
    /// `fetch_src` (a remote name or clone URL) when the branch isn't local yet.
    fn create_pr(
        &self,
        forge: BoxForge,
        fetch_src: &str,
        n: u64,
        opts: &CreateOpts,
    ) -> Result<PathBuf> {
        let info = forge.as_ref().and_then(|f| f.pr_by_number(n).ok());
        let provider = forge.as_ref().map_or(Provider::GitHub, |f| f.provider());
        let branch = info
            .map(|i| i.head)
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| format!("pr-{n}"));

        if let Some(w) = self.find(&branch)? {
            return Ok(w.path);
        }
        if !self.repo.branch_exists(&branch)? {
            let refspec = format!("{}:refs/heads/{branch}", provider.pull_ref(n));
            self.repo.fetch_ref(fetch_src, &refspec)?;
        }
        // The head branch now exists locally; check it out (no extra fetch).
        self.create(&branch, &branch_opts(opts, &branch, false))
    }

    /// Create a worktree on a new branch `<n>-<slug>` derived from the issue
    /// title, off the default base. Without a title the slug degrades to
    /// `issue-<n>`.
    fn create_issue(&self, forge: BoxForge, n: u64, opts: &CreateOpts) -> Result<PathBuf> {
        let branch = issue_branch(forge, n);
        self.create(&branch, &branch_opts(opts, &branch, opts.fetch))
    }

    /// Find the existing worktree for PR `n` by its head branch (from the API),
    /// falling back to a `pr-<n>` worktree.
    fn find_pr_worktree(&self, forge: BoxForge, n: u64) -> Result<PathBuf> {
        let head = forge
            .as_ref()
            .and_then(|f| f.pr_by_number(n).ok())
            .map(|i| i.head)
            .filter(|h| !h.is_empty());
        for name in head.into_iter().chain([format!("pr-{n}")]) {
            if let Some(w) = self.find(&name)? {
                return Ok(w.path);
            }
        }
        Err(CoreError::NoWorktreeForRef(format!("#{n}")))
    }

    /// Find the existing worktree for issue `n`: its `<n>-<slug>` branch (from
    /// the API) or any worktree whose branch/folder begins with `<n>-`.
    fn find_issue_worktree(&self, forge: BoxForge, n: u64) -> Result<PathBuf> {
        if let Some(issue) = forge.as_ref().and_then(|f| f.issue(n).ok()) {
            let branch = format!("{n}-{}", slugify(&issue.title));
            if let Some(w) = self.find(&branch)? {
                return Ok(w.path);
            }
        }
        let prefix = format!("{n}-");
        self.list()?
            .into_iter()
            .find(|w| starts_with_prefix(w, &prefix))
            .map(|w| w.path)
            .ok_or_else(|| CoreError::NoWorktreeForRef(format!("#{n}")))
    }

    /// Resolve a forge URL to an existing worktree path.
    fn path_for_url(&self, u: &ForgeUrl) -> Result<PathBuf> {
        match &u.kind {
            ForgeUrlKind::Branch(b) => self.path_for(b),
            ForgeUrlKind::Pr(n) => self.find_pr_worktree(self.forge_for_url(u)?, *n),
            ForgeUrlKind::Issue(n) => self.find_issue_worktree(self.forge_for_url(u)?, *n),
        }
    }

    /// Build a forge for a self-describing URL: its host + provider hint mean no
    /// `grove.provider` / `grove.forge.host` config is required.
    fn forge_for_url(&self, u: &ForgeUrl) -> Result<BoxForge> {
        let synthetic = format!("https://{}/{}/{}", u.host, u.owner, u.repo);
        let provider = u.provider.map(Provider::as_str);
        let opts = ForgeOptions {
            provider,
            host: Some(&u.host),
            token: self.config.forge_token.as_deref(),
        };
        Ok(grove_forge::build_forge(&synthetic, self.root(), &opts)?)
    }
}

/// The branch name for issue `n`: `<n>-<slug>` from its title, else `issue-<n>`
/// when no title is available (no forge, or the request failed / degraded).
fn issue_branch(forge: BoxForge, n: u64) -> String {
    let slug = forge
        .as_ref()
        .and_then(|f| f.issue(n).ok())
        .map(|i| slugify(&i.title))
        .filter(|s| !s.is_empty());
    match slug {
        Some(s) => format!("{n}-{s}"),
        None => format!("issue-{n}"),
    }
}

/// Whether a worktree's branch or folder name begins with `prefix`.
fn starts_with_prefix(w: &Worktree, prefix: &str) -> bool {
    w.branch.as_deref().is_some_and(|b| b.starts_with(prefix))
        || w.folder_name().is_some_and(|f| f.starts_with(prefix))
}

/// The https clone URL for a forge URL's repo (the source for a PR-head fetch).
fn clone_url(u: &ForgeUrl) -> String {
    format!("https://{}/{}/{}.git", u.host, u.owner, u.repo)
}

/// Clone `opts` but pin the branch (and whether to fetch first). Used to route a
/// resolved ref through the normal [`Grove::create`] path.
fn branch_opts(opts: &CreateOpts, branch: &str, fetch: bool) -> CreateOpts {
    CreateOpts {
        branch: Some(branch.to_string()),
        fetch,
        ..opts.clone()
    }
}
