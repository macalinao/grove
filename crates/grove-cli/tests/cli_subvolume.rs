//! `grove.worktrees.subvolume`: worktrees as btrfs subvolumes, with a plain-dir
//! fallback off btrfs. Run with `TMPDIR` on a btrfs mount to exercise real
//! subvolumes; elsewhere these cover the fallback.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok, stderr};
use grove_core::subvol;

#[test]
fn off_by_default_makes_plain_dirs() {
    let r = TestRepo::new();
    let out = r.grove(&["new", "feature", "--no-fetch"]);
    ok(&out);
    assert!(!subvol::is_subvolume(&r.wt("feature")));
    assert!(!stderr(&out).contains("subvolume"));
}

#[test]
fn new_and_rm_with_subvolume_on() {
    let r = TestRepo::new();
    r.set_config("grove.worktrees.subvolume", "true");
    let out = r.grove(&["new", "feature", "--no-fetch"]);
    ok(&out);
    let wt = r.wt("feature");
    assert!(wt.join(".git").exists());
    if subvol::on_btrfs(wt.parent().unwrap()) {
        assert!(subvol::is_subvolume(&wt));
    } else {
        assert!(!subvol::is_subvolume(&wt));
        assert!(stderr(&out).contains("using a plain directory"));
    }

    ok(&r.grove(&["rm", "feature", "--yes"]));
    assert!(!wt.exists());
    assert!(!r.worktree_folders().contains(&"feature".to_string()));
}

#[test]
fn rm_keeps_a_dirty_subvolume_worktree_without_force() {
    let r = TestRepo::new();
    r.set_config("grove.worktrees.subvolume", "true");
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    let wt = r.wt("feature");
    std::fs::write(wt.join("untracked.txt"), "x").unwrap();

    failed(&r.grove(&["rm", "feature", "--yes"]));
    assert!(wt.join("untracked.txt").exists());

    ok(&r.grove(&["rm", "feature", "--yes", "--force"]));
    assert!(!wt.exists());
}

#[test]
fn failed_add_leaves_no_subvolume_behind() {
    let r = TestRepo::new();
    r.set_config("grove.worktrees.subvolume", "true");
    // An unknown base ref makes `git worktree add` fail after the dir is made.
    failed(&r.grove(&["new", "feature", "--no-fetch", "--from", "no-such-ref"]));
    assert!(!r.wt("feature").exists());
}
