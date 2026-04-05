use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::DefaultTerminal;

use crate::app::App;

pub fn run_tui() -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::default();
    let result = run_app(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run_app(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| crate::ui::render(frame, app))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Right | KeyCode::Char('l') => app.next_screen(),
                    KeyCode::Left | KeyCode::Char('h') => app.previous_screen(),
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}
