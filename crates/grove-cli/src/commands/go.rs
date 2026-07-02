use anyhow::Result;
use bpaf::Bpaf;
use grove_core::{ForgeRef, Grove};

/// Print the path to a worktree (for `cd "$(grove go x)"`).
///
/// The argument is a *forge ref*: a branch/folder name, a `#42` / `pr/42`
/// shorthand, or a PR/branch/issue URL — resolved to the matching existing
/// worktree.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Go {
    /// Worktree name or forge ref (branch, `#42`, or a URL)
    #[bpaf(positional("NAME"), complete(crate::complete::worktree_names))]
    name: String,
}

pub fn execute(args: &Go) -> Result<()> {
    let grove = Grove::open()?;
    let path = grove.path_for_ref(&ForgeRef::parse(&args.name))?;
    println!("{}", path.display());
    Ok(())
}
