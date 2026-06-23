//! Validate [`TaskSpec`]s into an executable [`TaskGraph`].
//!
//! Turns raw specs into a [`TaskGraph`], detecting duplicate names, unknown
//! dependencies, and cycles, and resolving each task's `needs` to dependency
//! indices (into the preserved input order).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::{Result, ScheduledTask, TaskGraph, TaskSpec, TasksError};

/// Build a validated [`TaskGraph`] from `specs`, preserving input order.
///
/// # Errors
///
/// Returns [`TasksError::DuplicateTask`] if two specs share a name,
/// [`TasksError::UnknownDependency`] if a `needs` entry names no task, and
/// [`TasksError::Cycle`] if the `needs` edges form a cycle.
pub fn build_graph(specs: Vec<TaskSpec>) -> Result<TaskGraph> {
    let index = build_index(&specs)?;
    let tasks = resolve_tasks(specs, &index)?;
    detect_cycle(&tasks)?;
    Ok(TaskGraph { tasks })
}

/// Map each task name to its index, rejecting duplicates.
fn build_index(specs: &[TaskSpec]) -> Result<BTreeMap<String, usize>> {
    let mut index = BTreeMap::new();
    for (i, spec) in specs.iter().enumerate() {
        if index.insert(spec.name.clone(), i).is_some() {
            return Err(TasksError::DuplicateTask(spec.name.clone()));
        }
    }
    Ok(index)
}

/// Resolve every task's `needs` names to deduped dependency indices.
fn resolve_tasks(
    specs: Vec<TaskSpec>,
    index: &BTreeMap<String, usize>,
) -> Result<Vec<ScheduledTask>> {
    specs
        .into_iter()
        .map(|spec| resolve_one(spec, index))
        .collect()
}

/// Resolve a single task's dependencies into a deduped index list.
fn resolve_one(spec: TaskSpec, index: &BTreeMap<String, usize>) -> Result<ScheduledTask> {
    let mut deps = Vec::with_capacity(spec.needs.len());
    for dep in &spec.needs {
        let target =
            index
                .get(dep.as_str())
                .copied()
                .ok_or_else(|| TasksError::UnknownDependency {
                    task: spec.name.clone(),
                    dep: dep.clone(),
                })?;
        if !deps.contains(&target) {
            deps.push(target);
        }
    }
    Ok(ScheduledTask { spec, deps })
}

/// Marks used during depth-first cycle detection.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Unvisited,
    InProgress,
    Done,
}

/// Detect a cycle in the resolved dependency edges.
///
/// On success every node reaches [`Mark::Done`]; otherwise the discovered cycle
/// path is reported via [`TasksError::Cycle`].
fn detect_cycle(tasks: &[ScheduledTask]) -> Result<()> {
    let mut marks = alloc::vec![Mark::Unvisited; tasks.len()];
    for start in 0..tasks.len() {
        if marks[start] == Mark::Unvisited {
            if let Some(cycle) = visit(start, tasks, &mut marks) {
                return Err(TasksError::Cycle(format_cycle(tasks, &cycle)));
            }
        }
    }
    Ok(())
}

/// Iterative DFS from `start`; returns the node indices on a cycle if one is
/// found. The returned path runs from the repeated node back around to itself.
fn visit(start: usize, tasks: &[ScheduledTask], marks: &mut [Mark]) -> Option<Vec<usize>> {
    // Stack of (node, next-dependency-cursor); `path` mirrors the active nodes.
    let mut stack: Vec<(usize, usize)> = alloc::vec![(start, 0)];
    let mut path: Vec<usize> = alloc::vec![start];
    marks[start] = Mark::InProgress;

    while let Some(&(node, cursor)) = stack.last() {
        if let Some(dep) = next_dep(tasks, node, cursor) {
            step_cursor(&mut stack);
            if let Some(cycle) = descend(dep, marks, &mut stack, &mut path) {
                return Some(cycle);
            }
        } else {
            marks[node] = Mark::Done;
            stack.pop();
            path.pop();
        }
    }
    None
}

/// Follow the edge to `dep`: report a cycle if `dep` is on the active path,
/// or push it onto the stack when newly discovered.
fn descend(
    dep: usize,
    marks: &mut [Mark],
    stack: &mut Vec<(usize, usize)>,
    path: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    match marks[dep] {
        Mark::InProgress => return Some(extract_cycle(path, dep)),
        Mark::Unvisited => {
            marks[dep] = Mark::InProgress;
            stack.push((dep, 0));
            path.push(dep);
        }
        Mark::Done => {}
    }
    None
}

/// The `cursor`-th dependency of `node`, if any.
fn next_dep(tasks: &[ScheduledTask], node: usize, cursor: usize) -> Option<usize> {
    tasks[node].deps.get(cursor).copied()
}

/// Advance the top-of-stack dependency cursor by one.
fn step_cursor(stack: &mut [(usize, usize)]) {
    if let Some(top) = stack.last_mut() {
        top.1 += 1;
    }
}

/// Slice `path` from the first occurrence of `dep` to the end, then append
/// `dep` again so the rendered cycle closes on itself (`a -> b -> a`).
fn extract_cycle(path: &[usize], dep: usize) -> Vec<usize> {
    let start = path.iter().position(|&n| n == dep).unwrap_or(0);
    let mut cycle = path[start..].to_vec();
    cycle.push(dep);
    cycle
}

/// Render a cycle's node indices as `a -> b -> c`.
fn format_cycle(tasks: &[ScheduledTask], cycle: &[usize]) -> String {
    cycle
        .iter()
        .map(|&i| tasks[i].spec.name.as_str())
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::TaskRun;
    use alloc::vec;

    fn spec(name: &str, needs: &[&str]) -> TaskSpec {
        TaskSpec {
            name: name.into(),
            run: TaskRun::Shell("true".into()),
            needs: needs.iter().map(|n| (*n).into()).collect(),
        }
    }

    fn deps_of(graph: &TaskGraph, name: &str) -> Vec<usize> {
        graph
            .tasks
            .iter()
            .find(|t| t.spec.name == name)
            .map(|t| t.deps.clone())
            .unwrap()
    }

    #[test]
    fn empty_input_yields_empty_graph() {
        let graph = build_graph(vec![]).unwrap();
        assert!(graph.is_empty());
        assert_eq!(graph.len(), 0);
    }

    #[test]
    fn duplicate_name_rejected() {
        let err = build_graph(vec![spec("a", &[]), spec("a", &[])]).unwrap_err();
        assert_eq!(err, TasksError::DuplicateTask("a".into()));
    }

    #[test]
    fn unknown_dependency_rejected() {
        let err = build_graph(vec![spec("a", &["ghost"])]).unwrap_err();
        assert_eq!(
            err,
            TasksError::UnknownDependency {
                task: "a".into(),
                dep: "ghost".into(),
            }
        );
    }

    #[test]
    fn self_cycle_detected() {
        let err = build_graph(vec![spec("a", &["a"])]).unwrap_err();
        assert_eq!(err, TasksError::Cycle("a -> a".into()));
    }

    #[test]
    fn two_node_cycle_detected() {
        let err = build_graph(vec![spec("a", &["b"]), spec("b", &["a"])]).unwrap_err();
        assert_eq!(err, TasksError::Cycle("a -> b -> a".into()));
    }

    #[test]
    fn three_node_cycle_detected() {
        let err = build_graph(vec![
            spec("a", &["b"]),
            spec("b", &["c"]),
            spec("c", &["a"]),
        ])
        .unwrap_err();
        assert_eq!(err, TasksError::Cycle("a -> b -> c -> a".into()));
    }

    #[test]
    fn order_preserved() {
        let graph = build_graph(vec![spec("x", &[]), spec("y", &[]), spec("z", &[])]).unwrap();
        let names: Vec<_> = graph.tasks.iter().map(|t| t.spec.name.as_str()).collect();
        assert_eq!(names, vec!["x", "y", "z"]);
    }

    #[test]
    fn linear_chain_resolves() {
        // a <- b <- c
        let graph =
            build_graph(vec![spec("a", &[]), spec("b", &["a"]), spec("c", &["b"])]).unwrap();
        assert_eq!(deps_of(&graph, "a"), vec![]);
        assert_eq!(deps_of(&graph, "b"), vec![0]);
        assert_eq!(deps_of(&graph, "c"), vec![1]);
    }

    #[test]
    fn diamond_resolves_correct_indices() {
        // a at 0; b at 1 needs a; c at 2 needs a; d at 3 needs b and c.
        let graph = build_graph(vec![
            spec("a", &[]),
            spec("b", &["a"]),
            spec("c", &["a"]),
            spec("d", &["b", "c"]),
        ])
        .unwrap();
        assert_eq!(deps_of(&graph, "a"), vec![]);
        assert_eq!(deps_of(&graph, "b"), vec![0]);
        assert_eq!(deps_of(&graph, "c"), vec![0]);
        assert_eq!(deps_of(&graph, "d"), vec![1, 2]);
    }

    #[test]
    fn duplicate_needs_entries_deduped() {
        let graph = build_graph(vec![spec("a", &[]), spec("b", &["a", "a", "a"])]).unwrap();
        assert_eq!(deps_of(&graph, "b"), vec![0]);
    }

    #[test]
    fn dependencies_may_point_forward() {
        // a (index 0) needs b (index 1): forward reference is valid (acyclic).
        let graph = build_graph(vec![spec("a", &["b"]), spec("b", &[])]).unwrap();
        assert_eq!(deps_of(&graph, "a"), vec![1]);
        assert_eq!(deps_of(&graph, "b"), vec![]);
    }
}
