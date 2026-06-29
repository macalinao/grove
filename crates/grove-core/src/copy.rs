//! Smart file copy engine for `grove copy`.
//!
//! Copies files (typically untracked config/env files such as `.env`) from a
//! source worktree into a target worktree by glob. Patterns come from the
//! resolved [`grove_config::Config`] (or the CLI). Walks the source tree,
//! skipping `.git`, and copies every file whose path matches an include glob
//! and no exclude glob.

use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
pub use reflink_copy::ReflinkSupport;

use crate::{CoreError, Result};

/// Detect whether copy-on-write reflinks work between `from` and `to` (e.g.
/// both on the same btrfs/XFS/APFS/ReFS volume). Used by `grove doctor` to tell
/// the user whether worktree copies will be cheap clones.
///
/// `reflink_copy::check_reflink_support` is only meaningful on Windows; on Unix
/// it always reports `Unknown`, so we run a real probe instead — create a tiny
/// temp file under `from` and attempt to reflink it into `to`. The probe files
/// are removed immediately. The actual copy path ([`copy_dirs`]) does the right
/// thing regardless via [`reflink_copy::reflink_or_copy`]; this is for reporting.
#[must_use]
pub fn reflink_support(from: &Path, to: &Path) -> ReflinkSupport {
    if cfg!(windows) {
        return reflink_copy::check_reflink_support(from, to).unwrap_or(ReflinkSupport::Unknown);
    }
    if !from.is_dir() || !to.is_dir() {
        return ReflinkSupport::Unknown;
    }
    let nonce = probe_nonce();
    let src = from.join(format!(".grove-reflink-probe-src-{nonce}"));
    let dst = to.join(format!(".grove-reflink-probe-dst-{nonce}"));
    let outcome = fs::write(&src, b"grove-reflink-probe").and_then(|()| {
        // `reflink` (not `reflink_or_copy`) errors when CoW is unavailable —
        // that's what distinguishes Supported from NotSupported.
        reflink_copy::reflink(&src, &dst)
    });
    let _ = fs::remove_file(&src);
    let _ = fs::remove_file(&dst);
    match outcome {
        Ok(()) => ReflinkSupport::Supported,
        Err(_) => ReflinkSupport::NotSupported,
    }
}

/// A best-effort unique suffix for probe files (no randomness needed).
fn probe_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        ^ u128::from(std::process::id())
}

/// What to copy from a source worktree into a target one.
#[derive(Debug, Default, Clone)]
pub struct CopySpec {
    /// Glob patterns of individual files to copy.
    pub include: Vec<String>,
    /// Glob patterns of files to skip.
    pub exclude: Vec<String>,
    /// Whole directories to copy (copy-on-write where supported).
    pub include_dirs: Vec<String>,
    /// Glob patterns of subpaths to prune from copied directories.
    pub exclude_dirs: Vec<String>,
}

impl CopySpec {
    /// Is there nothing to copy?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.include_dirs.is_empty()
    }
}

/// Copy everything described by `spec` from `from` into `to`.
///
/// Files matching the include globs are copied first, then each include
/// directory is cloned wholesale (copy-on-write where the filesystem allows),
/// pruning any subpath matching an `exclude_dirs` glob. Returns every relative
/// path that was copied (or, when `dry_run`, that *would* be copied).
///
/// # Errors
/// Returns [`CoreError::Glob`] on an invalid pattern or [`CoreError::Io`] for a
/// filesystem error.
pub fn copy_into(from: &Path, to: &Path, spec: &CopySpec, dry_run: bool) -> Result<Vec<PathBuf>> {
    let mut copied = copy_files(from, to, &spec.include, &spec.exclude, dry_run)?;
    copied.extend(copy_dirs(
        from,
        to,
        &spec.include_dirs,
        &spec.exclude_dirs,
        dry_run,
    )?);
    copied.sort();
    copied.dedup();
    Ok(copied)
}

/// Read include globs from a `.worktreeinclude` file at `root` (gitignore-style:
/// one pattern per line, `#` comments and blank lines ignored).
///
/// Returns an empty vector when the file is absent.
///
/// # Errors
/// Returns [`CoreError::Io`] if the file exists but cannot be read.
pub fn read_worktreeinclude(root: &Path) -> Result<Vec<String>> {
    let path = root.join(".worktreeinclude");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|source| CoreError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect())
}

/// Copy files matching `include` (and not `exclude`) from `from` into `to`.
///
/// Returns the relative paths that were copied (or, when `dry_run`, that
/// *would* be copied). Parent directories are created under `to` as needed.
/// The `.git` directory in `from` is always skipped.
///
/// # Errors
/// Returns [`CoreError::Glob`] on an invalid glob pattern, or [`CoreError::Io`]
/// for any filesystem error while walking or copying.
pub fn copy_files(
    from: &Path,
    to: &Path,
    include: &[String],
    exclude: &[String],
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    if include.is_empty() {
        return Ok(Vec::new());
    }
    let include_set = build_globset(include)?;
    let exclude_set = build_globset(exclude)?;

    let mut matches = Vec::new();
    collect_matches(from, from, &include_set, &exclude_set, &mut matches)?;
    matches.sort();

    if !dry_run {
        for rel in &matches {
            copy_one(from, to, rel)?;
        }
    }
    Ok(matches)
}

/// Copy each directory in `dirs` from `from` into `to`, skipping any subpath
/// that matches an `exclude_dirs` glob. Each file is cloned copy-on-write via a
/// reflink where the filesystem supports it (btrfs/XFS/APFS/ReFS), falling back
/// to a normal copy otherwise; symlinks are recreated as links.
///
/// Returns the relative directory paths that were copied (or would be, when
/// `dry_run`). A directory missing in `from` is silently skipped.
///
/// # Errors
/// Returns [`CoreError::Glob`] on an invalid exclude pattern or
/// [`CoreError::Io`] for a filesystem error.
pub fn copy_dirs(
    from: &Path,
    to: &Path,
    dirs: &[String],
    exclude_dirs: &[String],
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    if dirs.is_empty() {
        return Ok(Vec::new());
    }
    let exclude_set = build_globset(exclude_dirs)?;
    let mut copied = Vec::new();
    for dir in dirs {
        let rel = PathBuf::from(dir);
        let src = from.join(&rel);
        if !src.is_dir() {
            continue;
        }
        copied.push(rel.clone());
        if dry_run {
            continue;
        }
        copy_tree(&src, &to.join(&rel), to, &exclude_set)?;
    }
    Ok(copied)
}

/// Recursively copy `src` into `dst`, reflinking files where possible and
/// skipping entries whose path (relative to `to_root`) matches `exclude`.
fn copy_tree(src: &Path, dst: &Path, to_root: &Path, exclude: &GlobSet) -> Result<()> {
    fs::create_dir_all(dst).map_err(|source| CoreError::Io {
        path: dst.to_path_buf(),
        source,
    })?;
    let entries = fs::read_dir(src).map_err(|source| CoreError::Io {
        path: src.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CoreError::Io {
            path: src.to_path_buf(),
            source,
        })?;
        let child_src = entry.path();
        let child_dst = dst.join(entry.file_name());
        if let Ok(rel) = child_dst.strip_prefix(to_root) {
            if exclude.is_match(rel) {
                continue;
            }
        }
        let file_type = entry.file_type().map_err(|source| CoreError::Io {
            path: child_src.clone(),
            source,
        })?;
        if file_type.is_dir() {
            copy_tree(&child_src, &child_dst, to_root, exclude)?;
        } else if file_type.is_symlink() {
            copy_symlink(&child_src, &child_dst)?;
        } else {
            // Reflink (copy-on-write) when supported; falls back to a full copy.
            reflink_copy::reflink_or_copy(&child_src, &child_dst).map_err(|source| {
                CoreError::Io {
                    path: child_dst.clone(),
                    source,
                }
            })?;
        }
    }
    Ok(())
}

/// Recreate the symlink at `src` (pointing at the same target) at `dst`.
fn copy_symlink(src: &Path, dst: &Path) -> Result<()> {
    let target = fs::read_link(src).map_err(|source| CoreError::Io {
        path: src.to_path_buf(),
        source,
    })?;
    symlink_to(&target, dst)
}

#[cfg(unix)]
fn symlink_to(target: &Path, dst: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, dst).map_err(|source| CoreError::Io {
        path: dst.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn symlink_to(target: &Path, dst: &Path) -> Result<()> {
    // Non-unix: best-effort copy of the link target's contents.
    fs::copy(target, dst)
        .map(|_| ())
        .map_err(|source| CoreError::Io {
            path: dst.to_path_buf(),
            source,
        })
}

/// Build a [`GlobSet`] from `patterns` (empty patterns yield an empty set).
fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        let glob = Glob::new(pat).map_err(|source| CoreError::Glob {
            pattern: pat.clone(),
            source,
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|source| CoreError::Glob {
        pattern: patterns.join(", "),
        source,
    })
}

/// Recursively gather relative paths under `dir` matching the glob sets.
fn collect_matches(
    root: &Path,
    dir: &Path,
    include: &GlobSet,
    exclude: &GlobSet,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|source| CoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            if is_git_dir(&path) {
                continue;
            }
            collect_matches(root, &path, include, exclude, out)?;
        } else if file_type.is_file() {
            consider_file(root, &path, include, exclude, out);
        }
    }
    Ok(())
}

/// Push `path`'s relative form to `out` if it matches include and not exclude.
fn consider_file(
    root: &Path,
    path: &Path,
    include: &GlobSet,
    exclude: &GlobSet,
    out: &mut Vec<PathBuf>,
) {
    let Ok(rel) = path.strip_prefix(root) else {
        return;
    };
    if include.is_match(rel) && !exclude.is_match(rel) {
        out.push(rel.to_path_buf());
    }
}

/// Is `path` the `.git` directory (or git metadata file boundary)?
fn is_git_dir(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == ".git")
}

/// Copy the single relative path `rel` from `from` into `to`.
fn copy_one(from: &Path, to: &Path, rel: &Path) -> Result<()> {
    let src = from.join(rel);
    let dst = to.join(rel);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::copy(&src, &dst).map_err(|source| CoreError::Io { path: dst, source })?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A self-cleaning temp directory.
    struct TmpDir {
        path: PathBuf,
    }

    impl TmpDir {
        fn new(tag: &str) -> TmpDir {
            let mut path = std::env::temp_dir();
            let unique = format!(
                "grove-copy-{tag}-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            path.push(unique);
            fs::create_dir_all(&path).unwrap();
            TmpDir { path }
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.path.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, contents).unwrap();
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn copies_included_skips_git_and_excluded() {
        let from = TmpDir::new("from");
        let to = TmpDir::new("to");
        from.write(".env.example", "ENV=1");
        from.write("node_modules/x", "junk");
        from.write(".git/x", "gitjunk");
        from.write("src/main.rs", "fn main() {}");

        let include = s(&["**/.env.example", ".env.example", "node_modules/**"]);
        let exclude = s(&["node_modules/**"]);
        let copied = copy_files(&from.path, &to.path, &include, &exclude, false).unwrap();

        assert_eq!(copied, vec![PathBuf::from(".env.example")]);
        assert!(to.path.join(".env.example").exists());
        assert!(!to.path.join("node_modules/x").exists());
        assert!(!to.path.join(".git/x").exists());
        assert!(!to.path.join("src/main.rs").exists());
    }

    #[test]
    fn includes_node_modules_when_matched_and_not_excluded() {
        let from = TmpDir::new("from2");
        let to = TmpDir::new("to2");
        from.write("node_modules/x", "junk");

        let include = s(&["node_modules/**"]);
        let copied = copy_files(&from.path, &to.path, &include, &[], false).unwrap();

        assert_eq!(copied, vec![PathBuf::from("node_modules/x")]);
        assert!(to.path.join("node_modules/x").exists());
    }

    #[test]
    fn dry_run_copies_nothing() {
        let from = TmpDir::new("from3");
        let to = TmpDir::new("to3");
        from.write(".env", "SECRET=1");

        let include = s(&[".env"]);
        let copied = copy_files(&from.path, &to.path, &include, &[], true).unwrap();

        assert_eq!(copied, vec![PathBuf::from(".env")]);
        assert!(!to.path.join(".env").exists());
    }

    #[test]
    fn copies_dirs_and_prunes_excluded_subpaths() {
        let from = TmpDir::new("dirfrom");
        let to = TmpDir::new("dirto");
        from.write("node_modules/pkg/index.js", "x");
        from.write("node_modules/.cache/blob", "junk");
        from.write("vendor/keep", "v");

        let copied = copy_dirs(
            &from.path,
            &to.path,
            &s(&["node_modules", "vendor"]),
            &s(&["node_modules/.cache"]),
            false,
        )
        .unwrap();

        assert!(copied.contains(&PathBuf::from("node_modules")));
        assert!(copied.contains(&PathBuf::from("vendor")));
        assert!(to.path.join("node_modules/pkg/index.js").exists());
        assert!(to.path.join("vendor/keep").exists());
        assert!(!to.path.join("node_modules/.cache").exists());
    }

    #[test]
    fn missing_dir_is_skipped() {
        let from = TmpDir::new("dirfrom2");
        let to = TmpDir::new("dirto2");
        let copied = copy_dirs(&from.path, &to.path, &s(&["nope"]), &[], false).unwrap();
        assert!(copied.is_empty());
    }

    #[test]
    fn reads_worktreeinclude_ignoring_comments() {
        let root = TmpDir::new("wti");
        root.write(".worktreeinclude", "# comment\n.env\n\n*.local\n");
        let pats = read_worktreeinclude(&root.path).unwrap();
        assert_eq!(pats, vec![".env".to_string(), "*.local".to_string()]);
    }

    #[test]
    fn empty_include_copies_nothing() {
        let from = TmpDir::new("from4");
        let to = TmpDir::new("to4");
        from.write(".env", "X=1");

        let copied = copy_files(&from.path, &to.path, &[], &[], false).unwrap();
        assert!(copied.is_empty());
        assert!(!to.path.join(".env").exists());
    }
}
