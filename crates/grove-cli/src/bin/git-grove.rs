//! `git-grove` — lets `git grove …` dispatch to grove as a git subcommand.
//!
//! git resolves `git grove` by searching `PATH` for a `git-grove` executable
//! and running it with the remaining arguments, so this is just `grove` under
//! the name git expects.

fn main() -> anyhow::Result<()> {
    grove_cli::run()
}
