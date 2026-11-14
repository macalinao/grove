//! What `grove clean` needs beyond the PR state: whether a worktree still
//! holds work (uncommitted changes, commits on no remote), whether a process
//! works in it, and deleting it gently.
//!
//! A big delete on btrfs can hold the filesystem's locks for minutes, so
//! [`delete_slowly`] unlinks a chunk of entries at a time with a pause after
//! each, and [`idle_priority`] drops the process (and the `git` it runs) to
//! idle IO priority and the lowest CPU priority first.

use core::time::Duration;
use std::io;
use std::path::{Path, PathBuf};

use grove_git::Repo;

/// How [`delete_slowly`] paces itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    /// Entries unlinked between pauses.
    pub chunk: usize,
    /// The pause after each chunk.
    pub pause: Duration,
}

impl Default for Pace {
    fn default() -> Self {
        Pace {
            chunk: 2000,
            pause: Duration::from_secs(1),
        }
    }
}

/// Why a worktree whose PR is done still can't go: its uncommitted changes
/// and commits on no remote (empty: nothing to lose). `pr_head` is the PR's
/// head commit: the forge keeps it after the branch is deleted, so commits it
/// reaches count as pushed (a squash-merged branch whose remote branch was
/// pruned).
///
/// # Errors
/// A human-readable reason when git can't tell (the caller keeps the
/// worktree).
pub fn work_left(worktree: &Path, pr_head: Option<&str>) -> Result<Vec<String>, String> {
    let repo = Repo::discover_from(worktree).map_err(|e| e.to_string())?;
    let mut out = vec![];
    let dirty = repo.status_porcelain().map_err(|e| e.to_string())?;
    if !dirty.is_empty() {
        out.push(plural(dirty.len() as u64, "uncommitted change"));
    }
    let head = pr_head
        .filter(|sha| !sha.is_empty())
        .filter(|sha| repo.has_ref(&format!("{sha}^{{commit}}")).unwrap_or(false));
    let unpushed = repo.unpushed(head.as_slice()).map_err(|e| e.to_string())?;
    if unpushed > 0 {
        out.push(format!("{} on no remote", plural(unpushed, "commit")));
    }
    Ok(out)
}

/// A process (other than this one) whose working directory is `worktree` or
/// inside it, if any: its pid. Only processes this user may inspect are seen
/// (`/proc/<pid>/cwd`); off Linux, none.
pub fn process_in(worktree: &Path) -> Option<u32> {
    let root = worktree.canonicalize().ok()?;
    let me = std::process::id();
    let entries = std::fs::read_dir("/proc").ok()?;
    entries.flatten().find_map(|e| {
        let pid: u32 = e.file_name().to_str()?.parse().ok()?;
        if pid == me {
            return None;
        }
        let cwd = std::fs::read_link(e.path().join("cwd")).ok()?;
        cwd.starts_with(&root).then_some(pid)
    })
}

/// Lower this process (and the children it spawns) to idle IO priority and
/// nice 19. Best-effort: a no-op off Linux or where it isn't allowed.
pub fn idle_priority() {
    #[cfg(target_os = "linux")]
    {
        const IOPRIO_WHO_PROCESS: libc::c_long = 1;
        const IOPRIO_CLASS_IDLE: libc::c_long = 3;
        const IOPRIO_CLASS_SHIFT: libc::c_long = 13;
        // SAFETY: ioprio_set(2) and setpriority(2) on this process only take
        // plain integers; failure is reported through the return value,
        // which is ignored (best-effort).
        unsafe {
            libc::syscall(
                libc::SYS_ioprio_set,
                IOPRIO_WHO_PROCESS,
                0,
                IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT,
            );
            libc::setpriority(libc::PRIO_PROCESS, 0, 19);
        }
    }
}

/// Delete each of `paths` (files or whole directories, never following
/// symlinks) depth-first, pausing after every `pace.chunk` entries. Missing
/// paths are skipped. Returns how many entries went.
///
/// # Errors
/// The first entry that can't be read or removed.
pub fn delete_slowly(paths: &[PathBuf], pace: Pace) -> io::Result<u64> {
    let mut d = Deleter {
        pace,
        in_chunk: 0,
        total: 0,
    };
    for p in paths {
        match std::fs::symlink_metadata(p) {
            Ok(m) => d.remove(p, m.is_dir())?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(d.total)
}

struct Deleter {
    pace: Pace,
    in_chunk: usize,
    total: u64,
}

impl Deleter {
    fn remove(&mut self, path: &Path, dir: bool) -> io::Result<()> {
        if dir {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let is_dir = entry.file_type()?.is_dir();
                self.remove(&entry.path(), is_dir)?;
            }
            std::fs::remove_dir(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        self.total += 1;
        self.in_chunk += 1;
        if self.in_chunk >= self.pace.chunk.max(1) {
            self.in_chunk = 0;
            std::thread::sleep(self.pace.pause);
        }
        Ok(())
    }
}

fn plural(n: u64, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn deletes_trees_depth_first_and_counts_entries() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("target");
        std::fs::create_dir_all(t.join("a/b")).unwrap();
        std::fs::write(t.join("a/b/f"), "x").unwrap();
        std::fs::write(t.join("g"), "y").unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "z").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), t.join("link")).unwrap();
        let pace = Pace {
            chunk: 2,
            pause: Duration::ZERO,
        };
        let n = delete_slowly(&[t.clone(), file.clone(), dir.path().join("gone")], pace).unwrap();
        assert!(!t.exists() && !file.exists());
        // The symlink went, not what it points to.
        assert!(dir.path().is_dir());
        assert_eq!(n, if cfg!(unix) { 7 } else { 6 });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sees_a_process_working_inside() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("sub");
        std::fs::create_dir(&inner).unwrap();
        assert_eq!(process_in(dir.path()), None);
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&inner)
            .spawn()
            .unwrap();
        let found = process_in(dir.path());
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found, Some(child.id()));
    }
}
