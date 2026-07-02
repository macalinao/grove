use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use console::style;
use grove_core::Grove;
use grove_tasks::{ExecOpts, RunReport, TaskRun, TaskRunner, TaskSpec, TaskStatus};

/// Run the worktree task graph defined in `grove.kdl`.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Tasks {
    /// List the task graph instead of running it
    #[bpaf(long, switch)]
    list: bool,
    /// Max tasks to run concurrently (default: CPU count)
    #[bpaf(long, argument("N"))]
    concurrency: Option<usize>,
    /// Keep running independent tasks after a failure
    #[bpaf(long, switch)]
    keep_going: bool,
}

pub fn execute(args: &Tasks) -> Result<()> {
    let grove = Grove::open()?;
    let dir = grove.repo.cwd().to_path_buf();
    let specs = load_tasks(&dir)?;

    if specs.is_empty() {
        eprintln!("no tasks defined (add a `tasks {{ … }}` block to grove.kdl)");
        return Ok(());
    }
    if args.list {
        // Listing the graph is always safe — only RUNNING is gated.
        list_tasks(&specs);
        return Ok(());
    }

    // Refuse to execute untrusted grove.kdl commands.
    let git_dir = grove.repo.git_common_dir()?;
    ensure_trusted(&dir, &git_dir)?;

    let opts = ExecOpts {
        concurrency: args
            .concurrency
            .unwrap_or_else(|| ExecOpts::default().concurrency),
        keep_going: args.keep_going,
    };
    let runner = ShellRunner::new(dir);
    run_graph(specs, opts, &runner)
}

/// Load and parse the `tasks { … }` block from `<dir>/grove.kdl` (empty if none).
pub fn load_tasks(dir: &Path) -> Result<Vec<TaskSpec>> {
    let path = dir.join("grove.kdl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let src = std::fs::read_to_string(&path)?;
    let doc: kdl::KdlDocument = src.parse().map_err(|e: kdl::KdlError| anyhow!("{e}"))?;
    Ok(grove_tasks::parse_tasks(&doc)?)
}

/// Build + run the graph, print a per-task summary, and error if any failed.
pub fn run_graph(specs: Vec<TaskSpec>, opts: ExecOpts, runner: &dyn TaskRunner) -> Result<()> {
    let report = grove_tasks::run(specs, opts, runner)?;
    print_summary(&report);
    if report.failed() {
        return Err(anyhow!("one or more tasks failed"));
    }
    Ok(())
}

/// Load the tasks for `dir` and run them rooted there. Used by `grove new`.
pub fn run_for(dir: &Path, opts: ExecOpts) -> Result<()> {
    let specs = load_tasks(dir)?;
    if specs.is_empty() {
        return Ok(());
    }
    // Refuse to execute untrusted grove.kdl commands.
    let grove = Grove::open()?;
    let git_dir = grove.repo.git_common_dir()?;
    ensure_trusted(dir, &git_dir)?;

    let runner = ShellRunner::new(dir.to_path_buf());
    run_graph(specs, opts, &runner)
}

/// Refuse to run if `<dir>/grove.kdl` exists and is not trusted.
///
/// # Errors
/// Returns an error instructing the user to run `grove trust` when the
/// `grove.kdl` at `dir` has not been approved.
fn ensure_trusted(dir: &Path, git_dir: &Path) -> Result<()> {
    if grove_core::is_trusted(dir, git_dir) {
        return Ok(());
    }
    Err(anyhow!(
        "refusing to run untrusted grove.kdl tasks — review {} and run `grove trust` to approve",
        dir.join("grove.kdl").display()
    ))
}

fn list_tasks(specs: &[TaskSpec]) {
    for spec in specs {
        let needs = if spec.needs.is_empty() {
            String::new()
        } else {
            format!("  (needs: {})", spec.needs.join(", "))
        };
        println!("{}{}", style(&spec.name).bold(), style(needs).dim());
    }
}

fn print_summary(report: &RunReport) {
    for (name, status) in &report.outcomes {
        match status {
            TaskStatus::Success => println!("{} {name}", style("✓").green()),
            TaskStatus::Failed(msg) => println!("{} {name}: {msg}", style("✗").red()),
            TaskStatus::Skipped => {
                println!("{} {name} {}", style("•").dim(), style("(skipped)").dim());
            }
        }
    }
}

/// Runs each task's shell command in a fixed directory, prefixing its output.
pub struct ShellRunner {
    dir: PathBuf,
}

impl ShellRunner {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        ShellRunner { dir }
    }
}

impl TaskRunner for ShellRunner {
    fn run(&self, task: &TaskSpec) -> core::result::Result<(), String> {
        let TaskRun::Shell(cmd) = &task.run;
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&self.dir)
            .env("GROVE_WORKTREE", &self.dir)
            .output()
            .map_err(|e| format!("failed to spawn shell for '{}': {e}", task.name))?;

        let label = style(format!("[{}]", task.name)).cyan().to_string();
        let mut buf = String::new();
        append_lines(&mut buf, &label, &output.stdout);
        append_lines(&mut buf, &label, &output.stderr);
        if !buf.is_empty() {
            eprint!("{buf}");
        }

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "`{cmd}` exited with status {}",
                output.status.code().unwrap_or(-1)
            ))
        }
    }
}

/// Append each line of `bytes` to `buf`, prefixed with `label`.
fn append_lines(buf: &mut String, label: &str, bytes: &[u8]) {
    for line in String::from_utf8_lossy(bytes).lines() {
        buf.push_str(label);
        buf.push(' ');
        buf.push_str(line);
        buf.push('\n');
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn shell_task(name: &str, cmd: &str) -> TaskSpec {
        TaskSpec {
            name: name.to_owned(),
            run: TaskRun::Shell(cmd.to_owned()),
            needs: Vec::new(),
        }
    }

    #[test]
    fn shell_runner_reports_success() {
        let runner = ShellRunner::new(std::env::temp_dir());
        assert!(runner.run(&shell_task("ok", "true")).is_ok());
    }

    #[test]
    fn shell_runner_reports_failure() {
        let runner = ShellRunner::new(std::env::temp_dir());
        let err = runner.run(&shell_task("bad", "exit 3")).unwrap_err();
        assert!(err.contains('3'), "msg: {err}");
    }

    #[test]
    fn load_tasks_missing_file_is_empty() {
        let dir = std::env::temp_dir().join("grove-no-such-dir-xyz");
        assert!(load_tasks(&dir).unwrap().is_empty());
    }
}
