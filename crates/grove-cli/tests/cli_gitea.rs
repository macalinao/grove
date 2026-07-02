//! `grove clean` against the native Gitea forge.
//!
//! Gitea is served over HTTP (`/api/v1`, a thin `reqwest` client), so these
//! tests stand up a tiny local HTTP server and point the forge at it — no
//! network, no `tea` binary. Gitea's list-pulls endpoint has no `head` filter,
//! so the mock returns the full PR list every time and `grove` matches the
//! head branch client-side. All hostnames use `gitea.myhost.com`.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use common::{TestRepo, ok};

/// Start a mock Gitea REST server on an ephemeral port; returns its base URL.
fn start_mock_gitea() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            handle(stream);
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// Answer any request with the canned pulls list (path/query are ignored;
/// `grove` filters by head branch client-side).
fn handle(mut stream: std::net::TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    // Drain the remaining request headers.
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line == "\r\n" || line == "\n" => break,
            Ok(_) => {}
        }
    }
    let body = pulls_body();
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// The full pulls list: `state` + `merged` bool + head/base refs, mirroring
/// Gitea's `GET /repos/{owner}/{repo}/pulls`.
fn pulls_body() -> String {
    let pull = |state: &str, merged: bool, head: &str| {
        format!(
            r#"{{"state":"{state}","merged":{merged},"base":{{"ref":"main"}},"head":{{"ref":"{head}"}}}}"#
        )
    };
    let pulls = [
        pull("closed", true, "merged-x"),
        pull("open", false, "open-y"),
        pull("closed", false, "closed-z"),
    ];
    format!("[{}]", pulls.join(","))
}

/// A repo whose `origin` is a Gitea host, with the forge pointed at the mock.
/// The Gitea host name auto-detects the provider (no `grove.provider` needed).
fn setup() -> TestRepo {
    let r = TestRepo::new();
    r.git(&[
        "remote",
        "add",
        "origin",
        "https://gitea.myhost.com/owner/repo.git",
    ]);
    r.set_config("grove.forge.host", &start_mock_gitea());
    r.set_config("grove.forge.token", "test-token");
    r
}

fn clean(r: &TestRepo, args: &[&str]) -> std::process::Output {
    let mut full = vec!["clean"];
    full.extend_from_slice(args);
    r.grove(&full)
}

#[test]
fn removes_merged_keeps_open() {
    let r = setup();
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    ok(&r.grove(&["new", "open-y", "--no-fetch"]));
    ok(&clean(&r, &["--merged", "--yes"]));
    assert!(
        !r.wt("merged-x").exists(),
        "merged worktree should be removed"
    );
    assert!(r.wt("open-y").is_dir(), "open worktree should be kept");
}

#[test]
fn closed_flag_removes_closed() {
    let r = setup();
    ok(&r.grove(&["new", "closed-z", "--no-fetch"]));
    ok(&clean(&r, &["--closed", "--yes"]));
    assert!(!r.wt("closed-z").exists());
}

#[test]
fn to_filter_limits_by_base_branch() {
    let r = setup();
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    // The mock reports base = main; filtering to develop should skip it.
    ok(&clean(&r, &["--merged", "--to", "develop", "--yes"]));
    assert!(r.wt("merged-x").is_dir(), "base mismatch should keep it");
    // Filtering to main should remove it.
    ok(&clean(&r, &["--merged", "--to", "main", "--yes"]));
    assert!(!r.wt("merged-x").exists());
}

#[test]
fn provider_override_on_neutral_host() {
    // A non-Gitea-looking host: detection needs the explicit `grove.provider`.
    let r = TestRepo::new();
    r.git(&[
        "remote",
        "add",
        "origin",
        "https://git.example.com/owner/repo.git",
    ]);
    r.set_config("grove.provider", "gitea");
    r.set_config("grove.forge.host", &start_mock_gitea());
    r.set_config("grove.forge.token", "test-token");
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    ok(&clean(&r, &["--merged", "--yes"]));
    assert!(!r.wt("merged-x").exists());
}
