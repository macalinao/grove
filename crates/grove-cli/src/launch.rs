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
/// Terminal editors that fork; launch in the background and don't wait.
const BACKGROUND_EDITORS: &[&str] = &["emacs"];
/// Editors launched as `cd <worktree> && <cmd> .` (gtr's `dot` feature).
const DOT_EDITORS: &[&str] = &["antigravity"];

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

/// Launch the named editor against `path`. Returns the exit code to propagate.
///
/// Honors gtr's adapter features: a `*.code-workspace` file is opened for
/// VS Code-style editors when present; `dot` editors run `cd <path> && cmd .`;
/// `background` editors are spawned without waiting. `none` is a no-op.
pub fn open_editor(grove: &Grove, name: &str, path: &Path) -> Result<i32> {
    if name == "none" {
        return Ok(0);
    }
    let argv = editor_command_argv(grove, name)?;
    let (program, fixed) = argv
        .split_first()
        .ok_or_else(|| anyhow!("editor adapter '{name}' has an empty command"))?;

    let mut cmd = Command::new(program);
    cmd.args(fixed);
    let workspace = WORKSPACE_EDITORS
        .contains(&name)
        .then(|| workspace_file(grove, path))
        .flatten();
    if let Some(ws) = workspace {
        cmd.arg(ws);
    } else if DOT_EDITORS.contains(&name) {
        cmd.current_dir(path).arg(".");
    } else {
        cmd.arg(path);
    }

    if BACKGROUND_EDITORS.contains(&name) {
        cmd.spawn()
            .map_err(|e| anyhow!("failed to launch `{program}`: {e}"))?;
        return Ok(0);
    }
    let status = cmd
        .status()
        .map_err(|e| anyhow!("failed to launch `{program}`: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

/// Launch the named AI tool with cwd set to `path`. Returns the exit code.
/// `none` is a no-op.
pub fn launch_ai(grove: &Grove, name: &str, path: &Path, extra: &[String]) -> Result<i32> {
    if name == "none" {
        return Ok(0);
    }
    let mut command = ai_command_argv(grove, name)?;
    command.extend_from_slice(extra);
    let (program, rest) = command
        .split_first()
        .ok_or_else(|| anyhow!("ai adapter '{name}' has an empty command"))?;
    let status = Command::new(program)
        .args(rest)
        .current_dir(path)
        .status()
        .map_err(|e| anyhow!("failed to launch `{program}`: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

/// Resolve an editor adapter to argv: a built-in, else a custom
/// `grove.editor.<name>.command` from config.
fn editor_command_argv(grove: &Grove, name: &str) -> Result<Vec<String>> {
    match editor_argv(name) {
        Ok(argv) => Ok(argv),
        Err(builtin_err) => custom_argv(grove, "editor", name)?.ok_or_else(|| anyhow!(builtin_err)),
    }
}

/// Resolve an AI adapter to argv: a built-in, else a custom
/// `grove.ai.<name>.command` from config.
fn ai_command_argv(grove: &Grove, name: &str) -> Result<Vec<String>> {
    match ai_argv(name) {
        Ok(argv) => Ok(argv),
        Err(builtin_err) => custom_argv(grove, "ai", name)?.ok_or_else(|| anyhow!(builtin_err)),
    }
}

/// Read a user-defined adapter command from `grove.<kind>.<name>.command` and
/// split it into argv on whitespace.
fn custom_argv(grove: &Grove, kind: &str, name: &str) -> Result<Option<Vec<String>>> {
    let key = format!("grove.{kind}.{name}.command");
    Ok(grove.repo.config_get(&key)?.and_then(|cmd| {
        let argv: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
        (!argv.is_empty()).then_some(argv)
    }))
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
