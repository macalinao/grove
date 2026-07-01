//! `grove trust` + `grove tasks` — the trust gate and the task DAG.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok};

const KDL: &str = r#"
tasks {
    task "first"  { run "echo a >> ORDER" }
    task "second" { run "echo b >> ORDER"; needs "first" }
}
"#;

#[test]
fn tasks_refuse_to_run_until_trusted() {
    let r = TestRepo::new();
    r.write("grove.kdl", KDL);
    let err = failed(&r.grove(&["tasks"]));
    assert!(err.to_lowercase().contains("trust"), "stderr: {err}");
    assert!(!r.path("ORDER").exists(), "no task should have run");
}

#[test]
fn tasks_list_is_safe_without_trust() {
    let r = TestRepo::new();
    r.write("grove.kdl", KDL);
    let out = ok(&r.grove(&["tasks", "--list"]));
    assert!(
        out.contains("first") && out.contains("second"),
        "list: {out}"
    );
    assert!(!r.path("ORDER").exists());
}

#[test]
fn trust_then_tasks_run_in_dependency_order() {
    let r = TestRepo::new();
    r.write("grove.kdl", KDL);
    ok(&r.grove(&["trust"]));
    ok(&r.grove(&["tasks"]));
    let order = std::fs::read_to_string(r.path("ORDER")).unwrap();
    assert_eq!(order, "a\nb\n", "second must run after first");
}

#[test]
fn trust_status_reports_untrusted_then_trusted() {
    let r = TestRepo::new();
    r.write("grove.kdl", KDL);
    assert!(
        ok(&r.grove(&["trust", "--list"]))
            .to_lowercase()
            .contains("untrusted")
    );
    ok(&r.grove(&["trust"]));
    assert!(
        ok(&r.grove(&["trust", "--list"]))
            .to_lowercase()
            .contains("trusted")
    );
}

#[test]
fn editing_kdl_revokes_trust() {
    let r = TestRepo::new();
    r.write("grove.kdl", KDL);
    ok(&r.grove(&["trust"]));
    // Changing the file changes its hash, so trust no longer applies.
    r.write("grove.kdl", &format!("{KDL}\n// changed\n"));
    let err = failed(&r.grove(&["tasks"]));
    assert!(err.to_lowercase().contains("trust"));
}

#[test]
fn no_kdl_means_nothing_to_trust() {
    let r = TestRepo::new();
    let out = ok(&r.grove(&["tasks"]));
    assert!(out.to_lowercase().contains("no tasks") || out.is_empty());
}
