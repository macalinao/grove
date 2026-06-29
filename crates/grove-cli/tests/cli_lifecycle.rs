//! `grove rm` / `mv` — removal, rename, confirmation gating, and hooks.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok, stderr};

fn branch_exists(r: &TestRepo, name: &str) -> bool {
    r.git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/heads/{name}"),
    ])
    .status
    .success()
}

#[test]
fn rm_removes_worktree_with_yes() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    assert!(r.wt("feature").is_dir());
    ok(&r.grove(&["rm", "feature", "--yes"]));
    assert!(!r.wt("feature").exists());
    // Branch is kept by default.
    assert!(branch_exists(&r, "feature"));
}

#[test]
fn rm_delete_branch_also_removes_branch() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    ok(&r.grove(&["rm", "feature", "--yes", "--delete-branch"]));
    assert!(!branch_exists(&r, "feature"));
}

#[test]
fn rm_without_yes_on_non_tty_aborts() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    // stdin is not a tty under the test harness, so an unconfirmed rm aborts.
    let out = r.grove(&["rm", "feature"]);
    ok(&out);
    assert!(stderr(&out).to_lowercase().contains("abort"));
    assert!(r.wt("feature").is_dir(), "worktree must survive the abort");
}

#[test]
fn rm_multiple_worktrees() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "a", "--no-fetch"]));
    ok(&r.grove(&["new", "b", "--no-fetch"]));
    ok(&r.grove(&["rm", "a", "b", "--yes"]));
    assert!(!r.wt("a").exists());
    assert!(!r.wt("b").exists());
}

#[test]
fn mv_renames_worktree_and_branch() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "old", "--no-fetch"]));
    ok(&r.grove(&["mv", "old", "new", "--yes"]));
    assert!(!r.wt("old").exists());
    assert!(r.wt("new").is_dir());
    assert!(branch_exists(&r, "new"));
    assert!(!branch_exists(&r, "old"));
}

#[test]
fn mv_without_yes_on_non_tty_aborts() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "old", "--no-fetch"]));
    let out = r.grove(&["mv", "old", "new"]);
    ok(&out);
    assert!(r.wt("old").is_dir());
    assert!(!r.wt("new").exists());
}

#[test]
fn pre_and_post_remove_hooks_run() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    // Hooks write marker files; preRemove runs in the worktree, postRemove in the repo.
    r.add_config(
        "grove.hooks.preRemove",
        &format!("touch {}", r.path("PRE_RAN").display()),
    );
    r.add_config(
        "grove.hooks.postRemove",
        &format!("touch {}", r.path("POST_RAN").display()),
    );
    ok(&r.grove(&["rm", "feature", "--yes"]));
    assert!(r.path("PRE_RAN").exists(), "preRemove hook should have run");
    assert!(
        r.path("POST_RAN").exists(),
        "postRemove hook should have run"
    );
}

#[test]
fn pre_remove_failure_aborts_without_force() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    r.add_config("grove.hooks.preRemove", "exit 1");
    let err = failed(&r.grove(&["rm", "feature", "--yes"]));
    assert!(err.to_lowercase().contains("hook") || err.to_lowercase().contains("preremove"));
    assert!(
        r.wt("feature").is_dir(),
        "failed preRemove must not remove the worktree"
    );
}

#[test]
fn pre_remove_failure_is_overridden_by_force() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    r.add_config("grove.hooks.preRemove", "exit 1");
    ok(&r.grove(&["rm", "feature", "--yes", "--force"]));
    assert!(!r.wt("feature").exists());
}
