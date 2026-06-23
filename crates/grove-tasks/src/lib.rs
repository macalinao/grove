//! Task graph for Grove worktree creation — the parallel DAG that replaces
//! gtr's serial `postCreate`.
//!
//! Pipeline:
//!   1. [`parse_tasks`] (`parse.rs`): a KDL `tasks { … }` block → [`TaskSpec`]s.
//!   2. [`build_graph`] (`graph.rs`): validate specs into an executable
//!      [`TaskGraph`] — duplicate / unknown-dependency / cycle detection,
//!      resolving each task's `needs` to dependency indices.
//!   3. [`execute`] (`exec.rs`): run the graph with bounded parallelism,
//!      honoring `needs` edges and the failure policy.
//!
//! The types here are the frozen contract between those three stages; each is
//! implemented in its own module.

mod exec;
mod graph;
mod parse;

pub use exec::execute;
pub use graph::build_graph;
pub use parse::parse_tasks;

/// Errors from building or running a task graph.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum TasksError {
    #[error("task '{0}' has no command")]
    EmptyTask(String),

    #[error("duplicate task name '{0}'")]
    DuplicateTask(String),

    #[error("task '{task}' depends on unknown task '{dep}'")]
    UnknownDependency { task: String, dep: String },

    #[error("dependency cycle through: {0}")]
    Cycle(String),

    #[error("invalid tasks config: {0}")]
    Parse(String),
}

/// Convenience alias for fallible task-graph operations.
pub type Result<T> = core::result::Result<T, TasksError>;

/// What a task does.
///
/// Built-in verbs (`copy`, `ensure-db`) are intentionally out of scope for now;
/// only shell `run` tasks are modelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRun {
    /// `run "<shell command>"` — executed via the platform shell.
    Shell(String),
}

/// A task as declared in config, before validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub name: String,
    pub run: TaskRun,
    /// Names of tasks that must complete before this one (the `needs` edges).
    pub needs: Vec<String>,
}

/// A task placed in the validated graph: its spec plus the indices (into
/// [`TaskGraph::tasks`]) of the tasks it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTask {
    pub spec: TaskSpec,
    pub deps: Vec<usize>,
}

/// A validated, executable task DAG. Built by [`build_graph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskGraph {
    /// Tasks in a stable order; `deps` reference earlier-or-later indices here.
    pub tasks: Vec<ScheduledTask>,
}

impl TaskGraph {
    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

/// Runs a single task's command. Abstracted so the executor is testable with a
/// fake runner, and so the CLI can inject the worktree dir + environment.
///
/// Implementations must be `Sync` (the executor calls `run` from many threads).
pub trait TaskRunner: Sync {
    /// Run `task`; return `Err(message)` on failure.
    fn run(&self, task: &TaskSpec) -> core::result::Result<(), String>;
}

/// Options controlling [`execute`].
#[derive(Debug, Clone, Copy)]
pub struct ExecOpts {
    /// Maximum tasks to run concurrently.
    pub concurrency: usize,
    /// On failure, keep running tasks that don't depend on the failed one.
    pub keep_going: bool,
}

impl Default for ExecOpts {
    fn default() -> Self {
        let concurrency = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(1);
        ExecOpts {
            concurrency,
            keep_going: false,
        }
    }
}

/// Outcome of a single task after a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Success,
    Failed(String),
    /// A dependency failed, so this task was never started.
    Skipped,
}

/// The result of executing a [`TaskGraph`].
#[derive(Debug, Clone)]
pub struct RunReport {
    /// `(task name, status)` for every task in the graph.
    pub outcomes: Vec<(String, TaskStatus)>,
}

impl RunReport {
    /// Did any task fail?
    #[must_use]
    pub fn failed(&self) -> bool {
        self.outcomes
            .iter()
            .any(|(_, s)| matches!(s, TaskStatus::Failed(_)))
    }
}

/// Convenience: build a graph from `specs` and execute it.
pub fn run(specs: Vec<TaskSpec>, opts: ExecOpts, runner: &dyn TaskRunner) -> Result<RunReport> {
    let graph = build_graph(specs)?;
    Ok(execute(&graph, opts, runner))
}
