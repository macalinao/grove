use std::process::Command;

use anyhow::{Result, anyhow, bail};
use bpaf::Bpaf;
use grove_core::Grove;

/// Run a command inside a worktree.
///
/// Use `--` before commands that take their own flags:
/// `grove run feat -- cargo test --release`.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Run {
    /// Branch or folder name of the worktree
    #[bpaf(positional("NAME"), complete(crate::complete::worktree_names))]
    name: String,
    /// Command and arguments to run inside the worktree
    #[bpaf(any("COMMAND", Some), many)]
    command: Vec<String>,
}

pub fn execute(args: Run) -> Result<()> {
    let grove = Grove::open()?;
    let path = grove.path_for(&args.name)?;

    let Some((program, rest)) = args.command.split_first() else {
        bail!("no command given; usage: grove run <worktree> -- <command>...");
    };

    let status = Command::new(program)
        .args(rest)
        .current_dir(&path)
        .status()
        .map_err(|e| anyhow!("failed to run `{program}`: {e}"))?;

    std::process::exit(status.code().unwrap_or(1));
}
