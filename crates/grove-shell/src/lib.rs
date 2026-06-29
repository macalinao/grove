//! Shell integration snippets emitted by `grove init <shell>`.
//!
//! The emitted snippet defines a `grove` shell function that intercepts
//! `grove cd <name>`: it resolves the worktree path via `grove go` and then
//! `cd`s the *current* shell there (a binary can't change its parent's
//! directory). Everything else passes through to the real binary.
//!
//! The db-env hook (design spec §7.5) plugs into the same snippet later.

use core::str::FromStr;

/// Shells Grove can emit integration for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
        }
    }
}

/// Error parsing a shell name.
#[derive(Debug, thiserror::Error)]
#[error("unknown shell '{0}' (expected: bash, zsh, fish)")]
pub struct UnknownShell(pub String);

impl FromStr for Shell {
    type Err = UnknownShell;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            other => Err(UnknownShell(other.to_owned())),
        }
    }
}

/// The shell integration script for `shell`.
///
/// Eval it from your shell rc, e.g. `eval "$(grove init zsh)"` (bash/zsh) or
/// `grove init fish | source` (fish).
#[must_use]
pub fn init_script(shell: Shell) -> String {
    match shell {
        Shell::Bash | Shell::Zsh => POSIX.to_owned(),
        Shell::Fish => FISH.to_owned(),
    }
}

const POSIX: &str = r#"grove() {
  if [ "$1" = "cd" ]; then
    shift
    local _grove_dir
    _grove_dir="$(command grove go "$@")" || return $?
    if [ -n "$_grove_dir" ]; then
      builtin cd "$_grove_dir" || return $?
      eval "$(command grove post-cd "$_grove_dir" 2>/dev/null)"
    fi
  else
    command grove "$@"
  fi
}
"#;

const FISH: &str = r#"function grove
    if test "$argv[1]" = cd
        set -l _grove_dir (command grove go $argv[2..-1]); or return $status
        if test -n "$_grove_dir"
            builtin cd $_grove_dir; or return $status
            command grove post-cd $_grove_dir 2>/dev/null | source
        end
    else
        command grove $argv
    end
end
"#;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_shell_names() {
        assert_eq!("bash".parse::<Shell>().unwrap(), Shell::Bash);
        assert_eq!("zsh".parse::<Shell>().unwrap(), Shell::Zsh);
        assert_eq!("fish".parse::<Shell>().unwrap(), Shell::Fish);
        assert!("tcsh".parse::<Shell>().is_err());
    }

    #[test]
    fn posix_script_defines_function_and_cd() {
        for shell in [Shell::Bash, Shell::Zsh] {
            let s = init_script(shell);
            assert!(s.contains("grove() {"), "{}", shell.as_str());
            assert!(s.contains("command grove go"));
            assert!(s.contains("builtin cd"));
            assert!(s.contains("command grove \"$@\""));
        }
    }

    #[test]
    fn fish_script_defines_function_and_cd() {
        let s = init_script(Shell::Fish);
        assert!(s.contains("function grove"));
        assert!(s.contains("command grove go"));
        assert!(s.contains("builtin cd"));
    }
}
