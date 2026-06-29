use anyhow::Result;
use bpaf::Bpaf;
use grove_shell::Shell;

/// Print shell-completion setup for `grove`.
///
/// Grove's completions are powered by bpaf's dynamic, env-driven completion;
/// this emits the one line that wires it into your shell. Eval/source it from
/// your shell rc (e.g. `source <(grove completion bash)`).
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Completion {
    /// Shell to emit completion setup for: bash, zsh, or fish
    #[bpaf(positional("SHELL"))]
    shell: String,
}

pub fn execute(args: &Completion) -> Result<()> {
    let shell: Shell = args.shell.parse()?;
    let line = match shell {
        Shell::Bash => "source <(COMPLETE=bash grove)",
        Shell::Zsh => "source <(COMPLETE=zsh grove)",
        Shell::Fish => "source (COMPLETE=fish grove | psub)",
    };
    println!("{line}");
    Ok(())
}
