//! `grove` — a Rust git worktree runner.
//!
//! M1 wires up the core worktree commands (`new`, `list`, `go`, `run`, `rm`,
//! `mv`) plus `doctor`. Forge-aware commands (`clean --merged`, `pr`),
//! adapters (`editor`, `ai`), `copy`, `config`, and the task graph land in
//! later milestones. Shell completions come from bpaf's env-driven
//! autocomplete (`COMPLETE=bash grove …`), not a subcommand.

use anyhow::Result;

mod commands;
mod hooks;
mod launch;
mod ui;

fn main() -> Result<()> {
    let opts = commands::opts().run();
    ui::apply_color();
    commands::execute(opts.command)
}
