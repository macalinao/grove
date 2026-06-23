pub mod ai;
pub mod cd;
pub mod config;
pub mod doctor;
pub mod editor;
pub mod go;
pub mod init;
pub mod list;
pub mod mv;
pub mod new;
pub mod rm;
pub mod run;
pub mod tasks;
pub mod trust;
pub mod version;

use anyhow::Result;
use bpaf::Bpaf;

/// Tend your git worktrees.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(options, version(env!("CARGO_PKG_VERSION")), fallback_to_usage)]
pub struct Opts {
    #[bpaf(external)]
    pub command: Command,
}

#[derive(Debug, Clone, Bpaf)]
pub enum Command {
    /// Create a new worktree (and branch)
    New(#[bpaf(external(new::new))] new::New),

    /// List worktrees
    List(#[bpaf(external(list::list))] list::List),

    /// Print the path to a worktree (for `cd "$(grove go x)"`)
    Go(#[bpaf(external(go::go))] go::Go),

    /// Change directory to a worktree (requires shell integration)
    Cd(#[bpaf(external(cd::cd))] cd::Cd),

    /// Run a command inside a worktree
    Run(#[bpaf(external(run::run))] run::Run),

    /// Open a worktree in an editor
    Editor(#[bpaf(external(editor::editor))] editor::Editor),

    /// Launch an AI tool inside a worktree
    Ai(#[bpaf(external(ai::ai))] ai::Ai),

    /// Run the worktree task graph from grove.kdl
    Tasks(#[bpaf(external(tasks::tasks))] tasks::Tasks),

    /// Get, set, list, or unset grove.* configuration
    Config(#[bpaf(external(config::config))] config::Config),

    /// Approve this repo's grove.kdl so its tasks may run
    Trust(#[bpaf(external(trust::trust))] trust::Trust),

    /// Remove worktree(s)
    Rm(#[bpaf(external(rm::rm))] rm::Rm),

    /// Rename a worktree and its branch
    Mv(#[bpaf(external(mv::mv))] mv::Mv),

    /// Print shell integration (eval it from your shell rc)
    Init(#[bpaf(external(init::init))] init::Init),

    /// Diagnose the Grove environment
    Doctor(#[bpaf(external(doctor::doctor))] doctor::Doctor),

    /// Show detailed version and build information
    Version(#[bpaf(external(version::version))] version::Version),
}

pub fn execute(cmd: Command) -> Result<()> {
    match cmd {
        Command::New(args) => new::execute(args),
        Command::List(args) => list::execute(args),
        Command::Go(args) => go::execute(&args),
        Command::Cd(args) => cd::execute(&args),
        Command::Run(args) => run::execute(args),
        Command::Editor(args) => editor::execute(args),
        Command::Ai(args) => ai::execute(args),
        Command::Tasks(args) => tasks::execute(&args),
        Command::Config(args) => config::execute(&args),
        Command::Trust(args) => trust::execute(&args),
        Command::Rm(args) => rm::execute(&args),
        Command::Mv(args) => mv::execute(&args),
        Command::Init(args) => init::execute(&args),
        Command::Doctor(_) => doctor::execute(),
        Command::Version(_) => version::execute(),
    }
}
