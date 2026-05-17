//! `GET /v1/organizations` — organizations visible to the authenticated user (REST API).

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

const ORGANIZATIONS_PATH: &str = "/v1/organizations";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrgRow {
    pub id: String,
    pub name: Option<String>,
}

impl OrgRow {
    fn sort_key(&self) -> (String, String) {
        (
            self.name.clone().unwrap_or_default().to_ascii_lowercase(),
            self.id.clone(),
        )
    }

    /// Prefer org name when present for selection rows and browse headers.
    pub fn display_primary(&self) -> String {
        if let Some(ref n) = self.name {
            let t = n.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
        self.id.clone()
    }
}

/// Organizations from a **`GET /v1/organizations`** payload (`organizations` array or JSON array root).
pub fn organization_rows_from_body(body: &Value) -> Vec<OrgRow> {
    let arrays: &[&[Value]] = if let Some(a) = body.get("organizations").and_then(|v| v.as_array())
    {
        &[a.as_slice()]
    } else if let Some(a) = body.as_array() {
        &[a.as_slice()]
    } else {
        &[]
    };
    let mut rows: Vec<OrgRow> = Vec::new();
    for arr in arrays {
        for item in *arr {
            let Some(id) = item.get("id").and_then(|x| x.as_str()).map(str::trim) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let name = item
                .get("name")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            rows.push(OrgRow {
                id: id.to_string(),
                name,
            });
        }
    }
    rows.sort_by_key(|r| r.sort_key());
    rows.dedup_by(|a, b| a.id == b.id);
    rows
}

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

fn append_cf_1003_ops_hint(mut message: String) -> String {
    if message.contains("error code: 1003") {
        message.push_str(
            "\n  Hint (ops): Cloudflare 1003 means the Worker hit Management without a hostname Cloudflare accepts (often MANAGEMENT_ENDPOINT is an IP literal). Use https://mgt.authdog.com/graphql on authdog-api-prod-v2—or remove a conflicting Workers secret overriding wrangler.toml.",
        );
    }
    message
}

fn organizations_error_body_preview(status: reqwest::StatusCode, body: &str) -> String {
    let base = if let Ok(v) = serde_json::from_str::<Value>(body) {
        let detail = v
            .get("detail")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty());
        let err = v
            .get("error")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty());
        match (err, detail) {
            (Some(e), Some(d)) => format!("{status} — {e}: {d}"),
            (Some(e), None) => format!("{status} — {e}"),
            (None, Some(d)) => format!("{status} — {d}"),
            (None, None) => format!("{status} — {}", summarize_body_preview(body, 520)),
        }
    } else {
        format!("{status} — {}", summarize_body_preview(body, 520))
    };
    append_cf_1003_ops_hint(base)
}

/// `GET …/v1/organizations` with the access token.
pub fn fetch_organizations(access_token: &str) -> Result<Value> {
    let origin = crate::whoami::api_origin();
    let base = origin.trim_end_matches('/');
    let url = format!("{base}{ORGANIZATIONS_PATH}");
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("authdog-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build HTTP client for organizations")?;
    let resp = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(access_token)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().context("read organizations response body")?;
    if !status.is_success() {
        anyhow::bail!("{}", organizations_error_body_preview(status, &body))
    }
    serde_json::from_str(&body).context("organizations response is not valid JSON")
}

/// Text for **`/organizations`**: pretty JSON (+ optional credentials path line).
pub fn compose_organizations_report(
    access_token: &str,
    credentials_file_note: Option<String>,
) -> String {
    let origin = crate::whoami::api_origin();
    let base_shown = origin.trim_end_matches('/');
    let orgs_note = format!("{base_shown}{ORGANIZATIONS_PATH}");

    let body = match fetch_organizations(access_token) {
        Ok(ref v) => {
            let json = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
            format!("── Organizations ({orgs_note}) ──\n{json}")
        }
        Err(err) => {
            format!("── Organizations ({orgs_note}) ──\n  (could not load) {err:#}")
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
    fn parses_organization_rows() {
        let v: Value =
            serde_json::from_str(r#"{"organizations":[{"id":"o1","name":"Acme"},{"id":"o2"}]}"#)
                .unwrap();
        let rows = organization_rows_from_body(&v);
        assert_eq!(rows.len(), 2);
        let o1 = rows.iter().find(|r| r.id == "o1").expect("o1");
        let o2 = rows.iter().find(|r| r.id == "o2").expect("o2");
        assert_eq!(o1.name.as_deref(), Some("Acme"));
        assert!(o2.name.is_none());
    }
    #[test]
    fn cf_1003_detail_appends_ops_hint() {
        let body = r#"{"error":"Failed","detail":"403: error code: 1003 | 403: error code: 1003"}"#;
        let s = organizations_error_body_preview(reqwest::StatusCode::FORBIDDEN, body);
        assert!(s.contains("error code: 1003"));
        assert!(s.contains("Hint (ops):"));
        assert!(s.contains("mgt.authdog.com"));
    }

    #[test]
    fn error_preview_extracts_rest_detail_json() {
        let body =
            r#"{"error":"Failed to fetch organizations","detail":"upstream: graphql rejected"}"#;
        let s = organizations_error_body_preview(reqwest::StatusCode::FORBIDDEN, body);
        assert!(s.contains("403"), "expected status in preview: {s}");
        assert!(s.contains("Failed to fetch organizations"), "{s}");
        assert!(s.contains("graphql rejected"), "{s}");
    }
}
