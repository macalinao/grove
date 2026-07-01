use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::Grove;

use crate::ui;

/// Rename a worktree and its branch.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Mv {
    /// Force the move even if the worktree is locked
    #[bpaf(long, switch)]
    force: bool,
    /// Skip the confirmation prompt
    #[bpaf(short('y'), long, switch)]
    yes: bool,
    /// Current branch or folder name
    #[bpaf(positional("OLD"), complete(crate::complete::worktree_names))]
    old: String,
    /// New name (branch + folder)
    #[bpaf(positional("NEW"))]
    new: String,
}

pub fn execute(args: &Mv) -> Result<()> {
    let grove = Grove::open()?;
    if !ui::confirm(&format!("Rename {} → {}?", args.old, args.new), args.yes)? {
        eprintln!("aborted");
        return Ok(());
    }
    let dest = grove.rename(&args.old, &args.new, args.force)?;
    eprintln!(
        "{} renamed {} → {} ({})",
        style("✓").green(),
        args.old,
        args.new,
        dest.display()
    );
    Ok(())
}
