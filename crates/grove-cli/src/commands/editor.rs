use anyhow::Result;
use bpaf::Bpaf;
use grove_core::Grove;

use crate::launch;

/// Open a worktree in an editor.
///
/// Uses `--editor` if given, else the `grove.editor.default` setting. VS
/// Code-style editors open a `*.code-workspace` file when one is configured
/// (`grove.editor.workspace`) or found in the worktree.
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
    let editor = launch::editor_name(&grove, args.editor.as_deref())?;
    let code = launch::open_editor(&grove, &editor, &path)?;
    std::process::exit(code);
}
