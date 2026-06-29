use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use console::style;
use grove_core::{Grove, PrState, RemoveOpts};

use crate::ui;

/// Remove worktrees whose pull/merge requests are merged or closed.
///
/// Detects the forge from the remote URL and queries `gh`/`glab` for each
/// worktree's branch. Stale registry entries (missing directories) are pruned
/// regardless of the flags.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Clean {
    /// Remove worktrees whose PR/MR has been merged
    #[bpaf(long, switch)]
    merged: bool,
    /// Remove worktrees whose PR/MR has been closed without merging
    #[bpaf(long, switch)]
    closed: bool,
    /// Only consider PRs/MRs targeting this base ref
    #[bpaf(long, argument("REF"))]
    to: Option<String>,
    /// Preview without removing anything
    #[bpaf(short('n'), long, switch)]
    dry_run: bool,
    /// Skip the confirmation prompt
    #[bpaf(short('y'), long, switch)]
    yes: bool,
    /// Remove even with uncommitted changes
    #[bpaf(short('f'), long, switch)]
    force: bool,
}

pub fn execute(args: &Clean) -> Result<()> {
    let grove = Grove::open()?;
    // Always tidy stale registry entries (matches gtr).
    grove.repo.worktree_prune()?;

    if !args.merged && !args.closed {
        eprintln!(
            "pruned stale worktree entries; pass --merged and/or --closed to clean by PR state"
        );
        return Ok(());
    }

    let forge = grove.forge()?.ok_or_else(|| {
        anyhow!("no forge detected for the remote; set grove.provider to github or gitlab")
    })?;

    let targets = collect_targets(&grove, &forge, args)?;
    if targets.is_empty() {
        eprintln!("no worktrees match");
        return Ok(());
    }

    for (wt, branch, state) in &targets {
        eprintln!(
            "  {} {branch} ({}) — {}",
            style("•").dim(),
            describe(*state),
            wt.display()
        );
    }
    if args.dry_run {
        eprintln!("{} dry run: nothing removed", style("i").blue());
        return Ok(());
    }
    if !ui::confirm(&format!("Remove {} worktree(s)?", targets.len()), args.yes)? {
        eprintln!("aborted");
        return Ok(());
    }

    for (_, branch, _) in &targets {
        grove.remove(
            branch,
            &RemoveOpts {
                delete_branch: true,
                force: args.force,
            },
        )?;
        eprintln!("{} removed {branch}", style("✓").green());
    }
    Ok(())
}

/// Find worktrees whose PR/MR state matches the requested flags.
fn collect_targets(
    grove: &Grove,
    forge: &dyn grove_core::Forge,
    args: &Clean,
) -> Result<Vec<(std::path::PathBuf, String, PrState)>> {
    let root = grove.root();
    let mut targets = Vec::new();
    for wt in grove.list()? {
        // Never touch the main worktree.
        if wt.path == root {
            continue;
        }
        let Some(branch) = wt.branch.clone() else {
            continue;
        };
        let Some(pr) = forge.pr_for_branch(&branch)? else {
            continue;
        };
        let wanted = (args.merged && pr.state == PrState::Merged)
            || (args.closed && pr.state == PrState::Closed);
        if !wanted {
            continue;
        }
        if let Some(to) = &args.to {
            if !pr.base.is_empty() && &pr.base != to {
                continue;
            }
        }
        targets.push((wt.path, branch, pr.state));
    }
    Ok(targets)
}

fn describe(state: PrState) -> &'static str {
    match state {
        PrState::Merged => "merged",
        PrState::Closed => "closed",
        PrState::Open => "open",
    }
}
