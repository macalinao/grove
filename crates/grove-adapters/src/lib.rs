//! Editor and AI-tool adapters.
//!
//! An adapter maps a logical name (`cursor`, `claude`, …) to an argv template
//! launched against a worktree path. The registry is user-extensible via
//! `grove.editor.<name>.command` / `grove.ai.<name>.command`.
//!
//! Built-in adapter tables map each known name to a launch argv (program plus
//! fixed arguments). Editors take the worktree path as a trailing argument; AI
//! tools are launched with the worktree as the working directory and receive no
//! path argument.

use std::path::Path;

/// Built-in editor adapters: `(name, argv)` where `argv` is the program and any
/// fixed arguments, WITHOUT the trailing worktree path (the caller appends it).
///
/// Mirrors gtr's `_EDITOR_REGISTRY` (program names match gtr's, e.g. `agy` for
/// antigravity, `subl` for sublime), plus grove extras (`code`, `windsurf`).
const EDITORS: &[(&str, &[&str])] = &[
    ("vscode", &["code"]),
    ("code", &["code"]),
    ("cursor", &["cursor"]),
    ("zed", &["zed"]),
    ("windsurf", &["windsurf"]),
    ("antigravity", &["agy"]),
    ("atom", &["atom"]),
    ("emacs", &["emacs"]),
    ("idea", &["idea"]),
    ("nano", &["nano"]),
    ("nvim", &["nvim"]),
    ("pycharm", &["pycharm"]),
    ("sublime", &["subl"]),
    ("vim", &["vim"]),
    ("webstorm", &["webstorm"]),
];

/// Built-in AI-tool adapters: `(name, argv)`. AI tools run with cwd set to the
/// worktree and receive NO path argument. Mirrors gtr's `_AI_REGISTRY` plus its
/// custom `cursor` adapter (program `cursor-agent`).
const AI_TOOLS: &[(&str, &[&str])] = &[
    ("claude", &["claude"]),
    ("aider", &["aider"]),
    ("auggie", &["auggie"]),
    ("codex", &["codex"]),
    ("continue", &["cn"]),
    ("copilot", &["copilot"]),
    ("cursor", &["cursor-agent"]),
    ("cursor-agent", &["cursor-agent"]),
    ("gemini", &["gemini"]),
    ("opencode", &["opencode"]),
];

fn lookup(table: &[(&str, &[&str])], name: &str) -> Option<Vec<String>> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, argv)| argv.iter().map(|s| (*s).to_owned()).collect())
}

/// Resolve a built-in editor adapter to its launch argv (program plus fixed
/// arguments), WITHOUT the worktree path. The caller appends the path.
///
/// # Errors
///
/// Returns [`AdapterError::Unknown`] if `name` is not a known editor.
pub fn editor_argv(name: &str) -> Result<Vec<String>> {
    lookup(EDITORS, name).ok_or_else(|| AdapterError::Unknown {
        kind: AdapterKind::Editor,
        name: name.to_owned(),
    })
}

/// Resolve a built-in AI-tool adapter to its launch argv (program plus fixed
/// arguments). AI tools take no path argument; run them with cwd=worktree.
///
/// # Errors
///
/// Returns [`AdapterError::Unknown`] if `name` is not a known AI tool.
pub fn ai_argv(name: &str) -> Result<Vec<String>> {
    lookup(AI_TOOLS, name).ok_or_else(|| AdapterError::Unknown {
        kind: AdapterKind::Ai,
        name: name.to_owned(),
    })
}

/// List the names of all built-in adapters of the given `kind`.
#[must_use]
pub fn list(kind: AdapterKind) -> Vec<&'static str> {
    let table = match kind {
        AdapterKind::Editor => EDITORS,
        AdapterKind::Ai => AI_TOOLS,
    };
    table.iter().map(|(n, _)| *n).collect()
}

/// Errors from resolving or launching an adapter.
#[derive(thiserror::Error, Debug)]
pub enum AdapterError {
    #[error("unknown {kind:?} adapter: {name}")]
    Unknown { kind: AdapterKind, name: String },
}

/// Convenience alias for fallible adapter operations.
pub type Result<T> = core::result::Result<T, AdapterError>;

/// A launchable tool (editor or AI assistant) bound to a worktree.
pub trait Adapter {
    /// Canonical adapter name (e.g. `cursor`).
    fn name(&self) -> &str;
    /// Build the argv to launch this tool against `worktree`.
    ///
    /// # Errors
    /// Returns [`AdapterError`] if the launch command cannot be built.
    fn command(&self, worktree: &Path, extra_args: &[String]) -> Result<Vec<String>>;
}

/// Kinds of adapters Grove knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Editor,
    Ai,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{AdapterError, AdapterKind, ai_argv, editor_argv, list};

    #[test]
    fn known_editors_resolve_to_expected_program() {
        assert_eq!(editor_argv("vscode").unwrap(), vec!["code".to_owned()]);
        assert_eq!(editor_argv("code").unwrap(), vec!["code".to_owned()]);
        assert_eq!(editor_argv("cursor").unwrap(), vec!["cursor".to_owned()]);
        assert_eq!(editor_argv("zed").unwrap(), vec!["zed".to_owned()]);
        assert_eq!(editor_argv("vim").unwrap(), vec!["vim".to_owned()]);
        assert_eq!(
            editor_argv("windsurf").unwrap(),
            vec!["windsurf".to_owned()]
        );
        // Program names match gtr's registry, not the adapter name.
        assert_eq!(editor_argv("antigravity").unwrap(), vec!["agy".to_owned()]);
        assert_eq!(editor_argv("sublime").unwrap(), vec!["subl".to_owned()]);
        assert_eq!(editor_argv("vscode").unwrap(), vec!["code".to_owned()]);
        for name in ["atom", "emacs", "idea", "nvim", "pycharm", "webstorm"] {
            assert_eq!(editor_argv(name).unwrap(), vec![name.to_owned()]);
        }
    }

    #[test]
    fn known_ai_tools_resolve_to_expected_program() {
        assert_eq!(ai_argv("claude").unwrap(), vec!["claude".to_owned()]);
        assert_eq!(ai_argv("aider").unwrap(), vec!["aider".to_owned()]);
        assert_eq!(ai_argv("codex").unwrap(), vec!["codex".to_owned()]);
        assert_eq!(ai_argv("gemini").unwrap(), vec!["gemini".to_owned()]);
        assert_eq!(ai_argv("opencode").unwrap(), vec!["opencode".to_owned()]);
        assert_eq!(ai_argv("auggie").unwrap(), vec!["auggie".to_owned()]);
        assert_eq!(ai_argv("copilot").unwrap(), vec!["copilot".to_owned()]);
        // gtr's `continue` runs `cn`; `cursor` runs `cursor-agent`.
        assert_eq!(ai_argv("continue").unwrap(), vec!["cn".to_owned()]);
        assert_eq!(ai_argv("cursor").unwrap(), vec!["cursor-agent".to_owned()]);
        assert_eq!(
            ai_argv("cursor-agent").unwrap(),
            vec!["cursor-agent".to_owned()]
        );
    }

    #[test]
    fn unknown_editor_is_unknown_error() {
        let err = editor_argv("nope").unwrap_err();
        assert!(matches!(
            err,
            AdapterError::Unknown {
                kind: AdapterKind::Editor,
                ..
            }
        ));
    }

    #[test]
    fn unknown_ai_is_unknown_error() {
        let err = ai_argv("nope").unwrap_err();
        assert!(matches!(
            err,
            AdapterError::Unknown {
                kind: AdapterKind::Ai,
                ..
            }
        ));
    }

    #[test]
    fn list_is_non_empty_for_both_kinds() {
        assert!(!list(AdapterKind::Editor).is_empty());
        assert!(!list(AdapterKind::Ai).is_empty());
    }
}
