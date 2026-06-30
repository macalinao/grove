//! Dynamic shell-completion helpers (bpaf `complete`).
//!
//! These run inside the real binary when the shell asks for completions
//! (`COMPLETE=bash grove …`). They must never panic and stay fast: if the repo
//! can't be discovered we simply offer no suggestions.

use std::collections::HashSet;

use grove_core::Grove;

/// Complete a worktree `NAME` positional (`cd`, `go`, `run`, `editor`, `ai`,
/// `rm`, `mv`) with the branch and folder names of existing worktrees, plus the
/// `1` main-repo alias — exactly the targets `grove` itself accepts.
///
/// bpaf filters the returned candidates against the word being typed, so we
/// return the full set.
#[must_use]
pub fn worktree_names(_input: &String) -> Vec<(String, Option<String>)> {
    let Ok(grove) = Grove::open() else {
        return Vec::new();
    };
    let root = grove.root().to_path_buf();
    let Ok(worktrees) = grove.list() else {
        return Vec::new();
    };

    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut seen = HashSet::new();
    for w in worktrees {
        let kind = if w.path == root {
            "main worktree"
        } else {
            "worktree"
        };
        // The branch name is what users normally type.
        if let Some(branch) = &w.branch {
            if seen.insert(branch.clone()) {
                out.push((branch.clone(), Some(kind.to_string())));
            }
        }
        // Offer the folder name too when it differs from the branch.
        if let Some(folder) = w.folder_name() {
            if Some(folder) != w.branch.as_deref() && seen.insert(folder.to_string()) {
                out.push((folder.to_string(), Some("folder".to_string())));
            }
        }
    }
    // gtr's `1` alias for the main repo.
    if seen.insert("1".to_string()) {
        out.push(("1".to_string(), Some("main worktree".to_string())));
    }
    out
}
