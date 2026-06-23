use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::{CreateOpts, Grove};

/// Create a new worktree (and branch).
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct New {
    /// Start point for a newly created branch
    #[bpaf(long, argument("REF"))]
    from: Option<String>,
    /// Override the worktree folder name
    #[bpaf(long, argument("NAME"))]
    folder: Option<String>,
    /// Allow a branch already checked out in another worktree
    #[bpaf(long, switch)]
    force: bool,
    /// Print the new worktree path on stdout (for shell integration)
    #[bpaf(long, switch)]
    print_path: bool,
    /// Branch name (also the worktree folder name, slashes sanitized)
    #[bpaf(positional("BRANCH"))]
    branch: String,
}

pub fn execute(args: New) -> Result<()> {
    let grove = Grove::open()?;
    let path = grove.create(
        &args.branch,
        &CreateOpts {
            branch: Some(args.branch.clone()),
            base: args.from,
            folder: args.folder,
            force: args.force,
        },
    )?;

    if args.print_path {
        println!("{}", path.display());
    } else {
        eprintln!(
            "{} created worktree {} at {}",
            style("✓").green(),
            style(&args.branch).bold(),
            path.display()
        );
    }
    Ok(())
}
