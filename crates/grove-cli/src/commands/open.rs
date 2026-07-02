use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use grove_core::Grove;

use crate::browser;

/// Open the current worktree's branch on the forge — its pull request page when
/// one exists, otherwise the branch page.
///
/// Looks up an open/merged/closed PR for the current branch (`pr_for_branch`)
/// and opens its page; if none is found (or the forge can't be reached) it
/// falls back to the branch's page.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Open {
    /// Print the URL instead of opening it in a browser
    #[bpaf(long, switch)]
    print: bool,
}

pub fn execute(args: &Open) -> Result<()> {
    let grove = Grove::open()?;
    let forge = grove.forge()?.ok_or_else(|| {
        anyhow!("no forge detected for the remote; set grove.provider to github, gitea, or gitlab")
    })?;
    let head = grove
        .repo
        .current_branch()?
        .ok_or_else(|| anyhow!("HEAD is detached; check out a branch first"))?;
    // Prefer the PR page; fall back to the branch page when there is no PR or
    // the forge query fails (e.g. offline), so `open` still points somewhere.
    let url = match forge.pr_for_branch(&head) {
        Ok(Some(pr)) if pr.number != 0 => forge.pr_url(pr.number)?,
        _ => forge.branch_url(&head)?,
    };
    browser::open_or_print(url.as_str(), args.print)
}
