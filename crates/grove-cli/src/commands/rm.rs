use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::{Grove, RemoveOpts};

use crate::{hooks, ui};

/// Remove worktree(s).
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Rm {
    /// Also delete the associated branch
    #[bpaf(long, switch)]
    delete_branch: bool,
    /// Remove even with uncommitted changes
    #[bpaf(long, switch)]
    force: bool,
    /// Skip the confirmation prompt
    #[bpaf(short('y'), long, switch)]
    yes: bool,
    /// Branch or folder name(s) of the worktree(s) to remove
    #[bpaf(
        positional("NAME"),
        complete(crate::complete::worktree_names),
        some("expected at least one worktree name")
    )]
    names: Vec<String>,
}

pub fn execute(args: &Rm) -> Result<()> {
    let grove = Grove::open()?;

    let prompt = format!(
        "Remove {} worktree(s): {}?",
        args.names.len(),
        args.names.join(", ")
    );
    if !ui::confirm(&prompt, args.yes)? {
        eprintln!("aborted");
        return Ok(());
    }

    for name in &args.names {
        remove_one(&grove, name, args)?;
    }
    Ok(())
}

/// Remove a single worktree, running pre/post-remove hooks around it.
fn remove_one(grove: &Grove, name: &str, args: &Rm) -> Result<()> {
    let wt = grove
        .find(name)?
        .ok_or_else(|| anyhow::anyhow!("no worktree matching '{name}'"))?;
    let branch = wt.branch.clone().unwrap_or_default();

    // preRemove runs in the worktree before it is gone; a failure aborts the
    // removal unless --force was given.
    if let Err(e) = hooks::run(
        grove,
        "preRemove",
        &grove.config.hook_pre_remove,
        &wt.path,
        &branch,
    ) {
        if args.force {
            eprintln!("{} {e} (continuing: --force)", style("!").yellow());
        } else {
            return Err(e);
        }
    }

    grove.remove(
        name,
        &RemoveOpts {
            delete_branch: args.delete_branch,
            force: args.force,
        },
    )?;
    eprintln!("{} removed worktree {name}", style("✓").green());

    // postRemove runs in the main repo; failures are warnings, not errors.
    if let Err(e) = hooks::run(
        grove,
        "postRemove",
        &grove.config.hook_post_remove,
        grove.root(),
        &branch,
    ) {
        eprintln!("{} {e}", style("!").yellow());
    }
    Ok(())
}
