use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::{Grove, RemoveOpts};

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
    /// Branch or folder name(s) of the worktree(s) to remove
    #[bpaf(positional("NAME"), some("expected at least one worktree name"))]
    names: Vec<String>,
}

pub fn execute(args: &Rm) -> Result<()> {
    let grove = Grove::open()?;
    for name in &args.names {
        grove.remove(
            name,
            &RemoveOpts {
                delete_branch: args.delete_branch,
                force: args.force,
            },
        )?;
        eprintln!("{} removed worktree {name}", style("✓").green());
    }
    Ok(())
}
