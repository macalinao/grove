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

/// The shell integration script for `shell`, defining a function named `name`
/// (default `grove`; override with `grove init <shell> --as <name>`).
///
/// Eval it from your shell rc, e.g. `eval "$(grove init zsh)"` (bash/zsh) or
/// `grove init fish | source` (fish). The function intercepts `<name> cd` (with
/// an fzf picker when no argument is given) and `<name> new … --cd`; everything
/// else passes through to the real `grove` binary.
#[must_use]
pub fn init_script(shell: Shell, name: &str) -> String {
    let template = match shell {
        Shell::Bash | Shell::Zsh => POSIX,
        Shell::Fish => FISH,
    };
    template.replace("__FN__", name)
}

const POSIX: &str = r#"__FN__() {
  case "$1" in
    cd)
      shift
      local _grove_dir=""
      if [ "$#" -eq 0 ]; then
        if ! command -v fzf >/dev/null 2>&1; then
          echo "grove cd: pass a worktree name, or install fzf for an interactive picker" >&2
          return 1
        fi
        _grove_dir="$(command grove list --porcelain \
          | fzf --ansi --delimiter='\t' --with-nth=2,3 --prompt='worktree> ' \
                --preview 'git -C {1} log --oneline -15 2>/dev/null; echo; git -C {1} status -s 2>/dev/null' \
                --header 'enter=cd  ctrl-e=editor  ctrl-a=ai  ctrl-d=delete' \
                --bind 'ctrl-e:execute(command grove editor {2})' \
                --bind 'ctrl-a:execute(command grove ai {2})' \
                --bind 'ctrl-d:execute(command grove rm {2} --yes)+reload(command grove list --porcelain)' \
          | cut -f1)"
        [ -z "$_grove_dir" ] && return 0
      else
        _grove_dir="$(command grove go "$@")" || return $?
      fi
      if [ -n "$_grove_dir" ]; then
        builtin cd "$_grove_dir" || return $?
        eval "$(command grove post-cd "$_grove_dir" 2>/dev/null)"
      fi
      ;;
    new)
      case " $* " in
        *" --cd "*)
          local _grove_dir
          _grove_dir="$(command grove "$@")" || return $?
          if [ -n "$_grove_dir" ]; then
            builtin cd "$_grove_dir" || return $?
            eval "$(command grove post-cd "$_grove_dir" 2>/dev/null)"
          fi
          ;;
        *)
          command grove "$@"
          ;;
      esac
      ;;
    *)
      command grove "$@"
      ;;
  esac
}
"#;

const FISH: &str = r#"function __FN__
    switch "$argv[1]"
        case cd
            set -l _grove_dir
            if test (count $argv) -eq 1
                if not command -v fzf >/dev/null 2>&1
                    echo "grove cd: pass a worktree name, or install fzf for an interactive picker" >&2
                    return 1
                end
                set _grove_dir (command grove list --porcelain \
                    | fzf --ansi --delimiter=\t --with-nth=2,3 --prompt='worktree> ' \
                          --preview 'git -C {1} log --oneline -15 2>/dev/null; echo; git -C {1} status -s 2>/dev/null' \
                          --header 'enter=cd  ctrl-e=editor  ctrl-a=ai  ctrl-d=delete' \
                          --bind 'ctrl-e:execute(command grove editor {2})' \
                          --bind 'ctrl-a:execute(command grove ai {2})' \
                          --bind 'ctrl-d:execute(command grove rm {2} --yes)+reload(command grove list --porcelain)' \
                    | cut -f1)
                test -z "$_grove_dir"; and return 0
            else
                set _grove_dir (command grove go $argv[2..-1]); or return $status
            end
            if test -n "$_grove_dir"
                builtin cd $_grove_dir; or return $status
                command grove post-cd $_grove_dir 2>/dev/null | source
            end
        case new
            if contains -- --cd $argv
                set -l _grove_dir (command grove $argv); or return $status
                if test -n "$_grove_dir"
                    builtin cd $_grove_dir; or return $status
                    command grove post-cd $_grove_dir 2>/dev/null | source
                end
            else
                command grove $argv
            end
        case '*'
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
            let s = init_script(shell, "grove");
            assert!(s.contains("grove() {"), "{}", shell.as_str());
            assert!(s.contains("command grove go"));
            assert!(s.contains("builtin cd"));
            assert!(s.contains("command grove \"$@\""));
        }
    }

    #[test]
    fn fish_script_defines_function_and_cd() {
        let s = init_script(Shell::Fish, "grove");
        assert!(s.contains("function grove"));
        assert!(s.contains("command grove go"));
        assert!(s.contains("builtin cd"));
    }

    #[test]
    fn scripts_wire_fzf_picker_and_post_cd() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = init_script(shell, "grove");
            assert!(s.contains("fzf"), "{} picker", shell.as_str());
            assert!(s.contains("grove list --porcelain"), "{}", shell.as_str());
            assert!(s.contains("grove post-cd"), "{} postCd", shell.as_str());
        }
    }

    #[test]
    fn picker_has_keybindings() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = init_script(shell, "grove");
            assert!(s.contains("--preview"), "{} preview", shell.as_str());
            for bind in ["ctrl-e", "ctrl-a", "ctrl-d"] {
                assert!(s.contains(bind), "{} missing {bind}", shell.as_str());
            }
        }
    }

    #[test]
    fn new_cd_is_intercepted() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let s = init_script(shell, "grove");
            assert!(s.contains("--cd"), "{} new --cd", shell.as_str());
        }
    }

    #[test]
    fn custom_function_name_via_as() {
        let posix = init_script(Shell::Bash, "gw");
        assert!(posix.contains("gw() {"));
        // Internal calls still target the real `grove` binary.
        assert!(posix.contains("command grove"));
        assert!(!posix.contains("grove() {"));
        let fish = init_script(Shell::Fish, "gw");
        assert!(fish.contains("function gw"));
    }
}
