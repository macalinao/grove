//! Differential tests: run the same scenario through **grove** and the real
//! **gtr** binary and compare filesystem outcomes (worktree folders, branches,
//! copied files). Skipped unless a `gtr` binary is available — set `GROVE_GTR`
//! to its path, or have `gtr` on `PATH` (the flake dev shell provides it).
//!
//! Only behaviors that should match by design are compared; the two tools
//! diverge in output text and config namespace, which these tests don't touch.
#![allow(clippy::unwrap_used)]

mod common;

use std::process::Command;

use common::TestRepo;

/// Locate a runnable `gtr`, or `None` to skip.
fn find_gtr() -> Option<String> {
    if let Ok(p) = std::env::var("GROVE_GTR") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    let runs = Command::new("gtr")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    runs.then(|| "gtr".to_string())
}

macro_rules! gtr_or_skip {
    () => {
        match find_gtr() {
            Some(g) => g,
            None => {
                eprintln!("SKIP: gtr not found (set GROVE_GTR or add gtr to PATH)");
                return;
            }
        }
    };
}

/// A grove repo and a gtr repo, each with a local `origin` so both resolve the
/// same default base without a network.
struct Pair {
    grove: TestRepo,
    gtr_repo: TestRepo,
    gtr: String,
}

impl Pair {
    fn new(gtr: String) -> Pair {
        let grove = TestRepo::new();
        let gtr_repo = TestRepo::new();
        grove.add_origin();
        gtr_repo.add_origin();
        Pair {
            grove,
            gtr_repo,
            gtr,
        }
    }

    /// Run `grove <args>` on the grove side.
    fn g(&self, args: &[&str]) -> std::process::Output {
        self.grove.grove(args)
    }

    /// Run `gtr <args>` on the gtr side.
    fn t(&self, args: &[&str]) -> std::process::Output {
        self.gtr_repo.tool(&self.gtr, args)
    }

    /// Assert worktree folder names and local branches match across the two.
    fn assert_same_worktrees_and_branches(&self) {
        assert_eq!(
            self.grove.worktree_folders(),
            self.gtr_repo.worktree_folders(),
            "worktree folder names differ (grove vs gtr)"
        );
        assert_eq!(
            self.grove.local_branches(),
            self.gtr_repo.local_branches(),
            "local branches differ (grove vs gtr)"
        );
    }
}

#[test]
fn create_sanitizes_and_locates_identically() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    for branch in ["feature/auth", "bug#42", "release/v1.0"] {
        p.g(&["new", branch, "--no-fetch"]);
        p.t(&["new", branch, "--no-fetch", "--yes"]);
    }
    p.assert_same_worktrees_and_branches();
}

#[test]
fn copy_files_on_create_matches() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    // Same untracked file + include pattern on each side (own namespace).
    p.grove.write(".env", "SECRET=1");
    p.gtr_repo.write(".env", "SECRET=1");
    p.grove.add_config("grove.copy.include", ".env");
    p.gtr_repo.add_config("gtr.copy.include", ".env");

    p.g(&["new", "feature", "--no-fetch"]);
    p.t(&["new", "feature", "--no-fetch", "--yes"]);

    assert_eq!(
        p.grove.files_in("feature"),
        p.gtr_repo.files_in("feature"),
        "copied file set differs (grove vs gtr)"
    );
    assert!(p.grove.files_in("feature").contains(&".env".to_string()));
}

#[test]
fn copy_directory_with_exclude_matches() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    for r in [&p.grove, &p.gtr_repo] {
        r.write("node_modules/pkg/index.js", "x");
        r.write("node_modules/.cache/blob", "junk");
    }
    p.grove.add_config("grove.copy.includeDirs", "node_modules");
    p.grove
        .add_config("grove.copy.excludeDirs", "node_modules/.cache");
    p.gtr_repo
        .add_config("gtr.copy.includeDirs", "node_modules");
    p.gtr_repo
        .add_config("gtr.copy.excludeDirs", "node_modules/.cache");

    p.g(&["new", "feature", "--no-fetch"]);
    p.t(&["new", "feature", "--no-fetch", "--yes"]);

    assert_eq!(
        p.grove.files_in("feature"),
        p.gtr_repo.files_in("feature"),
        "directory copy result differs (grove vs gtr)"
    );
    let files = p.grove.files_in("feature");
    assert!(files.iter().any(|f| f.contains("node_modules/pkg")));
    assert!(!files.iter().any(|f| f.contains(".cache")));
}

#[test]
fn gtrconfig_file_drives_copy_identically() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    // The SAME real .gtrconfig (gtr's on-disk schema) feeds both tools.
    let cfg = "[copy]\n\tinclude = .env*\n\tincludeDirs = vendor\n";
    for r in [&p.grove, &p.gtr_repo] {
        r.write(".gtrconfig", cfg);
        r.write(".env.local", "X=1");
        r.write("vendor/lib.rs", "x");
    }
    p.g(&["new", "feature", "--no-fetch"]);
    p.t(&["new", "feature", "--no-fetch", "--yes"]);
    assert_eq!(
        p.grove.files_in("feature"),
        p.gtr_repo.files_in("feature"),
        "files copied from a shared .gtrconfig differ (grove vs gtr)"
    );
    assert!(
        p.grove
            .files_in("feature")
            .iter()
            .any(|f| f == ".env.local")
    );
    assert!(
        p.grove
            .files_in("feature")
            .iter()
            .any(|f| f.contains("vendor/"))
    );
}

#[test]
fn prefix_and_name_suffix_match() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    p.grove.add_config("grove.worktrees.prefix", "wt-");
    p.gtr_repo.add_config("gtr.worktrees.prefix", "wt-");
    p.g(&["new", "feature", "--no-fetch"]);
    p.t(&["new", "feature", "--no-fetch", "--yes"]);
    p.g(&["new", "other", "--no-fetch", "--name", "backend"]);
    p.t(&["new", "other", "--no-fetch", "--yes", "--name", "backend"]);
    p.assert_same_worktrees_and_branches();
}

#[test]
fn list_porcelain_status_column_matches() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    p.g(&["new", "feature", "--no-fetch"]);
    p.t(&["new", "feature", "--no-fetch", "--yes"]);
    // Normalize: replace each absolute path with its trailing folder name so
    // only the (branch, status) shape is compared.
    let norm = |out: &std::process::Output| -> Vec<(String, String)> {
        common::stdout(out)
            .lines()
            .filter_map(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                (c.len() == 3).then(|| (c[1].to_string(), c[2].to_string()))
            })
            .collect()
    };
    assert_eq!(
        norm(&p.g(&["list", "--porcelain"])),
        norm(&p.t(&["list", "--porcelain"])),
        "porcelain (branch, status) columns differ"
    );
}

/// Documents an intentional divergence: in a repo with no remote and no
/// explicit base, gtr errors (`invalid reference: origin/main`) while grove
/// falls back to HEAD and succeeds. This pins that grove is the more forgiving
/// of the two; if it ever regresses to gtr's behavior, this test fails.
#[test]
fn no_remote_new_grove_succeeds_where_gtr_fails() {
    let gtr = gtr_or_skip!();
    // Fresh repos WITHOUT add_origin().
    let grove = TestRepo::new();
    let gtr_repo = TestRepo::new();

    let g = grove.grove(&["new", "x", "--no-fetch"]);
    assert!(
        g.status.success(),
        "grove should fall back to HEAD: {}",
        common::stderr(&g)
    );
    assert!(grove.wt("x").is_dir());

    let t = gtr_repo.tool(&gtr, &["new", "x", "--no-fetch", "--yes"]);
    assert!(
        !t.status.success(),
        "gtr is expected to fail without a remote base"
    );
}

#[test]
fn track_auto_upstream_matches() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    // Publish origin/shared on both sides, drop it locally.
    for r in [&p.grove, &p.gtr_repo] {
        r.git(&["branch", "shared"]);
        r.git(&["push", "-q", "origin", "shared"]);
        r.git(&["branch", "-D", "shared"]);
    }
    p.g(&["new", "shared", "--no-fetch"]);
    p.t(&["new", "shared", "--no-fetch", "--yes"]);
    let upstream = |r: &TestRepo| {
        String::from_utf8_lossy(
            &r.git(&["rev-parse", "--abbrev-ref", "shared@{upstream}"])
                .stdout,
        )
        .trim()
        .to_string()
    };
    assert_eq!(
        upstream(&p.grove),
        upstream(&p.gtr_repo),
        "default-track upstream differs (grove vs gtr)"
    );
}

#[test]
fn remove_matches() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    p.g(&["new", "feature", "--no-fetch"]);
    p.t(&["new", "feature", "--no-fetch", "--yes"]);
    p.g(&["rm", "feature", "--yes", "--delete-branch"]);
    p.t(&["rm", "feature", "--yes", "--delete-branch"]);
    p.assert_same_worktrees_and_branches();
    assert!(p.grove.worktree_folders().is_empty());
}

#[test]
fn rename_matches() {
    let gtr = gtr_or_skip!();
    let p = Pair::new(gtr);
    p.g(&["new", "old", "--no-fetch"]);
    p.t(&["new", "old", "--no-fetch", "--yes"]);
    p.g(&["mv", "old", "renamed", "--yes"]);
    p.t(&["mv", "old", "renamed", "--yes"]);
    p.assert_same_worktrees_and_branches();
}
