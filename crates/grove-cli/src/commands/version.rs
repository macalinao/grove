use anyhow::Result;
use bpaf::Bpaf;

/// Show detailed version and build information.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Version {}

pub fn execute() -> Result<()> {
    println!("grove {}", env!("CARGO_PKG_VERSION"));
    println!("{}", env!("CARGO_PKG_REPOSITORY"));
    Ok(())
}
