use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &App) {
    let vertical = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ]);
    let [header, body, footer] = vertical.areas(frame.area());

    let titles = ["Home", "Jobs", "Settings"]
        .iter()
        .map(|title| Line::from(*title))
        .collect::<Vec<_>>();
    let selected = match app.screen {
        Screen::Home => 0,
        Screen::Jobs => 1,
        Screen::Settings => 2,
    };

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("DuckDocs"))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .select(selected);
    frame.render_widget(tabs, header);

    match app.screen {
        Screen::Home => render_home(frame, body),
        Screen::Jobs => render_jobs(frame, body),
        Screen::Settings => render_settings(frame, body),
    }

    let footer_text = Paragraph::new("q quit | h/l or arrow keys switch screens")
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer_text, footer);
}

fn render_home(frame: &mut Frame, area: ratatui::layout::Rect) {
    let [intro_area, actions_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let intro = Paragraph::new(
        "DuckDocs TUI will orchestrate parse jobs against an external engine.\n\n\
         Planned commands:\n\
         - parse a file\n\
         - inspect recent jobs\n\
         - configure engines",
    )
    .block(Block::default().borders(Borders::ALL).title("Home"));
    frame.render_widget(intro, intro_area);

    let shortcuts = List::new(vec![
        ListItem::new("Parse file"),
        ListItem::new("Batch parse"),
        ListItem::new("Engines"),
        ListItem::new("Settings"),
    ])
    .block(Block::default().borders(Borders::ALL).title("Actions"));
    frame.render_widget(shortcuts, actions_area);
}

fn render_jobs(frame: &mut Frame, area: ratatui::layout::Rect) {
    let jobs = List::new(vec![
        ListItem::new(Line::from(vec![
            Span::styled("stub", Style::default().fg(Color::Green)),
            Span::raw("  ready"),
        ])),
        ListItem::new("No completed jobs yet"),
    ])
    .block(Block::default().borders(Borders::ALL).title("Jobs"));
    frame.render_widget(jobs, area);
}

fn render_settings(frame: &mut Frame, area: ratatui::layout::Rect) {
    let settings = Paragraph::new(
        "Default engine: stub\nTransport: local process placeholder\nOutput: schema-driven result handling",
    )
    .block(Block::default().borders(Borders::ALL).title("Settings"));
    frame.render_widget(settings, area);
}
