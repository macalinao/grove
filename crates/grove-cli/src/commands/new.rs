use anyhow::{Result, bail};
use bpaf::Bpaf;
use console::style;
use grove_core::{CopySpec, CreateOpts, ForgeRef, Grove, TrackMode, copy_into};
use grove_tasks::ExecOpts;

use crate::commands::tasks;
use crate::{hooks, launch};

/// Create a new worktree (and branch).
///
/// The positional argument is a *forge ref*: a plain branch name, a `#42` /
/// `pr/42` shorthand, or a full PR / branch / issue URL from GitHub or Gitea
/// (e.g. `https://github.com/o/r/pull/42`,
/// `https://gitea.myhost.com/o/r/src/branch/feat/foo`). A PR ref checks out the
/// PR head (fork PRs included); an issue ref opens a new `<n>-<slug>` branch.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct New {
    /// Start point for a newly created branch
    #[bpaf(long, argument("REF"))]
    from: Option<String>,
    /// Base the new branch on the current HEAD instead of the default branch
    #[bpaf(long)]
    from_current: bool,
    /// Remote for default base refs and tracking (default: grove.defaultRemote or origin)
    #[bpaf(long, argument("NAME"))]
    remote: Option<String>,
    /// Upstream tracking mode: auto, remote, local, none
    #[bpaf(long, argument("MODE"))]
    track: Option<String>,
    /// Override the worktree folder name
    #[bpaf(long, argument("NAME"))]
    folder: Option<String>,
    /// Folder-name suffix (appended to the sanitized branch name)
    #[bpaf(long, argument("SUFFIX"))]
    name: Option<String>,
    /// Allow a branch already checked out in another worktree
    #[bpaf(long, switch)]
    force: bool,
    /// Skip fetching the remote before creating
    #[bpaf(long, switch)]
    no_fetch: bool,
    /// Skip copying configured files into the new worktree
    #[bpaf(long, switch)]
    no_copy: bool,
    /// Skip running postCreate hooks
    #[bpaf(long, switch)]
    no_hooks: bool,
    /// Skip running the grove.kdl task graph after creation
    #[bpaf(long, switch)]
    no_tasks: bool,
    /// Open the new worktree in an editor after creating it
    #[bpaf(short('e'), long, switch)]
    editor: bool,
    /// Launch an AI tool in the new worktree after creating it
    #[bpaf(short('a'), long, switch)]
    ai: bool,
    /// Print the new worktree path on stdout (for shell integration)
    #[bpaf(long, switch)]
    print_path: bool,
    /// Create and cd into the worktree (requires shell integration; prints path)
    #[bpaf(long, switch)]
    cd: bool,
    /// Create a worktree from PR number N (alias for a `#N` ref)
    #[bpaf(long, argument("N"))]
    pr: Option<u64>,
    /// Create a `<N>-<slug>` branch from issue number N
    #[bpaf(long, argument("N"))]
    issue: Option<u64>,
    /// Forge ref: a branch name, `#42` / `pr/42`, or a PR/branch/issue URL
    #[bpaf(positional("REF"))]
    reference: Option<String>,
}

pub fn execute(args: New) -> Result<()> {
    let grove = Grove::open()?;
    let forge_ref = build_ref(&args)?;
    let path = grove.create_ref(&forge_ref, &create_opts(&args))?;

    // `--cd` prints the path (on stdout) so the shell wrapper can cd into it.
    if args.print_path || args.cd {
        println!("{}", path.display());
    } else {
        eprintln!(
            "{} created worktree {} at {}",
            style("✓").green(),
            style(&forge_ref.to_string()).bold(),
            path.display()
        );
    }

    let branch = resolved_branch(&grove, &path, &forge_ref);
    if !args.no_copy {
        copy_configured(&grove, &path)?;
    }
    if !args.no_hooks {
        hooks::run(
            &grove,
            "postCreate",
            &grove.config.hook_post_create,
            &path,
            &branch,
        )?;
    }
    if !args.no_tasks {
        tasks::run_for(&path, ExecOpts::default())?;
    }
    if args.editor {
        let name = launch::editor_name(&grove, None)?;
        launch::open_editor(&grove, &name, &path)?;
    }
    if args.ai {
        let name = launch::ai_name(&grove, None)?;
        launch::launch_ai(&grove, &name, &path, &[])?;
    }
    Ok(())
}

/// Build the [`ForgeRef`] from `--pr` / `--issue` (aliases) or the positional
/// argument. Exactly one source must be present.
fn build_ref(args: &New) -> Result<ForgeRef> {
    match (args.pr, args.issue, args.reference.as_deref()) {
        (Some(n), None, None) => Ok(ForgeRef::Pr(n)),
        (None, Some(n), None) => Ok(ForgeRef::Issue(n)),
        (None, None, Some(r)) => Ok(ForgeRef::parse(r)),
        (None, None, None) => bail!("provide a branch/ref, or --pr N, or --issue N"),
        _ => bail!("--pr, --issue, and a positional ref are mutually exclusive"),
    }
}

/// Assemble [`CreateOpts`] from the parsed flags (branch is set per-ref later).
fn create_opts(args: &New) -> CreateOpts {
    CreateOpts {
        branch: None,
        base: args.from.clone(),
        from_current: args.from_current,
        folder: args.folder.clone(),
        name: args.name.clone(),
        force: args.force,
        fetch: !args.no_fetch,
        remote: args.remote.clone(),
        track: args
            .track
            .as_deref()
            .map_or(TrackMode::Auto, TrackMode::parse),
    }
}

/// The branch checked out at `path` (for the hook environment), falling back to
/// the ref's display form when the worktree can't be re-read.
fn resolved_branch(grove: &Grove, path: &std::path::Path, forge_ref: &ForgeRef) -> String {
    grove
        .list()
        .ok()
        .into_iter()
        .flatten()
        .find(|w| w.path == path)
        .and_then(|w| w.branch)
        .unwrap_or_else(|| forge_ref.to_string())
}

/// Copy configured files/dirs from the main worktree into the new one.
fn copy_configured(grove: &Grove, dest: &std::path::Path) -> Result<()> {
    let spec: CopySpec = grove.copy_spec()?;
    if spec.is_empty() {
        return Ok(());
    }
    let copied = copy_into(grove.root(), dest, &spec, false)?;
    if !copied.is_empty() {
        eprintln!(
            "{} copied {} item(s) from the main worktree",
            style("✓").green(),
            copied.len()
        );
    }
    Ok(())
}
