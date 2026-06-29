//! Adapters (`editor`/`ai`/`adapter`), `init`, `completion`, `doctor`.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TestRepo, failed, ok, stdout};

#[test]
fn adapter_lists_editors_and_ai_tools() {
    let r = TestRepo::new();
    let out = ok(&r.grove(&["adapter"]));
    assert!(out.contains("Editors"), "adapter: {out}");
    assert!(out.contains("AI tools"));
    assert!(out.contains("cursor") && out.contains("claude"));
}

#[test]
fn completion_emits_setup_for_each_shell() {
    let r = TestRepo::new();
    assert!(ok(&r.grove(&["completion", "bash"])).contains("COMPLETE=bash"));
    assert!(ok(&r.grove(&["completion", "zsh"])).contains("COMPLETE=zsh"));
    assert!(ok(&r.grove(&["completion", "fish"])).contains("COMPLETE=fish"));
}

#[test]
fn completion_rejects_unknown_shell() {
    let r = TestRepo::new();
    failed(&r.grove(&["completion", "tcsh"]));
}

#[test]
fn init_emits_function_picker_and_post_cd() {
    let r = TestRepo::new();
    for shell in ["bash", "zsh", "fish"] {
        let out = ok(&r.grove(&["init", shell]));
        assert!(out.contains("grove"), "{shell}: missing function");
        assert!(out.contains("fzf"), "{shell}: missing fzf picker");
        assert!(out.contains("post-cd"), "{shell}: missing postCd");
        assert!(out.contains("--cd"), "{shell}: missing new --cd handling");
    }
}

#[test]
fn init_as_uses_custom_function_name() {
    let r = TestRepo::new();
    let bash = ok(&r.grove(&["init", "bash", "--as", "gw"]));
    assert!(bash.contains("gw() {"), "bash: {bash}");
    let fish = ok(&r.grove(&["init", "fish", "--as", "gw"]));
    assert!(fish.contains("function gw"), "fish: {fish}");
}

#[test]
fn doctor_reports_repo_and_cow_status() {
    let r = TestRepo::new();
    let out = r.grove(&["doctor"]);
    // doctor exits non-zero only if a check fails; in a valid repo it succeeds.
    let text = format!("{}{}", stdout(&out), common::stderr(&out));
    assert!(text.contains("repository"), "doctor: {text}");
    assert!(
        text.contains("copy-on-write"),
        "doctor should report CoW: {text}"
    );
}

#[test]
fn custom_editor_adapter_from_config_receives_worktree_path() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch", "--no-copy"]));
    // A fake editor records the path argument it was given.
    r.write_exec(
        "myeditor",
        "#!/bin/sh\nprintf '%s' \"$1\" > \"$GROVE_TEST_MARK\"\n",
    );
    r.set_config("grove.editor.myed.command", "myeditor");
    let mark = r.path("EDITOR_ARG");
    let out = r.grove_env(
        &["editor", "feature", "--editor", "myed"],
        &[("GROVE_TEST_MARK", &mark.to_string_lossy())],
    );
    assert!(out.status.success(), "stderr: {}", common::stderr(&out));
    assert_eq!(
        std::fs::read_to_string(&mark).unwrap(),
        r.wt("feature").to_string_lossy()
    );
}

#[test]
fn custom_ai_adapter_runs_in_worktree_cwd() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch", "--no-copy"]));
    // A fake AI tool records its working directory (AI tools get cwd=worktree).
    r.write_exec("myai", "#!/bin/sh\npwd > \"$GROVE_TEST_MARK\"\n");
    r.set_config("grove.ai.ma.command", "myai");
    let mark = r.path("AI_CWD");
    let out = r.grove_env(
        &["ai", "feature", "--ai", "ma"],
        &[("GROVE_TEST_MARK", &mark.to_string_lossy())],
    );
    assert!(out.status.success(), "stderr: {}", common::stderr(&out));
    assert_eq!(
        std::fs::read_to_string(&mark).unwrap().trim(),
        r.wt("feature").to_string_lossy()
    );
}

#[test]
fn unknown_editor_adapter_errors() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch", "--no-copy"]));
    failed(&r.grove(&["editor", "feature", "--editor", "does-not-exist"]));
}

#[test]
fn editor_and_ai_none_are_noops() {
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch", "--no-copy"]));
    // gtr's `none` adapter: don't launch anything, succeed.
    ok(&r.grove(&["editor", "feature", "--editor", "none"]));
    ok(&r.grove(&["ai", "feature", "--ai", "none"]));
}

#[test]
fn adapter_lists_gtr_built_ins() {
    let r = TestRepo::new();
    let out = ok(&r.grove(&["adapter"]));
    // A representative slice of gtr's registry (with gtr's program names).
    for name in [
        "antigravity",
        "emacs",
        "sublime",
        "webstorm",
        "auggie",
        "copilot",
        "continue",
    ] {
        assert!(out.contains(name), "adapter list missing {name}: {out}");
    }
}
