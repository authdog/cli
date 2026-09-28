//! Conventional process-level CLI.

use crate::cli_login::{run_browser_login_blocking, CliAuthConfig};
use crate::session_store::{clear_session, load_session, StoredSession};
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::io::Write;

#[derive(Debug, Parser)]
#[command(
    name = "authdog",
    version,
    about = "Authdog CLI",
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
    /// Show local login status.
    Status,
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

pub fn run(cli: Cli) -> Result<()> {
    let format = cli.format();
    match cli.command {
        Command::Login => {
            run_browser_login_blocking(&CliAuthConfig::from_env())?;
            emit(
                format,
                "Logged in to Authdog.",
                json!({ "logged_in": true }),
            )?;
        }
        Command::Logout => {
            clear_session()?;
            emit(
                format,
                "Logged out. Local credentials removed.",
                json!({ "logged_in": false }),
            )?;
        }
        Command::Status => emit_session(format, load_session()?)?,
    }
    Ok(())
}

fn emit_session(format: OutputFormat, session: Option<StoredSession>) -> Result<()> {
    let path = crate::session_store::credentials_path()?
        .display()
        .to_string();
    let logged_in = session.is_some();
    let value = json!({
        "logged_in": logged_in,
        "credentials_path": path,
    });
    let text = format!(
        "Logged in: {}\nCredentials: {path}",
        if logged_in { "yes" } else { "no" }
    );
    emit(format, &text, value)
}

fn emit(format: OutputFormat, text: &str, value: Value) -> Result<()> {
    match format {
        OutputFormat::Text => {
            if !text.is_empty() {
                println!("{text}");
            }
        }
        OutputFormat::Json => {
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &value)?;
            writeln!(stdout)?;
        }
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
    fn parses_login_command() {
        let cli = Cli::try_parse_from(["authdog", "login"]).unwrap();
        assert!(matches!(cli.command, Command::Login));
        assert_eq!(cli.format(), OutputFormat::Text);
    }

    #[test]
    fn parses_status_json_flag() {
        let cli = Cli::try_parse_from(["authdog", "status", "--json"]).unwrap();
        assert!(matches!(cli.command, Command::Status));
        assert_eq!(cli.format(), OutputFormat::Json);
    }

    #[test]
    fn no_args_requests_help() {
        assert!(Cli::try_parse_from(["authdog"]).is_err());
    }
}
