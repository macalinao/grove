use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::Grove;

/// List worktrees.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct List {
    /// Machine-readable output: `path⇥branch⇥status`, one worktree per line
    #[bpaf(long, switch)]
    porcelain: bool,
}

pub fn execute(args: List) -> Result<()> {
    let grove = Grove::open()?;

    if args.porcelain {
        for w in grove.list()? {
            let branch = w.branch.as_deref().unwrap_or("");
            // Match gtr's status precedence: locked > prunable > detached > ok.
            let status = if w.locked {
                "locked"
            } else if w.prunable {
                "prunable"
            } else if w.detached {
                "detached"
            } else {
                "ok"
            };
            println!("{}\t{branch}\t{status}", w.path.display());
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
