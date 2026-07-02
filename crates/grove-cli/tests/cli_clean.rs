//! `grove clean` — PR-state cleanup against the native GitHub forge.
//!
//! GitHub is served over HTTP (`octocrab`), so these tests stand up a tiny
//! local HTTP server and point `grove.forge.host` at it (no network, no `gh`).
//! The server derives PR state from the queried `--head` branch name:
//! `merged-*` → merged, `closed-*` → closed, `open-*` → open, else no PR.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use common::{TestRepo, failed, ok};

/// Start a mock GitHub REST server on an ephemeral port; returns its base URL.
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

/// Answer one `GET .../pulls?head=owner:<branch>...` request with canned JSON.
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
    let pull = |extra: &str| format!(r#"[{{"state":{extra},"base":{{"ref":"main"}}}}]"#);
    if branch.starts_with("merged-") {
        pull(r#""closed","merged_at":"2026-01-01T00:00:00Z""#)
    } else if branch.starts_with("closed-") {
        pull(r#""closed","merged_at":null"#)
    } else if branch.starts_with("open-") {
        pull(r#""open","merged_at":null"#)
    } else {
        "[]".to_string()
    }
}

fn setup() -> TestRepo {
    let r = TestRepo::new();
    r.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/owner/repo.git",
    ]);
    // Point the native GitHub forge at the local mock and short-circuit token
    // discovery so the test never touches the developer's real `gh` creds.
    r.set_config("grove.forge.host", &start_mock_github());
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
fn dry_run_keeps_everything() {
    let r = setup();
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    ok(&clean(&r, &["--merged", "--dry-run"]));
    assert!(r.wt("merged-x").is_dir(), "dry-run must not remove");
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
fn no_flags_just_prunes() {
    let r = setup();
    let out = clean(&r, &[]);
    ok(&out);
    assert!(common::stderr(&out).to_lowercase().contains("prune"));
}

#[test]
fn errors_without_a_forge_remote() {
    // No origin remote at all → provider can't be detected.
    let r = TestRepo::new();
    let err = failed(&r.grove(&["clean", "--merged"]));
    assert!(err.to_lowercase().contains("forge") || err.to_lowercase().contains("provider"));
}
