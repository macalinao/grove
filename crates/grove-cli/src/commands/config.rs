use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use console::style;
use grove_core::Grove;

/// Get, set, list, or unset `grove.*` configuration.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Config {
    /// Operate on global (~/.gitconfig) config instead of this repo
    #[bpaf(long, switch)]
    global: bool,
    #[bpaf(external(config_action))]
    action: ConfigAction,
}

#[derive(Debug, Clone, Bpaf)]
pub enum ConfigAction {
    /// Print a config value
    #[bpaf(command)]
    Get {
        /// Config key (a bare `worktrees.dir` is qualified to `grove.worktrees.dir`)
        #[bpaf(positional("KEY"))]
        key: String,
    },
    /// Set a config value
    #[bpaf(command)]
    Set {
        #[bpaf(positional("KEY"))]
        key: String,
        #[bpaf(positional("VALUE"))]
        value: String,
    },
    /// Remove a config value
    #[bpaf(command)]
    Unset {
        #[bpaf(positional("KEY"))]
        key: String,
    },
    /// List all grove.* config values
    #[bpaf(command)]
    List,
}

pub fn execute(args: &Config) -> Result<()> {
    let grove = Grove::open()?;
    match &args.action {
        ConfigAction::Get { key } => get(&grove, key),
        ConfigAction::Set { key, value } => set(&grove, key, value, args.global),
        ConfigAction::Unset { key } => unset(&grove, key, args.global),
        ConfigAction::List => list(&grove),
    }
}

fn get(grove: &Grove, key: &str) -> Result<()> {
    let key = qualify(key);
    match grove.repo.config_get(&key)? {
        Some(value) => {
            println!("{value}");
            Ok(())
        }
        None => Err(anyhow!("{key} is not set")),
    }
}

fn set(grove: &Grove, key: &str, value: &str, global: bool) -> Result<()> {
    let key = qualify(key);
    grove.repo.config_set(&key, value, global)?;
    eprintln!("{} {key} = {value}", style("✓").green());
    Ok(())
}

fn unset(grove: &Grove, key: &str, global: bool) -> Result<()> {
    let key = qualify(key);
    grove.repo.config_unset(&key, global)?;
    eprintln!("{} unset {key}", style("✓").green());
    Ok(())
}

fn list(grove: &Grove) -> Result<()> {
    for (key, value) in grove.repo.config_list_grove()? {
        println!("{key} = {value}");
    }
    Ok(())
}

/// Qualify a bare key (`worktrees.dir`) into the `grove.` namespace.
fn qualify(key: &str) -> String {
    if key.starts_with("grove.") {
        key.to_string()
    } else {
        format!("grove.{key}")
    }
}
