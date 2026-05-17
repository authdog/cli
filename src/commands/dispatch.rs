//! Slash-command dispatch into [`crate::app::App`] session output.

use crate::app::App;
use crate::commands::registry::CMDS;

use authdog_cli::cli_login;
use authdog_cli::organizations;
use authdog_cli::projects;
use authdog_cli::session_store;
use authdog_cli::tenants;
use authdog_cli::whoami;
use std::cmp;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SubmitEffect {
    None,
    BrowserLogin,
}

pub fn apply_submit(app: &mut App, line: &str) -> SubmitEffect {
    let line = line.trim();
    if line.is_empty() {
        app.status_clear_at = None;
        app.status = None;
        app.status_err = false;
        return SubmitEffect::None;
    }

    app.status_clear_at = None;

    let first = line
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches('/')
        .to_ascii_lowercase();

    match first.as_str() {
        "quit" | "q" => {
            app.quit = true;
            SubmitEffect::None
        }
        "" | "help" | "h" | "?" => {
            let buf: String = CMDS
                .iter()
                .map(|c| format!("/{:<14} — {}", c.name, c.desc))
                .collect::<Vec<_>>()
                .join("\n");
            app.status = Some(buf);
            app.status_err = false;
            SubmitEffect::None
        }
        "login" => {
            let cfg = cli_login::CliAuthConfig::from_env();
            app.status = Some(format!(
                "Opening browser ({}/signin/{} …).\nAUTHDOG_IDENTITY_ORIGIN overrides the host (default: {}).",
                cfg.identity_origin, cfg.environment_id, cli_login::DEFAULT_IDENTITY_ORIGIN,
            ));
            app.status_err = false;
            SubmitEffect::BrowserLogin
        }
        "logout" => match session_store::clear_session() {
            Ok(()) => {
                app.status =
                    Some("Signed out locally (credentials file removed).\nRun /login to sign in again.".into());
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        "whoami" | "me" => match session_store::load_session() {
            Ok(Some(s)) => {
                app.status = Some(whoami::compose_whoami_report(
                    &s.access_token,
                    session_store::credentials_path()
                        .ok()
                        .map(|path| format!("credentials file: {}", path.display())),
                ));
                app.status_err = false;
                SubmitEffect::None
            }
            Ok(None) => {
                app.status = Some(
                    "Not logged in (/whoami).\nTry /login, or use /status to confirm files."
                        .into(),
                );
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        "tenants" => match session_store::load_session() {
            Ok(Some(s)) => {
                app.status = Some(tenants::compose_tenants_report(
                    &s.access_token,
                    session_store::credentials_path()
                        .ok()
                        .map(|path| format!("credentials file: {}", path.display())),
                ));
                app.status_err = false;
                SubmitEffect::None
            }
            Ok(None) => {
                app.status = Some(
                    "Not logged in (/tenants).\nTry /login, or use /status to confirm files."
                        .into(),
                );
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        "tenant" => match session_store::load_session() {
            Ok(Some(s)) => {
                let tokens: Vec<&str> = line.split_whitespace().collect();
                match tokens.len() {
                    1 => {
                        let msg = match &s.current_tenant_id {
                            Some(id) => format!("Current tenant:\n{id}"),
                            None => "No current tenant set.\nUse `/tenant <uuid>` (see `/tenants`).\n`/tenant clear` unsets.".into(),
                        };
                        app.status = Some(msg);
                        app.status_err = false;
                    }
                    2 => {
                        let arg = tokens[1];
                        if arg.eq_ignore_ascii_case("clear") || arg.eq_ignore_ascii_case("unset") {
                            match session_store::set_current_tenant_id(None) {
                                Ok(()) => {
                                    app.status = Some("Current tenant cleared.".into());
                                    app.status_err = false;
                                }
                                Err(err) => {
                                    app.status = Some(format!("{err:#}"));
                                    app.status_err = true;
                                }
                            }
                        } else {
                            let tid = arg.trim();
                            if Uuid::parse_str(tid).is_err() {
                                app.status = Some(format!(
                                    "Invalid tenant UUID `{tid}`.\nExpected a value like 00000000-0000-4000-8000-000000000001."
                                ));
                                app.status_err = true;
                            } else {
                                let mut allowed = true;
                                let mut warning = String::new();
                                match tenants::fetch_tenants(&s.access_token) {
                                    Ok(v) => {
                                        let ids = tenants::tenant_ids_from_body(&v);
                                        if !ids.iter().any(|x| x.as_str() == tid) {
                                            allowed = false;
                                        }
                                    }
                                    Err(e) => {
                                        warning = format!(
                                            "\n(warning: could not verify against /tenants: {e:#})"
                                        );
                                    }
                                }
                                if !allowed {
                                    app.status = Some(format!(
                                        "Tenant id not found in /tenants listing:\n{tid}\nRun `/tenants` for ids."
                                    ));
                                    app.status_err = true;
                                } else {
                                    match session_store::set_current_tenant_id(Some(tid.to_string()))
                                    {
                                        Ok(()) => {
                                            app.status = Some(format!(
                                                "Current tenant set to:\n{tid}{warning}"
                                            ));
                                            app.status_err = false;
                                        }
                                        Err(err) => {
                                            app.status = Some(format!("{err:#}"));
                                            app.status_err = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        app.status = Some(
                            "Usage:\n  /tenant — show current tenant\n  /tenant <uuid> — set tenant\n  /tenant clear — unset tenant".into(),
                        );
                        app.status_err = true;
                    }
                }
                SubmitEffect::None
            }
            Ok(None) => {
                app.status = Some(
                    "Not logged in (/tenant).\nTry /login, or use /status to confirm files."
                        .into(),
                );
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        "projects" => match session_store::load_session() {
            Ok(Some(s)) => {
                match &s.current_tenant_id {
                    None => {
                        app.status = Some(
                            "No current tenant.\nUse `/tenant <uuid>` first (`/tenants` lists ids)."
                                .into(),
                        );
                        app.status_err = false;
                    }
                    Some(tid) => {
                        app.status = Some(projects::compose_projects_report(
                            &s.access_token,
                            tid.as_str(),
                            session_store::credentials_path()
                                .ok()
                                .map(|path| format!("credentials file: {}", path.display())),
                        ));
                        app.status_err = false;
                    }
                }
                SubmitEffect::None
            }
            Ok(None) => {
                app.status = Some(
                    "Not logged in (/projects).\nTry /login, or use /status to confirm files."
                        .into(),
                );
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        "organizations" | "orgs" => match session_store::load_session() {
            Ok(Some(s)) => {
                app.status = Some(organizations::compose_organizations_report(
                    &s.access_token,
                    session_store::credentials_path()
                        .ok()
                        .map(|path| format!("credentials file: {}", path.display())),
                ));
                app.status_err = false;
                SubmitEffect::None
            }
            Ok(None) => {
                app.status = Some(
                    "Not logged in (/organizations).\nTry /login, or use /status to confirm files."
                        .into(),
                );
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        "status" => match session_store::load_session() {
            Ok(Some(s)) => {
                let p = cmp::min(28, s.access_token.len());
                let preview = if p == 0 {
                    String::new()
                } else {
                    s.access_token[..p].to_string()
                };
                let path_show = session_store::credentials_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "(unknown)".into());
                let tenant_line = s
                    .current_tenant_id
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .map(|id| format!("\nCurrent tenant: {id}"))
                    .unwrap_or_else(|| "\nCurrent tenant: (none)".into());
                app.status = Some(format!(
                    "Session file: {path_show}\nAccess token preview: {preview}… ({} chars)\nRefresh token: {} chars{tenant_line}",
                    s.access_token.len(),
                    s.refresh_token.len(),
                ));
                app.status_err = false;
                SubmitEffect::None
            }
            Ok(None) => {
                app.status = Some("Not logged in (no credentials.json). Try /login.".into());
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        _other => {
            if line.starts_with('/') {
                app.status = Some(format!(
                    "unknown command: {}",
                    line.split_whitespace().next().unwrap_or("")
                ));
                app.status_err = true;
            } else {
                app.status = Some(line.to_string());
                app.status_err = false;
            }
            SubmitEffect::None
        }
    }
}
