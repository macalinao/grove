use std::process::Command;

use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::Grove;

/// Diagnose the Grove environment.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Doctor {}

pub fn execute() -> Result<()> {
    let mut ok = true;
    let check = style("✓").green();
    let cross = style("✗").red();

    // git present?
    match Command::new("git").arg("--version").output() {
        Ok(o) if o.status.success() => {
            println!("{check} {}", String::from_utf8_lossy(&o.stdout).trim());
        }
        _ => {
            println!("{cross} git not found on PATH");
            ok = false;
        }
    }

    // inside a repo?
    match Grove::open() {
        Ok(grove) => {
            println!("{check} repository: {}", grove.root().display());
            println!("  worktrees dir: {}", grove.worktrees_dir().display());
            match grove.list() {
                Ok(wts) => println!("  worktrees: {}", wts.len()),
                Err(e) => {
                    println!("{cross} listing worktrees: {e}");
                    ok = false;
                }
            }
            let editor = grove.config.editor_default.as_deref().unwrap_or("(unset)");
            let ai = grove.config.ai_default.as_deref().unwrap_or("(unset)");
            println!("  default editor: {editor}");
            println!("  default ai: {ai}");
        }
        Err(e) => {
            println!("{cross} not inside a git repository ({e})");
            ok = false;
        }
    }

    if !ok {
        std::process::exit(1);
    }
    Ok(())
}
