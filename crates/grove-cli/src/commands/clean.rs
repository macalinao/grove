use core::time::Duration;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use console::style;
use grove_core::sweep::{self, Pace};
use grove_core::{Grove, PrInfo, PrState, RemoveOpts, Repo, Worktree, subvol};
use indicatif::{ProgressBar, ProgressStyle};

use crate::ui;

/// Remove worktrees whose pull/merge requests are merged or closed.
///
/// Detects the forge from the remote URL and queries `gh`/`glab` for each
/// worktree's branch. A worktree with uncommitted changes or commits on no
/// remote is kept (unless `--force`), and so is one a process works in.
/// Stale registry entries (missing directories) are pruned regardless of the
/// flags.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Clean {
    /// Remove worktrees whose PR/MR has been merged
    #[bpaf(long, switch)]
    merged: bool,
    /// Remove worktrees whose PR/MR has been closed without merging
    #[bpaf(long, switch)]
    closed: bool,
    /// Only consider PRs/MRs targeting this base ref
    #[bpaf(long, argument("REF"))]
    to: Option<String>,
    /// Preview without removing anything
    #[bpaf(short('n'), long, switch)]
    dry_run: bool,
    /// Skip the confirmation prompt
    #[bpaf(short('y'), long, switch)]
    yes: bool,
    /// Remove even with uncommitted changes or unpushed commits
    #[bpaf(short('f'), long, switch)]
    force: bool,
    /// Delete at idle IO priority, a chunk of files at a time (a big delete
    /// can stall btrfs for minutes): git-ignored files first, then the rest
    #[bpaf(long, switch)]
    slow: bool,
    /// In a worktree kept for its work, delete this git-ignored directory
    /// (e.g. `target`); repeatable
    #[bpaf(long, argument("DIR"), many)]
    trim: Vec<String>,
    /// Keep a worktree when this shell command exits 0 (run in the
    /// worktree with `WORKTREE_PATH`, `BRANCH` and `REPO_ROOT` set)
    #[bpaf(long("in-use"), argument("CMD"))]
    in_use: Option<String>,
}

/// A worktree whose PR/MR matches, and its PR.
struct Target {
    wt: Worktree,
    branch: String,
    pr: PrInfo,
}

/// What happens to a [`Target`].
enum Action {
    Remove,
    /// Kept for its work (why), but these git-ignored dirs go.
    Trim(Vec<PathBuf>, String),
    /// Kept (why).
    Keep(String),
}

pub fn execute(args: &Clean) -> Result<()> {
    let grove = Grove::open()?;
    // Always tidy stale registry entries (matches gtr).
    grove.repo.worktree_prune()?;

    if !args.merged && !args.closed {
        eprintln!(
            "pruned stale worktree entries; pass --merged and/or --closed to clean by PR state"
        );
        return Ok(());
    }

    let forge = grove.forge()?.ok_or_else(|| {
        anyhow!("no forge detected for the remote; set grove.provider to github, gitea, or gitlab")
    })?;

    let targets = collect_targets(&grove, forge.as_ref(), args)?;
    if targets.is_empty() {
        eprintln!("no worktrees match");
        return Ok(());
    }

    let plan: Vec<(Target, Action)> = targets
        .into_iter()
        .map(|t| {
            let action = decide(&grove, &t, args);
            (t, action)
        })
        .collect();
    for (t, action) in &plan {
        eprintln!(
            "  {} {} ({}) — {}",
            style("•").dim(),
            t.branch,
            describe(t.pr.state),
            t.wt.path.display()
        );
        match action {
            Action::Remove => {}
            Action::Trim(dirs, why) => eprintln!("      kept ({why}); trim {}", names(dirs)),
            Action::Keep(why) => eprintln!("      kept ({why})"),
        }
    }
    if args.dry_run {
        eprintln!("{} dry run: nothing removed", style("i").blue());
        return Ok(());
    }
    let removals = plan
        .iter()
        .filter(|(_, a)| matches!(a, Action::Remove))
        .count();
    let trims = plan
        .iter()
        .filter(|(_, a)| matches!(a, Action::Trim(..)))
        .count();
    if removals + trims == 0 {
        return Ok(());
    }
    let question = if trims == 0 {
        format!("Remove {removals} worktree(s)?")
    } else {
        format!("Remove {removals} worktree(s) and trim {trims}?")
    };
    if !ui::confirm(&question, args.yes)? {
        eprintln!("aborted");
        return Ok(());
    }
    if args.slow {
        sweep::idle_priority();
    }
    carry_out(&grove, &plan, args)
}

/// Remove and trim what the plan says, going on past failures.
fn carry_out(grove: &Grove, plan: &[(Target, Action)], args: &Clean) -> Result<()> {
    let pace = pace(args.slow);
    let mut failed = 0;
    for (t, action) in plan {
        let done = match action {
            Action::Keep(_) => continue,
            Action::Trim(dirs, _) => sweep::delete_slowly(dirs, pace)
                .map(|_| format!("trimmed {} in {}", names(dirs), t.branch))
                .map_err(|e| anyhow!("trimming {}: {e}", t.wt.path.display())),
            Action::Remove => {
                remove(grove, t, args, pace).map(|()| format!("removed {}", t.branch))
            }
        };
        match done {
            Ok(msg) => eprintln!("{} {msg}", style("✓").green()),
            Err(e) => {
                failed += 1;
                eprintln!("{} {}: {e:#}", style("✗").red(), t.branch);
            }
        }
    }
    if failed > 0 {
        return Err(anyhow!("{failed} worktree(s) could not be cleaned"));
    }
    Ok(())
}

/// Remove one worktree and its branch; with `--slow`, its git-ignored files
/// go first, a chunk at a time (a subvolume is one `btrfs subvolume delete`).
fn remove(grove: &Grove, t: &Target, args: &Clean, pace: Pace) -> Result<()> {
    if args.slow && !subvol::is_subvolume(&t.wt.path) {
        let ignored = Repo::discover_from(&t.wt.path)?.ignored_entries()?;
        let paths: Vec<PathBuf> = ignored.iter().map(|p| t.wt.path.join(p)).collect();
        sweep::delete_slowly(&paths, pace)?;
    }
    grove.remove(
        &t.branch,
        &RemoveOpts {
            delete_branch: true,
            force: args.force,
        },
    )?;
    Ok(())
}

/// Whether `t` goes: kept when locked, in use, or (without `--force`) holding
/// work, then trimming `--trim`'s git-ignored dirs.
fn decide(grove: &Grove, t: &Target, args: &Clean) -> Action {
    if t.wt.locked {
        return Action::Keep("locked".into());
    }
    if let Some(pid) = sweep::process_in(&t.wt.path) {
        return Action::Keep(format!("in use by pid {pid}"));
    }
    if let Some(cmd) = &args.in_use {
        match in_use(grove, t, cmd) {
            Ok(false) => {}
            Ok(true) => return Action::Keep(format!("in use (`{cmd}`)")),
            Err(e) => return Action::Keep(e),
        }
    }
    if args.force {
        return Action::Remove;
    }
    let why = match sweep::work_left(&t.wt.path, Some(&t.pr.head_sha)) {
        Ok(left) if left.is_empty() => return Action::Remove,
        Ok(left) => left.join(", "),
        Err(e) => return Action::Keep(e),
    };
    let dirs = trimmable(t, &args.trim);
    if dirs.is_empty() {
        Action::Keep(why)
    } else {
        Action::Trim(dirs, why)
    }
}

/// `--in-use CMD` for `t`: exit 0 in use, 1 not; anything else is why it
/// stays.
fn in_use(grove: &Grove, t: &Target, cmd: &str) -> Result<bool, String> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&t.wt.path)
        .env("REPO_ROOT", grove.root())
        .env("WORKTREE_PATH", &t.wt.path)
        .env("BRANCH", &t.branch)
        .status()
        .map_err(|e| format!("running `{cmd}`: {e}"))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => Err(format!(
            "`{cmd}` exited with {}",
            code.map_or_else(|| "a signal".into(), |c| c.to_string())
        )),
    }
}

/// The `--trim` dirs present in `t` that git ignores.
fn trimmable(t: &Target, dirs: &[String]) -> Vec<PathBuf> {
    let Ok(repo) = Repo::discover_from(&t.wt.path) else {
        return vec![];
    };
    dirs.iter()
        .map(|d| d.trim_matches('/'))
        .filter(|d| !d.is_empty() && t.wt.path.join(d).is_dir())
        .filter(|d| repo.is_ignored(d).unwrap_or(false))
        .map(|d| t.wt.path.join(d))
        .collect()
}

fn pace(slow: bool) -> Pace {
    if slow {
        Pace::default()
    } else {
        Pace {
            chunk: usize::MAX,
            pause: Duration::ZERO,
        }
    }
}

fn names(dirs: &[PathBuf]) -> String {
    dirs.iter()
        .filter_map(|d| d.file_name())
        .map(|n| format!("{}/", n.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Find worktrees whose PR/MR state matches the requested flags.
///
/// Each candidate branch means a `gh`/`glab` network round-trip, so a spinner
/// reports progress while they run (hidden when stderr isn't a terminal).
fn collect_targets(
    grove: &Grove,
    forge: &dyn grove_core::Forge,
    args: &Clean,
) -> Result<Vec<Target>> {
    let root = grove.root();
    // Candidates: every non-main worktree that has a branch to query.
    let candidates: Vec<(Worktree, String)> = grove
        .list()?
        .into_iter()
        .filter(|wt| wt.path != root)
        .filter_map(|wt| wt.branch.clone().map(|b| (wt, b)))
        .collect();

    let progress = progress_bar(candidates.len() as u64);
    let mut targets = Vec::new();
    for (wt, branch) in candidates {
        progress.set_message(branch.clone());
        let pr = forge.pr_for_branch(&branch)?;
        progress.inc(1);
        let Some(pr) = pr else {
            continue;
        };
        let wanted = (args.merged && pr.state == PrState::Merged)
            || (args.closed && pr.state == PrState::Closed);
        if !wanted {
            continue;
        }
        if let Some(to) = &args.to {
            if !pr.base.is_empty() && &pr.base != to {
                continue;
            }
        }
        targets.push(Target { wt, branch, pr });
    }
    progress.finish_and_clear();
    Ok(targets)
}

/// A spinner over `len` forge queries, or a hidden no-op bar when there's
/// nothing to show or stderr isn't a terminal (so piped output stays clean).
fn progress_bar(len: u64) -> ProgressBar {
    if len == 0 || !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::new(len);
    bar.set_style(
        ProgressStyle::with_template("{spinner:.blue} checking PRs {pos}/{len} {wide_msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

fn describe(state: PrState) -> &'static str {
    match state {
        PrState::Merged => "merged",
        PrState::Closed => "closed",
        PrState::Open => "open",
    }
}
