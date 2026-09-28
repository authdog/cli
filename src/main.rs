//! Authdog CLI.

use anyhow::Result;
use authdog_cli::cli::{self, Cli};
use clap::Parser;

fn main() -> Result<()> {
    cli::run(Cli::parse())
}
