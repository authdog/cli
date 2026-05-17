//! `GET /v1/tenants/{tenantId}/projects` and application environment helpers (REST API).

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

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

fn projects_error_body_preview(status: reqwest::StatusCode, body: &str) -> String {
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

/// `GET …/v1/tenants/{tenant_id}/projects` with the access token.
pub fn fetch_projects(access_token: &str, tenant_id: &str) -> Result<Value> {
    let origin = crate::whoami::api_origin();
    let base = origin.trim_end_matches('/');
    let url = format!("{base}/v1/tenants/{tenant_id}/projects");
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("authdog-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build HTTP client for projects")?;
    let resp = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(access_token)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().context("read projects response body")?;
    if !status.is_success() {
        anyhow::bail!("{}", projects_error_body_preview(status, &body))
    }
    serde_json::from_str(&body).context("projects response is not valid JSON")
}

/// Text for **`/projects`**: pretty JSON (+ optional credentials path line).
pub fn compose_projects_report(
    access_token: &str,
    tenant_id: &str,
    credentials_file_note: Option<String>,
) -> String {
    let origin = crate::whoami::api_origin();
    let base_shown = origin.trim_end_matches('/');
    let projects_note = format!("{base_shown}/v1/tenants/{tenant_id}/projects");

    let body = match fetch_projects(access_token, tenant_id) {
        Ok(ref v) => {
            let json = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
            format!("── Projects ({projects_note}) ──\n{json}")
        }
        Err(err) => {
            format!("── Projects ({projects_note}) ──\n  (could not load) {err:#}")
        }
    };

    let mut sections = vec![body];
    if let Some(note) = credentials_file_note {
        sections.push(String::new());
        sections.push(note);
    }
    sections.join("\n")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRow {
    pub id: String,
    pub name: Option<String>,
    pub project_type: Option<String>,
}

impl ProjectRow {
    fn sort_key(&self) -> (String, String) {
        let name = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_ascii_lowercase();
        (name, self.id.clone())
    }

    /// Primary label for list rows (`name`, else id).
    pub fn display_primary(&self) -> String {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.id.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentRow {
    pub id: String,
    pub name: Option<String>,
}

impl EnvironmentRow {
    fn sort_key(&self) -> (String, String) {
        let name = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_ascii_lowercase();
        (name, self.id.clone())
    }

    pub fn display_primary(&self) -> String {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.id.clone())
    }
}

/// Projects array from **`GET /v1/tenants/{id}/projects`** (`projects` array on object or root array).
pub fn project_rows_from_body(body: &Value) -> Vec<ProjectRow> {
    let arrays: &[&[Value]] = if let Some(a) = body.get("projects").and_then(|v| v.as_array()) {
        &[a.as_slice()]
    } else if let Some(a) = body.as_array() {
        &[a.as_slice()]
    } else {
        &[]
    };
    let mut out: Vec<ProjectRow> = Vec::new();
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
            let project_type = item
                .get("type")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            out.push(ProjectRow {
                id: id.to_string(),
                name,
                project_type,
            });
        }
    }
    out.sort_by_key(|r| r.sort_key());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// Environments array from **`GET …/applications/{id}/environments`** (`environments` on object).
pub fn environment_rows_from_body(body: &Value) -> Vec<EnvironmentRow> {
    let arrays: &[&[Value]] = if let Some(a) = body.get("environments").and_then(|v| v.as_array()) {
        &[a.as_slice()]
    } else if let Some(a) = body.as_array() {
        &[a.as_slice()]
    } else {
        &[]
    };
    let mut out: Vec<EnvironmentRow> = Vec::new();
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
            out.push(EnvironmentRow {
                id: id.to_string(),
                name,
            });
        }
    }
    out.sort_by_key(|r| r.sort_key());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// `GET …/v1/tenants/{tenant_id}/applications/{application_id}/environments`
pub fn fetch_application_environments(
    access_token: &str,
    tenant_id: &str,
    application_id: &str,
) -> Result<Value> {
    let origin = crate::whoami::api_origin();
    let base = origin.trim_end_matches('/');
    let url = format!("{base}/v1/tenants/{tenant_id}/applications/{application_id}/environments");
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("authdog-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build HTTP client for application environments")?;
    let resp = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(access_token)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .context("read application environments response body")?;
    if !status.is_success() {
        anyhow::bail!("{}", projects_error_body_preview(status, &body))
    }
    serde_json::from_str(&body).context("application environments response is not valid JSON")
}

/// Session output after `/browse` finishes on an environment.
pub fn compose_selected_environment_report(
    access_token: &str,
    tenant_id: &str,
    application_id: &str,
    environment_id: &str,
    snapshot: Value,
    credentials_file_note: Option<String>,
) -> String {
    let origin = crate::whoami::api_origin();
    let base_shown = origin.trim_end_matches('/');
    let path_note =
        format!("{base_shown}/v1/tenants/{tenant_id}/applications/{application_id}/environments",);
    let selected = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| snapshot.to_string());
    let mut sections = vec![format!(
        "── Project environment ──\nTenant: {tenant_id}\nProject (application): {application_id}\nEnvironment: {environment_id}\nEndpoint context: {path_note}\n{selected}",
    )];
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
    fn cf_1003_detail_appends_ops_hint() {
        let body = r#"{"error":"Failed","detail":"403: error code: 1003 | 403: error code: 1003"}"#;
        let s = projects_error_body_preview(reqwest::StatusCode::FORBIDDEN, body);
        assert!(s.contains("error code: 1003"));
        assert!(s.contains("Hint (ops):"));
    }

    #[test]
    fn error_preview_extracts_rest_detail_json() {
        let body = r#"{"error":"Failed to fetch projects","detail":"upstream: graphql rejected"}"#;
        let s = projects_error_body_preview(reqwest::StatusCode::FORBIDDEN, body);
        assert!(s.contains("403"), "expected status in preview: {s}");
        assert!(s.contains("Failed to fetch projects"), "{s}");
    }
}
