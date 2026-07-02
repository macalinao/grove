//! `grove list` / `go` / `run`, including the `1` main-repo alias.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok, stdout};

#[test]
fn list_shows_main_and_new_worktrees() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    let out = ok(&r.grove(&["list"]));
    assert!(out.contains("main"), "list: {out}");
    assert!(out.contains("feature"), "list: {out}");
}

#[test]
fn list_json_emits_parseable_worktree_array() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    let out = ok(&r.grove(&["list", "--json"]));

    let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let arr = parsed.as_array().expect("a JSON array");
    // main + feature.
    assert_eq!(arr.len(), 2, "json: {out}");

    let feature = arr
        .iter()
        .find(|w| w["branch"] == "feature")
        .expect("feature worktree present");
    assert_eq!(feature["status"], "ok");
    assert_eq!(feature["path"], *r.wt("feature").to_string_lossy());
    assert_eq!(feature["locked"], false);
    assert!(feature["head"].is_string(), "head is the commit sha");
    // No forge remote configured, so PR annotation is absent.
    assert!(feature.get("pr").is_none(), "no forge -> no pr: {out}");
}

#[test]
fn list_porcelain_is_tab_separated_path_branch_status() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    let out = ok(&r.grove(&["list", "--porcelain"]));
    let main_line = out
        .lines()
        .find(|l| l.contains("\tmain\t"))
        .expect("a main line");
    let cols: Vec<&str> = main_line.split('\t').collect();
    assert_eq!(cols.len(), 3, "porcelain has 3 tab-separated columns");
    assert_eq!(cols[0], r.root.to_string_lossy());
    assert_eq!(cols[1], "main");
    // The feature worktree path is present.
    assert!(out.contains(&*r.wt("feature").to_string_lossy()));
}

#[test]
fn go_prints_worktree_path() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    let out = ok(&r.grove(&["go", "feature"]));
    assert_eq!(out.trim(), r.wt("feature").to_string_lossy());
}

#[test]
fn go_1_resolves_to_main_worktree() {
    let r = TestRepo::new();
    let out = ok(&r.grove(&["go", "1"]));
    assert_eq!(out.trim(), r.root.to_string_lossy());
}

#[test]
fn go_unknown_worktree_errors() {
    let r = TestRepo::new();
    let err = failed(&r.grove(&["go", "nope"]));
    assert!(err.to_lowercase().contains("nope") || err.to_lowercase().contains("no worktree"));
}

#[test]
fn run_executes_in_worktree_dir() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    let out = ok(&r.grove(&["run", "feature", "--", "pwd"]));
    assert_eq!(out.trim(), r.wt("feature").to_string_lossy());
}

#[test]
fn run_1_executes_in_main_worktree() {
    let r = TestRepo::new();
    let out = ok(&r.grove(&["run", "1", "--", "pwd"]));
    assert_eq!(out.trim(), r.root.to_string_lossy());
}

#[test]
fn run_propagates_child_exit_code() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    let out = r.grove(&["run", "feature", "--", "sh", "-c", "exit 7"]);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn version_succeeds_and_mentions_grove_or_version() {
    let r = TestRepo::new();
    let out = r.grove(&["version"]);
    ok(&out);
    let text = format!("{}{}", stdout(&out), common::stderr(&out)).to_lowercase();
    assert!(
        text.contains("grove") || text.contains("0."),
        "version: {text}"
    );
}
