//! JWT decode for `/whoami` (**signature not verified** — reference only), plus **`/v1/userinfo`**.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
#[cfg(feature = "desktop")]
use reqwest::blocking::Client;
use serde_json::{Map, Value};
use std::cmp;
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
    cmp::min(34, cmp::max(11, cmp::max(w1, w2)))
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

/// Full **`/whoami`** text: **`GET /v1/userinfo`** JSON (+ optional **`credentials file:`** note).
#[cfg(feature = "desktop")]
pub fn compose_whoami_report(access_token: &str, credentials_file_note: Option<String>) -> String {
    let origin = api_origin();
    let base_shown = origin.trim_end_matches('/');
    let userinfo_note = format!("{base_shown}{USERINFO_PATH}");

    let identity_section = match fetch_identity_userinfo(access_token) {
        Ok(ref v) => {
            let json = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
            format!("── Identity ({userinfo_note}) ──\n{json}")
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
