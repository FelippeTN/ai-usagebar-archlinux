//! `claude-usagebar-tui`: a one-screen ratatui dashboard over the same fetch
//! and cache as the widget.

mod state;
mod view;

use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::app::Settings;
use crate::error::{AppError, Result};

pub use state::App;

pub fn run(settings: Settings) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Other(format!("tokio: {e}")))?;
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &rt, &mut App::new(settings));
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    rt: &tokio::runtime::Runtime,
    app: &mut App,
) -> Result<()> {
    let io = |e: std::io::Error| AppError::Other(format!("terminal: {e}"));
    while !app.quit {
        if let Some(force) = app.pending_refresh() {
            app.refreshing = true;
            terminal.draw(|f| view::draw(f, app)).map_err(io)?;
            app.refresh(rt, force);
        }
        terminal.draw(|f| view::draw(f, app)).map_err(io)?;
        if event::poll(Duration::from_millis(500)).map_err(io)?
            && let Event::Key(key) = event::read().map_err(io)?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.quit = true
                }
                KeyCode::Char('r') => app.force_refresh = true,
                _ => {}
            }
        }
    }
    Ok(())
}
