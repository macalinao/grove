use anyhow::Result;
use bpaf::Bpaf;
use grove_shell::Shell;

/// Print shell integration for `grove cd` / `grove new --cd` (eval from your rc).
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Init {
    /// Name for the emitted shell function (default: grove)
    #[bpaf(long("as"), argument("NAME"), fallback("grove".to_string()))]
    name: String,
    /// Shell to emit integration for: bash, zsh, or fish
    #[bpaf(positional("SHELL"))]
    shell: String,
}

pub fn execute(args: &Init) -> Result<()> {
    let shell: Shell = args.shell.parse()?;
    print!("{}", grove_shell::init_script(shell, &args.name));
    Ok(())
}
