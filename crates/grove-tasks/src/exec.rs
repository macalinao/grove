//! Concurrent execution of a [`TaskGraph`].
//!
//! Tasks run as soon as all their dependencies have finished with
//! [`TaskStatus::Success`]. If any dependency failed or was skipped, the
//! dependent task is skipped (transitively). Execution honours a concurrency
//! cap and an optional fail-fast (`keep_going == false`) policy.

use core::result::Result;
use std::sync::{Condvar, Mutex, PoisonError};
use std::thread;

use crate::{ExecOpts, RunReport, TaskGraph, TaskRunner, TaskStatus};

/// Mutable, lock-protected execution state shared across workers.
struct State {
    /// Final status per task, `None` until the task settles.
    status: Vec<Option<TaskStatus>>,
    /// Whether a worker has claimed the task (running or settled).
    claimed: Vec<bool>,
    /// Count of tasks not yet settled.
    remaining: usize,
    /// Set once any task has failed; used for fail-fast.
    any_failed: bool,
}

impl State {
    fn new(len: usize) -> Self {
        Self {
            status: vec![None; len],
            claimed: vec![false; len],
            remaining: len,
            any_failed: false,
        }
    }
}

/// Lock a mutex, recovering the guard if a worker panicked while holding it.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether every dependency of `idx` has settled as `Success`.
fn deps_succeeded(graph: &TaskGraph, state: &State, idx: usize) -> bool {
    graph.tasks[idx]
        .deps
        .iter()
        .all(|&d| matches!(state.status[d], Some(TaskStatus::Success)))
}

/// Whether any dependency of `idx` settled as failed or skipped.
fn deps_doomed(graph: &TaskGraph, state: &State, idx: usize) -> bool {
    graph.tasks[idx].deps.iter().any(|&d| {
        matches!(
            state.status[d],
            Some(TaskStatus::Failed(_) | TaskStatus::Skipped)
        )
    })
}

/// Mark every unclaimed task whose deps are doomed as `Skipped`, repeating
/// until no further task can be skipped (skips propagate transitively).
fn mark_skips(graph: &TaskGraph, state: &mut State) {
    let mut changed = true;
    while changed {
        changed = false;
        for idx in 0..graph.tasks.len() {
            if state.claimed[idx] || state.status[idx].is_some() {
                continue;
            }
            if deps_doomed(graph, state, idx) {
                state.claimed[idx] = true;
                state.status[idx] = Some(TaskStatus::Skipped);
                state.remaining -= 1;
                changed = true;
            }
        }
    }
}

/// Outcome of trying to claim the next task.
enum Claim {
    /// A task index was claimed and should be run.
    Run(usize),
    /// Nothing is startable right now, but work is still outstanding.
    Wait,
    /// All tasks have settled.
    Done,
}

/// Pick the next startable task, applying skip propagation and fail-fast first.
fn next_claim(graph: &TaskGraph, opts: ExecOpts, state: &mut State) -> Claim {
    mark_skips(graph, state);
    if state.remaining == 0 {
        return Claim::Done;
    }
    // Fail-fast: once anything failed, start nothing new.
    if !opts.keep_going && state.any_failed {
        return Claim::Wait;
    }
    for idx in 0..graph.tasks.len() {
        if state.claimed[idx] || state.status[idx].is_some() {
            continue;
        }
        if deps_succeeded(graph, state, idx) {
            state.claimed[idx] = true;
            return Claim::Run(idx);
        }
    }
    Claim::Wait
}

/// Record a finished task's outcome.
fn record(state: &mut State, idx: usize, status: TaskStatus) {
    if matches!(status, TaskStatus::Failed(_)) {
        state.any_failed = true;
    }
    state.status[idx] = Some(status);
    state.remaining -= 1;
}

/// One worker: repeatedly claim and run tasks until the graph is settled.
fn worker(
    graph: &TaskGraph,
    opts: ExecOpts,
    runner: &dyn TaskRunner,
    mutex: &Mutex<State>,
    cv: &Condvar,
) {
    loop {
        let idx = {
            let mut guard = lock(mutex);
            loop {
                match next_claim(graph, opts, &mut guard) {
                    Claim::Run(i) => break i,
                    Claim::Done => {
                        cv.notify_all();
                        return;
                    }
                    Claim::Wait => {
                        guard = cv.wait(guard).unwrap_or_else(PoisonError::into_inner);
                    }
                }
            }
        };

        let status = match runner.run(&graph.tasks[idx].spec) {
            Result::Ok(()) => TaskStatus::Success,
            Result::Err(msg) => TaskStatus::Failed(msg),
        };

        {
            let mut guard = lock(mutex);
            record(&mut guard, idx, status);
        }
        cv.notify_all();
    }
}

/// Execute `graph` with `runner`, returning a [`RunReport`] in graph order.
#[must_use]
pub fn execute(graph: &TaskGraph, opts: ExecOpts, runner: &dyn TaskRunner) -> RunReport {
    let len = graph.tasks.len();
    let mutex = Mutex::new(State::new(len));
    let cv = Condvar::new();
    let workers = opts.concurrency.clamp(1, len.max(1));

    if len > 0 {
        thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| worker(graph, opts, runner, &mutex, &cv));
            }
        });
    }

    let state = mutex.into_inner().unwrap_or_else(PoisonError::into_inner);
    let outcomes = graph
        .tasks
        .iter()
        .zip(state.status)
        .map(|(task, status)| {
            (
                task.spec.name.clone(),
                status.unwrap_or(TaskStatus::Skipped),
            )
        })
        .collect();
    RunReport { outcomes }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{ScheduledTask, TaskRun, TaskSpec};
    use core::sync::atomic::{AtomicUsize, Ordering};
    use core::time::Duration;
    use std::collections::HashSet;

    /// Test runner that records completion order, tracks peak concurrency, and
    /// can be configured to fail specific task names.
    struct FakeRunner {
        completed: Mutex<Vec<String>>,
        fail: HashSet<String>,
        live: AtomicUsize,
        max_live: AtomicUsize,
    }

    impl FakeRunner {
        fn new(fail: &[&str]) -> Self {
            Self {
                completed: Mutex::new(Vec::new()),
                fail: fail.iter().map(|s| (*s).to_owned()).collect(),
                live: AtomicUsize::new(0),
                max_live: AtomicUsize::new(0),
            }
        }

        fn order(&self) -> Vec<String> {
            self.completed.lock().unwrap().clone()
        }

        fn max_concurrency(&self) -> usize {
            self.max_live.load(Ordering::SeqCst)
        }
    }

    impl TaskRunner for FakeRunner {
        fn run(&self, task: &TaskSpec) -> Result<(), String> {
            let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_live.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(20));
            self.live.fetch_sub(1, Ordering::SeqCst);
            self.completed.lock().unwrap().push(task.name.clone());
            if self.fail.contains(&task.name) {
                return Err(format!("{} failed", task.name));
            }
            Ok(())
        }
    }

    fn spec(name: &str) -> TaskSpec {
        TaskSpec {
            name: name.to_owned(),
            run: TaskRun::Shell("true".to_owned()),
            needs: Vec::new(),
        }
    }

    fn task(name: &str, deps: Vec<usize>) -> ScheduledTask {
        ScheduledTask {
            spec: spec(name),
            deps,
        }
    }

    fn status_of<'r>(report: &'r RunReport, name: &str) -> &'r TaskStatus {
        &report.outcomes.iter().find(|(n, _)| n == name).unwrap().1
    }

    fn pos(order: &[String], name: &str) -> usize {
        order.iter().position(|n| n == name).unwrap()
    }

    #[test]
    fn linear_chain_orders_and_succeeds() {
        // a -> b -> c
        let graph = TaskGraph {
            tasks: vec![task("a", vec![]), task("b", vec![0]), task("c", vec![1])],
        };
        let runner = FakeRunner::new(&[]);
        let report = execute(
            &graph,
            ExecOpts {
                concurrency: 4,
                keep_going: false,
            },
            &runner,
        );
        for name in ["a", "b", "c"] {
            assert_eq!(status_of(&report, name), &TaskStatus::Success);
        }
        let order = runner.order();
        assert!(pos(&order, "a") < pos(&order, "b"));
        assert!(pos(&order, "b") < pos(&order, "c"));
    }

    #[test]
    fn diamond_runs_branches_concurrently() {
        // a -> {b, c} -> d
        let graph = TaskGraph {
            tasks: vec![
                task("a", vec![]),
                task("b", vec![0]),
                task("c", vec![0]),
                task("d", vec![1, 2]),
            ],
        };
        let runner = FakeRunner::new(&[]);
        let report = execute(
            &graph,
            ExecOpts {
                concurrency: 2,
                keep_going: false,
            },
            &runner,
        );
        for name in ["a", "b", "c", "d"] {
            assert_eq!(status_of(&report, name), &TaskStatus::Success);
        }
        assert!(runner.max_concurrency() >= 2);
        let order = runner.order();
        assert!(pos(&order, "a") < pos(&order, "b"));
        assert!(pos(&order, "a") < pos(&order, "c"));
        assert_eq!(pos(&order, "d"), order.len() - 1);
    }

    #[test]
    fn concurrency_one_is_serial() {
        let graph = TaskGraph {
            tasks: vec![
                task("a", vec![]),
                task("b", vec![0]),
                task("c", vec![0]),
                task("d", vec![1, 2]),
            ],
        };
        let runner = FakeRunner::new(&[]);
        let report = execute(
            &graph,
            ExecOpts {
                concurrency: 1,
                keep_going: true,
            },
            &runner,
        );
        for name in ["a", "b", "c", "d"] {
            assert_eq!(status_of(&report, name), &TaskStatus::Success);
        }
        assert_eq!(runner.max_concurrency(), 1);
    }

    #[test]
    fn failure_skips_dependent() {
        // a -> b(fail) -> c
        let graph = TaskGraph {
            tasks: vec![task("a", vec![]), task("b", vec![0]), task("c", vec![1])],
        };
        let runner = FakeRunner::new(&["b"]);
        let report = execute(
            &graph,
            ExecOpts {
                concurrency: 4,
                keep_going: true,
            },
            &runner,
        );
        assert_eq!(status_of(&report, "a"), &TaskStatus::Success);
        assert!(matches!(status_of(&report, "b"), TaskStatus::Failed(_)));
        assert_eq!(status_of(&report, "c"), &TaskStatus::Skipped);
    }

    #[test]
    fn fail_fast_skips_unstarted() {
        // root -> b(fail) -> gate -> d. b fails; gate (and transitively d)
        // depend on it, so with keep_going=false they are skipped, never run.
        let graph = TaskGraph {
            tasks: vec![
                task("root", vec![]),
                task("b", vec![0]),
                task("gate", vec![1]),
                task("d", vec![2]),
            ],
        };
        let runner = FakeRunner::new(&["b"]);
        let report = execute(
            &graph,
            ExecOpts {
                concurrency: 4,
                keep_going: false,
            },
            &runner,
        );
        assert_eq!(status_of(&report, "root"), &TaskStatus::Success);
        assert!(matches!(status_of(&report, "b"), TaskStatus::Failed(_)));
        assert_eq!(status_of(&report, "gate"), &TaskStatus::Skipped);
        assert_eq!(status_of(&report, "d"), &TaskStatus::Skipped);
        assert!(!runner.order().contains(&"d".to_owned()));
    }

    #[test]
    fn keep_going_runs_independent_branch() {
        // root -> b(fail) -> c   (doomed)
        // root -> e -> f         (healthy)
        let graph = TaskGraph {
            tasks: vec![
                task("root", vec![]),
                task("b", vec![0]),
                task("c", vec![1]),
                task("e", vec![0]),
                task("f", vec![3]),
            ],
        };
        let runner = FakeRunner::new(&["b"]);
        let report = execute(
            &graph,
            ExecOpts {
                concurrency: 4,
                keep_going: true,
            },
            &runner,
        );
        assert_eq!(status_of(&report, "root"), &TaskStatus::Success);
        assert!(matches!(status_of(&report, "b"), TaskStatus::Failed(_)));
        assert_eq!(status_of(&report, "c"), &TaskStatus::Skipped);
        assert_eq!(status_of(&report, "e"), &TaskStatus::Success);
        assert_eq!(status_of(&report, "f"), &TaskStatus::Success);
    }
}
