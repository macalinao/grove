//! Btrfs subvolume worktrees (`grove.worktrees.subvolume`).
//!
//! Its own crate (std and rustix only) so tools that manage worktrees
//! their own way can use it without grove's forge and config stack.
//!
//! On btrfs, a worktree that is its own subvolume gets its own fs tree, so
//! its checkout and builds don't contend on the parent subvolume's tree lock,
//! and deleting it is one `btrfs subvolume delete` (cleaned up lazily by the
//! kernel) instead of an unlink per file.
//!
//! Shells out to `btrfs` (btrfs-progs) like the rest of grove shells out to
//! `git`. Unprivileged `btrfs subvolume delete` needs the filesystem mounted
//! with `user_subvol_rm_allowed`; without it the caller falls back to
//! `git worktree remove`, which empties the dir and then `rmdir`s the empty
//! subvolume (allowed without root since Linux 4.18).

use std::path::Path;
use std::process::Command;

/// Whether `path` is on a btrfs filesystem.
#[cfg(target_os = "linux")]
pub fn on_btrfs(path: &Path) -> bool {
    const BTRFS_SUPER_MAGIC: u64 = 0x9123_683E;
    rustix::fs::statfs(path).is_ok_and(|s| {
        // `f_type`'s integer type varies by target; the magic fits in 32 bits.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ty = u64::from(s.f_type as u32);
        ty == BTRFS_SUPER_MAGIC
    })
}

/// Whether `path` is on a btrfs filesystem (never, off Linux).
#[cfg(not(target_os = "linux"))]
pub fn on_btrfs(_path: &Path) -> bool {
    false
}

/// Whether `path` is the root of a btrfs subvolume (its inode is always 256).
pub fn is_subvolume(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        on_btrfs(path)
            && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && m.ino() == 256)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// Create an empty subvolume at `path` (which must not exist; its parent
/// must). Returns why not when it can't.
///
/// # Errors
/// A human-readable reason: the parent isn't on btrfs, `btrfs` isn't
/// installed, or `btrfs subvolume create` failed.
pub fn create(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or(path);
    if !on_btrfs(parent) {
        return Err(format!("{} is not on btrfs", parent.display()));
    }
    btrfs(&["subvolume", "create"], path)
}

/// The argv of [`delete`], for callers that run commands their own way.
pub fn delete_argv(path: &Path) -> Vec<String> {
    argv(&["subvolume", "delete"], &[path])
}

fn argv(args: &[&str], paths: &[&Path]) -> Vec<String> {
    let mut v = vec!["btrfs".to_owned()];
    v.extend(args.iter().map(|a| (*a).to_owned()));
    v.push("--".to_owned());
    v.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
    v
}

/// Delete the subvolume at `path`, contents and all.
///
/// # Errors
/// A human-readable reason when `btrfs subvolume delete` fails (typically
/// EPERM: no root and no `user_subvol_rm_allowed`).
pub fn delete(path: &Path) -> Result<(), String> {
    run(&delete_argv(path))
}

fn btrfs(args: &[&str], path: &Path) -> Result<(), String> {
    run(&argv(args, &[path]))
}

fn run(argv: &[String]) -> Result<(), String> {
    let out = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|e| format!("running btrfs: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let reason = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("failed");
    let what = argv[1..].iter().take_while(|a| *a != "--").cloned();
    Err(format!(
        "btrfs {}: {reason}",
        what.collect::<Vec<_>>().join(" ")
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn plain_dir_is_not_a_subvolume() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("plain");
        std::fs::create_dir(&sub).unwrap();
        assert!(!is_subvolume(&sub));
        assert!(!is_subvolume(&dir.path().join("missing")));
    }

    #[test]
    fn argvs() {
        assert_eq!(
            delete_argv(Path::new("/a/b")),
            ["btrfs", "subvolume", "delete", "--", "/a/b"]
        );
    }

    #[test]
    fn create_refuses_off_btrfs_or_reports_why() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("wt");
        match create(&target) {
            // On btrfs (e.g. a dev box) it really makes one; clean up.
            Ok(()) => {
                assert!(is_subvolume(&target));
                std::fs::remove_dir(&target).unwrap();
            }
            Err(why) => {
                assert!(!why.is_empty());
                assert!(!target.exists());
            }
        }
    }
}
