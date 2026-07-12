//! Authdog CLI — conventional commands plus an optional fullscreen interface.

mod dashboard;

use anyhow::Result;
use authdog_cli::cli::{self, Cli, RunAction};
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

fn main() -> Result<()> {
    match cli::run(Cli::parse())? {
        RunAction::Exit => Ok(()),
        RunAction::Ui => run_ui(),
    }
}

fn run_ui() -> Result<()> {
    let mut terminal = ratatui::init();
    if let Err(e) = execute!(std::io::stdout(), EnableMouseCapture) {
        eprintln!("note: mouse/wheel scrolling unavailable ({e})");
    }

    let run_res = dashboard::App::new().run(&mut terminal);

    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    run_res
}
