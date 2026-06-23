//! Shell integration snippets emitted by `grove init <shell>`.
//!
//! Installs the `grove cd` navigation wrapper and (when `grove.db.shellHook`
//! is enabled) the directory-change hook that re-exports each worktree's
//! database env (`DATABASE_URL`, …) on `cd` — see design spec §7.5.
//!
//! Status: scaffolding for `grove init`. Per-shell snippet generation lands in
//! M3 alongside the db env hook.

/// Shells Grove can emit integration for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
}

impl Shell {
    pub fn as_str(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Pwsh => "pwsh",
        }
    }
}
