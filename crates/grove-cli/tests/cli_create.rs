//! `grove new` — creation, naming/sanitization, base resolution, copy-on-create.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok, stdout};

#[test]
fn creates_worktree_and_branch() {
    let r = TestRepo::new();
    let out = r.grove(&["new", "feature", "--no-fetch"]);
    ok(&out);
    assert!(r.wt("feature").is_dir(), "worktree dir should exist");
    // The branch exists and is checked out in the new worktree.
    let branches = String::from_utf8_lossy(&r.git(&["branch", "--list"]).stdout).into_owned();
    assert!(branches.contains("feature"), "branch list: {branches}");
}

#[test]
fn sanitizes_slashes_into_folder_name() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature/auth", "--no-fetch"]));
    // Folder uses the sanitized name…
    assert!(r.wt("feature-auth").is_dir());
    // …but the branch keeps its real name.
    let go = ok(&r.grove(&["go", "feature/auth"]));
    assert_eq!(go.trim(), r.wt("feature-auth").to_string_lossy());
}

#[test]
fn sanitizes_hash_and_special_chars() {
    let r = TestRepo::new();
    // These are valid git branch names; only the folder is sanitized.
    ok(&r.grove(&["new", "bug#42", "--no-fetch"]));
    assert!(
        r.wt("bug42").is_dir(),
        "'#' should be dropped from the folder"
    );

    ok(&r.grove(&["new", "feat+a@b", "--no-fetch"]));
    assert!(
        r.wt("feat-a-b").is_dir(),
        "'+' and '@' should become '-' in the folder"
    );
    // The branch keeps its original name.
    assert_eq!(
        ok(&r.grove(&["go", "feat+a@b"])).trim(),
        r.wt("feat-a-b").to_string_lossy()
    );
}

#[test]
fn folder_override_and_name_suffix() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch", "--folder", "custom-dir"]));
    assert!(r.wt("custom-dir").is_dir());

    ok(&r.grove(&["new", "feature2", "--no-fetch", "--name", "backend"]));
    assert!(r.wt("feature2-backend").is_dir());
}

#[test]
fn refuses_when_destination_exists() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "dup", "--no-fetch"]));
    let err = failed(&r.grove(&["new", "dup", "--no-fetch"]));
    assert!(
        err.to_lowercase().contains("exist") || err.to_lowercase().contains("already"),
        "stderr: {err}"
    );
}

#[test]
fn print_path_emits_only_the_path_on_stdout() {
    let r = TestRepo::new();
    let out = r.grove(&["new", "feature", "--no-fetch", "--print-path"]);
    ok(&out);
    assert_eq!(stdout(&out).trim(), r.wt("feature").to_string_lossy());
}

#[test]
fn copies_configured_files_on_create() {
    let r = TestRepo::new();
    r.write(".env", "SECRET=1");
    r.write("keep.txt", "no");
    r.add_config("grove.copy.include", ".env");
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    assert!(
        r.wt("feature").join(".env").is_file(),
        ".env should be copied"
    );
    assert!(
        !r.wt("feature").join("keep.txt").exists(),
        "non-matching files must not be copied"
    );
}

#[test]
fn copies_directories_with_exclude_on_create() {
    let r = TestRepo::new();
    r.write("node_modules/pkg/index.js", "x");
    r.write("node_modules/.cache/blob", "junk");
    r.add_config("grove.copy.includeDirs", "node_modules");
    r.add_config("grove.copy.excludeDirs", "node_modules/.cache");
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    assert!(r.wt("feature").join("node_modules/pkg/index.js").is_file());
    assert!(
        !r.wt("feature").join("node_modules/.cache").exists(),
        "excluded subdir must be pruned"
    );
}

#[test]
fn honors_worktreeinclude_file() {
    let r = TestRepo::new();
    r.write(".env.local", "X=1");
    r.write(".worktreeinclude", "# comment\n.env.local\n");
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    assert!(r.wt("feature").join(".env.local").is_file());
}

#[test]
fn no_copy_skips_copying() {
    let r = TestRepo::new();
    r.write(".env", "SECRET=1");
    r.add_config("grove.copy.include", ".env");
    ok(&r.grove(&["new", "feature", "--no-fetch", "--no-copy"]));
    assert!(!r.wt("feature").join(".env").exists());
}

#[test]
fn from_bases_new_branch_on_given_ref() {
    let r = TestRepo::new();
    // Make a second commit on main, tag the first.
    r.git(&["tag", "v0"]);
    r.write("a.txt", "a");
    r.git(&["add", "a.txt"]);
    r.git(&["commit", "-q", "-m", "second"]);
    ok(&r.grove(&["new", "hotfix", "--no-fetch", "--from", "v0"]));
    // The worktree's HEAD should be at v0 (a.txt absent).
    assert!(!r.wt("hotfix").join("a.txt").exists());
}
