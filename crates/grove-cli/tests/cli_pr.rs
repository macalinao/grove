//! `grove pr` / `grove open` — reverse-direction forge URLs (worktree → forge).
//!
//! `grove pr` builds a compare URL purely from config (no network), so it is
//! tested against a plain GitHub remote. `grove open` queries the forge for a
//! PR, so it points the native GitHub client at a tiny local mock server (as
//! `cli_clean` does) and asserts the PR page vs. the branch-page fallback.
//! `--print` is used throughout so no browser is launched.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use common::{TestRepo, ok};

/// Start a mock GitHub REST server on an ephemeral port; returns its base URL.
/// A `haspr-*` head branch yields a numbered open PR; anything else yields `[]`.
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
    let body = body_for(&request_line);
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// Map the requested head branch to a pulls-list JSON body.
fn body_for(request_line: &str) -> String {
    let branch = request_line
        .split_whitespace()
        .nth(1)
        .and_then(|target| target.split("head=").nth(1))
        .map(|rest| rest.split(['&', ' ']).next().unwrap_or("").to_string())
        .map(|head| head.replace("%3A", ":"))
        .and_then(|head| head.rsplit(':').next().map(str::to_string))
        .unwrap_or_default();
    if branch.starts_with("haspr-") {
        r#"[{"number":42,"state":"open","base":{"ref":"main"},"head":{"ref":"haspr-x"}}]"#
            .to_string()
    } else {
        "[]".to_string()
    }
}

/// A repo with a GitHub `origin` and a default base branch; `grove pr` needs no
/// network so no mock is wired here.
fn pr_repo() -> TestRepo {
    let r = TestRepo::new();
    r.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/owner/repo.git",
    ]);
    r.set_config("grove.defaultBranch", "main");
    r
}

/// A repo whose native GitHub forge points at the local mock server.
fn open_repo() -> TestRepo {
    let r = pr_repo();
    r.set_config("grove.forge.host", &start_mock_github());
    r.set_config("grove.forge.token", "test-token");
    r
}

#[test]
fn pr_prints_github_compare_url_with_expand() {
    let r = pr_repo();
    ok(&r.grove(&["new", "feat-x", "--no-fetch"]));
    let out = r.grove_in(&r.wt("feat-x"), &["pr", "--print"]);
    let url = ok(&out);
    assert_eq!(
        url.trim(),
        "https://github.com/owner/repo/compare/main...feat-x?expand=1"
    );
}

#[test]
fn open_prints_pr_page_when_a_pr_exists() {
    let r = open_repo();
    ok(&r.grove(&["new", "haspr-x", "--no-fetch"]));
    let out = r.grove_in(&r.wt("haspr-x"), &["open", "--print"]);
    let url = ok(&out);
    assert!(
        url.trim().ends_with("/owner/repo/pull/42"),
        "expected PR page, got {url:?}"
    );
}

#[test]
fn open_falls_back_to_branch_page_without_a_pr() {
    let r = open_repo();
    ok(&r.grove(&["new", "nopr-y", "--no-fetch"]));
    let out = r.grove_in(&r.wt("nopr-y"), &["open", "--print"]);
    let url = ok(&out);
    assert!(
        url.trim().ends_with("/owner/repo/tree/nopr-y"),
        "expected branch page, got {url:?}"
    );
}
