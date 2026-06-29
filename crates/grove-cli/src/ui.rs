//! Small terminal-interaction helpers.

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use anyhow::Result;

/// Apply color preferences, mirroring gtr's precedence:
/// `NO_COLOR` (disable, per no-color.org) > `GROVE_COLOR` (`always`/`never`) >
/// the `grove.ui.color` / `gtr.ui.color` git-config value > `console`'s
/// auto-detection.
pub fn apply_color() {
    if std::env::var_os("NO_COLOR").is_some() {
        console::set_colors_enabled(false);
        return;
    }
    match std::env::var("GROVE_COLOR").as_deref() {
        Ok("always") => {
            console::set_colors_enabled(true);
            return;
        }
        Ok("never") => {
            console::set_colors_enabled(false);
            return;
        }
        _ => {}
    }
    // Lightweight git-config read (covers the common case without loading the
    // full layered config); a missing repo/key is a harmless no-op.
    if let Some(v) = config_color() {
        match v.as_str() {
            "always" => console::set_colors_enabled(true),
            "never" => console::set_colors_enabled(false),
            _ => {}
        }
    }
}

/// Read `grove.ui.color`, falling back to `gtr.ui.color`, from git config.
fn config_color() -> Option<String> {
    for key in ["grove.ui.color", "gtr.ui.color"] {
        let out = std::process::Command::new("git")
            .args(["config", "--get", key])
            .output()
            .ok()?;
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
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
