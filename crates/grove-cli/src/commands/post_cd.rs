use anyhow::Result;
use bpaf::Bpaf;
use grove_core::Grove;

/// Emit `postCd` hook commands for the shell integration to `eval`.
///
/// Internal: the `grove` shell function runs this after `grove cd`/`grove new
/// --cd` changes directory, then `eval`s the output so hooks can mutate the
/// current shell (e.g. re-source env files). Output is empty unless the repo's
/// `grove.kdl` is trusted.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command("post-cd"), fallback_to_usage)]
pub struct PostCd {
    /// Worktree path that was just entered
    #[bpaf(positional("DIR"))]
    dir: String,
}

pub fn execute(args: &PostCd) -> Result<()> {
    let grove = Grove::open()?;
    let commands = &grove.config.hook_post_cd;
    if commands.is_empty() {
        return Ok(());
    }
    // Never emit commands for eval from an untrusted grove.kdl.
    let git_dir = grove.repo.git_common_dir()?;
    if !grove_core::is_trusted(grove.root(), &git_dir) {
        return Ok(());
    }

    let branch = grove.repo.current_branch()?.unwrap_or_default();
    println!(
        "export REPO_ROOT={}",
        shell_quote(&grove.root().to_string_lossy())
    );
    println!("export WORKTREE_PATH={}", shell_quote(&args.dir));
    println!("export BRANCH={}", shell_quote(&branch));
    for cmd in commands {
        println!("{cmd}");
    }
    Ok(())
}

/// Single-quote a value for safe inclusion in `eval`'d shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
