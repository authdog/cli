//! `GET /v1/tenants` — tenants visible to the authenticated user (REST API).

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

const TENANTS_PATH: &str = "/v1/tenants";

fn summarize_body_preview(body: &str, max: usize) -> String {
    if body.len() <= max {
        return body.to_string();
    }
    let mut cut = max;
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{} … (+{} bytes)", &body[..cut], body.len() - cut)
}

fn tenants_error_body_preview(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        let detail = v
            .get("detail")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty());
        let err = v
            .get("error")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty());
        return match (err, detail) {
            (Some(e), Some(d)) => format!("{status} — {e}: {d}"),
            (Some(e), None) => format!("{status} — {e}"),
            (None, Some(d)) => format!("{status} — {d}"),
            (None, None) => format!("{status} — {}", summarize_body_preview(body, 520)),
        };
    }
    format!("{status} — {}", summarize_body_preview(body, 520))
}

/// `GET …/v1/tenants` with the access token.
pub fn fetch_tenants(access_token: &str) -> Result<Value> {
    let origin = crate::whoami::api_origin();
    let base = origin.trim_end_matches('/');
    let url = format!("{base}{TENANTS_PATH}");
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("authdog-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build HTTP client for tenants")?;
    let resp = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(access_token)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().context("read tenants response body")?;
    if !status.is_success() {
        anyhow::bail!("{}", tenants_error_body_preview(status, &body))
    }
    serde_json::from_str(&body).context("tenants response is not valid JSON")
}

/// Text for **`/tenants`**: pretty JSON (+ optional credentials path line).
pub fn compose_tenants_report(access_token: &str, credentials_file_note: Option<String>) -> String {
    let origin = crate::whoami::api_origin();
    let base_shown = origin.trim_end_matches('/');
    let tenants_note = format!("{base_shown}{TENANTS_PATH}");

    let body = match fetch_tenants(access_token) {
        Ok(ref v) => {
            let json = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
            format!("── Tenants ({tenants_note}) ──\n{json}")
        }
        Err(err) => {
            format!("── Tenants ({tenants_note}) ──\n  (could not load) {err:#}")
        }
    };

    let mut sections = vec![body];
    if let Some(note) = credentials_file_note {
        sections.push(String::new());
        sections.push(note);
    }
    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_preview_extracts_rest_detail_json() {
        let body = r#"{"error":"Failed to fetch tenants","detail":"upstream: graphql rejected"}"#;
        let s = tenants_error_body_preview(reqwest::StatusCode::FORBIDDEN, body);
        assert!(s.contains("403"), "expected status in preview: {s}");
        assert!(s.contains("Failed to fetch tenants"), "{s}");
        assert!(s.contains("graphql rejected"), "{s}");
    }
}
