use crate::config::Config;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Containers,
    Routes,
    Status,
}

pub struct App {
    current_tab: Tab,
    containers_state: ListState,
    routes_state: ListState,
    should_quit: bool,
    status_message: Option<String>,
}

impl App {
    fn new() -> Self {
        let mut containers_state = ListState::default();
        containers_state.select(Some(0));

        let mut routes_state = ListState::default();
        routes_state.select(Some(0));

        Self {
            current_tab: Tab::Containers,
            containers_state,
            routes_state,
            should_quit: false,
            status_message: None,
        }
    }

    fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Containers => Tab::Routes,
            Tab::Routes => Tab::Status,
            Tab::Status => Tab::Containers,
        };
    }

    fn previous_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Containers => Tab::Status,
            Tab::Routes => Tab::Containers,
            Tab::Status => Tab::Routes,
        };
    }

    fn next_item(&mut self) {
        match self.current_tab {
            Tab::Containers => {
                let state = &mut self.containers_state;
                let i = match state.selected() {
                    Some(i) => {
                        let config = Config::load().ok();
                        if let Some(cfg) = config {
                            if i >= cfg.containers.len().saturating_sub(1) {
                                0
                            } else {
                                i + 1
                            }
                        } else {
                            0
                        }
                    }
                    None => 0,
                };
                state.select(Some(i));
            }
            Tab::Routes => {
                let state = &mut self.routes_state;
                let i = match state.selected() {
                    Some(i) => {
                        let config = Config::load().ok();
                        if let Some(cfg) = config {
                            if i >= cfg.routes.len().saturating_sub(1) {
                                0
                            } else {
                                i + 1
                            }
                        } else {
                            0
                        }
                    }
                    None => 0,
                };
                state.select(Some(i));
            }
            Tab::Status => {}
        }
    }

    fn previous_item(&mut self) {
        match self.current_tab {
            Tab::Containers => {
                let state = &mut self.containers_state;
                let i = match state.selected() {
                    Some(i) => {
                        let config = Config::load().ok();
                        if let Some(cfg) = config {
                            if i == 0 {
                                cfg.containers.len().saturating_sub(1)
                            } else {
                                i - 1
                            }
                        } else {
                            0
                        }
                    }
                    None => 0,
                };
                state.select(Some(i));
            }
            Tab::Routes => {
                let state = &mut self.routes_state;
                let i = match state.selected() {
                    Some(i) => {
                        let config = Config::load().ok();
                        if let Some(cfg) = config {
                            if i == 0 {
                                cfg.routes.len().saturating_sub(1)
                            } else {
                                i - 1
                            }
                        } else {
                            0
                        }
                    }
                    None => 0,
                };
                state.select(Some(i));
            }
            Tab::Status => {}
        }
    }

    fn set_status(&mut self, message: String) {
        self.status_message = Some(message);
    }
}

pub async fn run_tui() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.should_quit = true;
                        }
                        KeyCode::Tab => {
                            app.next_tab();
                        }
                        KeyCode::BackTab => {
                            app.previous_tab();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.next_item();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.previous_item();
                        }
                        KeyCode::Char('r') => {
                            app.set_status("Reloading proxy...".to_string());
                            // In a real implementation, we'd trigger reload_proxy here
                        }
                        KeyCode::Char('s') => {
                            app.set_status("Starting proxy...".to_string());
                            // In a real implementation, we'd trigger start_proxy here
                        }
                        KeyCode::Char('x') => {
                            app.set_status("Stopping proxy...".to_string());
                            // In a real implementation, we'd trigger stop_proxy here
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_tabs(f, app, chunks[0]);
    render_content(f, app, chunks[1]);
    render_footer(f, app, chunks[2]);
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles = ["Containers", "Routes", "Status"];
    let mut tab_spans = Vec::new();

    for (i, title) in titles.iter().enumerate() {
        let tab = match i {
            0 => Tab::Containers,
            1 => Tab::Routes,
            _ => Tab::Status,
        };

        let style = if tab == app.current_tab {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        tab_spans.push(Span::styled(format!(" {} ", title), style));
        if i < titles.len() - 1 {
            tab_spans.push(Span::raw(" | "));
        }
    }

    let tabs = Paragraph::new(Line::from(tab_spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Proxy Manager"),
    );

    f.render_widget(tabs, area);
}

fn render_content(f: &mut Frame, app: &mut App, area: Rect) {
    match app.current_tab {
        Tab::Containers => render_containers(f, app, area),
        Tab::Routes => render_routes(f, app, area),
        Tab::Status => render_status(f, area),
    }
}

fn render_containers(f: &mut Frame, app: &mut App, area: Rect) {
    let config = Config::load().unwrap_or_default();

    let items: Vec<ListItem> = config
        .containers
        .iter()
        .map(|c| {
            let label = c
                .label
                .as_ref()
                .map(|l| format!(" - {}", l))
                .unwrap_or_default();
            let port = c.port.unwrap_or(8000);
            let network = c.network.as_deref().unwrap_or(&config.network);
            let content = format!("{}:{}@{}{}", c.name, port, network, label);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Configured Containers"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.containers_state);
}

fn render_routes(f: &mut Frame, app: &mut App, area: Rect) {
    let config = Config::load().unwrap_or_default();

    let items: Vec<ListItem> = config
        .routes
        .iter()
        .map(|r| {
            let target_container = config.find_container(&r.target);
            let internal_port = target_container
                .map(|c| config.get_internal_port(c))
                .unwrap_or(8000);
            let content = format!("{} -> {}:{}", r.host_port, r.target, internal_port);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Active Routes"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.routes_state);
}

fn render_status(f: &mut Frame, area: Rect) {
    let config = Config::load().unwrap_or_default();

    let mut lines = vec![
        Line::from(format!("Proxy Name: {}", config.proxy_name)),
        Line::from(format!("Network: {}", config.network)),
        Line::from(""),
        Line::from(format!("Containers: {}", config.containers.len())),
        Line::from(format!("Routes: {}", config.routes.len())),
    ];

    if !config.routes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("Port Mappings:"));
        for route in &config.routes {
            lines.push(Line::from(format!(
                "  {} -> {}",
                route.host_port, route.target
            )));
        }
    }

    let paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Proxy Status"));

    f.render_widget(paragraph, area);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let help_text = if let Some(msg) = &app.status_message {
        msg.clone()
    } else {
        "q/ESC: Quit | Tab/Shift+Tab: Switch tabs | ↑↓/jk: Navigate | r: Reload | s: Start | x: Stop".to_string()
    };

    let footer = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let app = App::new();
        assert_eq!(app.current_tab, Tab::Containers);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_tab_navigation() {
        let mut app = App::new();
        assert_eq!(app.current_tab, Tab::Containers);

        app.next_tab();
        assert_eq!(app.current_tab, Tab::Routes);

        app.next_tab();
        assert_eq!(app.current_tab, Tab::Status);

        app.next_tab();
        assert_eq!(app.current_tab, Tab::Containers);
    }
}
