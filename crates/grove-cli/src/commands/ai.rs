use anyhow::Result;
use bpaf::Bpaf;
use grove_core::Grove;

use crate::launch;

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
    let tool = launch::ai_name(&grove, args.tool.as_deref())?;
    let status = launch::launch_ai(&grove, &tool, &path, &args.extra)?;
    std::process::exit(status.code().unwrap_or(1));
}
