use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_core::{Grove, TrustStatus};

/// Approve this repo's `grove.kdl` so its tasks may run.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Trust {
    /// Show the current trust status without changing it
    #[bpaf(long, switch)]
    list: bool,
}

pub fn execute(args: &Trust) -> Result<()> {
    let grove = Grove::open()?;
    let root = grove.root().to_path_buf();
    let git_dir = grove.repo.git_common_dir()?;

    if args.list {
        print_status(grove_core::trust_status(&root, &git_dir));
        return Ok(());
    }

    match grove_core::trust_status(&root, &git_dir) {
        TrustStatus::NoConfig => {
            eprintln!("nothing to trust (no grove.kdl)");
        }
        TrustStatus::Trusted => {
            eprintln!("{} grove.kdl already trusted", style("✓").green());
        }
        TrustStatus::Untrusted => {
            grove_core::record_trust(&root, &git_dir)?;
            eprintln!(
                "{} trusted grove.kdl — its tasks may now run",
                style("✓").green()
            );
        }
    }
    Ok(())
}

fn print_status(status: TrustStatus) {
    match status {
        TrustStatus::NoConfig => println!("{} no grove.kdl", style("•").dim()),
        TrustStatus::Trusted => println!("{} trusted", style("✓").green()),
        TrustStatus::Untrusted => println!(
            "{} untrusted — run `grove trust` to approve",
            style("✗").red()
        ),
    }
}
