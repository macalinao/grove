//! Parse a KDL `tasks { … }` block into [`TaskSpec`]s.
//!
//! Shape:
//! ```kdl
//! tasks {
//!     task "install" { run "bun install" }
//!     task "build"   { run "cargo build"; needs "install" }
//!     task "codegen" { run "bun run codegen"; needs "install" "build" }
//! }
//! ```
//! A missing top-level `tasks` node (or one without children) yields an empty
//! list. Unknown child nodes are ignored for forward-compatibility.

use kdl::{KdlDocument, KdlNode};

use crate::{Result, TaskRun, TaskSpec, TasksError};

/// Parse a `tasks { … }` block from `doc` into [`TaskSpec`]s.
///
/// # Errors
///
/// Returns [`TasksError::Parse`] if a `task` lacks a name or a `run` command.
pub fn parse_tasks(doc: &KdlDocument) -> Result<Vec<TaskSpec>> {
    let Some(children) = doc.get("tasks").and_then(KdlNode::children) else {
        return Ok(Vec::new());
    };

    children
        .nodes()
        .iter()
        .filter(|node| node.name().value() == "task")
        .map(parse_one_task)
        .collect()
}

/// Parse a single `task "name" { … }` node into a [`TaskSpec`].
fn parse_one_task(node: &KdlNode) -> Result<TaskSpec> {
    let name = first_positional_string(node).ok_or_else(|| {
        TasksError::Parse("task is missing a name (expected `task \"<name>\"`)".to_owned())
    })?;

    let body = node.children();
    let run = body
        .and_then(parse_run)
        .ok_or_else(|| TasksError::Parse(format!("task '{name}' is missing a `run` command")))?;
    let needs = body.map(parse_needs).unwrap_or_default();

    Ok(TaskSpec { name, run, needs })
}

/// The shell command from the task body's `run` node, if present.
fn parse_run(body: &KdlDocument) -> Option<TaskRun> {
    let node = body.get("run")?;
    first_positional_string(node).map(TaskRun::Shell)
}

/// All dependency names from every `needs` node in the task body, in order.
fn parse_needs(body: &KdlDocument) -> Vec<String> {
    body.nodes()
        .iter()
        .filter(|node| node.name().value() == "needs")
        .flat_map(positional_strings)
        .collect()
}

/// The first positional (unnamed) string argument of `node`.
fn first_positional_string(node: &KdlNode) -> Option<String> {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .map(str::to_owned)
}

/// Every positional (unnamed) string argument of `node`, in order.
fn positional_strings(node: &KdlNode) -> Vec<String> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .filter_map(|e| e.value().as_string())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<Vec<TaskSpec>> {
        let doc: KdlDocument = src.parse().expect("test KDL should parse");
        parse_tasks(&doc)
    }

    #[test]
    fn empty_doc_is_empty() {
        assert_eq!(parse("").unwrap(), vec![]);
    }

    #[test]
    fn no_tasks_node_is_empty() {
        let src = r#"editor { default "cursor" }"#;
        assert_eq!(parse(src).unwrap(), vec![]);
    }

    #[test]
    fn empty_tasks_block_is_empty() {
        assert_eq!(parse("tasks { }").unwrap(), vec![]);
    }

    #[test]
    fn single_task_with_run() {
        let src = r#"tasks { task "install" { run "bun install" } }"#;
        assert_eq!(
            parse(src).unwrap(),
            vec![TaskSpec {
                name: "install".to_owned(),
                run: TaskRun::Shell("bun install".to_owned()),
                needs: vec![],
            }]
        );
    }

    #[test]
    fn multiple_tasks() {
        let src = r#"
            tasks {
                task "install" { run "bun install" }
                task "build"   { run "cargo build" }
            }
        "#;
        let specs = parse(src).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "install");
        assert_eq!(specs[1].name, "build");
        assert_eq!(specs[1].run, TaskRun::Shell("cargo build".to_owned()));
    }

    #[test]
    fn needs_single_dep() {
        let src = r#"tasks { task "build" { run "cargo build"; needs "install" } }"#;
        assert_eq!(parse(src).unwrap()[0].needs, vec!["install".to_owned()]);
    }

    #[test]
    fn needs_multiple_deps_one_node() {
        let src = r#"tasks { task "codegen" { run "bun run codegen"; needs "install" "build" } }"#;
        assert_eq!(
            parse(src).unwrap()[0].needs,
            vec!["install".to_owned(), "build".to_owned()]
        );
    }

    #[test]
    fn multiple_needs_nodes_accumulate() {
        let src = r#"
            tasks {
                task "codegen" {
                    run "bun run codegen"
                    needs "install"
                    needs "build" "lint"
                }
            }
        "#;
        assert_eq!(
            parse(src).unwrap()[0].needs,
            vec!["install".to_owned(), "build".to_owned(), "lint".to_owned()]
        );
    }

    #[test]
    fn missing_run_is_parse_error() {
        let src = r#"tasks { task "build" { needs "install" } }"#;
        match parse(src) {
            Err(TasksError::Parse(msg)) => assert!(msg.contains("build"), "msg: {msg}"),
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    #[test]
    fn missing_name_is_parse_error() {
        let src = r#"tasks { task { run "echo hi" } }"#;
        assert!(matches!(parse(src), Err(TasksError::Parse(_))));
    }

    #[test]
    fn unknown_child_node_ignored() {
        let src = r#"
            tasks {
                task "install" { run "bun install"; flavor "spicy" }
                widget "ignored"
            }
        "#;
        let specs = parse(src).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "install");
        assert_eq!(specs[0].needs, Vec::<String>::new());
    }
}
