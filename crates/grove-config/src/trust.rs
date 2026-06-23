//! Trust model for repo-shipped executable config.
//!
//! A `grove.kdl` can ship `tasks { run "…" }` commands that Grove will execute.
//! Running untrusted commands from a freshly cloned repo is dangerous, so the
//! user must approve a given `grove.kdl` (by content) via `grove trust` before
//! its tasks will run.
//!
//! Approval is recorded per-repo under the git common directory at
//! `<git-common-dir>/grove/trust`: a text file of approved SHA-256 hashes of
//! the `grove.kdl` contents, one hex digest per line. Changing the file's
//! contents changes its hash, so an edit re-requires approval.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{ConfigError, Result};

/// Trust state of the repo's `grove.kdl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStatus {
    /// No `grove.kdl` exists — nothing to trust.
    NoConfig,
    /// `grove.kdl` exists and its current contents are approved.
    Trusted,
    /// `grove.kdl` exists but its current contents are not approved.
    Untrusted,
}

/// Path to the `grove.kdl` under `root`.
fn config_path(root: &Path) -> PathBuf {
    root.join("grove.kdl")
}

/// Path to the trust ledger under the git common dir.
fn trust_path(git_dir: &Path) -> PathBuf {
    git_dir.join("grove").join("trust")
}

/// Compute the hex SHA-256 of the `grove.kdl` at `root`, if it exists.
fn config_hash(root: &Path) -> Result<Option<String>> {
    let path = config_path(root);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let digest = Sha256::digest(&bytes);
            Ok(Some(hex(&digest)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigError::Read { path, source }),
    }
}

/// Encode bytes as a lowercase hex string.
fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Writing to a String is infallible.
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Read the set of approved hashes from the trust ledger.
fn read_ledger(git_dir: &Path) -> Result<Vec<String>> {
    let path = trust_path(git_dir);
    match std::fs::read_to_string(&path) {
        Ok(contents) => Ok(contents
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(ToOwned::to_owned)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(ConfigError::Read { path, source }),
    }
}

/// Is the repo's current `grove.kdl` trusted?
///
/// Returns `true` if no `grove.kdl` exists, or if its hash is recorded in the
/// trust ledger. Errors are treated as "not trusted".
#[must_use]
pub fn is_trusted(root: &Path, git_dir: &Path) -> bool {
    matches!(
        trust_status(root, git_dir),
        TrustStatus::NoConfig | TrustStatus::Trusted
    )
}

/// Compute the [`TrustStatus`] of the repo's `grove.kdl`.
#[must_use]
pub fn trust_status(root: &Path, git_dir: &Path) -> TrustStatus {
    let Ok(Some(hash)) = config_hash(root) else {
        // No config (or unreadable): nothing to gate.
        return TrustStatus::NoConfig;
    };
    match read_ledger(git_dir) {
        Ok(hashes) if hashes.iter().any(|h| h == &hash) => TrustStatus::Trusted,
        _ => TrustStatus::Untrusted,
    }
}

/// Record trust for the repo's current `grove.kdl` (idempotent).
///
/// Appends the current config hash to the trust ledger. A no-op if there is no
/// `grove.kdl` or the hash is already recorded.
///
/// # Errors
/// Returns an error if the `grove.kdl` or trust ledger cannot be read, or the
/// ledger directory/file cannot be written.
pub fn record_trust(root: &Path, git_dir: &Path) -> Result<()> {
    let Some(hash) = config_hash(root)? else {
        return Ok(());
    };
    let existing = read_ledger(git_dir)?;
    if existing.iter().any(|h| h == &hash) {
        return Ok(());
    }

    let dir = trust_path(git_dir);
    let parent = dir.parent().unwrap_or(git_dir);
    std::fs::create_dir_all(parent).map_err(|source| ConfigError::Read {
        path: parent.to_path_buf(),
        source,
    })?;

    let mut contents = existing.join("\n");
    if !contents.is_empty() {
        contents.push('\n');
    }
    contents.push_str(&hash);
    contents.push('\n');

    let path = trust_path(git_dir);
    std::fs::write(&path, contents).map_err(|source| ConfigError::Read { path, source })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    struct Dirs {
        base: std::path::PathBuf,
        root: std::path::PathBuf,
        git: std::path::PathBuf,
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.base.as_path());
        }
    }

    fn dirs() -> Dirs {
        let base = std::env::temp_dir().join(format!(
            "grove-trust-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("root");
        let git = base.join("git");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&git).unwrap();
        Dirs { base, root, git }
    }

    fn write_config(root: &Path, contents: &str) {
        std::fs::write(root.join("grove.kdl"), contents).unwrap();
    }

    #[test]
    fn no_config_is_trusted() {
        let d = dirs();
        assert!(is_trusted(&d.root, &d.git));
        assert_eq!(trust_status(&d.root, &d.git), TrustStatus::NoConfig);
    }

    #[test]
    fn fresh_config_is_untrusted() {
        let d = dirs();
        write_config(&d.root, "tasks { build run \"echo hi\" }");
        assert!(!is_trusted(&d.root, &d.git));
        assert_eq!(trust_status(&d.root, &d.git), TrustStatus::Untrusted);
    }

    #[test]
    fn record_trust_makes_it_trusted() {
        let d = dirs();
        write_config(&d.root, "tasks { build run \"echo hi\" }");
        record_trust(&d.root, &d.git).unwrap();
        assert!(is_trusted(&d.root, &d.git));
        assert_eq!(trust_status(&d.root, &d.git), TrustStatus::Trusted);
    }

    #[test]
    fn record_trust_is_idempotent() {
        let d = dirs();
        write_config(&d.root, "tasks { build run \"echo hi\" }");
        record_trust(&d.root, &d.git).unwrap();
        record_trust(&d.root, &d.git).unwrap();
        let ledger = read_ledger(&d.git).unwrap();
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn changing_config_re_requires_trust() {
        let d = dirs();
        write_config(&d.root, "tasks { build run \"echo hi\" }");
        record_trust(&d.root, &d.git).unwrap();
        assert_eq!(trust_status(&d.root, &d.git), TrustStatus::Trusted);

        write_config(&d.root, "tasks { build run \"rm -rf /\" }");
        assert_eq!(trust_status(&d.root, &d.git), TrustStatus::Untrusted);
        assert!(!is_trusted(&d.root, &d.git));
    }

    #[test]
    fn record_trust_no_config_is_noop() {
        let d = dirs();
        record_trust(&d.root, &d.git).unwrap();
        assert!(read_ledger(&d.git).unwrap().is_empty());
    }
}
