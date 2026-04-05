use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, Screen};

const ACCENT_PURPLE: Color = Color::Rgb(168, 85, 247);
const ACCENT_BLUE: Color = Color::Rgb(59, 130, 246);
const ACCENT_CYAN: Color = Color::Rgb(6, 182, 212);
const ACCENT_GREEN: Color = Color::Rgb(16, 185, 129);
const TEXT_SECONDARY: Color = Color::Rgb(161, 161, 170);
const PANEL_BORDER: Color = Color::Rgb(68, 68, 92);

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
                .fg(ACCENT_CYAN)
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
    let [banner_area, lower_area] =
        Layout::vertical([Constraint::Length(11), Constraint::Min(0)]).areas(area);
    let [left_banner, right_banner] =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
            .areas(inner(banner_area));
    let [overview_area, actions_area] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
            .areas(lower_area);

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PANEL_BORDER))
            .title(Span::styled(
                " DuckDocs  v0.1.0 ",
                Style::default()
                    .fg(ACCENT_PURPLE)
                    .add_modifier(Modifier::BOLD),
            )),
        banner_area,
    );

    let left = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "Welcome back!",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "DuckDocs",
            Style::default()
                .fg(ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "schema-first parsing in the terminal",
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("stub", Style::default().fg(ACCENT_GREEN)),
            Span::raw("  ·  "),
            Span::styled("local", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  ·  "),
            Span::styled("markdown", Style::default().fg(ACCENT_CYAN)),
        ]),
    ]);
    frame.render_widget(left, left_banner);

    let [tips_area, recent_area] =
        Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
            .areas(right_banner);

    let tips = Paragraph::new(vec![
        Line::from(Span::styled(
            "Tips for getting started",
            Style::default()
                .fg(ACCENT_PURPLE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Parse a file, inspect job output, or switch engines.",
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("duckdocs parse", Style::default().fg(ACCENT_GREEN)),
            Span::raw(" invoice.pdf"),
        ]),
    ]);
    frame.render_widget(tips, tips_area);

    let recent = Paragraph::new(vec![
        Line::from(Span::styled(
            "Recent activity",
            Style::default()
                .fg(ACCENT_PURPLE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "No completed jobs yet",
            Style::default().fg(TEXT_SECONDARY),
        )),
    ]);
    frame.render_widget(recent, recent_area);

    let overview = Paragraph::new(
        "DuckDocs TUI orchestrates parse jobs against an external engine.\n\n\
         Start here:\n\
         - parse a file\n\
         - inspect recent jobs\n\
         - configure engines",
    )
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(overview, overview_area);

    let shortcuts = List::new(vec![
        ListItem::new("Parse file"),
        ListItem::new("Batch parse"),
        ListItem::new("Engines"),
        ListItem::new("Settings"),
    ])
    .block(Block::default().borders(Borders::ALL).title("Actions"));
    frame.render_widget(shortcuts, actions_area);
}

fn inner(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let [inner] = Layout::vertical([Constraint::Min(0)]).margin(1).areas(area);
    inner
}

fn render_jobs(frame: &mut Frame, area: ratatui::layout::Rect) {
    let jobs = List::new(vec![
        ListItem::new(Line::from(vec![
            Span::styled("stub", Style::default().fg(ACCENT_GREEN)),
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
