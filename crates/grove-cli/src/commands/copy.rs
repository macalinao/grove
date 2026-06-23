use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use bpaf::Bpaf;
use console::style;
use grove_core::{Grove, Worktree, copy_files};

/// Copy untracked config/env files from one worktree into others by glob.
///
/// Patterns come from `grove.kdl` (`copy { include ...; exclude ... }`) or git
/// config (`grove.copy.include` / `grove.copy.exclude`). By default the source
/// is the main worktree.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Copy {
    /// Copy into all worktrees except the source
    #[bpaf(long, switch)]
    all: bool,
    /// Show what would be copied without writing anything
    #[bpaf(short('n'), long, switch)]
    dry_run: bool,
    /// Source worktree to copy from (default: the main worktree)
    #[bpaf(long, argument("NAME"))]
    from: Option<String>,
    /// Target worktree name(s) to copy into
    #[bpaf(positional("NAME"))]
    targets: Vec<String>,
}

pub fn execute(args: Copy) -> Result<()> {
    let grove = Grove::open()?;
    let source = source_path(&grove, args.from.as_deref())?;
    let targets = target_paths(&grove, &source, &args)?;

    if targets.is_empty() {
        bail!("no target worktrees; pass worktree name(s) or --all");
    }

    let include = &grove.config.copy_include;
    let exclude = &grove.config.copy_exclude;
    if include.is_empty() {
        eprintln!(
            "{} no copy patterns configured (set copy.include in grove.kdl or grove.copy.include)",
            style("!").yellow()
        );
        return Ok(());
    }

    for target in &targets {
        copy_into(&source, target, include, exclude, args.dry_run)?;
    }
    Ok(())
}

/// Resolve the source worktree path (main worktree, or `--from`).
fn source_path(grove: &Grove, from: Option<&str>) -> Result<PathBuf> {
    match from {
        Some(name) => Ok(grove.path_for(name)?),
        None => Ok(grove.root().to_path_buf()),
    }
}

/// Resolve target worktree paths from explicit names or `--all`.
fn target_paths(grove: &Grove, source: &Path, args: &Copy) -> Result<Vec<PathBuf>> {
    if args.all {
        return Ok(grove
            .list()?
            .into_iter()
            .map(|w: Worktree| w.path)
            .filter(|p| p != source)
            .collect());
    }
    let mut paths = Vec::with_capacity(args.targets.len());
    for name in &args.targets {
        paths.push(grove.path_for(name)?);
    }
    Ok(paths)
}

/// Run the copy into a single `target`, reporting each file.
fn copy_into(
    source: &Path,
    target: &Path,
    include: &[String],
    exclude: &[String],
    dry_run: bool,
) -> Result<()> {
    let copied = copy_files(source, target, include, exclude, dry_run)?;
    let verb = if dry_run { "would copy" } else { "copied" };
    println!("{} {}", style("→").cyan(), target.display());
    for rel in &copied {
        println!("  {} {}", style(verb).dim(), rel.display());
    }
    if copied.is_empty() {
        println!("  {}", style("nothing matched").dim());
    }
    Ok(())
}
