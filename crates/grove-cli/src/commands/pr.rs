use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use grove_core::Grove;

use crate::browser;

/// Open the create-PR / compare page for the current worktree's branch.
///
/// Resolves the current branch (the PR head) and the base branch
/// (`grove.defaults.branch`, else the remote's default branch), then opens the
/// forge's compare URL in your browser. On GitHub the URL carries `?expand=1`
/// so the new-PR form is pre-filled; Gitea uses the plain compare page.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Pr {
    /// Print the URL instead of opening it in a browser
    #[bpaf(long, switch)]
    print: bool,
}

pub fn execute(args: &Pr) -> Result<()> {
    let grove = Grove::open()?;
    let forge = grove.forge()?.ok_or_else(|| {
        anyhow!("no forge detected for the remote; set grove.provider to github, gitea, or gitlab")
    })?;
    let head = grove
        .repo
        .current_branch()?
        .ok_or_else(|| anyhow!("HEAD is detached; check out a branch first"))?;
    let base = grove.base_branch()?.ok_or_else(|| {
        anyhow!("no base branch known; set grove.defaults.branch or a remote default branch")
    })?;
    let url = forge.compare_url(&base, &head)?;
    browser::open_or_print(url.as_str(), args.print)
}
