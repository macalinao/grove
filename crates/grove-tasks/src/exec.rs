//! Execute a [`TaskGraph`] with bounded parallelism.
//!
//! STUB — implemented by the exec agent. Runs ready tasks concurrently up to
//! `opts.concurrency`, honoring `needs` edges; on failure, dependents are
//! `Skipped` (and, unless `keep_going`, no new tasks start).

use crate::{ExecOpts, RunReport, TaskGraph, TaskRunner};

pub fn execute(graph: &TaskGraph, opts: ExecOpts, runner: &dyn TaskRunner) -> RunReport {
    let _ = (graph, opts, runner);
    unimplemented!("exec::execute")
}
