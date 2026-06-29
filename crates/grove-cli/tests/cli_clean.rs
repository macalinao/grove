//! `grove clean` — PR-state cleanup via a fake `gh` CLI on PATH.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok};

/// A fake `gh` that reports a PR state derived from the `--head` branch name:
/// `merged-*` → MERGED, `closed-*` → CLOSED, `open-*` → OPEN, else no PR.
const FAKE_GH: &str = r#"#!/bin/sh
head=""
while [ $# -gt 0 ]; do
  case "$1" in
    --head) head="$2"; shift 2 ;;
    *) shift ;;
  esac
done
case "$head" in
  merged-*) printf 'MERGED\tmain\n' ;;
  closed-*) printf 'CLOSED\tmain\n' ;;
  open-*)   printf 'OPEN\tmain\n' ;;
  *) : ;;
esac
"#;

fn setup() -> TestRepo {
    let r = TestRepo::new();
    r.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/owner/repo.git",
    ]);
    r.write_exec("gh", FAKE_GH);
    r
}

fn clean(r: &TestRepo, args: &[&str]) -> std::process::Output {
    let mut full = vec!["clean"];
    full.extend_from_slice(args);
    r.grove_env(&full, &[])
}

#[test]
fn removes_merged_keeps_open() {
    let r = setup();
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    ok(&r.grove(&["new", "open-y", "--no-fetch"]));
    ok(&clean(&r, &["--merged", "--yes"]));
    assert!(
        !r.wt("merged-x").exists(),
        "merged worktree should be removed"
    );
    assert!(r.wt("open-y").is_dir(), "open worktree should be kept");
}

#[test]
fn closed_flag_removes_closed() {
    let r = setup();
    ok(&r.grove(&["new", "closed-z", "--no-fetch"]));
    ok(&clean(&r, &["--closed", "--yes"]));
    assert!(!r.wt("closed-z").exists());
}

#[test]
fn dry_run_keeps_everything() {
    let r = setup();
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    ok(&clean(&r, &["--merged", "--dry-run"]));
    assert!(r.wt("merged-x").is_dir(), "dry-run must not remove");
}

#[test]
fn to_filter_limits_by_base_branch() {
    let r = setup();
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    // Fake gh reports base = main; filtering to develop should skip it.
    ok(&clean(&r, &["--merged", "--to", "develop", "--yes"]));
    assert!(r.wt("merged-x").is_dir(), "base mismatch should keep it");
    // Filtering to main should remove it.
    ok(&clean(&r, &["--merged", "--to", "main", "--yes"]));
    assert!(!r.wt("merged-x").exists());
}

#[test]
fn no_flags_just_prunes() {
    let r = setup();
    let out = clean(&r, &[]);
    ok(&out);
    assert!(common::stderr(&out).to_lowercase().contains("prune"));
}

#[test]
fn errors_without_a_forge_remote() {
    // No origin remote at all → provider can't be detected.
    let r = TestRepo::new();
    let err = failed(&r.grove(&["clean", "--merged"]));
    assert!(err.to_lowercase().contains("forge") || err.to_lowercase().contains("provider"));
}
