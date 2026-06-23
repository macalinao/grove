use anyhow::Result;
use bpaf::Bpaf;
use grove_core::Grove;

/// Print the path to a worktree (for `cd "$(grove go x)"`).
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Go {
    /// Branch or folder name of the worktree
    #[bpaf(positional("NAME"))]
    name: String,
}

pub fn execute(args: &Go) -> Result<()> {
    let grove = Grove::open()?;
    let path = grove.path_for(&args.name)?;
    println!("{}", path.display());
    Ok(())
}
