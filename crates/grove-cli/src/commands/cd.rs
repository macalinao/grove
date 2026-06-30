use anyhow::{Result, bail};
use bpaf::Bpaf;

/// Change directory to a worktree (requires shell integration).
///
/// Reaching the binary means the `grove` shell function isn't loaded — a binary
/// can't change its parent shell's directory, so this prints how to enable it.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Cd {
    /// Branch or folder name of the worktree
    #[bpaf(positional("NAME"), complete(crate::complete::worktree_names))]
    name: String,
}

pub fn execute(args: &Cd) -> Result<()> {
    let _ = &args.name;
    bail!(
        "`grove cd` needs shell integration (a binary can't cd its parent shell).\n\
         Add to your shell rc, then reload:\n  \
         bash/zsh:  eval \"$(grove init zsh)\"\n  \
         fish:      grove init fish | source\n\
         Or use `cd \"$(grove go {})\"`.",
        args.name
    );
}
