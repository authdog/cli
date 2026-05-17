//! Browser OAuth hosted on Identity (`/signin/...?cli_sess=`) + poll bridge (`/api/v1/cli/oauth/poll`).
//! IdP callbacks stay on `/api/v1/callback/:connection`; tokens are mirrored to CLI via D1 briefly.

use crate::session_store::{save_session, StoredSession};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

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

#[must_use]
pub fn cli_signin_url(cfg: &CliAuthConfig, session_id: impl AsRef<str>) -> String {
    format!(
        "{}/signin/{}?cli_sess={}",
        cfg.identity_origin,
        cfg.environment_id,
        session_id.as_ref(),
    )
}

#[must_use]
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

pub fn suspend_tui_for_shell_io() -> Result<()> {
    use crossterm::cursor::Show;
    use crossterm::execute;
    use crossterm::terminal::{disable_raw_mode, LeaveAlternateScreen};
    use std::io::{stdout, Write};
    stdout().flush()?;
    disable_raw_mode().context("disable_raw_mode")?;
    execute!(stdout(), LeaveAlternateScreen, Show).context("leave alt screen")?;
    Ok(())
}

pub fn resume_tui_io() -> Result<()> {
    use crossterm::cursor::Hide;
    use crossterm::execute;
    use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen};
    use std::io::{stdout, Write};
    execute!(stdout(), EnterAlternateScreen, Hide).context("enter alt screen")?;
    enable_raw_mode().context("enable_raw_mode")?;
    stdout().flush()?;
    Ok(())
}

/// Blocks in the shell (TUI paused): open browser, poll until OAuth completes or timeout.
pub fn run_browser_login_blocking(cfg: &CliAuthConfig) -> Result<()> {
    let sess = Uuid::new_v4().to_string();
    let signin_url = cli_signin_url(cfg, &sess);
    open::that(&signin_url).with_context(|| format!("open {signin_url}"))?;

    eprintln!("Browser opened.\nWaiting for OAuth to finish (polling Identity) …");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("HTTP client")?;
    let poll_url = cli_poll_url(cfg, &sess);
    let deadline = Instant::now() + Duration::from_secs(8 * 60);

    loop {
        if Instant::now() > deadline {
            anyhow::bail!("login timed out; try /login again");
        }
        match client.get(&poll_url).send() {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = r.text().unwrap_or_default();
                match poll_step_for_response(status, &body) {
                    Ok(PollStep::Wait(d)) => thread::sleep(d),
                    Ok(PollStep::Tokens {
                        access_token,
                        refresh_token,
                    }) => {
                        save_session(&StoredSession {
                            access_token,
                            refresh_token,
                        })?;
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(err) => {
                eprintln!("poll request failed ({err}); retrying …");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_urls_strip_trailing_slash_on_origin() {
        let cfg = CliAuthConfig::new("https://identity.authdog.com/", "env-id");
        let sid = "00000000-0000-4000-b000-000000000042";
        assert_eq!(
            cli_signin_url(&cfg, sid),
            "https://identity.authdog.com/signin/env-id?cli_sess=00000000-0000-4000-b000-000000000042"
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
