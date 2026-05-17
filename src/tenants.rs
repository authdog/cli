//! `GET /v1/tenants` — tenants visible to the authenticated user (REST API).

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

const TENANTS_PATH: &str = "/v1/tenants";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantRow {
    pub id: String,
    pub name: Option<String>,
    pub organization_id: Option<String>,
}

impl TenantRow {
    pub fn sort_key(&self) -> (String, String) {
        let n = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_ascii_lowercase();
        (n, self.id.clone())
    }
}

fn tenant_org_id_hint(item: &Value) -> Option<String> {
    for key in [
        "organization_id",
        "organizationId",
        "org_id",
        "organizationUUID",
        "organizationUuid",
    ] {
        if let Some(v) = item.get(key).and_then(|x| x.as_str()) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    item.get("organization")
        .and_then(|o| o.get("id"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn tenant_name_hint(item: &Value) -> Option<String> {
    for key in ["name", "title", "displayName", "display_name"] {
        if let Some(v) = item.get(key).and_then(|x| x.as_str()) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

/// Tenant rows extracted from **`GET /v1/tenants`** JSON (`tenants` array or JSON array root).
pub fn tenant_rows_from_body(body: &Value) -> Vec<TenantRow> {
    let arrays: &[&[Value]] = if let Some(a) = body.get("tenants").and_then(|v| v.as_array()) {
        &[a.as_slice()]
    } else if let Some(a) = body.as_array() {
        &[a.as_slice()]
    } else {
        &[]
    };
    let mut out: Vec<TenantRow> = Vec::new();
    for arr in arrays {
        for item in *arr {
            let Some(id) = item.get("id").and_then(|x| x.as_str()).map(str::trim) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let name = tenant_name_hint(item);
            let organization_id = tenant_org_id_hint(item);
            out.push(TenantRow {
                id: id.to_string(),
                name,
                organization_id,
            });
        }
    }
    out.sort_by_key(|r| r.sort_key());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// If any tenant declares an owning organization id, restrict to **`org_id`**. Otherwise returns full list.
///
/// When filtering finds no rows but **`all`** non-empty (schema mismatch upstream), callers may widen with the message.
pub fn filter_tenants_for_organization(
    all: &[TenantRow],
    org_id: &str,
) -> (Vec<TenantRow>, Option<String>) {
    if org_id.is_empty() || all.is_empty() {
        return (all.to_vec(), None);
    }
    let linked = all.iter().any(|t| t.organization_id.is_some());
    if !linked {
        return (all.to_vec(), None);
    }
    let matched: Vec<TenantRow> = all
        .iter()
        .filter(|t| t.organization_id.as_deref() == Some(org_id))
        .cloned()
        .collect();
    if matched.is_empty() && !all.is_empty() {
        (
            all.to_vec(),
            Some(format!(
                "No `/v1/tenants` rows reference organization `{org_id}`; showing every tenant."
            )),
        )
    } else {
        (matched, None)
    }
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

fn tenants_error_body_preview(status: reqwest::StatusCode, body: &str) -> String {
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

/// Tenant ids from a **`GET /v1/tenants`** JSON body (`tenants` array or root array of objects).
pub fn tenant_ids_from_body(body: &Value) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let arrays: &[&[Value]] = if let Some(a) = body.get("tenants").and_then(|v| v.as_array()) {
        &[a.as_slice()]
    } else if let Some(a) = body.as_array() {
        &[a.as_slice()]
    } else {
        &[]
    };
    for arr in arrays {
        for item in *arr {
            if let Some(id) = item
                .get("id")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                ids.push(id.to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
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
    fn cf_1003_detail_appends_ops_hint() {
        let body = r#"{"error":"Failed","detail":"403: error code: 1003 | 403: error code: 1003"}"#;
        let s = tenants_error_body_preview(reqwest::StatusCode::FORBIDDEN, body);
        assert!(s.contains("error code: 1003"));
        assert!(s.contains("Hint (ops):"));
        assert!(s.contains("mgt.authdog.com"));
    }

    #[test]
    fn collects_ids_from_tenants_field() {
        let v: Value =
            serde_json::from_str(r#"{"tenants":[{"id":"a"},{"id":"b"},{"foo":1}]}"#).unwrap();
        assert_eq!(
            tenant_ids_from_body(&v),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn tenant_rows_and_org_filter_helpers() {
        let v: Value = serde_json::from_str(
            r#"{"tenants":[
               {"id":"t1","name":"Alpha","organizationId":"org-a"},
               {"id":"t2","organization_id":"org-b"}
            ]}"#,
        )
        .unwrap();
        let rows = tenant_rows_from_body(&v);
        assert_eq!(rows.len(), 2);
        let t1 = rows.iter().find(|r| r.id == "t1").unwrap();
        let t2 = rows.iter().find(|r| r.id == "t2").unwrap();
        assert_eq!(t1.organization_id.as_deref(), Some("org-a"));
        assert_eq!(t2.organization_id.as_deref(), Some("org-b"));

        let (m, _) = filter_tenants_for_organization(&rows, "org-a");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "t1");
    }

    #[test]
    fn filter_without_org_links_returns_everyone() {
        let rows = vec![
            TenantRow {
                id: "a".into(),
                name: None,
                organization_id: None,
            },
            TenantRow {
                id: "b".into(),
                name: None,
                organization_id: None,
            },
        ];
        let (all, _) = filter_tenants_for_organization(&rows, "anything");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn filter_no_match_fallback_lists_all_and_warns() {
        let rows = vec![
            TenantRow {
                id: "a".into(),
                name: None,
                organization_id: Some("org-1".into()),
            },
            TenantRow {
                id: "b".into(),
                name: None,
                organization_id: Some("org-2".into()),
            },
        ];
        let (wide, hint) = filter_tenants_for_organization(&rows, "missing-org");
        assert_eq!(wide.len(), 2);
        assert!(hint.unwrap().contains("missing-org"));
    }

    #[test]
    fn error_preview_extracts_rest_detail_json() {
        let body = r#"{"error":"Failed to fetch tenants","detail":"upstream: graphql rejected"}"#;
        let s = tenants_error_body_preview(reqwest::StatusCode::FORBIDDEN, body);
        assert!(s.contains("403"), "expected status in preview: {s}");
        assert!(s.contains("Failed to fetch tenants"), "{s}");
        assert!(s.contains("graphql rejected"), "{s}");
    }
}
