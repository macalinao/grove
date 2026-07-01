//! `grove` — a Rust git worktree runner.
//!
//! The `grove` and `git-grove` binaries are thin shells over [`run`]; the
//! latter lets `git grove …` dispatch here as a git subcommand. Shell
//! completions come from bpaf's dynamic completion (`grove completion <shell>`
//! wires it up).

use anyhow::Result;

mod commands;
mod complete;
mod hooks;
mod launch;
mod ui;

/// Parse argv and run the selected command. Shared entry point for the `grove`
/// and `git-grove` binaries.
///
/// # Errors
/// Propagates any error from the selected subcommand.
pub fn run() -> Result<()> {
    let opts = commands::opts().run();
    ui::apply_color();
    commands::execute(opts.command)
}

#[cfg(test)]
mod tests {
    use super::commands::opts;

    /// bpaf's structural invariants (positionals/commands must be rightmost,
    /// no ambiguous parsers). Catches the kind of ordering bug that makes
    /// dynamic completion panic at runtime, across every subcommand.
    #[test]
    fn cli_parser_invariants_hold() {
        opts().check_invariants(true);
    }
}
