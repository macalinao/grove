//! Lifecycle hook execution (`postCreate`, `preRemove`, `postRemove`).
//!
//! Hooks are shell commands declared in config. Because config can come from a
//! committed `grove.kdl`, running them is gated behind the same trust check as
//! the task graph — an untrusted `grove.kdl` refuses to run until `grove trust`.
//! `postCd` is not run here: a binary can't affect its parent shell, so it is
//! emitted into the shell integration instead.

use std::path::Path;
use std::process::Command;

use anyhow::{Result, anyhow};
use console::style;
use grove_core::Grove;

/// Run each command in `commands` as `sh -c` inside `dir`, with the standard
/// hook environment (`REPO_ROOT`, `WORKTREE_PATH`, `BRANCH`).
///
/// No-op when `commands` is empty. The first failing command aborts and returns
/// an error; callers decide whether that is fatal (`postCreate`, `preRemove`)
/// or a warning (`postRemove`).
///
/// # Errors
/// Returns an error if the `grove.kdl` is untrusted, or if a hook command
/// cannot be spawned or exits non-zero.
pub fn run(
    grove: &Grove,
    label: &str,
    commands: &[String],
    dir: &Path,
    branch: &str,
) -> Result<()> {
    if commands.is_empty() {
        return Ok(());
    }
    ensure_trusted(grove)?;

    let repo_root = grove.root().to_string_lossy().into_owned();
    let worktree = dir.to_string_lossy().into_owned();
    for cmd in commands {
        eprintln!("{} {label}: {cmd}", style("›").dim());
        let status = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(dir)
            .env("REPO_ROOT", &repo_root)
            .env("WORKTREE_PATH", &worktree)
            .env("BRANCH", branch)
            .status()
            .map_err(|e| anyhow!("failed to run {label} hook `{cmd}`: {e}"))?;
        if !status.success() {
            return Err(anyhow!(
                "{label} hook failed (`{cmd}` exited with {})",
                status.code().unwrap_or(-1)
            ));
        }
    }
    Ok(())
}

/// Refuse to run hooks when a `grove.kdl` exists at the repo root and is not
/// trusted (commands defined only in git config still run, since no `grove.kdl`
/// means `is_trusted` is true).
fn ensure_trusted(grove: &Grove) -> Result<()> {
    let git_dir = grove.repo.git_common_dir()?;
    if grove_core::is_trusted(grove.root(), &git_dir) {
        return Ok(());
    }
    Err(anyhow!(
        "refusing to run untrusted hooks — review {} and run `grove trust` to approve",
        grove.root().join("grove.kdl").display()
    ))
}
