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

use crate::{CoreError, Result};

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
    fn empty_include_copies_nothing() {
        let from = TmpDir::new("from4");
        let to = TmpDir::new("to4");
        from.write(".env", "X=1");

        let copied = copy_files(&from.path, &to.path, &[], &[], false).unwrap();
        assert!(copied.is_empty());
        assert!(!to.path.join(".env").exists());
    }
}
