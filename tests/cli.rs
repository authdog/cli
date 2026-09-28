use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn authdog() -> Command {
    Command::cargo_bin("authdog").expect("authdog test binary")
}

#[test]
fn help_exposes_login_logout_status() {
    authdog()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("login"))
        .stdout(predicate::str::contains("logout"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("--output"));
}

#[test]
fn no_arguments_prints_help() {
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
  "refresh_token": "refresh-secret"
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
    assert!(!stdout.contains("access-secret"));
    assert!(!stdout.contains("refresh-secret"));
}

#[test]
fn logout_removes_credentials() {
    let config = tempfile::tempdir().unwrap();
    let creds = config.path().join("credentials.json");
    fs::write(
        &creds,
        r#"{"access_token":"access","refresh_token":"refresh"}"#,
    )
    .unwrap();
    assert!(creds.exists());

    authdog()
        .env("AUTHDOG_CONFIG_DIR", config.path())
        .args(["logout"])
        .assert()
        .success();

    assert!(!creds.exists());
}
