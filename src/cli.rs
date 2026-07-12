//! Conventional process-level CLI. Fullscreen interface remains available through `authdog ui`.

use crate::actions::{self, ContextResource};
use crate::cli_login::{run_browser_login_blocking, CliAuthConfig};
use crate::session_store::StoredSession;
use crate::whoami::format_identity_pretty_display;
use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};

#[derive(Debug, Parser)]
#[command(
    name = "authdog",
    version,
    about = "Inspect Authdog identity and resource context",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Output format for command data.
    #[arg(short = 'o', long, value_enum, default_value_t, global = true)]
    pub output: OutputFormat,

    /// Shorthand for `--output json`.
    #[arg(long, global = true, conflicts_with = "output")]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Sign in through the hosted Authdog browser flow.
    Login,
    /// Delete locally stored credentials.
    Logout,
    /// Show local login and context status.
    Status,
    /// Fetch the current server-checked identity.
    #[command(alias = "me")]
    Whoami,
    /// List organizations visible to the current user.
    #[command(alias = "orgs")]
    Organizations(ListOnly),
    /// List tenants, optionally scoped to an organization.
    Tenants(TenantList),
    /// List projects for a tenant.
    Projects(ProjectList),
    /// List environments for a tenant and project.
    Environments(EnvironmentList),
    /// Show or update locally selected resource context.
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    /// Launch the existing fullscreen interactive interface.
    Ui,
}

#[derive(Debug, Args)]
pub struct ListOnly {
    #[command(subcommand)]
    command: ListCommand,
}

#[derive(Debug, Subcommand)]
enum ListCommand {
    List,
}

#[derive(Debug, Args)]
pub struct TenantList {
    #[command(subcommand)]
    command: TenantCommand,
}

#[derive(Debug, Subcommand)]
enum TenantCommand {
    List {
        #[arg(long)]
        organization: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct ProjectList {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    List {
        #[arg(long)]
        tenant: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct EnvironmentList {
    #[command(subcommand)]
    command: EnvironmentCommand,
}

#[derive(Debug, Subcommand)]
enum EnvironmentCommand {
    List {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    Show,
    Set {
        #[arg(value_enum)]
        resource: ContextResource,
        id: String,
    },
    Clear {
        #[arg(value_enum)]
        resource: Option<ContextResource>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunAction {
    Exit,
    Ui,
}

impl Cli {
    fn format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            self.output
        }
    }
}

pub fn run(cli: Cli) -> Result<RunAction> {
    let format = cli.format();
    match cli.command {
        Command::Ui => return Ok(RunAction::Ui),
        Command::Login => {
            run_browser_login_blocking(&CliAuthConfig::from_env())?;
            emit(
                format,
                "Logged in to Authdog.",
                json!({ "logged_in": true }),
            )?;
        }
        Command::Logout => {
            actions::logout()?;
            emit(
                format,
                "Logged out. Local credentials removed.",
                json!({ "logged_in": false }),
            )?;
        }
        Command::Status => emit_session(format, actions::status()?.session, true)?,
        Command::Whoami => {
            let value = actions::identity()?;
            emit(format, &format_identity_pretty_display(&value), value)?;
        }
        Command::Organizations(args) => {
            let ListCommand::List = args.command;
            let rows = actions::list_organizations()?;
            let normalized = Value::Array(
                rows.iter()
                    .map(|row| json!({ "id": row.id, "name": row.name }))
                    .collect(),
            );
            emit(
                format,
                &format_named_rows(
                    "ID",
                    "NAME",
                    rows.iter().map(|r| (&r.id, r.name.as_deref())),
                ),
                normalized,
            )?;
        }
        Command::Tenants(args) => {
            let TenantCommand::List { organization } = args.command;
            let rows = actions::list_tenants(organization.as_deref())?;
            let normalized = Value::Array(
                rows.iter()
                    .map(|row| {
                        json!({
                            "id": row.id,
                            "name": row.name,
                            "organization_id": row.organization_id,
                            "organization_ids": row.organization_ids,
                        })
                    })
                    .collect(),
            );
            emit(
                format,
                &format_named_rows(
                    "ID",
                    "NAME",
                    rows.iter().map(|r| (&r.id, r.name.as_deref())),
                ),
                normalized,
            )?;
        }
        Command::Projects(args) => {
            let ProjectCommand::List { tenant } = args.command;
            let rows = actions::list_projects(tenant.as_deref())?;
            let normalized = Value::Array(
                rows.iter()
                    .map(|row| json!({ "id": row.id, "name": row.name, "type": row.project_type }))
                    .collect(),
            );
            emit(
                format,
                &format_named_rows(
                    "ID",
                    "NAME",
                    rows.iter().map(|r| (&r.id, r.name.as_deref())),
                ),
                normalized,
            )?;
        }
        Command::Environments(args) => {
            let EnvironmentCommand::List { tenant, project } = args.command;
            let rows = actions::list_environments(tenant.as_deref(), project.as_deref())?;
            let normalized = Value::Array(
                rows.iter()
                    .map(|row| json!({ "id": row.id, "name": row.name }))
                    .collect(),
            );
            emit(
                format,
                &format_named_rows(
                    "ID",
                    "NAME",
                    rows.iter().map(|r| (&r.id, r.name.as_deref())),
                ),
                normalized,
            )?;
        }
        Command::Context { command } => match command {
            ContextCommand::Show => emit_session(format, actions::status()?.session, false)?,
            ContextCommand::Set { resource, id } => {
                let id = actions::set_context(resource, id)?;
                emit(
                    format,
                    &format!("Current {} set to {id}.", resource.name()),
                    json!({ "resource": resource.name(), "id": id }),
                )?;
            }
            ContextCommand::Clear { resource } => {
                if let Some(resource) = resource {
                    actions::clear_context(Some(resource))?;
                    emit(
                        format,
                        &format!("Current {} cleared.", resource.name()),
                        json!({ "resource": resource.name(), "id": null }),
                    )?;
                } else {
                    actions::clear_context(None)?;
                    emit(
                        format,
                        "All resource context cleared.",
                        json!({ "context": context_json(None) }),
                    )?;
                }
            }
        },
    }
    Ok(RunAction::Exit)
}

fn context_json(session: Option<&StoredSession>) -> Value {
    json!({
        "organization_id": session.and_then(|s| s.current_organization_id.as_deref()),
        "tenant_id": session.and_then(|s| s.current_tenant_id.as_deref()),
        "project_id": session.and_then(|s| s.current_application_id.as_deref()),
        "environment_id": session.and_then(|s| s.current_environment_id.as_deref()),
    })
}

fn emit_session(
    format: OutputFormat,
    session: Option<StoredSession>,
    include_login: bool,
) -> Result<()> {
    let path = actions::status()?.credentials_path.display().to_string();
    let logged_in = session.is_some();
    let value = if include_login {
        json!({
            "logged_in": logged_in,
            "credentials_path": path,
            "context": context_json(session.as_ref()),
        })
    } else {
        context_json(session.as_ref())
    };
    let mut text = if include_login {
        format!(
            "Logged in: {}\nCredentials: {path}\n",
            if logged_in { "yes" } else { "no" }
        )
    } else {
        String::new()
    };
    let context = session.as_ref();
    text.push_str(&format!(
        "Organization: {}\nTenant: {}\nProject: {}\nEnvironment: {}",
        display_scope(context.and_then(|s| s.current_organization_id.as_deref())),
        display_scope(context.and_then(|s| s.current_tenant_id.as_deref())),
        display_scope(context.and_then(|s| s.current_application_id.as_deref())),
        display_scope(context.and_then(|s| s.current_environment_id.as_deref())),
    ));
    emit(format, &text, value)
}

fn display_scope(value: Option<&str>) -> &str {
    value.filter(|v| !v.is_empty()).unwrap_or("(none)")
}

fn format_named_rows<'a>(
    id_heading: &str,
    name_heading: &str,
    rows: impl Iterator<Item = (&'a String, Option<&'a str>)>,
) -> String {
    let rows: Vec<_> = rows.collect();
    if rows.is_empty() {
        return "No resources found.".to_string();
    }
    let id_width = rows
        .iter()
        .map(|(id, _)| id.len())
        .max()
        .unwrap_or(0)
        .max(id_heading.len());
    let mut lines = vec![format!("{id_heading:<id_width$}  {name_heading}")];
    lines.push(format!(
        "{}  {}",
        "-".repeat(id_width),
        "-".repeat(name_heading.len())
    ));
    lines.extend(rows.into_iter().map(|(id, name)| {
        format!(
            "{id:<id_width$}  {}",
            name.filter(|v| !v.is_empty()).unwrap_or("-")
        )
    }));
    lines.join("\n")
}

fn emit(format: OutputFormat, text: &str, value: Value) -> Result<()> {
    match format {
        OutputFormat::Text => println!("{text}"),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_tree_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_ui_and_global_json_after_subcommand() {
        let cli = Cli::try_parse_from(["authdog", "ui", "--json"]).unwrap();
        assert!(matches!(cli.command, Command::Ui));
        assert_eq!(cli.format(), OutputFormat::Json);
    }

    #[test]
    fn parses_environment_scope_flags() {
        let cli = Cli::try_parse_from([
            "authdog",
            "environments",
            "list",
            "--tenant",
            "t1",
            "--project",
            "p1",
        ])
        .unwrap();
        assert!(matches!(cli.command, Command::Environments(_)));
    }

    #[test]
    fn no_args_requests_help() {
        assert!(Cli::try_parse_from(["authdog"]).is_err());
    }

    #[test]
    fn context_json_never_contains_tokens() {
        let session = StoredSession {
            access_token: "secret-access".into(),
            refresh_token: "secret-refresh".into(),
            current_organization_id: Some("o1".into()),
            current_tenant_id: None,
            current_application_id: None,
            current_environment_id: None,
        };
        let rendered = context_json(Some(&session)).to_string();
        assert!(!rendered.contains("secret"));
        assert!(rendered.contains("o1"));
    }
}
