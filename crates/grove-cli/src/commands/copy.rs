use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use bpaf::Bpaf;
use console::style;
use grove_core::{CopySpec, Grove, Worktree, copy_into};

/// Copy untracked config/env files and directories from one worktree into
/// others.
///
/// Patterns come from `grove.kdl` (`copy { include; exclude; includeDirs;
/// excludeDirs }`), git config (`grove.copy.*`), and a `.worktreeinclude` file.
/// Directories are cloned copy-on-write where supported. The source defaults to
/// the main worktree.
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

    let spec = grove.copy_spec()?;
    if spec.is_empty() {
        eprintln!(
            "{} no copy patterns configured (set copy.include/includeDirs in grove.kdl, grove.copy.*, or .worktreeinclude)",
            style("!").yellow()
        );
        return Ok(());
    }

    for target in &targets {
        copy_one(&source, target, &spec, args.dry_run)?;
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

/// Run the copy into a single `target`, reporting each item.
fn copy_one(source: &Path, target: &Path, spec: &CopySpec, dry_run: bool) -> Result<()> {
    let copied = copy_into(source, target, spec, dry_run)?;
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
