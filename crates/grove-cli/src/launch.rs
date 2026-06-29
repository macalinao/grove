//! Shared helpers for launching editor and AI-tool adapters against a worktree.
//!
//! Used by `grove editor`, `grove ai`, and `grove new -e/-a`. Editors receive
//! the worktree path (or a `*.code-workspace` file for VS Code-style editors)
//! as a trailing argument; AI tools run with the worktree as their cwd.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};
use grove_adapters::{ai_argv, editor_argv};
use grove_core::Grove;

/// Editors that understand `*.code-workspace` files.
const WORKSPACE_EDITORS: &[&str] = &["vscode", "code", "cursor", "antigravity", "windsurf"];

/// Resolve the editor adapter name (`--editor` override, else config).
pub fn editor_name(grove: &Grove, override_name: Option<&str>) -> Result<String> {
    override_name
        .map(str::to_string)
        .or_else(|| grove.config.editor_default.clone())
        .ok_or_else(|| {
            anyhow!("no editor configured; set grove.editor.default or pass --editor <NAME>")
        })
}

/// Resolve the AI adapter name (`--ai` override, else config).
pub fn ai_name(grove: &Grove, override_name: Option<&str>) -> Result<String> {
    override_name
        .map(str::to_string)
        .or_else(|| grove.config.ai_default.clone())
        .ok_or_else(|| anyhow!("no AI tool configured; set grove.ai.default or pass --ai <NAME>"))
}

/// Launch the named editor against `path`, waiting for it to exit.
///
/// For VS Code-style editors a workspace file is passed instead of the folder
/// when one is configured (`grove.editor.workspace`) or auto-detected.
pub fn open_editor(grove: &Grove, name: &str, path: &Path) -> Result<std::process::ExitStatus> {
    let mut command = editor_argv(name)?;
    command.push(editor_target(grove, name, path));
    let (program, rest) = command
        .split_first()
        .ok_or_else(|| anyhow!("editor adapter '{name}' has an empty command"))?;
    Command::new(program)
        .args(rest)
        .status()
        .map_err(|e| anyhow!("failed to launch `{program}`: {e}"))
}

/// Launch the named AI tool with cwd set to `path`, waiting for it to exit.
pub fn launch_ai(name: &str, path: &Path, extra: &[String]) -> Result<std::process::ExitStatus> {
    let mut command = ai_argv(name)?;
    command.extend_from_slice(extra);
    let (program, rest) = command
        .split_first()
        .ok_or_else(|| anyhow!("ai adapter '{name}' has an empty command"))?;
    Command::new(program)
        .args(rest)
        .current_dir(path)
        .status()
        .map_err(|e| anyhow!("failed to launch `{program}`: {e}"))
}

/// The trailing argument an editor opens: a workspace file when applicable,
/// otherwise the worktree directory.
fn editor_target(grove: &Grove, name: &str, path: &Path) -> String {
    if WORKSPACE_EDITORS.contains(&name) {
        if let Some(ws) = workspace_file(grove, path) {
            return ws.to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

/// Resolve a `*.code-workspace` file for `path`, honoring `grove.editor.workspace`
/// (`none` disables; a relative path selects a file; otherwise auto-detect one).
fn workspace_file(grove: &Grove, path: &Path) -> Option<PathBuf> {
    match grove.config.editor_workspace.as_deref() {
        Some("none") => None,
        Some(rel) => {
            let candidate = path.join(rel);
            candidate.is_file().then_some(candidate)
        }
        None => detect_workspace(path),
    }
}

/// Find the first `*.code-workspace` file directly under `path`.
fn detect_workspace(path: &Path) -> Option<PathBuf> {
    let mut hits: Vec<PathBuf> = std::fs::read_dir(path)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "code-workspace"))
        .collect();
    hits.sort();
    hits.into_iter().next()
}
