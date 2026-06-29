//! Smart file copy engine for `grove copy`.
//!
//! Copies files (typically untracked config/env files such as `.env`) from a
//! source worktree into a target worktree by glob. Patterns come from the
//! resolved [`grove_config::Config`] (or the CLI). Walks the source tree,
//! skipping `.git`, and copies every file whose path matches an include glob
//! and no exclude glob.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::{CoreError, Result};

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

/// Copy each directory in `dirs` from `from` into `to`, pruning subpaths that
/// match an `exclude_dirs` glob. Uses copy-on-write when the platform supports
/// it, falling back to a plain recursive copy.
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
        let dst = to.join(&rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        cow_copy_dir(&src, &dst)?;
        prune_excludes(to, &rel, &exclude_set)?;
    }
    Ok(copied)
}

/// Copy `src` directory to `dst` using a copy-on-write clone where available.
///
/// On macOS this uses `cp -cR` (APFS clone); on Linux `cp -a --reflink=auto`
/// (btrfs/xfs reflink, plain copy otherwise). If `cp` is unavailable or fails,
/// falls back to a recursive in-process copy.
fn cow_copy_dir(src: &Path, dst: &Path) -> Result<()> {
    let args: &[&str] = if cfg!(target_os = "macos") {
        &["-cR"]
    } else {
        &["-a", "--reflink=auto"]
    };
    let ran = Command::new("cp")
        .args(args)
        .arg(src)
        .arg(dst)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ran {
        return Ok(());
    }
    recursive_copy(src, dst)
}

/// Plain recursive directory copy (fallback when CoW `cp` is unavailable).
fn recursive_copy(src: &Path, dst: &Path) -> Result<()> {
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
        let file_type = entry.file_type().map_err(|source| CoreError::Io {
            path: entry.path(),
            source,
        })?;
        let child_src = entry.path();
        let child_dst = dst.join(entry.file_name());
        if file_type.is_dir() {
            recursive_copy(&child_src, &child_dst)?;
        } else {
            fs::copy(&child_src, &child_dst).map_err(|source| CoreError::Io {
                path: child_dst,
                source,
            })?;
        }
    }
    Ok(())
}

/// Remove any path under `<to>/<rel>` whose path relative to `to` matches an
/// `exclude` glob. Walks top-down and deletes the matched subtree.
fn prune_excludes(to: &Path, rel: &Path, exclude: &GlobSet) -> Result<()> {
    if exclude.is_empty() {
        return Ok(());
    }
    let mut to_remove = Vec::new();
    collect_excluded(to, &to.join(rel), exclude, &mut to_remove)?;
    // Deepest paths first so a parent removal never invalidates a child path.
    to_remove.sort_by_key(|p| core::cmp::Reverse(p.components().count()));
    for path in to_remove {
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

/// Gather absolute paths under `dir` that match an exclude glob (relative to `to`).
fn collect_excluded(
    to: &Path,
    dir: &Path,
    exclude: &GlobSet,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry.map_err(|source| CoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if let Ok(rel) = path.strip_prefix(to) {
            if exclude.is_match(rel) {
                out.push(path.clone());
                continue;
            }
        }
        if path.is_dir() {
            collect_excluded(to, &path, exclude, out)?;
        }
    }
    Ok(())
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
