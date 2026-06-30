//! `grove` binary entry point. The implementation lives in the `grove_cli`
//! library so the `git-grove` binary can share it.

fn main() -> anyhow::Result<()> {
    grove_cli::run()
}
