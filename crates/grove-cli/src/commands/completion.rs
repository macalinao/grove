use anyhow::Result;
use bpaf::Bpaf;
use grove_shell::Shell;

/// Print shell-completion setup for `grove`.
///
/// Grove's completions are powered by bpaf's dynamic completion; this emits the
/// one line that wires it into your shell. Eval/source it from your shell rc
/// (e.g. `source <(grove completion bash)`).
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Completion {
    /// Shell to emit completion setup for: bash, zsh, or fish
    #[bpaf(positional("SHELL"))]
    shell: String,
}

pub fn execute(args: &Completion) -> Result<()> {
    let shell: Shell = args.shell.parse()?;
    // bpaf registers itself by emitting a per-shell script; sourcing it wires
    // up the dynamic `--bpaf-complete-rev` protocol the binary speaks.
    let line = match shell {
        Shell::Bash => "source <(grove --bpaf-complete-style-bash)",
        Shell::Zsh => "source <(grove --bpaf-complete-style-zsh)",
        Shell::Fish => "grove --bpaf-complete-style-fish | source",
    };
    println!("{line}");
    Ok(())
}
