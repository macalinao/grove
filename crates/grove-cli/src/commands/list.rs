use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::{Grove, Worktree};
use serde::Serialize;

/// List worktrees.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct List {
    /// Machine-readable output: `path⇥branch⇥status`, one worktree per line
    #[bpaf(long, switch)]
    porcelain: bool,
    /// Emit a JSON array of worktrees (for scripting; overrides --porcelain)
    #[bpaf(long, switch)]
    json: bool,
}

/// One worktree in `grove list --json` output — a stable schema for scripting.
#[derive(Serialize)]
struct WorktreeJson {
    path: String,
    branch: Option<String>,
    head: Option<String>,
    /// `ok` | `locked` | `prunable` | `detached` (mirrors `--porcelain`).
    status: &'static str,
    bare: bool,
    detached: bool,
    locked: bool,
    prunable: bool,
}

pub fn execute(args: List) -> Result<()> {
    let grove = Grove::open()?;

    if args.json {
        let items: Vec<WorktreeJson> = grove.list()?.into_iter().map(to_json).collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if args.porcelain {
        for w in grove.list()? {
            let branch = w.branch.as_deref().unwrap_or("");
            println!("{}\t{branch}\t{}", w.path.display(), status_str(&w));
        }
        return Ok(());
    }

    let worktrees = grove.list()?;
    let width = worktrees
        .iter()
        .filter_map(|w| w.branch.as_deref().map(str::len))
        .max()
        .unwrap_or(0)
        .max(6);

    for w in worktrees {
        // Pad first, then colour, so ANSI codes don't throw off alignment.
        let (name, is_branch) = match &w.branch {
            Some(b) => (b.clone(), true),
            None if w.detached => ("(detached)".to_owned(), false),
            None => ("(bare)".to_owned(), false),
        };
        let padded = format!("{name:<width$}");
        let label = if is_branch {
            style(padded).green()
        } else {
            style(padded).dim()
        };
        println!("{label}  {}", w.path.display());
    }
    Ok(())
}

/// gtr's status precedence: locked > prunable > detached > ok.
fn status_str(w: &Worktree) -> &'static str {
    if w.locked {
        "locked"
    } else if w.prunable {
        "prunable"
    } else if w.detached {
        "detached"
    } else {
        "ok"
    }
}

fn to_json(w: Worktree) -> WorktreeJson {
    WorktreeJson {
        path: w.path.display().to_string(),
        status: status_str(&w),
        branch: w.branch,
        head: w.head,
        bare: w.bare,
        detached: w.detached,
        locked: w.locked,
        prunable: w.prunable,
    }
}
