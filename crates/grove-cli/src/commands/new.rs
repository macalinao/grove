use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::{CopySpec, CreateOpts, Grove, TrackMode, copy_into};
use grove_tasks::ExecOpts;

use crate::commands::tasks;
use crate::{hooks, launch};

/// Create a new worktree (and branch).
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
    /// Branch name (also the worktree folder name, slashes sanitized)
    #[bpaf(positional("BRANCH"))]
    branch: String,
}

pub fn execute(args: New) -> Result<()> {
    let grove = Grove::open()?;
    let path = grove.create(
        &args.branch,
        &CreateOpts {
            branch: Some(args.branch.clone()),
            base: args.from,
            from_current: args.from_current,
            folder: args.folder,
            name: args.name,
            force: args.force,
            fetch: !args.no_fetch,
            remote: args.remote,
            track: args
                .track
                .as_deref()
                .map_or(TrackMode::Auto, TrackMode::parse),
        },
    )?;

    if args.print_path {
        println!("{}", path.display());
    } else {
        eprintln!(
            "{} created worktree {} at {}",
            style("✓").green(),
            style(&args.branch).bold(),
            path.display()
        );
    }

    if !args.no_copy {
        copy_configured(&grove, &path)?;
    }
    if !args.no_hooks {
        hooks::run(
            &grove,
            "postCreate",
            &grove.config.hook_post_create,
            &path,
            &args.branch,
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
