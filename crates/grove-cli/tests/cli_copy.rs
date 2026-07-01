//! `grove copy` — glob files, directories, `--all`, `--dry-run`, `--from`.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, ok, stderr};

/// Create a worktree with copying disabled, so `grove copy` is what populates it.
fn worktree(r: &TestRepo, name: &str) {
    ok(&r.grove(&["new", name, "--no-fetch", "--no-copy"]));
}

#[test]
fn copies_files_matching_include_glob() {
    let r = TestRepo::new();
    r.write(".env", "S=1");
    r.write("other.txt", "no");
    r.add_config("grove.copy.include", ".env");
    worktree(&r, "feature");
    ok(&r.grove(&["copy", "feature"]));
    assert!(r.wt("feature").join(".env").is_file());
    assert!(!r.wt("feature").join("other.txt").exists());
}

#[test]
fn exclude_overrides_include() {
    let r = TestRepo::new();
    r.write("a.env", "1");
    r.write("secret.env", "2");
    r.add_config("grove.copy.include", "*.env");
    r.add_config("grove.copy.exclude", "secret.env");
    worktree(&r, "feature");
    ok(&r.grove(&["copy", "feature"]));
    assert!(r.wt("feature").join("a.env").is_file());
    assert!(!r.wt("feature").join("secret.env").exists());
}

#[test]
fn copies_directories() {
    let r = TestRepo::new();
    r.write("vendor/lib/x", "v");
    r.add_config("grove.copy.includeDirs", "vendor");
    worktree(&r, "feature");
    ok(&r.grove(&["copy", "feature"]));
    assert!(r.wt("feature").join("vendor/lib/x").is_file());
}

#[test]
fn dry_run_writes_nothing() {
    let r = TestRepo::new();
    r.write(".env", "1");
    r.add_config("grove.copy.include", ".env");
    worktree(&r, "feature");
    let out = ok(&r.grove(&["copy", "feature", "--dry-run"]));
    assert!(out.to_lowercase().contains("would") || out.contains(".env"));
    assert!(!r.wt("feature").join(".env").exists());
}

#[test]
fn all_copies_into_every_other_worktree() {
    let r = TestRepo::new();
    r.write(".env", "1");
    r.add_config("grove.copy.include", ".env");
    worktree(&r, "a");
    worktree(&r, "b");
    ok(&r.grove(&["copy", "--all"]));
    assert!(r.wt("a").join(".env").is_file());
    assert!(r.wt("b").join(".env").is_file());
}

#[test]
fn from_uses_alternate_source() {
    let r = TestRepo::new();
    r.add_config("grove.copy.include", "marker.txt");
    worktree(&r, "src");
    worktree(&r, "dst");
    // Put the file only in the `src` worktree, then copy src -> dst.
    std::fs::write(r.wt("src").join("marker.txt"), "hi").unwrap();
    ok(&r.grove(&["copy", "dst", "--from", "src"]));
    assert!(r.wt("dst").join("marker.txt").is_file());
}

#[test]
fn no_patterns_configured_is_a_friendly_noop() {
    let r = TestRepo::new();
    worktree(&r, "feature");
    let out = r.grove(&["copy", "feature"]);
    ok(&out);
    assert!(stderr(&out).to_lowercase().contains("no copy patterns"));
}
