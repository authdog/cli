//! `GET /v1/tenants` — tenants visible to the authenticated user (REST API).

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;
use url::Url;

pub const TENANTS_PATH: &str = "/v1/tenants";

/// Full **`GET …/v1/tenants`** URL for CLI copy (optional **`organization_id`** query when scoped).
pub fn tenants_list_url_for_display(api_origin: &str, organization_scope: Option<&str>) -> String {
    let base = api_origin.trim_end_matches('/');
    let path = format!("{base}{TENANTS_PATH}");
    let Some(oid) = organization_scope.map(str::trim).filter(|s| !s.is_empty()) else {
        return path;
    };
    let Ok(mut u) = Url::parse(&path) else {
        return format!("{path}?organization_id={oid}");
    };
    u.query_pairs_mut().append_pair("organization_id", oid);
    u.to_string()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantRow {
    pub id: String,
    pub name: Option<String>,
    pub organization_id: Option<String>,
    /// Set when **`GET /v1/tenants`** includes **`organizationIds`**. **`Some([])`** means the merged list did not associate that tenant with an org-linked slice (e.g. **`tenantsWithAccess`** only).
    pub organization_ids: Option<Vec<String>>,
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

fn tenant_organization_ids_array(item: &Value) -> Option<Vec<String>> {
    for key in ["organizationIds", "organization_ids"] {
        let Some(raw) = item.get(key).and_then(|x| x.as_array()) else {
            continue;
        };
        let mut ids: Vec<String> = Vec::new();
        for el in raw {
            let Some(s) = el.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                continue;
            };
            ids.push(s.to_string());
        }
        ids.sort();
        ids.dedup();
        return Some(ids);
    }
    None
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
            let organization_ids_raw = tenant_organization_ids_array(item);
            let scalar_org = tenant_org_id_hint(item);
            let organization_ids =
                organization_ids_raw.or_else(|| scalar_org.as_ref().map(|s| vec![s.clone()]));
            let organization_id =
                scalar_org.or_else(|| organization_ids.as_ref().and_then(|v| v.first().cloned()));
            out.push(TenantRow {
                id: id.to_string(),
                name,
                organization_id,
                organization_ids,
            });
        }
    }
    out.sort_by_key(|r| r.sort_key());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

fn tenant_lists_organization(t: &TenantRow, org_id: &str) -> bool {
    if let Some(slice) = t.organization_ids.as_deref() {
        if slice.iter().any(|oid| oid == org_id) {
            return true;
        }
    }
    t.organization_id.as_deref() == Some(org_id)
}

/// At least one row carries a **non-empty** org link (**`organizationIds`** or scalar **`organizationId`**, …).
///
/// Rows with **`organizationIds: []`** alone do **not** count — the API uses that for grant-only merges
/// (see **`tenants/list handler`** merging **`userOrganizations`** + **`tenantsWithAccess`**).
fn aggregation_includes_organization_membership_hints(all: &[TenantRow]) -> bool {
    all.iter().any(|t| {
        if let Some(s) = t.organization_id.as_deref() {
            if !s.is_empty() {
                return true;
            }
        }
        t.organization_ids
            .as_ref()
            .is_some_and(|v| v.iter().any(|id| !id.is_empty()))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantOrgFilterMode {
    /// When org linkage is missing from the merged payload, keep returning **every** tenant (older gateways / unknown shapes).
    PermissiveLegacy,
    /// `/browse` after picking an organization: never show the full merged list as if it belonged to that org when no row has usable **`organizationIds`** data.
    BrowseOrganizationScoped,
}

fn filter_tenants_for_organization_inner(
    all: &[TenantRow],
    org_id: &str,
    mode: TenantOrgFilterMode,
) -> (Vec<TenantRow>, Option<String>) {
    if org_id.is_empty() || all.is_empty() {
        return (all.to_vec(), None);
    }

    if !aggregation_includes_organization_membership_hints(all) {
        return match mode {
            TenantOrgFilterMode::PermissiveLegacy => (all.to_vec(), None),
            TenantOrgFilterMode::BrowseOrganizationScoped => (
                vec![],
                Some(
                    "Cannot list tenants for this organization: `GET …/tenants` has no non-empty `organizationIds` (or scalar org id) on any row. \
The org + tenant merge likely failed for `userOrganizations` upstream while `tenantsWithAccess` still returned rows. \
Try again, or check the API / Management GraphQL."
                        .to_string(),
                ),
            ),
        };
    }

    let matched: Vec<TenantRow> = all
        .iter()
        .filter(|t| tenant_lists_organization(t, org_id))
        .cloned()
        .collect();

    if matched.is_empty() && !all.is_empty() {
        return (
            matched,
            Some(format!(
                "REST tenants response includes organization membership, but none of these tenants belong to `{org_id}`. Press Esc and try another organization."
            )),
        );
    }

    (matched, None)
}

/// Narrow **`GET /v1/tenants`** rows when the REST payload includes usable organization membership metadata.
///
/// Legacy servers that omit linkage keep the previous behaviour: return **all** tenants (some org pickers cannot be narrowed client-side alone).
pub fn filter_tenants_for_organization(
    all: &[TenantRow],
    org_id: &str,
) -> (Vec<TenantRow>, Option<String>) {
    filter_tenants_for_organization_inner(all, org_id, TenantOrgFilterMode::PermissiveLegacy)
}

/// Like [`filter_tenants_for_organization`], but for **`/browse`** after the user picks an organization — avoids showing **every** merged tenant when org linkage is missing from the payload.
pub fn filter_tenants_for_organization_for_browse(
    all: &[TenantRow],
    org_id: &str,
) -> (Vec<TenantRow>, Option<String>) {
    filter_tenants_for_organization_inner(
        all,
        org_id,
        TenantOrgFilterMode::BrowseOrganizationScoped,
    )
}

/// Parse **`GET /v1/tenants`** JSON and apply an optional organization scope for listings (**`/tenants`**, **`/tenant`**).
///
/// When **`organization_scope`** is **`Some`**, narrows rows using [`filter_tenants_for_organization_for_browse`]
/// so the UI stays correct if the gateway ignores `?organization_id=…` but includes per-tenant org metadata
/// (**`organizationIds`**, …). With **`None`**, returns every tenant row from the payload.
pub fn tenant_listing_rows_from_body(
    body: &Value,
    organization_scope: Option<&str>,
) -> (Vec<TenantRow>, Option<String>) {
    let all = tenant_rows_from_body(body);
    match organization_scope.map(str::trim).filter(|s| !s.is_empty()) {
        None => (all, None),
        Some(org_id) => filter_tenants_for_organization_for_browse(&all, org_id),
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
///
/// When **`scoped_organization_id`** is **`Some`**, sends **`organization_id`** and **`organizationId`**
/// (same value; some gateways normalize one form) so the server can return only tenants whose **`organizationIds`**
/// include that organization. When **`None`**, requests the full merged tenant list.
pub fn fetch_tenants(access_token: &str, scoped_organization_id: Option<&str>) -> Result<Value> {
    let origin = crate::whoami::api_origin();
    let base = origin.trim_end_matches('/');
    let url = format!("{base}{TENANTS_PATH}");
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("authdog-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build HTTP client for tenants")?;
    let base_req = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(access_token);
    let req = match scoped_organization_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(oid) => base_req.query(&[
            ("organization_id", oid),
            ("organizationId", oid),
        ]),
        None => base_req,
    };
    let resp = req.send().with_context(|| format!("GET {url}"))?;
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
pub fn compose_tenants_report(
    access_token: &str,
    organization_scope: Option<&str>,
    credentials_file_note: Option<String>,
) -> String {
    let origin = crate::whoami::api_origin();
    let base_shown = origin.trim_end_matches('/');
    let tenants_note = tenants_list_url_for_display(base_shown, organization_scope);

    let body = match fetch_tenants(access_token, organization_scope) {
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
    fn tenant_listing_rows_applies_org_scope_when_payload_has_org_metadata() {
        let v: Value = serde_json::from_str(
            r#"{"tenants":[
               {"id":"in-org","name":"A","organizationIds":["org-x"]},
               {"id":"other","name":"B","organizationIds":["org-y"]}
            ]}"#,
        )
        .unwrap();
        let (rows, note) = tenant_listing_rows_from_body(&v, Some("org-x"));
        assert!(note.is_none());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "in-org");
    }

    #[test]
    fn tenants_list_url_display_adds_org_query() {
        let u = tenants_list_url_for_display("https://api.example.com", Some("org-1"));
        assert_eq!(
            u,
            "https://api.example.com/v1/tenants?organization_id=org-1"
        );
        let base = tenants_list_url_for_display("https://api.example.com", None);
        assert_eq!(base, "https://api.example.com/v1/tenants");
        assert_eq!(
            tenants_list_url_for_display("https://api.example.com/", Some("  x  ")),
            "https://api.example.com/v1/tenants?organization_id=x"
        );
    }

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
        assert_eq!(t1.organization_ids.as_deref(), Some(&["org-a".into()][..]));
        assert_eq!(t2.organization_ids.as_deref(), Some(&["org-b".into()][..]));

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
                organization_ids: None,
            },
            TenantRow {
                id: "b".into(),
                name: None,
                organization_id: None,
                organization_ids: None,
            },
        ];
        let (all, _) = filter_tenants_for_organization(&rows, "anything");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn filter_with_declared_org_metadata_no_overlap_returns_empty() {
        let rows = vec![
            TenantRow {
                id: "a".into(),
                name: None,
                organization_id: Some("org-1".into()),
                organization_ids: Some(vec!["org-1".into()]),
            },
            TenantRow {
                id: "b".into(),
                name: None,
                organization_id: Some("org-2".into()),
                organization_ids: Some(vec!["org-2".into()]),
            },
        ];
        let (narrow, hint) = filter_tenants_for_organization(&rows, "missing-org");
        assert!(narrow.is_empty());
        assert!(hint.unwrap().contains("missing-org"));
    }

    #[test]
    fn filter_organization_ids_excludes_grant_only_tenant() {
        let v: Value = serde_json::from_str(
            r#"{"tenants":[
               {"id":"linked","organizationIds":["org-demo"]},
               {"id":"grant_only","organizationIds":[]}
            ]}"#,
        )
        .unwrap();
        let rows = tenant_rows_from_body(&v);
        let (picked, _) = filter_tenants_for_organization(&rows, "org-demo");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, "linked");
    }

    #[test]
    fn browse_strict_empty_when_every_row_only_has_empty_organization_ids() {
        let v: Value = serde_json::from_str(
            r#"{"tenants":[
               {"id":"a","organizationIds":[]},
               {"id":"b","organizationIds":[]}
            ]}"#,
        )
        .unwrap();
        let rows = tenant_rows_from_body(&v);
        let (narrow, hint) = filter_tenants_for_organization_for_browse(&rows, "org-x");
        assert!(narrow.is_empty());
        assert!(hint.unwrap().contains("organizationIds"));

        let (legacy, _) = filter_tenants_for_organization(&rows, "org-x");
        assert_eq!(legacy.len(), 2);
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
