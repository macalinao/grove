//! Forge refs: addressing worktrees by `#n`, `--pr` / `--issue`, and forge URLs
//! in `grove new` / `grove go`.
//!
//! Two offline strategies keep these hermetic:
//!   * The `#42` fork-fetch path needs no forge API — the PR head is fetched
//!     straight from `refs/pull/42/head` on a local bare `origin`, so the head
//!     checks out with zero network and the name degrades to `pr-42`.
//!   * The API-naming paths (`--pr` head branch, `--issue` `<n>-<slug>`) point a
//!     native GitHub forge at a tiny local mock server; the origin stays a
//!     `github.com` URL only so `owner/repo` parses. All hostnames use
//!     `github.com`.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use common::{TestRepo, ok, stdout};

/// A mock GitHub REST server: replies to `/pulls/<n>` with a PR whose head is
/// `feat/widget`, and to `/issues/<n>` with a titled issue. Any other path 404s.
fn start_mock_github() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            handle(stream);
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// Read one request line, drain headers, and answer based on the path.
fn handle(mut stream: std::net::TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line == "\r\n" || line == "\n" => break,
            Ok(_) => {}
        }
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("");
    let (status, body) = route(path);
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// Map a request path to a canned `(status, json)` response.
fn route(path: &str) -> (&'static str, &'static str) {
    if path.contains("/pulls/") {
        (
            "200 OK",
            r#"{"number":42,"state":"open","title":"Add widgets",
                "merged_at":null,"base":{"ref":"main"},
                "head":{"ref":"feat/widget",
                        "repo":{"name":"repo","owner":{"login":"contributor"}}}}"#,
        )
    } else if path.contains("/issues/") {
        ("200 OK", r#"{"number":88,"title":"Widgets should wobble"}"#)
    } else {
        ("404 Not Found", "{}")
    }
}

/// A repo whose `origin` is a `github.com` URL, with the native forge pointed at
/// a local mock (so `owner/repo` parses but no real network is touched).
fn mock_repo() -> TestRepo {
    let r = TestRepo::new();
    r.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/owner/repo.git",
    ]);
    r.set_config("grove.forge.host", &start_mock_github());
    r.set_config("grove.forge.token", "test-token");
    r
}

#[test]
fn hash_ref_fetches_pull_head_from_origin_without_a_forge() {
    // A local bare `origin` carrying `refs/pull/42/head` — no forge API at all.
    let r = TestRepo::new();
    r.add_origin();
    r.git(&["checkout", "-q", "-b", "pr-src"]);
    r.write("pr.txt", "from the PR");
    r.git(&["add", "."]);
    r.git(&["commit", "-q", "-m", "pr work"]);
    r.git(&["push", "-q", "origin", "pr-src:refs/pull/42/head"]);
    r.git(&["checkout", "-q", "main"]);
    r.git(&["branch", "-q", "-D", "pr-src"]);

    // No token / provider ⇒ name degrades to `pr-42`, head fetched from origin.
    ok(&r.grove(&["new", "#42"]));
    assert!(r.wt("pr-42").is_dir(), "worktree pr-42 should exist");
    assert!(
        r.wt("pr-42").join("pr.txt").is_file(),
        "the PR head content should be checked out"
    );
    assert!(r.local_branches().contains(&"pr-42".to_string()));

    // `grove go #42` resolves back to the same worktree path.
    let out = r.grove(&["go", "#42"]);
    assert_eq!(stdout(&out).trim(), r.wt("pr-42").to_string_lossy());
}

#[test]
fn pr_flag_names_worktree_after_the_head_branch() {
    let r = mock_repo();
    // The PR head branch already exists locally, so no fetch is needed; the
    // worktree is named after the real head (`feat/widget` → `feat-widget`).
    r.git(&["branch", "feat/widget"]);
    ok(&r.grove(&["new", "--pr", "42", "--no-fetch"]));
    assert!(
        r.wt("feat-widget").is_dir(),
        "worktree should be named after the PR head branch"
    );
}

#[test]
fn issue_flag_creates_slugged_branch_from_title() {
    let r = mock_repo();
    ok(&r.grove(&["new", "--issue", "88", "--no-fetch"]));
    assert!(
        r.wt("88-widgets-should-wobble").is_dir(),
        "issue worktree should be `<n>-<slug>`; folders: {:?}",
        r.worktree_folders()
    );
    assert!(
        r.local_branches()
            .contains(&"88-widgets-should-wobble".to_string())
    );
}

#[test]
fn go_errors_with_a_hint_when_no_worktree_matches() {
    let r = TestRepo::new();
    r.add_origin();
    let out = r.grove(&["go", "#7"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("grove new"), "expected a hint, got: {err}");
}

#[test]
fn plain_branch_ref_still_works() {
    // A bare name is a branch ref — today's behavior is unchanged.
    let r = TestRepo::new();
    ok(&r.grove(&["new", "feature", "--no-fetch"]));
    assert!(r.wt("feature").is_dir());
    let out = r.grove(&["go", "feature"]);
    assert_eq!(stdout(&out).trim(), r.wt("feature").to_string_lossy());
}
