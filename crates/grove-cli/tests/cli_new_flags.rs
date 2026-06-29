//! `grove new` flag behaviors that need a worktree/remote to observe:
//! `--from-current`, `--track`, `--no-hooks`, `-e/--editor`, `-a/--ai`.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, ok, stderr};

#[test]
fn from_current_bases_on_the_checked_out_branch() {
    let r = TestRepo::new();
    r.add_origin();
    // A feature branch with a commit that origin/main does NOT have.
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("only_on_feature.txt", "x");
    r.git(&["add", "only_on_feature.txt"]);
    r.git(&["commit", "-q", "-m", "feature work"]);

    // --from-current bases the new worktree on `feature` (file present)…
    ok(&r.grove(&["new", "variant", "--no-fetch", "--from-current"]));
    assert!(r.wt("variant").join("only_on_feature.txt").is_file());

    // …whereas the default base (origin/main) does not have it.
    ok(&r.grove(&["new", "plain", "--no-fetch"]));
    assert!(!r.wt("plain").join("only_on_feature.txt").exists());
}

#[test]
fn track_auto_tracks_an_existing_remote_branch() {
    let r = TestRepo::new();
    r.add_origin();
    // Publish a branch on origin, then drop it locally.
    r.git(&["branch", "shared"]);
    r.git(&["push", "-q", "origin", "shared"]);
    r.git(&["branch", "-D", "shared"]);

    // auto + existing remote branch → new branch tracks origin/shared.
    ok(&r.grove(&["new", "shared", "--no-fetch", "--track", "auto"]));
    let upstream = r.git(&["rev-parse", "--abbrev-ref", "shared@{upstream}"]);
    assert!(upstream.status.success(), "upstream should be set");
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "origin/shared"
    );
}

#[test]
fn track_remote_requires_the_remote_branch() {
    let r = TestRepo::new();
    r.add_origin();
    // No origin/missing → --track remote must fail.
    let out = r.grove(&["new", "missing", "--no-fetch", "--track", "remote"]);
    assert!(!out.status.success());
    assert!(!r.wt("missing").exists());
}

#[test]
fn track_local_requires_an_existing_local_branch() {
    let r = TestRepo::new();
    r.add_origin();
    let out = r.grove(&["new", "nope", "--no-fetch", "--track", "local"]);
    assert!(!out.status.success());
}

#[test]
fn no_hooks_skips_post_create_hook() {
    let r = TestRepo::new();
    r.add_config(
        "grove.hooks.postCreate",
        &format!("touch {}", r.path("HOOK_RAN").display()),
    );
    // Without --no-hooks the postCreate hook fires…
    ok(&r.grove(&["new", "a", "--no-fetch", "--from", "main"]));
    assert!(r.path("HOOK_RAN").exists());

    std::fs::remove_file(r.path("HOOK_RAN")).unwrap();
    // …and --no-hooks suppresses it.
    ok(&r.grove(&["new", "b", "--no-fetch", "--from", "main", "--no-hooks"]));
    assert!(!r.path("HOOK_RAN").exists());
}

#[test]
fn editor_flag_launches_editor_after_create() {
    let r = TestRepo::new();
    r.write_exec(
        "myeditor",
        "#!/bin/sh\nprintf '%s' \"$1\" > \"$GROVE_TEST_MARK\"\n",
    );
    r.set_config("grove.editor.default", "myed");
    r.set_config("grove.editor.myed.command", "myeditor");
    let mark = r.path("EDITOR_OPENED");
    let out = r.grove_env(
        &[
            "new",
            "feature",
            "--no-fetch",
            "--from",
            "main",
            "--no-copy",
            "-e",
        ],
        &[("GROVE_TEST_MARK", &mark.to_string_lossy())],
    );
    ok(&out);
    assert_eq!(
        std::fs::read_to_string(&mark).unwrap(),
        r.wt("feature").to_string_lossy()
    );
}

#[test]
fn ai_flag_launches_ai_after_create() {
    let r = TestRepo::new();
    r.write_exec("myai", "#!/bin/sh\npwd > \"$GROVE_TEST_MARK\"\n");
    r.set_config("grove.ai.default", "ma");
    r.set_config("grove.ai.ma.command", "myai");
    let mark = r.path("AI_STARTED");
    let out = r.grove_env(
        &[
            "new",
            "feature",
            "--no-fetch",
            "--from",
            "main",
            "--no-copy",
            "-a",
        ],
        &[("GROVE_TEST_MARK", &mark.to_string_lossy())],
    );
    ok(&out);
    assert_eq!(
        std::fs::read_to_string(&mark).unwrap().trim(),
        r.wt("feature").to_string_lossy()
    );
}

#[test]
fn color_config_never_suppresses_ansi() {
    let r = TestRepo::new();
    r.set_config("grove.ui.color", "always");
    // `grove config set` prints a styled ✓; with color=always it has ANSI codes.
    let out = r.grove(&["config", "set", "grove.foo", "bar"]);
    assert!(
        stderr(&out).contains('\u{1b}'),
        "expected ANSI with color=always"
    );

    r.set_config("grove.ui.color", "never");
    let out = r.grove(&["config", "set", "grove.foo", "baz"]);
    assert!(!stderr(&out).contains('\u{1b}'), "no ANSI with color=never");
}
