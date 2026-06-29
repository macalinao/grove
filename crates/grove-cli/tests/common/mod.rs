//! Shared harness for the `grove` behavioral integration tests.
//!
//! Each test spins up a throwaway git repo in a temp workspace and drives the
//! real `grove` binary against it, asserting gtr-equivalent behavior (worktree
//! creation, sanitization, copying, trust gating, clean, …). Git's global and
//! system config are redirected away from the developer's machine so results
//! are deterministic.
#![allow(clippy::unwrap_used, dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A temp git repository plus helpers to run `grove` against it.
pub struct TestRepo {
    /// Canonical temp workspace dir; the repo and its `-worktrees` sibling live here.
    pub workspace: PathBuf,
    /// Canonical path to the repo (`workspace/repo`).
    pub root: PathBuf,
    /// Isolated global git config file (so the dev's `~/.gitconfig` can't leak).
    pub global_cfg: PathBuf,
    /// Directory prepended to `PATH` for fake executables (gh, editors, …).
    pub bin: PathBuf,
}

impl TestRepo {
    /// Create a fresh repo on `main` with one empty commit.
    pub fn new() -> TestRepo {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let workspace = std::env::temp_dir().join(format!("grove-it-{nonce}"));
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let root = workspace.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let bin = workspace.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let global_cfg = workspace.join("gitconfig");
        std::fs::write(
            &global_cfg,
            "[user]\n\tname = Test\n\temail = test@example.com\n",
        )
        .unwrap();

        let repo = TestRepo {
            workspace,
            root,
            global_cfg,
            bin,
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        repo
    }

    /// Run `grove <args>` in the repo root.
    pub fn grove(&self, args: &[&str]) -> Output {
        self.grove_in(&self.root, args)
    }

    /// Run `grove <args>` in `dir`.
    pub fn grove_in(&self, dir: &Path, args: &[&str]) -> Output {
        self.base_cmd(env!("CARGO_BIN_EXE_grove"), dir)
            .args(args)
            .output()
            .unwrap()
    }

    /// Run `grove <args>` with extra env vars and the fake-bin dir on `PATH`.
    pub fn grove_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Output {
        let mut cmd = self.base_cmd(env!("CARGO_BIN_EXE_grove"), &self.root);
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{path}", self.bin.display()));
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.args(args).output().unwrap()
    }

    /// Run a raw `git <args>` in the repo root (isolated config).
    pub fn git(&self, args: &[&str]) -> Output {
        self.base_cmd("git", &self.root)
            .args(args)
            .output()
            .unwrap()
    }

    /// Run an arbitrary `program <args>` in the repo root (isolated config,
    /// fake-bin dir on PATH). Used to drive the upstream `gtr` binary.
    pub fn tool(&self, program: &str, args: &[&str]) -> Output {
        let mut cmd = self.base_cmd(program, &self.root);
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{path}", self.bin.display()));
        cmd.args(args).output().unwrap()
    }

    /// Give the repo a local bare `origin` with `main` pushed, so worktree tools
    /// can resolve `origin/main` as a default base without any network.
    pub fn add_origin(&self) {
        let remote = self.workspace.join("origin.git");
        Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&remote)
            .env("GIT_CONFIG_GLOBAL", &self.global_cfg)
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
        self.git(&["remote", "add", "origin", &remote.to_string_lossy()]);
        self.git(&["push", "-q", "origin", "main"]);
        self.git(&["fetch", "-q", "origin"]);
        self.git(&["remote", "set-head", "origin", "main"]);
    }

    /// Sorted folder names directly under the `<repo>-worktrees` base dir.
    pub fn worktree_folders(&self) -> Vec<String> {
        let base = self.workspace.join("repo-worktrees");
        let mut out: Vec<String> = std::fs::read_dir(&base)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }

    /// Sorted local branch names.
    pub fn local_branches(&self) -> Vec<String> {
        let out = self.git(&["branch", "--format=%(refname:short)"]);
        let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        v.sort();
        v
    }

    /// Sorted relative file paths inside a worktree folder (excluding `.git`).
    pub fn files_in(&self, folder: &str) -> Vec<String> {
        let base = self.workspace.join("repo-worktrees").join(folder);
        let mut out = Vec::new();
        collect_files(&base, &base, &mut out);
        out.sort();
        out
    }

    /// Set a local git config key in the repo.
    pub fn set_config(&self, key: &str, value: &str) {
        self.git(&["config", key, value]);
    }

    /// Add a value to a multi-valued local git config key.
    pub fn add_config(&self, key: &str, value: &str) {
        self.git(&["config", "--add", key, value]);
    }

    /// Write a file (creating parents) under the repo root.
    pub fn write(&self, rel: &str, contents: &str) {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }

    /// Write an executable fake (e.g. a fake `gh`) into the bin dir.
    pub fn write_exec(&self, name: &str, script: &str) {
        let p = self.bin.join(name);
        std::fs::write(&p, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// A path under the repo root.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// The expected worktree path for `folder` (`workspace/repo-worktrees/<folder>`).
    pub fn wt(&self, folder: &str) -> PathBuf {
        self.workspace.join("repo-worktrees").join(folder)
    }

    fn base_cmd(&self, program: &str, dir: &Path) -> Command {
        let mut cmd = Command::new(program);
        // Output is captured (piped), so `console` auto-detects no TTY and emits
        // no color by default; we don't force NO_COLOR, so the grove.ui.color
        // config path stays observable. GROVE_COLOR is cleared for determinism.
        cmd.current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", &self.global_cfg)
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GROVE_COLOR")
            .env_remove("NO_COLOR");
        cmd
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

/// Assert the command succeeded; return its stdout as a String.
pub fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "expected success, got {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        stdout(out),
        stderr(out),
    );
    stdout(out)
}

/// Assert the command failed (non-zero exit); return its stderr.
pub fn failed(out: &Output) -> String {
    assert!(
        !out.status.success(),
        "expected failure, but it succeeded\nstdout: {}",
        stdout(out),
    );
    stderr(out)
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Recursively collect file paths under `dir`, relative to `base`, skipping `.git`.
fn collect_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if path.is_dir() {
            collect_files(base, &path, out);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}
