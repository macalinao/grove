//! Small terminal-interaction helpers.

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use anyhow::Result;

/// Apply color preferences from the environment, mirroring gtr's precedence:
/// `NO_COLOR` (disable, per no-color.org) wins over `GROVE_COLOR`
/// (`always`/`never`/`auto`); otherwise `console`'s auto-detection stands.
pub fn apply_color_env() {
    if std::env::var_os("NO_COLOR").is_some() {
        console::set_colors_enabled(false);
        return;
    }
    match std::env::var("GROVE_COLOR").as_deref() {
        Ok("always") => console::set_colors_enabled(true),
        Ok("never") => console::set_colors_enabled(false),
        _ => {}
    }
}

/// Is `program` an executable found on `PATH`? Used by `grove adapter` to mark
/// adapters ready vs missing.
#[must_use]
pub fn is_on_path(program: &str) -> bool {
    // An explicit path is checked directly.
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
}

/// Ask the user to confirm a destructive action.
///
/// Returns `true` immediately when `assume_yes` is set. When stdin is not a
/// terminal (and `--yes` was not given) the action is refused rather than
/// silently proceeding. Otherwise prompts `… [y/N]` on stderr and reads stdin.
///
/// # Errors
/// Returns an error if reading from stdin fails.
pub fn confirm(prompt: &str, assume_yes: bool) -> Result<bool> {
    if assume_yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    eprint!("{prompt} [y/N] ");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
