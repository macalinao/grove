//! gtr-compatibility config readers (lowest precedence).
//!
//! Three compat sources feed grove config below its native `grove.*` git-config
//! and `grove.kdl`:
//!   1. `gtr.*` keys in the user's git config (migration convenience)
//!   2. `.gtrconfig` at the repo root — gtr's team-shared file
//!   3. `.groveconfig` at the repo root — grove's own file
//!
//! `.gtrconfig` uses gtr's exact on-disk schema (see gtr's `_CFG_KEY_MAP`):
//! unprefixed, INI-section keys like `worktrees.dir`, `defaults.editor`,
//! `editor.workspace`, `copy.include`, `hooks.postCreate`, `ui.color`. This is
//! NOT the same as gtr's git-config keys (`gtr.editor.default`, `gtr.hook.*`),
//! so the two are mapped separately. Best-effort: malformed files are skipped.

use std::path::Path;

use grove_git::Repo;

use crate::{ColorChoice, Config, TrackMode};

/// Apply `.gtrconfig` then `.groveconfig` (lowest-precedence files) onto `cfg`.
pub fn apply(cfg: &mut Config, root: &Path) {
    apply_gtrconfig(cfg, &root.join(".gtrconfig"));
    apply_groveconfig(cfg, &root.join(".groveconfig"));
}

/// Apply `gtr.*` keys from the merged git config (lower than the files), to ease
/// migrating a user who has gtr defaults in `~/.gitconfig`.
pub fn apply_gtr_gitconfig(cfg: &mut Config, repo: &Repo) {
    let get = |k: &str| repo.config_get(k).ok().flatten();
    let get_all = |k: &str| repo.config_get_all(k).unwrap_or_default();

    if let Some(v) = get("gtr.worktrees.dir") {
        cfg.worktrees_dir = Some(v);
    }
    if let Some(v) = get("gtr.worktrees.prefix") {
        cfg.worktrees_prefix = v;
    }
    if let Some(v) = get("gtr.editor.default") {
        cfg.editor_default = Some(v);
    }
    if let Some(v) = get("gtr.editor.workspace") {
        cfg.editor_workspace = Some(v);
    }
    if let Some(v) = get("gtr.ai.default") {
        cfg.ai_default = Some(v);
    }
    if let Some(v) = get("gtr.ui.color") {
        cfg.color = ColorChoice::parse(&v);
    }
    if let Some(v) = get("gtr.defaultRemote") {
        cfg.default_remote = Some(v);
    }
    if let Some(v) = get("gtr.defaultBranch") {
        cfg.default_branch = Some(v);
    }
    if let Some(v) = get("gtr.provider") {
        cfg.provider = Some(v);
    }
    set_if_present(&mut cfg.copy_include, get_all("gtr.copy.include"));
    set_if_present(&mut cfg.copy_exclude, get_all("gtr.copy.exclude"));
    set_if_present(&mut cfg.copy_include_dirs, get_all("gtr.copy.includeDirs"));
    set_if_present(&mut cfg.copy_exclude_dirs, get_all("gtr.copy.excludeDirs"));
    // git-config hook keys are singular `gtr.hook.*` (per gtr's source).
    set_if_present(&mut cfg.hook_post_create, get_all("gtr.hook.postCreate"));
    set_if_present(&mut cfg.hook_pre_remove, get_all("gtr.hook.preRemove"));
    set_if_present(&mut cfg.hook_post_remove, get_all("gtr.hook.postRemove"));
    set_if_present(&mut cfg.hook_post_cd, get_all("gtr.hook.postCd"));
}

/// Read a real `.gtrconfig` file using gtr's on-disk key schema.
fn apply_gtrconfig(cfg: &mut Config, path: &Path) {
    if !path.exists() {
        return;
    }
    let get = |k: &str| grove_git::config_file_get(path, k).ok().flatten();
    let get_all = |k: &str| grove_git::config_file_get_all(path, k).unwrap_or_default();

    if let Some(v) = get("worktrees.dir") {
        cfg.worktrees_dir = Some(v);
    }
    if let Some(v) = get("worktrees.prefix") {
        cfg.worktrees_prefix = v;
    }
    if let Some(v) = get("defaults.editor") {
        cfg.editor_default = Some(v);
    }
    if let Some(v) = get("editor.workspace") {
        cfg.editor_workspace = Some(v);
    }
    if let Some(v) = get("defaults.ai") {
        cfg.ai_default = Some(v);
    }
    if let Some(v) = get("ui.color") {
        cfg.color = ColorChoice::parse(&v);
    }
    if let Some(v) = get("defaults.remote") {
        cfg.default_remote = Some(v);
    }
    if let Some(v) = get("defaults.branch") {
        cfg.default_branch = Some(v);
    }
    if let Some(v) = get("defaults.provider") {
        cfg.provider = Some(v);
    }
    set_if_present(&mut cfg.copy_include, get_all("copy.include"));
    set_if_present(&mut cfg.copy_exclude, get_all("copy.exclude"));
    set_if_present(&mut cfg.copy_include_dirs, get_all("copy.includeDirs"));
    set_if_present(&mut cfg.copy_exclude_dirs, get_all("copy.excludeDirs"));
    set_if_present(&mut cfg.hook_post_create, get_all("hooks.postCreate"));
    set_if_present(&mut cfg.hook_pre_remove, get_all("hooks.preRemove"));
    set_if_present(&mut cfg.hook_post_remove, get_all("hooks.postRemove"));
    set_if_present(&mut cfg.hook_post_cd, get_all("hooks.postCd"));
}

/// Read a `.groveconfig` file using grove's own `grove.*` git-config key names.
fn apply_groveconfig(cfg: &mut Config, path: &Path) {
    if !path.exists() {
        return;
    }
    let get = |k: &str| grove_git::config_file_get(path, k).ok().flatten();
    let get_all = |k: &str| grove_git::config_file_get_all(path, k).unwrap_or_default();

    if let Some(v) = get("grove.worktrees.dir") {
        cfg.worktrees_dir = Some(v);
    }
    if let Some(v) = get("grove.worktrees.prefix") {
        cfg.worktrees_prefix = v;
    }
    if let Some(v) = get("grove.editor.default") {
        cfg.editor_default = Some(v);
    }
    if let Some(v) = get("grove.editor.workspace") {
        cfg.editor_workspace = Some(v);
    }
    if let Some(v) = get("grove.ai.default") {
        cfg.ai_default = Some(v);
    }
    if let Some(v) = get("grove.ui.color") {
        cfg.color = ColorChoice::parse(&v);
    }
    if let Some(v) = get("grove.defaultRemote") {
        cfg.default_remote = Some(v);
    }
    if let Some(v) = get("grove.defaultBranch") {
        cfg.default_branch = Some(v);
    }
    if let Some(v) = get("grove.track") {
        cfg.track = TrackMode::parse(&v);
    }
    if let Some(v) = get("grove.provider") {
        cfg.provider = Some(v);
    }
    set_if_present(&mut cfg.copy_include, get_all("grove.copy.include"));
    set_if_present(&mut cfg.copy_exclude, get_all("grove.copy.exclude"));
    set_if_present(
        &mut cfg.copy_include_dirs,
        get_all("grove.copy.includeDirs"),
    );
    set_if_present(
        &mut cfg.copy_exclude_dirs,
        get_all("grove.copy.excludeDirs"),
    );
    set_if_present(&mut cfg.hook_post_create, get_all("grove.hooks.postCreate"));
    set_if_present(&mut cfg.hook_pre_remove, get_all("grove.hooks.preRemove"));
    set_if_present(&mut cfg.hook_post_remove, get_all("grove.hooks.postRemove"));
    set_if_present(&mut cfg.hook_post_cd, get_all("grove.hooks.postCd"));
}

/// Overwrite `slot` only when `values` is non-empty.
fn set_if_present(slot: &mut Vec<String>, values: Vec<String>) {
    if !values.is_empty() {
        *slot = values;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::process::Command;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("grove-gtr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_key(file: &Path, key: &str, value: &str) {
        Command::new("git")
            .args(["config", "-f", &file.to_string_lossy(), key, value])
            .status()
            .unwrap();
    }

    fn add_key(file: &Path, key: &str, value: &str) {
        Command::new("git")
            .args(["config", "-f", &file.to_string_lossy(), "--add", key, value])
            .status()
            .unwrap();
    }

    #[test]
    fn reads_real_gtrconfig_schema() {
        // The keys a real gtr-written .gtrconfig uses (unprefixed, INI sections).
        let dir = temp_dir("gtr");
        let file = dir.join(".gtrconfig");
        write_key(&file, "worktrees.dir", "../wt");
        write_key(&file, "worktrees.prefix", "wt-");
        write_key(&file, "defaults.editor", "cursor");
        write_key(&file, "defaults.ai", "claude");
        write_key(&file, "editor.workspace", "app.code-workspace");
        write_key(&file, "ui.color", "never");
        write_key(&file, "defaults.remote", "upstream");
        write_key(&file, "defaults.branch", "trunk");
        write_key(&file, "defaults.provider", "gitlab");
        add_key(&file, "copy.include", ".env");
        add_key(&file, "copy.includeDirs", "node_modules");
        add_key(&file, "hooks.postCreate", "bun install");

        let mut cfg = Config::default();
        apply(&mut cfg, &dir);
        assert_eq!(cfg.worktrees_dir.as_deref(), Some("../wt"));
        assert_eq!(cfg.worktrees_prefix, "wt-");
        assert_eq!(cfg.editor_default.as_deref(), Some("cursor"));
        assert_eq!(cfg.ai_default.as_deref(), Some("claude"));
        assert_eq!(cfg.editor_workspace.as_deref(), Some("app.code-workspace"));
        assert_eq!(cfg.color, ColorChoice::Never);
        assert_eq!(cfg.default_remote.as_deref(), Some("upstream"));
        assert_eq!(cfg.default_branch.as_deref(), Some("trunk"));
        assert_eq!(cfg.provider.as_deref(), Some("gitlab"));
        assert_eq!(cfg.copy_include, vec![".env"]);
        assert_eq!(cfg.copy_include_dirs, vec!["node_modules"]);
        assert_eq!(cfg.hook_post_create, vec!["bun install"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn groveconfig_overrides_gtrconfig() {
        let dir = temp_dir("grove");
        write_key(&dir.join(".gtrconfig"), "defaults.editor", "cursor");
        write_key(&dir.join(".groveconfig"), "grove.editor.default", "zed");

        let mut cfg = Config::default();
        apply(&mut cfg, &dir);
        // .groveconfig is applied second, so it wins.
        assert_eq!(cfg.editor_default.as_deref(), Some("zed"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_files_are_noop() {
        let dir = temp_dir("none");
        let mut cfg = Config::default();
        apply(&mut cfg, &dir);
        assert!(cfg.worktrees_dir.is_none());
        assert!(cfg.editor_default.is_none());
        assert_eq!(cfg.color, ColorChoice::Auto);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
