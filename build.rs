//! Emits compile-time banner metadata (`cargo:rustc-env=…`).
use std::process::Command;

fn main() {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let naive = chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.naive_utc().date())
        .unwrap_or_default();
    let month_year = naive.format("%B-%Y").to_string();

    let sha = git_short_sha().unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=AUTHDOG_CLI_MONTH_YEAR={month_year}");
    println!("cargo:rustc-env=AUTHDOG_CLI_GIT_SHA={sha}");

    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");
}

fn git_short_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}
