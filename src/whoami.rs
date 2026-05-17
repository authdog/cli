//! JWT decode for `/whoami` (**signature not verified** — reference only), plus **`/v1/userinfo`**.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
#[cfg(feature = "desktop")]
use reqwest::blocking::Client;
use serde_json::{Map, Value};
#[cfg(feature = "desktop")]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default REST API origin (matches Authdog SDK `GET …/v1/userinfo`).
pub const DEFAULT_API_ORIGIN: &str = "https://api.authdog.com";

/// Never echo embedded session / opaque tokens.
const SKIP_KEYS: &[&str] = &["sid"];

const PREFERRED_KEYS: &[&str] = &[
    "sub",
    "externalid",
    "env",
    "iss",
    "aud",
    "egat",
    "iat",
    "exp",
];

#[cfg(feature = "desktop")]
const USERINFO_PATH: &str = "/v1/userinfo";

#[cfg(feature = "desktop")]
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

fn format_ts_secs(secs: i64) -> String {
    DateTime::<Utc>::from_timestamp(secs, 0)
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| format!("{secs} (invalid timestamp)"))
}

pub fn api_origin() -> String {
    std::env::var("AUTHDOG_API_ORIGIN").unwrap_or_else(|_| DEFAULT_API_ORIGIN.into())
}

/// Parse JWT payload JSON (middle segment, base64url). Does **not** verify the signature.
pub fn decode_jwt_claims(access_token: &str) -> Result<Value> {
    let parts: Vec<&str> = access_token.split('.').collect();
    anyhow::ensure!(
        parts.len() == 3,
        "not a JWT (expected header.payload.signature)"
    );
    let bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("could not decode JWT payload (base64url)")?;
    serde_json::from_slice(&bytes).context("JWT payload is not valid JSON")
}

/// `GET …/v1/userinfo` with the access token (**server-checked** identity record).
#[cfg(feature = "desktop")]
pub fn fetch_identity_userinfo(access_token: &str) -> Result<Value> {
    let origin = api_origin();
    let base = origin.trim_end_matches('/');
    let url = format!("{base}{USERINFO_PATH}");
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("authdog-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build HTTP client for userinfo")?;
    let resp = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(access_token)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().context("read userinfo response body")?;
    if !status.is_success() {
        anyhow::bail!(
            "userinfo {} — {}",
            status,
            summarize_body_preview(&body, 520)
        );
    }
    serde_json::from_str(&body).context("userinfo response is not valid JSON")
}

fn claim_kv_width(pref: &[&str], others: &[String]) -> usize {
    let w1 = pref.iter().copied().map(|k| k.len() + 1).max().unwrap_or(0);
    let w2 = others.iter().map(|k| k.len() + 1).max().unwrap_or(0);
    w1.max(w2).clamp(11, 34)
}

fn padded_claim_line(width: usize, key: &str, value_display: &str) -> String {
    format!(
        "  {:<lw$}{value}",
        format!("{}:", key),
        lw = width,
        value = value_display
    )
}

fn nested_claim_line(width: usize, key: &str, value_display: &str) -> String {
    format!(
        "    {:<lw$}{value}",
        format!("{}:", key),
        lw = width,
        value = value_display
    )
}

fn pretty_token_claim_lines(map: &Map<String, Value>, now_secs: i64) -> Vec<String> {
    let mut pref_seen: Vec<&str> = Vec::new();
    for &k in PREFERRED_KEYS {
        if SKIP_KEYS.contains(&k) {
            continue;
        }
        if map.contains_key(k) {
            pref_seen.push(k);
        }
    }

    let mut others: Vec<_> = map
        .keys()
        .filter(|k| !PREFERRED_KEYS.contains(&k.as_str()) && !SKIP_KEYS.contains(&k.as_str()))
        .cloned()
        .collect();
    others.sort();

    let w = claim_kv_width(&pref_seen, &others);
    let mut lines: Vec<String> = Vec::new();

    for k in pref_seen {
        let raw = map.get(k).unwrap_or(&Value::Null);
        if matches!(
            raw,
            Value::String(_) | Value::Bool(_) | Value::Number(_) | Value::Null
        ) {
            if k == "exp" || k == "iat" {
                if let Some(secs) = raw.as_i64().or_else(|| raw.as_u64().map(|u| u as i64)) {
                    let formatted = format_ts_secs(secs);
                    let suffix = if k == "exp" && secs <= now_secs {
                        " (expired)"
                    } else {
                        ""
                    };
                    lines.push(padded_claim_line(w, k, &format!("{formatted}{suffix}")));
                    continue;
                }
            }
            lines.push(padded_claim_line(w, k, &summarize_value(raw)));
            continue;
        }
        lines.push(padded_claim_line(
            w,
            k,
            &summarize_value_truncated(raw, 160),
        ));
    }

    if !others.is_empty() {
        lines.push(String::new());
        lines.push("  Other claims:".into());
        for key in others {
            let raw = map.get(&key).unwrap_or(&Value::Null);
            if matches!(
                raw,
                Value::String(_) | Value::Bool(_) | Value::Number(_) | Value::Null
            ) {
                if key == "exp" || key == "iat" {
                    if let Some(secs) = raw.as_i64().or_else(|| raw.as_u64().map(|u| u as i64)) {
                        let formatted = format_ts_secs(secs);
                        let suffix = if key == "exp" && secs <= now_secs {
                            " (expired)"
                        } else {
                            ""
                        };
                        lines.push(nested_claim_line(w, &key, &format!("{formatted}{suffix}")));
                        continue;
                    }
                }
                lines.push(nested_claim_line(w, &key, &summarize_value(raw)));
                continue;
            }
            lines.push(format!("    {}:", key));
            let nested = serde_json::to_string_pretty(raw).unwrap_or_else(|_| raw.to_string());
            for nl in nested.lines() {
                lines.push(format!("      {nl}"));
            }
        }
    }

    lines
}

pub fn render_whoami_from_claims(payload: &Value) -> String {
    pretty_token_claims_display(payload).unwrap_or_else(|e| e)
}

fn pretty_token_claims_display(payload: &Value) -> Result<String, String> {
    let Value::Object(map) = payload else {
        return Err("(unexpected non-object JWT payload)".into());
    };

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let lines = pretty_token_claim_lines(map, now_secs);

    if lines.is_empty() {
        return Err("(JWT payload empty or only filtered fields)".into());
    }

    Ok(lines.join("\n"))
}

fn summarize_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => "null".into(),
        Value::Array(_) | Value::Object(_) => summarize_value_truncated(v, 120),
    }
}

fn summarize_value_truncated(v: &Value, max: usize) -> String {
    let s = v.to_string();
    if s.len() <= max {
        return s;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}… (+{} chars)", &s[..cut], s.len() - cut)
}

pub fn describe_access_token(access_token: &str) -> Result<String> {
    Ok(render_whoami_from_claims(&decode_jwt_claims(access_token)?))
}

/// Human-readable **`GET …/v1/userinfo`** URL as shown in the CLI (origin + path).
#[cfg(feature = "desktop")]
pub fn userinfo_endpoint_display() -> String {
    format!("{}{}", api_origin().trim_end_matches('/'), USERINFO_PATH)
}

fn human_section_title(name: &str) -> String {
    let mut it = name.chars();
    let Some(head) = it.next() else {
        return name.to_string();
    };
    format!("{}{}", head.to_uppercase(), it.as_str())
}

fn truncated_cell(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max.saturating_sub(1);
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

fn leaf_to_display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => "—".into(),
        Value::Array(_) | Value::Object(_) => summarize_value_truncated(v, 200),
    }
}

fn sort_object_keys(m: &Map<String, Value>) -> Vec<String> {
    let mut keys: Vec<String> = m.keys().cloned().collect();
    keys.sort();
    keys
}

fn prioritized_columns(keys: &[String]) -> Vec<String> {
    const FRONT: &[&str] = &[
        "id", "type", "primary", "verified", "kind", "value", "address",
    ];
    const BACK: &[&str] = &["updatedAt", "createdAt"];

    let mut front: Vec<String> = Vec::new();
    let mut mid: Vec<String> = Vec::new();
    let mut back: Vec<String> = Vec::new();

    for k in keys {
        if FRONT.contains(&k.as_str()) {
            front.push(k.clone());
        } else if BACK.contains(&k.as_str()) {
            back.push(k.clone());
        } else {
            mid.push(k.clone());
        }
    }
    front.sort_by_key(|x| FRONT.iter().position(|y| *y == x.as_str()).unwrap_or(999));
    mid.sort();
    back.sort_by_key(|x| BACK.iter().position(|y| *y == x.as_str()).unwrap_or(999));

    front.extend(mid);
    front.extend(back);
    front
}

fn column_width_for_objects(
    rows: &[&Map<String, Value>],
    cols: &[String],
    max_w: usize,
) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for c in cols {
        let header = c.len();
        let body = rows
            .iter()
            .map(|r| leaf_to_display(r.get(c.as_str()).unwrap_or(&Value::Null)).len())
            .max()
            .unwrap_or(0);
        widths.push(header.max(body).clamp(4, max_w));
    }
    widths
}

fn padded_cell(text: &str, width: usize) -> String {
    let trunc = truncated_cell(text, width);
    format!("{trunc:<width$}")
}

fn array_of_objects_as_table(rows: &[&Map<String, Value>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut keyset = std::collections::BTreeSet::new();
    for r in rows {
        for k in r.keys() {
            keyset.insert(k.clone());
        }
    }
    let keys: Vec<String> = prioritized_columns(&keyset.into_iter().collect::<Vec<_>>());
    let col_w = column_width_for_objects(rows, &keys, 44);
    let idx_w = (rows.len() + 1).to_string().len().max(1);

    let mut lines: Vec<String> = Vec::new();
    let header_cells: Vec<String> = keys
        .iter()
        .zip(col_w.iter())
        .map(|(k, w)| padded_cell(k, *w))
        .collect();

    lines.push(format!(
        "      {:idx_w$}  {}",
        "#",
        header_cells.join("  "),
        idx_w = idx_w
    ));
    let rule_len = idx_w
        + 2
        + header_cells.iter().map(|s| s.len()).sum::<usize>()
        + 2usize.saturating_mul(header_cells.len().saturating_sub(1));
    lines.push(format!("      {}", "-".repeat(rule_len.min(120))));

    for (i, r) in rows.iter().enumerate() {
        let cells: Vec<String> = keys
            .iter()
            .zip(col_w.iter())
            .map(|(k, w)| padded_cell(&leaf_to_display(r.get(k).unwrap_or(&Value::Null)), *w))
            .collect();
        lines.push(format!(
            "      {:idx_w$}  {}",
            i + 1,
            cells.join("  "),
            idx_w = idx_w
        ));
    }
    lines.join("\n")
}

fn summarize_non_object_array(arr: &[Value]) -> String {
    if arr.is_empty() {
        return "(empty)".into();
    }
    let nested = serde_json::to_string(&Value::Array(arr.to_vec())).unwrap_or_else(|_| "[]".into());
    if nested.len() > 240 {
        truncated_cell(&nested, 239)
    } else {
        nested
    }
}

fn render_nested_object_section(
    label: &str,
    inner: &Map<String, Value>,
    indent_spaces: usize,
) -> String {
    let pad = " ".repeat(indent_spaces);
    let title = human_section_title(label);
    let underline = "-".repeat((title.len() + 8).clamp(16, 64));
    let body = render_object_readable(inner, indent_spaces + 2);
    format!("{pad}{title}\n{pad}{underline}\n{body}")
}

/// Render **`meta`/`session`/`user`-style envelope objects** plus unknown top-level keys.
fn render_object_readable(obj: &Map<String, Value>, indent_spaces: usize) -> String {
    let pad = " ".repeat(indent_spaces);
    let mut kv: Vec<(String, String)> = Vec::new();
    let mut blocks: Vec<String> = Vec::new();

    for k in sort_object_keys(obj) {
        let v = obj.get(k.as_str()).unwrap_or(&Value::Null);
        match v {
            Value::Object(m) if !m.is_empty() => {
                blocks.push(render_nested_object_section(&k, m, indent_spaces));
            }
            Value::Array(a) => {
                if a.is_empty() {
                    kv.push((k, "(empty)".into()));
                    continue;
                }
                let rows: Vec<_> = a.iter().filter_map(|row| row.as_object()).collect();
                if rows.len() == a.len() {
                    let title = human_section_title(k.as_str());
                    let note = format!(
                        "{pad}{title}  ({} {})",
                        rows.len(),
                        if rows.len() == 1 { "entry" } else { "entries" }
                    );
                    let tbl = array_of_objects_as_table(rows.as_slice());
                    blocks.push(format!("{note}\n{tbl}"));
                } else {
                    kv.push((k, summarize_non_object_array(a)));
                }
            }
            Value::Null => kv.push((k, "—".into())),
            _ => kv.push((k.clone(), leaf_to_display(v))),
        }
    }

    let mut chunks: Vec<String> = Vec::new();
    if !kv.is_empty() {
        let key_w = kv
            .iter()
            .map(|(k, _)| k.len() + 1)
            .max()
            .unwrap_or(0)
            .clamp(10, 36);
        let mut lines = Vec::new();
        for (k, vs) in &kv {
            lines.push(format!(
                "{pad}{:<kw$}  {}",
                format!("{}:", k),
                vs,
                kw = key_w
            ));
        }
        chunks.push(lines.join("\n"));
    }
    chunks.extend(blocks);
    chunks.join("\n")
}

fn format_section_block(section_key: &str, v: &Value) -> String {
    let title = human_section_title(section_key);
    let underline = "─".repeat((title.len() + 16).clamp(24, 64));
    match v {
        Value::Object(m) => {
            let body = render_object_readable(m, 2);
            format!("{title}\n{underline}\n{body}")
        }
        _ => {
            format!("{title}\n{underline}\n  {}", leaf_to_display(v))
        }
    }
}

fn envelope_key_order(root: &Map<String, Value>) -> Vec<String> {
    const BOOT: &[&str] = &["meta", "session", "user"];
    let mut out = Vec::new();
    for k in BOOT {
        if root.contains_key(*k) {
            out.push((*k).to_string());
        }
    }
    let mut rest: Vec<String> = root
        .keys()
        .filter(|k| !BOOT.contains(&k.as_str()))
        .cloned()
        .collect();
    rest.sort();
    out.extend(rest);
    out
}

/// Tabular "**Pretty**" view for **`GET /v1/userinfo`** payloads (**`/whoami`** Pretty tab).
pub fn format_identity_pretty_display(v: &Value) -> String {
    let Value::Object(root) = v else {
        return serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    };
    envelope_key_order(root)
        .into_iter()
        .filter_map(|k| root.get(k.as_str()).map(|vv| format_section_block(&k, vv)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Tabular "**Pretty**" text plus indented JSON (**Raw** tab) for **`/whoami`** Pretty vs Raw tabs.
#[cfg(feature = "desktop")]
pub fn format_identity_json_pair(v: &Value) -> (String, String) {
    let pretty = format_identity_pretty_display(v);
    let raw_json = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    (pretty, raw_json)
}

/// Full **`/whoami`** output: **`GET /v1/userinfo`** as tabular Pretty text (+ optional **`credentials file:`** note).
#[cfg(feature = "desktop")]
pub fn compose_whoami_report(access_token: &str, credentials_file_note: Option<String>) -> String {
    let userinfo_note = userinfo_endpoint_display();

    let identity_section = match fetch_identity_userinfo(access_token) {
        Ok(ref v) => {
            let (pretty, _) = format_identity_json_pair(v);
            format!("── Identity ({userinfo_note}) ──\n{pretty}")
        }
        Err(err) => {
            format!("── Identity ({userinfo_note}) ──\n  (could not load) {err:#}")
        }
    };

    let mut sections = vec![identity_section];

    if let Some(note) = credentials_file_note {
        sections.push(String::new());
        sections.push(note);
    }

    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    fn jwt_with_json_payload(obj_json: &str) -> String {
        let enc = URL_SAFE_NO_PAD.encode(obj_json.as_bytes());
        format!("xx.{enc}.sig")
    }

    #[test]
    fn identity_pretty_table_envelope() {
        let v = serde_json::json!({
            "meta": { "code": 200_u64, "message": "Success" },
            "session": { "remainingSeconds": 70856_u64 },
            "user": {
                "active": true,
                "displayName": "Jane Q. User",
                "addresses": [],
                "emails": [
                    {"id": "uuid-1", "type": serde_json::Value::Null, "value": "a@example.com"},
                ]
            }
        });
        let t = format_identity_pretty_display(&v);
        assert!(!t.trim_start().starts_with('{'));
        assert!(t.contains("Meta"));
        assert!(t.contains("code"));
        assert!(t.contains("200"));
        assert!(t.contains("Session"));
        assert!(t.contains("remainingSeconds"));
        assert!(t.contains("User"));
        assert!(t.contains("displayName"));
        assert!(t.contains("Emails"));
        assert!(t.contains('#'));
        assert!(t.contains("a@example.com"));
    }

    #[test]
    fn identity_raw_json_is_pretty_printed() {
        let v = serde_json::json!({
            "meta": { "code": 200_u64 },
            "user": { "id": "u1" }
        });
        let (_, raw) = format_identity_json_pair(&v);
        assert!(
            raw.contains('\n'),
            "expected indented JSON with newlines, got single line"
        );
        assert!(raw.contains("  \"meta\""));
        assert!(raw.contains("  \"user\""));
    }

    #[test]
    fn decodes_and_renders_key_claims() {
        let jwt = jwt_with_json_payload(
            r#"{"sub":"google-oauth:e1","externalid":"google-oauth:e1","env":"uuid-env","iat":1710000000,"exp":1710086400}"#,
        );
        let claims = decode_jwt_claims(&jwt).expect("decode");
        let text = render_whoami_from_claims(&claims);
        assert!(text.contains("sub"));
        assert!(text.contains("google-oauth:e1"));
        assert!(text.contains("env"));
        assert!(text.contains("uuid-env"));
    }

    #[test]
    fn skips_sid_claim() {
        let jwt = jwt_with_json_payload(
            r#"{"sub":"u1","sid":"SUPER_SECRET_SESSION","env":"e1","exp":9999999999}"#,
        );
        let claims = decode_jwt_claims(&jwt).unwrap();
        let text = render_whoami_from_claims(&claims);
        assert!(!text.contains("SUPER_SECRET"));
        assert!(!text.contains("sid"));
    }
}
