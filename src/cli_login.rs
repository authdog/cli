//! Browser OAuth hosted on Identity: `/signin/...` with `cli_sess` correlates browser + CLI.
//! The CLI listens on **`http://127.0.0.1`** and passes **`cli_redirect`** so Identity redirects the
//! browser here with **`?grant=`**; tokens are redeemed via **`POST /api/v1/cli/oauth/redeem`**.

use crate::session_store::{save_session, StoredSession};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;
use uuid::Uuid;

/// Browser redirect path the CLI listens on (must stay in sync with sign-in redirect).
pub const LOOPBACK_OAUTH_REDIRECT_PATH: &str = "/oauth/callback";

/// Default Identity host (hosted `/signin` + OAuth bridges).
pub const DEFAULT_IDENTITY_ORIGIN: &str = "https://identity.authdog.com";

/// Matches `apps/identity/src/commons.ts` `consoleProjectEnvironment` (production console tenant).
pub const DEFAULT_CONSOLE_ENVIRONMENT_ID: &str = "ed89ef1e-2e76-4674-8272-5634064ae293";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliAuthConfig {
    pub identity_origin: String,
    pub environment_id: String,
}

impl CliAuthConfig {
    #[must_use]
    pub fn new(identity_origin: impl AsRef<str>, environment_id: impl AsRef<str>) -> Self {
        Self {
            identity_origin: identity_origin.as_ref().trim_end_matches('/').to_string(),
            environment_id: environment_id.as_ref().to_string(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(
            std::env::var("AUTHDOG_IDENTITY_ORIGIN")
                .unwrap_or_else(|_| DEFAULT_IDENTITY_ORIGIN.to_string()),
            std::env::var("AUTHDOG_CONSOLE_ENVIRONMENT_ID")
                .unwrap_or_else(|_| DEFAULT_CONSOLE_ENVIRONMENT_ID.to_string()),
        )
    }
}

pub fn cli_redeem_url(cfg: &CliAuthConfig) -> String {
    format!("{}/api/v1/cli/oauth/redeem", cfg.identity_origin)
}

/// Hosted `/signin/...` URL with `cli_redirect` set to **`http://127.0.0.1:<port>/oauth/callback`**.
pub fn cli_signin_url(
    cfg: &CliAuthConfig,
    session_id: impl AsRef<str>,
    loopback_port: u16,
) -> Result<String> {
    let redirect = Url::parse(&format!(
        "http://127.0.0.1:{loopback_port}{}",
        LOOPBACK_OAUTH_REDIRECT_PATH
    ))
    .context("build cli_redirect loopback URL")?;

    let mut url = Url::parse(&format!(
        "{}/signin/{}",
        cfg.identity_origin, cfg.environment_id
    ))
    .context("parse Identity sign-in URL")?;

    url.query_pairs_mut()
        .append_pair("cli_sess", session_id.as_ref())
        .append_pair("cli_redirect", redirect.as_str());

    Ok(url.to_string())
}

#[must_use]
/// Polling fallback when Identity does not see a **`cli_redirect`** marker (hosted success-page flow).
pub fn cli_poll_url(cfg: &CliAuthConfig, session_id: impl AsRef<str>) -> String {
    format!(
        "{}/api/v1/cli/oauth/poll?session={}",
        cfg.identity_origin,
        session_id.as_ref(),
    )
}

#[derive(Debug, Deserialize)]
struct PollResp {
    status: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Result of interpreting one poll HTTP response (unit-testable).
#[derive(Debug, PartialEq, Eq)]
pub enum PollStep {
    /// Sleep then poll again (pending or unknown terminal status fallback).
    Wait(Duration),
    /// Terminal success.
    Tokens {
        access_token: String,
        refresh_token: String,
    },
}

/// Decide what to do after a poll response (covers HTTP status + JSON body).
pub fn poll_step_for_response(status: u16, body: &str) -> Result<PollStep> {
    match status {
        410 => anyhow::bail!(
            "login session expired server-side (HTTP {status}); close the browser tab and run /login again"
        ),
        s if !(200..300).contains(&s) => {
            anyhow::bail!("poll HTTP {status}: {body}");
        }
        _ => {}
    }

    let pr: PollResp = serde_json::from_str(body)
        .with_context(|| format!("invalid poll JSON (HTTP {status}): {body}"))?;

    match pr.status.as_str() {
        "pending" => Ok(PollStep::Wait(Duration::from_millis(750))),
        "complete" => {
            let at = pr
                .access_token
                .filter(|s| !s.is_empty())
                .context("poll missing access_token")?;
            let rt = pr
                .refresh_token
                .filter(|s| !s.is_empty())
                .context("poll missing refresh_token")?;
            Ok(PollStep::Tokens {
                access_token: at,
                refresh_token: rt,
            })
        }
        "error" => anyhow::bail!("{}", pr.error.unwrap_or_else(|| "poll error".to_string())),
        _ => Ok(PollStep::Wait(Duration::from_millis(750))),
    }
}

/// Identity redeem endpoint returns the same JSON shape as poll (status + tokens).
#[inline]
pub fn redeem_step_for_response(status: u16, body: &str) -> Result<PollStep> {
    poll_step_for_response(status, body)
}

fn looks_like_grant(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_grant_from_request_target(uri: &str) -> Option<String> {
    if !uri.starts_with('/') {
        return None;
    }
    let u = Url::parse(&format!("http://127.0.0.1{uri}")).ok()?;
    if u.path() != LOOPBACK_OAUTH_REDIRECT_PATH {
        return None;
    }
    for (k, v) in u.query_pairs() {
        if k == "grant" && looks_like_grant(v.as_ref()) {
            return Some(v.into_owned());
        }
    }
    None
}

fn drain_http_headers(reader: &mut impl BufRead) -> Result<()> {
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).context("read HTTP header")?;
        if n == 0 {
            anyhow::bail!("unexpected EOF reading HTTP headers");
        }
        if line == "\r\n" || line == "\n" {
            return Ok(());
        }
    }
}

fn respond_simple(
    stream: &mut TcpStream,
    status_line: &str,
    content_type: &str,
    body: &[u8],
    prevent_browser_cache: bool,
) -> Result<()> {
    let cache_headers = if prevent_browser_cache {
        "Cache-Control: no-store, max-age=0\r\nPragma: no-cache\r\n"
    } else {
        ""
    };
    let hdr = format!(
        "HTTP/1.1 {status_line}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {len}\r\n\
         {cache_headers}\
         Connection: close\r\n\
         \r\n",
        len = body.len(),
    );
    stream
        .write_all(hdr.as_bytes())
        .context("write HTTP headers")?;
    stream.write_all(body).context("write HTTP body")?;
    stream.flush().context("flush HTTP reply")?;
    Ok(())
}

/// Separate file stays readable; inlined at compile time (`include_str!`).
const LOOPBACK_CALLBACK_SUCCESS_HTML: &str = include_str!("../assets/oauth_callback_success.html");

fn loopback_grant_from_stream(stream: &mut TcpStream) -> Result<Option<String>> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));

    let mut reader = BufReader::new(stream.try_clone().context("dup TCP stream for HTTP read")?);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .context("read HTTP request line")?;

    drain_http_headers(&mut reader)?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        respond_simple(
            stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"bad request",
            false,
        )?;
        return Ok(None);
    }

    if !parts[0].eq_ignore_ascii_case("GET") {
        respond_simple(
            stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"use GET",
            false,
        )?;
        return Ok(None);
    }

    let uri = parts[1];
    if let Some(grant) = parse_grant_from_request_target(uri) {
        respond_simple(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            LOOPBACK_CALLBACK_SUCCESS_HTML.as_bytes(),
            true,
        )?;
        Ok(Some(grant))
    } else {
        respond_simple(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"expected /oauth/callback?grant=",
            false,
        )?;
        Ok(None)
    }
}

fn accept_loopback_grant(listener: &TcpListener, deadline: Instant) -> Result<String> {
    listener.set_nonblocking(true).context("set_nonblocking")?;
    loop {
        if Instant::now() > deadline {
            anyhow::bail!(
                "timed out waiting for browser redirect to http://127.0.0.1 (check VPN / firewall / proxy)"
            );
        }
        match listener.accept() {
            Ok((mut stream, _)) => match loopback_grant_from_stream(&mut stream) {
                Ok(Some(grant)) => {
                    let _ = stream.shutdown(Shutdown::Both);
                    return Ok(grant);
                }
                Ok(None) => {
                    let _ = stream.shutdown(Shutdown::Both);
                }
                Err(e) => {
                    let _ = stream.shutdown(Shutdown::Both);
                    return Err(e);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

pub fn suspend_tui_for_shell_io() -> Result<()> {
    use crossterm::cursor::Show;
    use crossterm::event::DisableMouseCapture;
    use crossterm::execute;
    use crossterm::terminal::{disable_raw_mode, LeaveAlternateScreen};
    use std::io::{stdout, Write};
    stdout().flush()?;
    disable_raw_mode().context("disable_raw_mode")?;
    execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen, Show)
        .context("leave alt screen")?;
    Ok(())
}

pub fn resume_tui_io() -> Result<()> {
    use crossterm::cursor::Hide;
    use crossterm::event::EnableMouseCapture;
    use crossterm::execute;
    use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen};
    use std::io::{stderr, stdout, Write};
    // Match `ratatui::try_init()` order so raw mode + alternate screen match startup.
    enable_raw_mode().context("enable_raw_mode")?;
    execute!(stdout(), EnterAlternateScreen, Hide).context("enter alt screen")?;
    // Match `main.rs`: restore wheel/trackpad scrolling after OAuth.
    if let Err(e) = execute!(stdout(), EnableMouseCapture) {
        eprintln!("note: mouse/wheel scrolling unavailable ({e})");
    }
    stdout().flush().context("flush stdout after resume")?;
    stderr().flush().context("flush stderr after resume")?;
    Ok(())
}

/// Opens the hosted sign-in page, listens for `http://127.0.0.1:<port>/oauth/callback?grant=…`, then redeems tokens.
pub fn run_browser_login_blocking(cfg: &CliAuthConfig) -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").context("bind 127.0.0.1 listener")?;
    listener.set_nonblocking(true)?;
    let loopback_port = listener.local_addr()?.port();
    let sess = Uuid::new_v4().to_string();
    let signin_url =
        cli_signin_url(cfg, &sess, loopback_port).context("build Identity sign-in URL")?;

    eprintln!("Redirecting to your browser for authentication…");

    open::that(&signin_url).with_context(|| format!("open {signin_url}"))?;

    let deadline_wait = Instant::now() + Duration::from_secs(8 * 60);
    let grant = accept_loopback_grant(&listener, deadline_wait)?;

    let redeem_url = cli_redeem_url(cfg);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .context("HTTP client")?;

    let redeem_body =
        serde_json::to_vec(&serde_json::json!({ "grant": grant })).context("serialize redeem")?;

    let redeem_deadline = Instant::now() + Duration::from_secs(2 * 60);
    let mut last_err: Option<anyhow::Error> = None;

    while Instant::now() < redeem_deadline {
        match client
            .post(&redeem_url)
            .header("Content-Type", "application/json")
            .body(redeem_body.clone())
            .send()
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body_text = resp.text().unwrap_or_default();
                match redeem_step_for_response(status, &body_text) {
                    Ok(PollStep::Tokens {
                        access_token,
                        refresh_token,
                    }) => {
                        let prev = crate::session_store::load_session().ok().flatten();
                        save_session(&StoredSession {
                            access_token,
                            refresh_token,
                            current_organization_id: prev
                                .as_ref()
                                .and_then(|s| s.current_organization_id.clone()),
                            current_tenant_id: prev
                                .as_ref()
                                .and_then(|s| s.current_tenant_id.clone()),
                            current_application_id: None,
                            current_environment_id: None,
                        })?;
                        return Ok(());
                    }
                    Ok(PollStep::Wait(_)) => {}
                    Err(e) => last_err = Some(e),
                }
            }
            Err(err) => {
                last_err = Some(err.into());
            }
        }
        thread::sleep(Duration::from_millis(500));
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("redeem request never succeeded")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grant_query_from_redirect_target() {
        let hex = format!("{:064x}", 1u128);
        let uri = format!("/oauth/callback?grant={hex}");
        assert_eq!(parse_grant_from_request_target(&uri), Some(hex.clone()));
        assert_eq!(parse_grant_from_request_target("/nope"), None);
        assert_eq!(
            parse_grant_from_request_target("/oauth/callback?grant=nothex"),
            None
        );
    }

    #[test]
    fn cli_urls_strip_trailing_slash_on_origin() {
        let cfg = CliAuthConfig::new("https://identity.authdog.com/", "env-id");
        let sid = "00000000-0000-4000-b000-000000000042";
        let signin =
            Url::parse(&cli_signin_url(&cfg, sid, 42_424).unwrap()).expect("identity sign-in url");
        assert_eq!(signin.host_str(), Some("identity.authdog.com"),);
        let mut qs = std::collections::HashMap::<String, String>::new();
        for (k, v) in signin.query_pairs() {
            qs.insert(k.into_owned(), v.into_owned());
        }
        assert_eq!(qs.get("cli_sess").map(String::as_str), Some(sid));
        assert_eq!(
            qs.get("cli_redirect").map(String::as_str),
            Some("http://127.0.0.1:42424/oauth/callback"),
        );
        assert_eq!(
            cli_poll_url(&cfg, sid),
            "https://identity.authdog.com/api/v1/cli/oauth/poll?session=00000000-0000-4000-b000-000000000042"
        );
    }

    #[test]
    fn poll_step_pending_maps_to_wait() {
        let step = poll_step_for_response(200, r#"{"status":"pending"}"#).unwrap();
        assert_eq!(step, PollStep::Wait(Duration::from_millis(750)));
    }

    #[test]
    fn poll_step_complete_requires_tokens() {
        let step = poll_step_for_response(
            200,
            r#"{"status":"complete","access_token":"aa","refresh_token":"bb"}"#,
        )
        .unwrap();
        assert_eq!(
            step,
            PollStep::Tokens {
                access_token: "aa".into(),
                refresh_token: "bb".into(),
            }
        );

        poll_step_for_response(
            200,
            r#"{"status":"complete","access_token":"","refresh_token":"b"}"#,
        )
        .expect_err("empty access_token");
    }

    #[test]
    fn poll_step_expired_returns_err() {
        let err = poll_step_for_response(410, r#"{}"#).expect_err("410");
        let _ = err;
        assert!(format!("{err:#}").contains("expired"), "{}", err);
    }

    #[test]
    fn poll_step_error_propagates_message() {
        let err =
            poll_step_for_response(200, r#"{"status":"error","error":"oops"}"#).expect_err("err");
        assert!(format!("{err:#}").contains("oops"));
    }

    #[test]
    fn poll_step_unknown_terminal_status_fallback_wait() {
        let step = poll_step_for_response(
            200,
            r#"{"status":"unexpected","access_token":"","refresh_token":""}"#,
        )
        .unwrap();
        assert_eq!(step, PollStep::Wait(Duration::from_millis(750)));
    }
}
