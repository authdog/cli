//! Authdog CLI — full-screen Ratatui (Crossterm) interface.

mod app;
mod browse;
mod commands;
mod tui_output;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    if let Err(e) = execute!(std::io::stdout(), EnableMouseCapture) {
        eprintln!("note: mouse/wheel scrolling unavailable ({e})");
    }

    let run_res = app::App::default().run(&mut terminal);

    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    run_res
}
