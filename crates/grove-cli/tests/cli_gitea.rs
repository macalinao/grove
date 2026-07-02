//! `grove clean` and `grove list --json` against the native Gitea forge.
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

/// The full pulls list: `number` + `state` + `merged` bool + head/base refs,
/// mirroring Gitea's `GET /repos/{owner}/{repo}/pulls`.
fn pulls_body() -> String {
    let pull = |number: u64, state: &str, merged: bool, head: &str| {
        format!(
            r#"{{"number":{number},"state":"{state}","merged":{merged},"base":{{"ref":"main"}},"head":{{"ref":"{head}"}}}}"#
        )
    };
    let pulls = [
        pull(1, "closed", true, "merged-x"),
        pull(2, "open", false, "open-y"),
        pull(3, "closed", false, "closed-z"),
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

#[test]
fn list_json_annotates_pr_number_state_and_url() {
    let r = setup();
    ok(&r.grove(&["new", "merged-x", "--no-fetch"]));
    ok(&r.grove(&["new", "open-y", "--no-fetch"]));
    let out = ok(&r.grove(&["list", "--json"]));

    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = parsed.as_array().unwrap();

    let merged = arr.iter().find(|w| w["branch"] == "merged-x").unwrap();
    assert_eq!(merged["pr"]["number"], 1);
    assert_eq!(merged["pr"]["state"], "merged");
    // Web URL is derived from the configured host + owner/repo (pulls, plural).
    assert!(
        merged["pr"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/owner/repo/pulls/1"),
        "pr url: {out}"
    );

    let open = arr.iter().find(|w| w["branch"] == "open-y").unwrap();
    assert_eq!(open["pr"]["number"], 2);
    assert_eq!(open["pr"]["state"], "open");

    // The main worktree has no matching PR, so `pr` is absent entirely.
    let main = arr.iter().find(|w| w["branch"] == "main").unwrap();
    assert!(main.get("pr").is_none(), "main should have no pr: {out}");
}

#[test]
fn list_json_pr_absent_when_forge_unreachable() {
    // A forge is configured but the host refuses connections: PR lookups fail
    // best-effort, so `pr` is simply absent and the command still succeeds.
    let r = TestRepo::new();
    r.git(&[
        "remote",
        "add",
        "origin",
        "https://gitea.myhost.com/owner/repo.git",
    ]);
    r.set_config("grove.forge.host", "http://127.0.0.1:1");
    r.set_config("grove.forge.token", "test-token");
    ok(&r.grove(&["new", "feature", "--no-fetch"]));

    let out = ok(&r.grove(&["list", "--json"]));
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let feature = parsed
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["branch"] == "feature")
        .unwrap();
    assert!(feature.get("pr").is_none(), "pr should be absent: {out}");
}
