//! Native `grove.kdl` reader.
//!
//! Only the subset needed for M1 is wired in (`worktrees`, `editor`, `ai`,
//! `ui`). The `database` and `tasks` blocks parse but are ignored until the
//! M4 / task-graph milestones consume them. Returns a human-readable message
//! on parse failure; the caller wraps it in [`crate::ConfigError::Kdl`].

use kdl::{KdlDocument, KdlNode};

use crate::{ColorChoice, Config, TrackMode};

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
        if let Some(v) = child_string(children, "workspace") {
            cfg.editor_workspace = Some(v);
        }
    }
    if let Some(children) = child_doc(&doc, "ai") {
        if let Some(v) = child_string(children, "default") {
            cfg.ai_default = Some(v);
        }
    }
    apply_defaults(cfg, &doc);
    apply_forge(cfg, &doc);
    apply_hooks(cfg, &doc);
    if let Some(children) = child_doc(&doc, "ui") {
        if let Some(v) = child_string(children, "color") {
            cfg.color = ColorChoice::parse(&v);
        }
    }
    apply_copy(cfg, &doc);

    Ok(())
}

/// The `defaults { remote; branch; track; provider }` block.
fn apply_defaults(cfg: &mut Config, doc: &KdlDocument) {
    let Some(children) = child_doc(doc, "defaults") else {
        return;
    };
    if let Some(v) = child_string(children, "remote") {
        cfg.default_remote = Some(v);
    }
    if let Some(v) = child_string(children, "branch") {
        cfg.default_branch = Some(v);
    }
    if let Some(v) = child_string(children, "track") {
        cfg.track = TrackMode::parse(&v);
    }
    if let Some(v) = child_string(children, "provider") {
        cfg.provider = Some(v);
    }
}

/// The `forge { host; token }` block.
fn apply_forge(cfg: &mut Config, doc: &KdlDocument) {
    let Some(children) = child_doc(doc, "forge") else {
        return;
    };
    if let Some(v) = child_string(children, "host") {
        cfg.forge_host = Some(v);
    }
    if let Some(v) = child_string(children, "token") {
        cfg.forge_token = Some(v);
    }
}

/// The `hooks { postCreate; preRemove; postRemove; postCd }` block.
fn apply_hooks(cfg: &mut Config, doc: &KdlDocument) {
    let Some(children) = child_doc(doc, "hooks") else {
        return;
    };
    set_if_present(
        &mut cfg.hook_post_create,
        child_strings_all(children, "postCreate"),
    );
    set_if_present(
        &mut cfg.hook_pre_remove,
        child_strings_all(children, "preRemove"),
    );
    set_if_present(
        &mut cfg.hook_post_remove,
        child_strings_all(children, "postRemove"),
    );
    set_if_present(&mut cfg.hook_post_cd, child_strings_all(children, "postCd"));
}

/// The `copy { include; exclude; includeDirs; excludeDirs }` block.
fn apply_copy(cfg: &mut Config, doc: &KdlDocument) {
    let Some(children) = child_doc(doc, "copy") else {
        return;
    };
    set_if_present(
        &mut cfg.copy_include,
        child_strings_all(children, "include"),
    );
    set_if_present(
        &mut cfg.copy_exclude,
        child_strings_all(children, "exclude"),
    );
    set_if_present(
        &mut cfg.copy_include_dirs,
        child_strings_all(children, "includeDirs"),
    );
    set_if_present(
        &mut cfg.copy_exclude_dirs,
        child_strings_all(children, "excludeDirs"),
    );
}

/// Overwrite `slot` only when `values` is non-empty.
fn set_if_present(slot: &mut Vec<String>, values: Vec<String>) {
    if !values.is_empty() {
        *slot = values;
    }
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

    #[test]
    fn reads_dirs_hooks_and_defaults() {
        let src = r#"
            copy { includeDirs "node_modules"; excludeDirs "node_modules/.cache" }
            hooks {
                postCreate "bun install"
                preRemove "echo bye"
                postRemove "echo gone"
                postCd "source .env"
            }
            defaults { remote "upstream"; branch "main"; track "remote"; provider "gitlab" }
            forge { host "https://gitea.myhost.com"; token "abc123" }
            editor { default "code"; workspace "app.code-workspace" }
        "#;
        let mut cfg = Config::default();
        apply(&mut cfg, src).unwrap();
        assert_eq!(cfg.copy_include_dirs, vec!["node_modules"]);
        assert_eq!(cfg.copy_exclude_dirs, vec!["node_modules/.cache"]);
        assert_eq!(cfg.hook_post_create, vec!["bun install"]);
        assert_eq!(cfg.hook_pre_remove, vec!["echo bye"]);
        assert_eq!(cfg.hook_post_remove, vec!["echo gone"]);
        assert_eq!(cfg.hook_post_cd, vec!["source .env"]);
        assert_eq!(cfg.default_remote.as_deref(), Some("upstream"));
        assert_eq!(cfg.default_branch.as_deref(), Some("main"));
        assert_eq!(cfg.track, TrackMode::Remote);
        assert_eq!(cfg.provider.as_deref(), Some("gitlab"));
        assert_eq!(cfg.forge_host.as_deref(), Some("https://gitea.myhost.com"));
        assert_eq!(cfg.forge_token.as_deref(), Some("abc123"));
        assert_eq!(cfg.editor_workspace.as_deref(), Some("app.code-workspace"));
    }
}
