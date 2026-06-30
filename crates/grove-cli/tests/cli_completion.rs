//! Shell completion for worktree-name positionals (`grove cd`, `go`, …).
//!
//! Drives bpaf's dynamic-completion protocol the same way the sourced shell
//! script does (`grove --bpaf-complete-rev=N …`) and asserts the candidates are
//! the repo's worktrees. Also guards against the bpaf positional-ordering bug
//! that makes completion panic at runtime.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, ok, stdout};

/// Pull bpaf's protocol revision out of the bash registration script, so these
/// tests don't hard-code a version-specific number.
fn bash_rev(r: &TestRepo) -> String {
    let script = stdout(&r.grove(&["--bpaf-complete-style-bash"]));
    let marker = "--bpaf-complete-rev=";
    let start = script.find(marker).expect("registration names a revision") + marker.len();
    let rev: String = script[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    assert!(!rev.is_empty(), "no revision in: {script}");
    rev
}

/// Ask the binary to complete `args` (the words after the program name), the
/// way the shell completion function does. Asserts it doesn't panic/fail.
fn complete(r: &TestRepo, rev: &str, args: &[&str]) -> String {
    let mut full = vec![format!("--bpaf-complete-rev={rev}")];
    full.extend(args.iter().map(|s| (*s).to_string()));
    let refs: Vec<&str> = full.iter().map(String::as_str).collect();
    let out = r.grove(&refs);
    assert!(
        out.status.success(),
        "completion must not panic: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout(&out)
}

#[test]
fn registration_script_uses_bpaf_protocol() {
    let r = TestRepo::new();
    let script = stdout(&r.grove(&["--bpaf-complete-style-bash"]));
    assert!(script.contains("complete -"), "script: {script}");
    assert!(script.contains("--bpaf-complete-rev="), "script: {script}");
}

#[test]
fn cd_completion_lists_worktree_names() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature-x", "--no-fetch"]));
    ok(&r.grove(&["new", "bugfix/login", "--no-fetch"]));

    let rev = bash_rev(&r);
    let out = complete(&r, &rev, &["cd", ""]);

    assert!(out.contains("feature-x"), "out: {out}");
    assert!(out.contains("bugfix/login"), "out: {out}");
    // The main worktree and gtr's `1` alias are offered too.
    assert!(out.contains("main worktree"), "out: {out}");
}

#[test]
fn go_completion_lists_worktree_names() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature-x", "--no-fetch"]));

    let rev = bash_rev(&r);
    let out = complete(&r, &rev, &["go", ""]);
    assert!(out.contains("feature-x"), "out: {out}");
}

#[test]
fn completion_outside_repo_does_not_panic() {
    let r = TestRepo::new();
    let rev = bash_rev(&r);
    // `r.workspace` is not itself a git repo; discovery fails and we offer
    // nothing — but the process must still exit cleanly.
    let mut full = vec![format!("--bpaf-complete-rev={rev}")];
    full.extend(["cd", ""].iter().map(|s| (*s).to_string()));
    let refs: Vec<&str> = full.iter().map(String::as_str).collect();
    let out = r.grove_in(&r.workspace, &refs);
    assert!(
        out.status.success(),
        "completion outside a repo must not panic: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
