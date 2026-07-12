use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn authdog() -> Command {
    Command::cargo_bin("authdog").expect("authdog test binary")
}

#[test]
fn help_exposes_conventional_commands_and_ui() {
    authdog()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("  ui "))
        .stdout(predicate::str::contains("organizations"))
        .stdout(predicate::str::contains("--output"));
}

#[test]
fn no_arguments_prints_help_without_starting_tui() {
    authdog()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage: authdog"));
}

#[test]
fn json_status_is_parseable_and_redacts_tokens() {
    let config = tempfile::tempdir().unwrap();
    fs::write(
        config.path().join("credentials.json"),
        r#"{
  "access_token": "access-secret",
  "refresh_token": "refresh-secret",
  "current_organization_id": "org-1",
  "current_tenant_id": "tenant-1"
}"#,
    )
    .unwrap();

    let output = authdog()
        .env("AUTHDOG_CONFIG_DIR", config.path())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["logged_in"], true);
    assert_eq!(value["context"]["organization_id"], "org-1");
    assert!(!stdout.contains("access-secret"));
    assert!(!stdout.contains("refresh-secret"));
}

#[test]
fn resource_commands_fail_clearly_when_logged_out() {
    let config = tempfile::tempdir().unwrap();
    authdog()
        .env("AUTHDOG_CONFIG_DIR", config.path())
        .args(["projects", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("run `authdog login`"));
}

#[test]
fn api_failures_use_stderr_and_nonzero_exit() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"error":"Forbidden","detail":"test policy"}"#;
        write!(
            stream,
            "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let config = tempfile::tempdir().unwrap();
    fs::write(
        config.path().join("credentials.json"),
        r#"{"access_token":"access","refresh_token":"refresh"}"#,
    )
    .unwrap();
    authdog()
        .env("AUTHDOG_CONFIG_DIR", config.path())
        .env("AUTHDOG_API_ORIGIN", format!("http://{address}"))
        .args(["organizations", "list", "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Forbidden: test policy"));
    server.join().unwrap();
}
