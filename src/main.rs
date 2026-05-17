//! Authdog CLI — full-screen Ratatui (Crossterm) interface.

mod app;
mod commands;
mod tui_output;

use anyhow::Result;

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    app::App::default().run(&mut terminal)?;
    ratatui::restore();
    Ok(())
}
