use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Tabs},
};
use std::io;

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Containers,
    Routes,
    Status,
}

pub struct App {
    config: Config,
    current_tab: Tab,
    selected_container: usize,
    selected_route: usize,
    show_help: bool,
    status_message: Option<String>,
}

impl App {
    pub fn new() -> anyhow::Result<Self> {
        let config = Config::load()?;
        Ok(Self {
            config,
            current_tab: Tab::Containers,
            selected_container: 0,
            selected_route: 0,
            show_help: false,
            status_message: None,
        })
    }

    pub fn refresh_config(&mut self) -> anyhow::Result<()> {
        self.config = Config::load()?;
        Ok(())
    }

    pub fn next_container(&mut self) {
        if !self.config.containers.is_empty() {
            self.selected_container = (self.selected_container + 1) % self.config.containers.len();
        }
    }

    pub fn previous_container(&mut self) {
        if !self.config.containers.is_empty() {
            self.selected_container = self.selected_container.saturating_sub(1);
        }
    }

    pub fn next_route(&mut self) {
        if !self.config.routes.is_empty() {
            self.selected_route = (self.selected_route + 1) % self.config.routes.len();
        }
    }

    pub fn previous_route(&mut self) {
        if !self.config.routes.is_empty() {
            self.selected_route = self.selected_route.saturating_sub(1);
        }
    }

    pub fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Containers => Tab::Routes,
            Tab::Routes => Tab::Status,
            Tab::Status => Tab::Containers,
        };
    }

    pub fn previous_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Containers => Tab::Status,
            Tab::Routes => Tab::Containers,
            Tab::Status => Tab::Routes,
        };
    }
}

pub async fn run_tui() -> anyhow::Result<()> {
    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app(terminal: &mut Terminal<impl Backend>, app: &mut App) -> anyhow::Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(250);

    loop {
        terminal.draw(|f| draw(f, app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| std::time::Duration::from_secs(0));

        if crossterm::event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('h') | KeyCode::Char('?') => app.show_help = !app.show_help,
                KeyCode::Tab => app.next_tab(),
                KeyCode::BackTab => app.previous_tab(),
                KeyCode::Right => {
                    if app.current_tab == Tab::Containers {
                        app.next_container();
                    } else if app.current_tab == Tab::Routes {
                        app.next_route();
                    } else {
                        app.next_tab();
                    }
                }
                KeyCode::Left => {
                    if app.current_tab == Tab::Containers {
                        app.previous_container();
                    } else if app.current_tab == Tab::Routes {
                        app.previous_route();
                    } else {
                        app.previous_tab();
                    }
                }
                KeyCode::Down => {
                    if app.current_tab == Tab::Containers {
                        app.next_container();
                    } else if app.current_tab == Tab::Routes {
                        app.next_route();
                    }
                }
                KeyCode::Up => {
                    if app.current_tab == Tab::Containers {
                        app.previous_container();
                    } else if app.current_tab == Tab::Routes {
                        app.previous_route();
                    }
                }
                KeyCode::Char('r') => {
                    // Refresh
                    if let Err(e) = app.refresh_config() {
                        app.status_message = Some(format!("Error: {}", e));
                    } else {
                        app.status_message = Some("Refreshed".to_string());
                    }
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = std::time::Instant::now();
        }
    }
}

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    // Title and tabs
    let titles = vec!["Containers", "Routes", "Status"];
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Proxy Manager"),
        )
        .select(match app.current_tab {
            Tab::Containers => 0,
            Tab::Routes => 1,
            Tab::Status => 2,
        })
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[0]);

    // Main content
    match app.current_tab {
        Tab::Containers => draw_containers(f, app, chunks[1]),
        Tab::Routes => draw_routes(f, app, chunks[1]),
        Tab::Status => draw_status(f, app, chunks[1]),
    }

    // Help bar
    let help_text = if app.show_help {
        "q/ESC: Quit | Tab: Next Tab | ←/→: Navigate | r: Refresh | ?: Toggle Help".to_string()
    } else {
        "Press ? for help".to_string()
    };
    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Gray));
    f.render_widget(help, chunks[2]);

    // Status message popup
    if let Some(msg) = &app.status_message {
        let popup = Paragraph::new(msg.as_str())
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .style(Style::default().fg(Color::Green));
        let area = centered_rect(60, 20, f.area());
        f.render_widget(Clear, area);
        f.render_widget(popup, area);
    }
}

fn draw_containers(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec!["Name", "Label", "Port", "Network"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .config
        .containers
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let style = if i == app.selected_container {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(c.name.clone()),
                Cell::from(c.label.clone().unwrap_or_default()),
                Cell::from(c.port.map(|p| p.to_string()).unwrap_or_default()),
                Cell::from(
                    c.network
                        .clone()
                        .unwrap_or_else(|| app.config.network.clone()),
                ),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        vec![
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(15),
            Constraint::Percentage(25),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Containers"));

    f.render_widget(table, area);
}

fn draw_routes(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec!["Host Port", "Target Container", "Internal Port"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .config
        .routes
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let style = if i == app.selected_route {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let internal_port = app
                .config
                .find_container(&r.target)
                .and_then(|c| c.port)
                .unwrap_or(crate::config::DEFAULT_PORT);
            Row::new(vec![
                Cell::from(r.host_port.to_string()),
                Cell::from(r.target.clone()),
                Cell::from(internal_port.to_string()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        vec![
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Routes"));

    f.render_widget(table, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let config_file = Config::config_file();
    let text = vec![
        Line::from(vec![
            Span::styled("Proxy Name: ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.config.proxy_name),
        ]),
        Line::from(vec![
            Span::styled("Network: ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.config.network),
        ]),
        Line::from(vec![
            Span::styled("Containers: ", Style::default().fg(Color::Yellow)),
            Span::raw(app.config.containers.len().to_string()),
        ]),
        Line::from(vec![
            Span::styled("Routes: ", Style::default().fg(Color::Yellow)),
            Span::raw(app.config.routes.len().to_string()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Config File: ", Style::default().fg(Color::Yellow)),
            Span::raw(config_file.to_string_lossy()),
        ]),
    ];

    let paragraph = Paragraph::new(Text::from(text))
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(paragraph, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
