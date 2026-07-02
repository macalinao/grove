use std::collections::HashMap;

use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::{Forge, Grove, PrInfo, PrState, Worktree};
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
    /// The branch's pull request, when a forge + token resolved one.
    /// Absent with no forge, no token, no PR, or offline (best-effort).
    #[serde(skip_serializing_if = "Option::is_none")]
    pr: Option<PrJson>,
}

/// A worktree branch's pull/merge request in `grove list --json`.
#[derive(Serialize, Clone)]
struct PrJson {
    number: u64,
    /// `open` | `merged` | `closed`.
    state: &'static str,
    /// The PR's web URL, when the provider can build one.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

pub fn execute(args: List) -> Result<()> {
    let grove = Grove::open()?;

    if args.json {
        return print_json(&grove);
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

/// Emit the JSON array, annotating each worktree with its forge PR when one
/// resolves. Forge lookups are best-effort: no forge / token / network leaves
/// `pr` absent and the command still succeeds.
fn print_json(grove: &Grove) -> Result<()> {
    let worktrees = grove.list()?;
    let prs = collect_prs(grove, &worktrees);
    let items: Vec<WorktreeJson> = worktrees.into_iter().map(|w| to_json(w, &prs)).collect();
    println!("{}", serde_json::to_string_pretty(&items)?);
    Ok(())
}

/// Resolve each branch's PR in one concurrent forge query, keyed by branch.
///
/// Best-effort throughout: a missing/undetected forge, a forge that fails to
/// build, or a branch with no PR simply contributes no entry.
fn collect_prs(grove: &Grove, worktrees: &[Worktree]) -> HashMap<String, PrJson> {
    let Ok(Some(forge)) = grove.forge() else {
        return HashMap::new();
    };
    let branches: Vec<String> = worktrees.iter().filter_map(|w| w.branch.clone()).collect();
    let infos = forge.prs_for_branches(&branches);
    branches
        .into_iter()
        .zip(infos)
        .filter_map(|(branch, info)| Some((branch, pr_json(forge.as_ref(), &info?))))
        .collect()
}

/// Build a [`PrJson`] from a [`PrInfo`], resolving the web URL best-effort.
fn pr_json(forge: &dyn Forge, pr: &PrInfo) -> PrJson {
    PrJson {
        number: pr.number,
        state: pr_state_str(pr.state),
        url: forge.pr_url(pr.number).ok().map(|u| u.to_string()),
    }
}

fn pr_state_str(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    }
}

fn to_json(w: Worktree, prs: &HashMap<String, PrJson>) -> WorktreeJson {
    let pr = w.branch.as_deref().and_then(|b| prs.get(b)).cloned();
    WorktreeJson {
        path: w.path.display().to_string(),
        status: status_str(&w),
        branch: w.branch,
        head: w.head,
        bare: w.bare,
        detached: w.detached,
        locked: w.locked,
        prunable: w.prunable,
        pr,
    }
}
