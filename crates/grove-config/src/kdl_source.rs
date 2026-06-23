//! Native `grove.kdl` reader.
//!
//! Only the subset needed for M1 is wired in (`worktrees`, `editor`, `ai`,
//! `ui`). The `database` and `tasks` blocks parse but are ignored until the
//! M4 / task-graph milestones consume them. Returns a human-readable message
//! on parse failure; the caller wraps it in [`crate::ConfigError::Kdl`].

use kdl::{KdlDocument, KdlNode};

use crate::{ColorChoice, Config};

pub fn apply(cfg: &mut Config, src: &str) -> core::result::Result<(), String> {
    let doc: KdlDocument = src.parse().map_err(|e: kdl::KdlError| e.to_string())?;

    if let Some(children) = child_doc(&doc, "worktrees") {
        if let Some(v) = child_string(children, "dir") {
            cfg.worktrees_dir = Some(v);
        }
        if let Some(v) = child_string(children, "prefix") {
            cfg.worktrees_prefix = v;
        }
    }
    if let Some(children) = child_doc(&doc, "editor") {
        if let Some(v) = child_string(children, "default") {
            cfg.editor_default = Some(v);
        }
    }
    if let Some(children) = child_doc(&doc, "ai") {
        if let Some(v) = child_string(children, "default") {
            cfg.ai_default = Some(v);
        }
    }
    if let Some(children) = child_doc(&doc, "ui") {
        if let Some(v) = child_string(children, "color") {
            cfg.color = match v.as_str() {
                "always" => ColorChoice::Always,
                "never" => ColorChoice::Never,
                _ => ColorChoice::Auto,
            };
        }
    }
    if let Some(children) = child_doc(&doc, "copy") {
        let include = child_strings_all(children, "include");
        if !include.is_empty() {
            cfg.copy_include = include;
        }
        let exclude = child_strings_all(children, "exclude");
        if !exclude.is_empty() {
            cfg.copy_exclude = exclude;
        }
    }

    Ok(())
}

/// All positional string arguments across every node named `name` in `parent`.
///
/// Supports both multiple positional args on one node (`include "a" "b"`) and
/// repeated nodes (`include "a"; include "b"`).
fn child_strings_all(parent: &KdlDocument, name: &str) -> Vec<String> {
    parent
        .nodes()
        .iter()
        .filter(|n| n.name().value() == name)
        .flat_map(KdlNode::entries)
        .filter(|e| e.name().is_none())
        .filter_map(|e| e.value().as_string())
        .map(str::to_string)
        .collect()
}

/// The child document of `parent`'s node named `name`, if it has a `{ ... }`.
fn child_doc<'a>(parent: &'a KdlDocument, name: &str) -> Option<&'a KdlDocument> {
    parent.get(name).and_then(KdlNode::children)
}

/// The first positional string argument of `parent`'s node named `name`.
fn child_string(parent: &KdlDocument, name: &str) -> Option<String> {
    let node = parent.get(name)?;
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .map(str::to_string)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_worktrees_and_editor() {
        let src = r#"
            worktrees { dir "../wt"; prefix "wt-" }
            editor { default "cursor" }
            ai { default "claude" }
            ui { color "never" }
        "#;
        let mut cfg = Config::default();
        apply(&mut cfg, src).unwrap();
        assert_eq!(cfg.worktrees_dir.as_deref(), Some("../wt"));
        assert_eq!(cfg.worktrees_prefix, "wt-");
        assert_eq!(cfg.editor_default.as_deref(), Some("cursor"));
        assert_eq!(cfg.ai_default.as_deref(), Some("claude"));
        assert_eq!(cfg.color, ColorChoice::Never);
    }

    #[test]
    fn reads_copy_multiple_args_and_repeated_nodes() {
        let src = r#"
            copy {
                include ".env" ".env.local"
                include "config/*.toml"
                exclude "*.secret"
            }
        "#;
        let mut cfg = Config::default();
        apply(&mut cfg, src).unwrap();
        assert_eq!(
            cfg.copy_include,
            vec![".env", ".env.local", "config/*.toml"]
        );
        assert_eq!(cfg.copy_exclude, vec!["*.secret"]);
    }
}
