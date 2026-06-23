use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::Grove;

/// Rename a worktree and its branch.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Mv {
    /// Force the move even if the worktree is locked
    #[bpaf(long, switch)]
    force: bool,
    /// Current branch or folder name
    #[bpaf(positional("OLD"))]
    old: String,
    /// New name (branch + folder)
    #[bpaf(positional("NEW"))]
    new: String,
}

pub fn execute(args: &Mv) -> Result<()> {
    let grove = Grove::open()?;
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
