//! Editor and AI-tool adapters.
//!
//! An adapter maps a logical name (`cursor`, `claude`, …) to an argv template
//! launched against a worktree path. The registry is user-extensible via
//! `grove.editor.<name>.command` / `grove.ai.<name>.command`.
//!
//! Status: scaffolding for the `editor` / `ai` commands. The built-in adapter
//! tables (Antigravity, Cursor, VS Code, Zed, …; Aider, Claude, Codex, …) are
//! filled in as those commands are implemented.

use std::path::Path;

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
    fn command(&self, worktree: &Path, extra_args: &[String]) -> Result<Vec<String>>;
}

/// Kinds of adapters Grove knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Editor,
    Ai,
}
