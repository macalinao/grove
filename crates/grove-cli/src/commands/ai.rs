use std::process::Command;

use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use grove_adapters::ai_argv;
use grove_core::Grove;

/// Launch an AI tool inside a worktree.
///
/// Uses `--ai` if given, else the `grove.ai.default` setting. Pass extra
/// arguments after `--`: `grove ai feat -- --model sonnet`.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Ai {
    /// Branch or folder name of the worktree
    #[bpaf(positional("NAME"))]
    name: String,
    /// AI tool adapter to use (overrides grove.ai.default)
    #[bpaf(long("ai"), argument("NAME"))]
    tool: Option<String>,
    /// Extra arguments passed through to the AI tool
    #[bpaf(any("ARG", Some), many)]
    extra: Vec<String>,
}

pub fn execute(args: Ai) -> Result<()> {
    let grove = Grove::open()?;
    let path = grove.path_for(&args.name)?;

    let tool = args
        .tool
        .or_else(|| grove.config.ai_default.clone())
        .ok_or_else(|| {
            anyhow!("no AI tool configured; set grove.ai.default or pass --ai <NAME>")
        })?;

    let mut command = ai_argv(&tool)?;
    command.extend(args.extra);

    let (program, rest) = command
        .split_first()
        .ok_or_else(|| anyhow!("ai adapter '{tool}' has an empty command"))?;

    let status = Command::new(program)
        .args(rest)
        .current_dir(&path)
        .status()
        .map_err(|e| anyhow!("failed to launch `{program}`: {e}"))?;

    std::process::exit(status.code().unwrap_or(1));
}
