/// Errors from the git process layer.
#[derive(thiserror::Error, Debug)]
pub enum GitError {
    #[error("failed to spawn `git {cmd}`: {source}")]
    Spawn { cmd: String, source: std::io::Error },

    #[error("`git {cmd}` failed: {stderr}")]
    Command { cmd: String, stderr: String },

    #[error("git produced non-UTF-8 output")]
    NotUtf8,

    #[error("getting current directory: {0}")]
    Cwd(#[source] std::io::Error),

    #[error("not inside a git repository")]
    NotARepo,

    #[error("repository has no worktrees")]
    NoWorktrees,
}
