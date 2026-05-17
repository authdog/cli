//! Slash-command dispatch into [`crate::app::App`] session output.

use crate::app::{App, ListingPicker, WhoamiJsonTab, WhoamiOutputPane};
use crate::commands::registry::CMDS;

use authdog_cli::cli_login;
use authdog_cli::organizations;
use authdog_cli::projects;
use authdog_cli::session_store;
use authdog_cli::tenants;
use authdog_cli::whoami;
use std::cmp;
use uuid::Uuid;

fn tenants_organization_scope(s: &session_store::StoredSession) -> Option<&str> {
    s.current_organization_id.as_deref().and_then(|id| {
        let t = id.trim();
        (!t.is_empty()).then_some(t)
    })
}

#[derive(Clone, PartialEq, Eq)]
pub enum SubmitEffect {
    None,
    BrowserLogin,
    /// Start interactive org → tenant → projects flow (handled in `app`).
    Browse {
        access_token: String,
    },
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

    if !matches!(
        first.as_str(),
        "tenants" | "organizations" | "orgs" | "browse" | "navigator" | "projects"
    ) {
        app.clear_listing_picker();
    }

    if first.as_str() != "whoami" && first.as_str() != "me" {
        app.whoami_output = None;
    }

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
        "logout" => {
            match session_store::clear_session() {
                Ok(()) => {
                    app.whoami_output = None;
                    app.status =
                    Some("Signed out locally (credentials file removed).\nRun /login to sign in again.".into());
                    app.status_err = false;
                    SubmitEffect::None
                }
                Err(err) => {
                    app.whoami_output = None;
                    app.status = Some(format!("{err:#}"));
                    app.status_err = true;
                    SubmitEffect::None
                }
            }
        }
        "whoami" | "me" => match session_store::load_session() {
            Ok(Some(s)) => {
                let cred_note = session_store::credentials_path()
                    .ok()
                    .map(|path| format!("credentials file: {}", path.display()));
                match whoami::fetch_identity_userinfo(&s.access_token) {
                    Ok(value) => {
                        let (pretty, raw) = whoami::format_identity_json_pair(&value);
                        app.status = None;
                        app.status_err = false;
                        app.whoami_output = Some(WhoamiOutputPane {
                            endpoint_note: whoami::userinfo_endpoint_display(),
                            pretty_json: pretty,
                            raw_json: raw,
                            credentials_note: cred_note,
                            tab: WhoamiJsonTab::Pretty,
                        });
                    }
                    Err(err) => {
                        app.whoami_output = None;
                        let userinfo_note = whoami::userinfo_endpoint_display();
                        let mut msg =
                            format!("── Identity ({userinfo_note}) ──\n  (could not load) {err:#}");
                        if let Some(note) = cred_note {
                            let t = note.trim();
                            if !t.is_empty() {
                                msg.push_str("\n\n");
                                msg.push_str(t);
                            }
                        }
                        app.status = Some(msg);
                        app.status_err = false;
                    }
                }
                SubmitEffect::None
            }
            Ok(None) => {
                app.whoami_output = None;
                app.status = Some(
                    "Not logged in (/whoami).\nTry /login, or use /status to confirm files.".into(),
                );
                app.status_err = false;
                SubmitEffect::None
            }
            Err(err) => {
                app.whoami_output = None;
                app.status = Some(format!("{err:#}"));
                app.status_err = true;
                SubmitEffect::None
            }
        },
        "tenants" => match session_store::load_session() {
            Ok(Some(s)) => {
                app.browse = None;
                let cred_note = session_store::credentials_path()
                    .ok()
                    .map(|path| format!("credentials file: {}", path.display()));
                let base = whoami::api_origin().trim_end_matches('/').to_string();
                let scoped_org = tenants_organization_scope(&s);
                let endpoint = tenants::tenants_list_url_for_display(&base, scoped_org);
                match tenants::fetch_tenants(&s.access_token, scoped_org) {
                    Ok(json) => {
                        let rows = tenants::tenant_rows_from_body(&json);
                        app.listing_picker = Some(ListingPicker::Tenants {
                            rows,
                            endpoint,
                            credentials_note: cred_note,
                        });
                        app.listing_list_state.select(Some(0));
                        app.status = None;
                        app.status_err = false;
                    }
                    Err(err) => {
                        app.status = Some(format!(
                            "── Tenants ({endpoint}) ──\n  (could not load) {err:#}"
                        ));
                        app.status_err = true;
                    }
                }
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
                let scoped_org = tenants_organization_scope(&s);
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
                                match tenants::fetch_tenants(&s.access_token, scoped_org) {
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
                                    match session_store::set_current_tenant_id(Some(
                                        tid.to_string(),
                                    )) {
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
                    "Not logged in (/tenant).\nTry /login, or use /status to confirm files.".into(),
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
                app.browse = None;
                match &s.current_tenant_id {
                    None => {
                        app.status = Some(
                            "No current tenant.\nUse `/tenant <uuid>` first (`/tenants` lists ids)."
                                .into(),
                        );
                        app.status_err = false;
                    }
                    Some(tid) => {
                        let cred_note = session_store::credentials_path()
                            .ok()
                            .map(|path| format!("credentials file: {}", path.display()));
                        let endpoint = projects::projects_endpoint_display(tid.as_str());
                        match projects::fetch_projects(&s.access_token, tid.as_str()) {
                            Ok(json) => {
                                let rows = projects::project_rows_from_body(&json);
                                app.listing_picker = Some(ListingPicker::Projects {
                                    rows,
                                    endpoint,
                                    credentials_note: cred_note,
                                });
                                app.listing_list_state.select(Some(0));
                                app.status = None;
                                app.status_err = false;
                            }
                            Err(err) => {
                                app.status = Some(format!(
                                    "── Projects ({endpoint}) ──\n  (could not load) {err:#}"
                                ));
                                app.status_err = true;
                            }
                        }
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
        "browse" | "navigator" => match session_store::load_session() {
            Ok(Some(s)) => {
                app.clear_listing_picker();
                app.status_clear_at = None;
                app.status = None;
                app.status_err = false;
                SubmitEffect::Browse {
                    access_token: s.access_token,
                }
            }
            Ok(None) => {
                app.status = Some(
                    "Not logged in (/browse).\nTry /login, or use /status to confirm files.".into(),
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
                app.browse = None;
                let cred_note = session_store::credentials_path()
                    .ok()
                    .map(|path| format!("credentials file: {}", path.display()));
                let base = whoami::api_origin().trim_end_matches('/').to_string();
                let endpoint = format!("{base}{}", organizations::ORGANIZATIONS_PATH);
                match organizations::fetch_organizations(&s.access_token) {
                    Ok(json) => {
                        let rows = organizations::organization_rows_from_body(&json);
                        app.listing_picker = Some(ListingPicker::Organizations {
                            rows,
                            endpoint,
                            credentials_note: cred_note,
                        });
                        app.listing_list_state.select(Some(0));
                        app.status = None;
                        app.status_err = false;
                    }
                    Err(err) => {
                        app.status = Some(format!(
                            "── Organizations ({endpoint}) ──\n  (could not load) {err:#}"
                        ));
                        app.status_err = true;
                    }
                }
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
                let org_line = s
                    .current_organization_id
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .map(|id| format!("\nCurrent organization: {id}"))
                    .unwrap_or_else(|| "\nCurrent organization: (none)".into());
                let tenant_line = s
                    .current_tenant_id
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .map(|id| format!("\nCurrent tenant: {id}"))
                    .unwrap_or_else(|| "\nCurrent tenant: (none)".into());
                let app_line = s
                    .current_application_id
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .map(|id| format!("\nCurrent project (application): {id}"))
                    .unwrap_or_else(|| "\nCurrent project (application): (none)".into());
                let env_line = s
                    .current_environment_id
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .map(|id| format!("\nCurrent environment: {id}"))
                    .unwrap_or_else(|| "\nCurrent environment: (none)".into());
                app.status = Some(format!(
                    "Session file: {path_show}\nAccess token preview: {preview}… ({} chars)\nRefresh token: {} chars{org_line}{tenant_line}{app_line}{env_line}",
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
