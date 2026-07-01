//! `grove config` — get/set/add/unset/list, bare-key qualification, scopes.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok};

#[test]
fn set_then_get_round_trips() {
    let r = TestRepo::new();
    ok(&r.grove(&["config", "set", "grove.editor.default", "cursor"]));
    assert_eq!(
        ok(&r.grove(&["config", "get", "grove.editor.default"])).trim(),
        "cursor"
    );
}

#[test]
fn bare_key_is_qualified_into_grove_namespace() {
    let r = TestRepo::new();
    ok(&r.grove(&["config", "set", "worktrees.prefix", "wt-"]));
    // Readable under both the bare and qualified key.
    assert_eq!(
        ok(&r.grove(&["config", "get", "worktrees.prefix"])).trim(),
        "wt-"
    );
    assert_eq!(
        ok(&r.grove(&["config", "get", "grove.worktrees.prefix"])).trim(),
        "wt-"
    );
    // And git stored it under grove.*.
    assert_eq!(
        String::from_utf8_lossy(&r.git(&["config", "--get", "grove.worktrees.prefix"]).stdout)
            .trim(),
        "wt-"
    );
}

#[test]
fn add_appends_multi_valued_key() {
    let r = TestRepo::new();
    ok(&r.grove(&["config", "add", "copy.include", ".env"]));
    ok(&r.grove(&["config", "add", "copy.include", "*.local"]));
    let values =
        String::from_utf8_lossy(&r.git(&["config", "--get-all", "grove.copy.include"]).stdout)
            .into_owned();
    assert!(
        values.contains(".env") && values.contains("*.local"),
        "values: {values}"
    );
}

#[test]
fn unset_removes_value() {
    let r = TestRepo::new();
    ok(&r.grove(&["config", "set", "grove.ai.default", "claude"]));
    ok(&r.grove(&["config", "unset", "grove.ai.default"]));
    failed(&r.grove(&["config", "get", "grove.ai.default"]));
}

#[test]
fn list_shows_grove_keys() {
    let r = TestRepo::new();
    ok(&r.grove(&["config", "set", "grove.editor.default", "zed"]));
    let out = ok(&r.grove(&["config", "list"]));
    assert!(
        out.contains("grove.editor.default") && out.contains("zed"),
        "list: {out}"
    );
}

#[test]
fn get_unknown_key_errors() {
    let r = TestRepo::new();
    failed(&r.grove(&["config", "get", "grove.nope.key"]));
}

#[test]
fn global_scope_writes_to_global_config() {
    let r = TestRepo::new();
    ok(&r.grove(&["config", "set", "--global", "grove.editor.default", "vim"]));
    // It must NOT be in the repo-local config…
    assert!(
        !r.git(&["config", "--local", "--get", "grove.editor.default"])
            .status
            .success()
    );
    // …but the isolated global file now contains it.
    let global = std::fs::read_to_string(&r.global_cfg).unwrap();
    assert!(global.contains("vim"), "global cfg: {global}");
}

#[test]
fn rejects_multiple_scopes() {
    let r = TestRepo::new();
    failed(&r.grove(&["config", "--global", "--system", "set", "grove.x.y", "z"]));
}

/// A `.gtrconfig` in gtr's on-disk schema, exercising scalars, list-valued
/// `copy.*`, and multi-valued `hooks.*`.
const SAMPLE_GTRCONFIG: &str = "\
[worktrees]
\tdir = ../wt
\tprefix = wt-
[defaults]
\teditor = cursor
\tai = claude
\tbranch = main
[copy]
\tinclude = *.env
\tinclude = .env.local
[hooks]
\tpostCreate = pnpm install
\tpostCreate = echo done
";

#[test]
fn migrate_writes_grove_kdl_from_gtrconfig() {
    let r = TestRepo::new();
    r.write(".gtrconfig", SAMPLE_GTRCONFIG);

    ok(&r.grove(&["config", "migrate"]));

    let kdl = std::fs::read_to_string(r.path("grove.kdl")).unwrap();
    assert!(kdl.contains("worktrees {"), "kdl: {kdl}");
    assert!(kdl.contains("dir \"../wt\""), "kdl: {kdl}");
    assert!(kdl.contains("default \"cursor\""), "kdl: {kdl}");
    // List values collapse onto one node; multi-valued hooks stay separate.
    assert!(
        kdl.contains("include \"*.env\" \".env.local\""),
        "kdl: {kdl}"
    );
    assert!(kdl.contains("postCreate \"pnpm install\""), "kdl: {kdl}");
    assert!(kdl.contains("postCreate \"echo done\""), "kdl: {kdl}");
}

#[test]
fn migrate_dry_run_prints_without_writing() {
    let r = TestRepo::new();
    r.write(".gtrconfig", SAMPLE_GTRCONFIG);

    let out = ok(&r.grove(&["config", "migrate", "--dry-run"]));
    assert!(out.contains("worktrees {"), "stdout: {out}");
    assert!(
        !r.path("grove.kdl").exists(),
        "grove.kdl must not be written"
    );
}

#[test]
fn migrate_refuses_to_clobber_without_force() {
    let r = TestRepo::new();
    r.write(".gtrconfig", SAMPLE_GTRCONFIG);
    r.write("grove.kdl", "// hand-written\n");

    failed(&r.grove(&["config", "migrate"]));
    // The existing file is untouched…
    assert_eq!(
        std::fs::read_to_string(r.path("grove.kdl")).unwrap(),
        "// hand-written\n"
    );
    // …until --force.
    ok(&r.grove(&["config", "migrate", "--force"]));
    assert!(
        std::fs::read_to_string(r.path("grove.kdl"))
            .unwrap()
            .contains("worktrees {")
    );
}

#[test]
fn migrate_errors_when_no_gtrconfig() {
    let r = TestRepo::new();
    failed(&r.grove(&["config", "migrate"]));
}
