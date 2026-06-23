//! gtr-compatibility config readers.
//!
//! Reads team config from `.gtrconfig` (gtr's `gtr.*` keys) and `.groveconfig`
//! (grove's `grove.*` keys) — both git-config INI files at the repo root — as
//! the lowest-precedence sources, so native grove config (git-config grove.*,
//! grove.kdl) always wins. Best-effort: malformed files are skipped silently.

use std::path::Path;

use crate::{ColorChoice, Config};

/// Apply `.gtrconfig` then `.groveconfig` (lowest precedence) onto `cfg`.
pub fn apply(cfg: &mut Config, root: &Path) {
    apply_file(cfg, &root.join(".gtrconfig"), "gtr");
    apply_file(cfg, &root.join(".groveconfig"), "grove");
}

fn apply_file(cfg: &mut Config, path: &Path, prefix: &str) {
    if !path.exists() {
        return;
    }
    if let Some(v) = get(path, prefix, "worktrees.dir") {
        cfg.worktrees_dir = Some(v);
    }
    if let Some(v) = get(path, prefix, "worktrees.prefix") {
        cfg.worktrees_prefix = v;
    }
    if let Some(v) = get(path, prefix, "editor.default") {
        cfg.editor_default = Some(v);
    }
    if let Some(v) = get(path, prefix, "ai.default") {
        cfg.ai_default = Some(v);
    }
    if let Some(v) = get(path, prefix, "ui.color") {
        cfg.color = ColorChoice::parse(&v);
    }
}

/// Read `<prefix>.<key>` from the INI `path`, ignoring errors.
fn get(path: &Path, prefix: &str, key: &str) -> Option<String> {
    grove_git::config_file_get(path, &format!("{prefix}.{key}"))
        .ok()
        .flatten()
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

    #[test]
    fn reads_gtrconfig_with_mapping() {
        let dir = temp_dir("gtr");
        let file = dir.join(".gtrconfig");
        write_key(&file, "gtr.worktrees.dir", "../wt");
        write_key(&file, "gtr.worktrees.prefix", "wt-");
        write_key(&file, "gtr.editor.default", "cursor");
        write_key(&file, "gtr.ui.color", "never");

        let mut cfg = Config::default();
        apply(&mut cfg, &dir);
        assert_eq!(cfg.worktrees_dir.as_deref(), Some("../wt"));
        assert_eq!(cfg.worktrees_prefix, "wt-");
        assert_eq!(cfg.editor_default.as_deref(), Some("cursor"));
        assert_eq!(cfg.color, ColorChoice::Never);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn groveconfig_overrides_gtrconfig() {
        let dir = temp_dir("grove");
        write_key(&dir.join(".gtrconfig"), "gtr.editor.default", "cursor");
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
        assert!(cfg.worktrees_prefix.is_empty());
        assert!(cfg.editor_default.is_none());
        assert_eq!(cfg.color, ColorChoice::Auto);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
