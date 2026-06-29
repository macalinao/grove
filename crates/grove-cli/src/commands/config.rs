use anyhow::{Result, anyhow};
use bpaf::Bpaf;
use console::style;
use grove_core::{ConfigScope, Grove};

/// Get, set, add, list, or unset `grove.*` configuration.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Config {
    /// Write to / read from the global (~/.gitconfig) scope
    #[bpaf(long, switch)]
    global: bool,
    /// Write to / read from the local (.git/config) scope (default)
    #[bpaf(long, switch)]
    local: bool,
    /// Read from the system (/etc/gitconfig) scope
    #[bpaf(long, switch)]
    system: bool,
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
    /// Set a config value (replacing any existing value)
    #[bpaf(command)]
    Set {
        #[bpaf(positional("KEY"))]
        key: String,
        #[bpaf(positional("VALUE"))]
        value: String,
    },
    /// Append a value to a multi-valued key
    #[bpaf(command)]
    Add {
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
    let scope = resolve_scope(args)?;
    let grove = Grove::open()?;
    match &args.action {
        ConfigAction::Get { key } => get(&grove, key),
        ConfigAction::Set { key, value } => set(&grove, key, value, scope),
        ConfigAction::Add { key, value } => add(&grove, key, value, scope),
        ConfigAction::Unset { key } => unset(&grove, key, scope),
        ConfigAction::List => list(&grove),
    }
}

/// Map the `--local/--global/--system` switches to a single scope, rejecting
/// mutually exclusive combinations.
fn resolve_scope(args: &Config) -> Result<ConfigScope> {
    match (args.local, args.global, args.system) {
        (false, false, false) | (true, false, false) => Ok(ConfigScope::Local),
        (false, true, false) => Ok(ConfigScope::Global),
        (false, false, true) => Ok(ConfigScope::System),
        _ => Err(anyhow!("choose at most one of --local, --global, --system")),
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

fn set(grove: &Grove, key: &str, value: &str, scope: ConfigScope) -> Result<()> {
    let key = qualify(key);
    grove.repo.config_set(&key, value, scope)?;
    eprintln!("{} {key} = {value}", style("✓").green());
    Ok(())
}

fn add(grove: &Grove, key: &str, value: &str, scope: ConfigScope) -> Result<()> {
    let key = qualify(key);
    grove.repo.config_add(&key, value, scope)?;
    eprintln!("{} {key} += {value}", style("✓").green());
    Ok(())
}

fn unset(grove: &Grove, key: &str, scope: ConfigScope) -> Result<()> {
    let key = qualify(key);
    grove.repo.config_unset(&key, scope)?;
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
