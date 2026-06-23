use std::process::Command;

use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use grove_adapters::editor_argv;
use grove_core::Grove;

/// Open a worktree in an editor.
///
/// Uses `--editor` if given, else the `grove.editor.default` setting.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Editor {
    /// Branch or folder name of the worktree
    #[bpaf(positional("NAME"))]
    name: String,
    /// Editor adapter to use (overrides grove.editor.default)
    #[bpaf(long("editor"), argument("NAME"))]
    editor: Option<String>,
}

pub fn execute(args: Editor) -> Result<()> {
    let grove = Grove::open()?;
    let path = grove.path_for(&args.name)?;

    let editor = args
        .editor
        .or_else(|| grove.config.editor_default.clone())
        .ok_or_else(|| {
            anyhow!("no editor configured; set grove.editor.default or pass --editor <NAME>")
        })?;

    let mut command = editor_argv(&editor)?;
    command.push(path.to_string_lossy().into_owned());

    let (program, rest) = command
        .split_first()
        .ok_or_else(|| anyhow!("editor adapter '{editor}' has an empty command"))?;

    let status = Command::new(program)
        .args(rest)
        .status()
        .map_err(|e| anyhow!("failed to launch `{program}`: {e}"))?;

    std::process::exit(status.code().unwrap_or(1));
}
