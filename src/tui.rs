use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs},
    Terminal,
};
use std::time::{Duration, Instant};

struct App {
    selected_tab: usize,
    containers: Vec<String>,
    routes: Vec<(u16, String)>,
    logs: Vec<String>,
    proxy_status: String,
    last_update: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            selected_tab: 0,
            containers: Vec::new(),
            routes: Vec::new(),
            logs: Vec::new(),
            proxy_status: String::new(),
            last_update: Instant::now(),
        }
    }

    async fn refresh_data(&mut self) {
        let config_result = crate::config::load_config();
        match config_result {
            Ok(config) => {
                self.containers = config
                    .containers
                    .iter()
                    .map(|c| {
                        let label = c
                            .label
                            .as_ref()
                            .map(|l| format!(" - {}", l))
                            .unwrap_or_default();
                        let port = c.port.unwrap_or(8000);
                        let net = c.network.as_ref().unwrap_or(&config.network);
                        format!("{}:{}@{}{}", c.name, port, net, label)
                    })
                    .collect();

                self.routes = config
                    .routes
                    .iter()
                    .map(|r| (r.host_port, r.target.clone()))
                    .collect();
            }
            Err(_) => {
                self.containers = Vec::new();
                self.routes = Vec::new();
            }
        }

        let proxy_name = crate::config::get_proxy_name(None);
        match crate::docker::DockerClient::new() {
            Ok(docker) => {
                let status = docker.get_container_status(&proxy_name).await;
                match status {
                    Ok(Some(s)) => {
                        self.proxy_status = format!("{} ({})", proxy_name, s);
                    }
                    Ok(None) => {
                        self.proxy_status = format!("{} (not running)", proxy_name);
                    }
                    Err(_) => {
                        self.proxy_status = format!("{} (unknown)", proxy_name);
                    }
                }
            }
            Err(_) => {
                self.proxy_status = format!("{} (Docker unavailable)", proxy_name);
            }
        }
    }
}

pub async fn run_tui() -> Result<()> {
    enable_raw_mode()?;

    let mut stdout = std::io::stdout();
    let backend = CrosstermBackend::new(&mut stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.refresh_data().await;

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(f.size());

            let tabs = Tabs::new::<Vec<&str>>(vec!["Containers", "Routes", "Status", "Logs"])
                .select(app.selected_tab)
                .style(Style::default().fg(Color::Gray))
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );

            f.render_widget(tabs, chunks[0]);

            match app.selected_tab {
                0 => render_containers_tab(f, &chunks[1], &app),
                1 => render_routes_tab(f, &chunks[1], &app),
                2 => render_status_tab(f, &chunks[1], &app),
                3 => render_logs_tab(f, &chunks[1], &app),
                _ => {}
            }

            let help = Paragraph::new("Arrow keys: Navigate | q: Quit | r: Refresh")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            f.render_widget(help, chunks[2]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Left => {
                        if app.selected_tab > 0 {
                            app.selected_tab -= 1;
                        }
                    }
                    KeyCode::Right => {
                        if app.selected_tab < 3 {
                            app.selected_tab += 1;
                        }
                    }
                    KeyCode::Char('r') => {
                        app.refresh_data().await;
                        app.last_update = Instant::now();
                    }
                    _ => {}
                }
            }
        }

        if app.last_update.elapsed() > Duration::from_secs(5) {
            app.refresh_data().await;
            app.last_update = Instant::now();
        }
    }

    disable_raw_mode()?;
    Ok(())
}

fn render_containers_tab(f: &mut ratatui::Frame, area: &ratatui::layout::Rect, app: &App) {
    let block = Block::default().title("Containers").borders(Borders::ALL);
    f.render_widget(block, *area);

    if app.containers.is_empty() {
        let paragraph = Paragraph::new(
            "No containers configured. Use 'proxy-manager add <name>' to add containers.",
        )
        .alignment(Alignment::Center);
        f.render_widget(paragraph, *area);
        return;
    }

    let items: Vec<ListItem> = app
        .containers
        .iter()
        .map(|c| ListItem::new(c.clone()))
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(Color::White));

    f.render_widget(list, *area);
}

fn render_routes_tab(f: &mut ratatui::Frame, area: &ratatui::layout::Rect, app: &App) {
    let block = Block::default().title("Routes").borders(Borders::ALL);
    f.render_widget(block, *area);

    if app.routes.is_empty() {
        let paragraph = Paragraph::new(
            "No routes configured. Use 'proxy-manager switch <container> [port]' to add routes.",
        )
        .alignment(Alignment::Center);
        f.render_widget(paragraph, *area);
        return;
    }

    let rows: Vec<Row> = app
        .routes
        .iter()
        .map(|(port, target)| {
            Row::new(vec![
                Cell::from(port.to_string()),
                Cell::from(target.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        &[Constraint::Percentage(30), Constraint::Percentage(70)],
    )
    .column_spacing(1);

    f.render_widget(table, *area);
}

fn render_status_tab(f: &mut ratatui::Frame, area: &ratatui::layout::Rect, app: &App) {
    let block = Block::default().title("Status").borders(Borders::ALL);
    f.render_widget(block, *area);

    let status_text = if app.proxy_status.is_empty() {
        "Proxy status: Unknown"
    } else {
        &app.proxy_status
    };

    let paragraph = Paragraph::new(status_text).alignment(Alignment::Center);
    f.render_widget(paragraph, *area);
}

fn render_logs_tab(f: &mut ratatui::Frame, area: &ratatui::layout::Rect, app: &App) {
    let block = Block::default().title("Logs").borders(Borders::ALL);
    f.render_widget(block, *area);

    if app.logs.is_empty() {
        let paragraph = Paragraph::new("No logs available. Start the proxy to see logs.")
            .alignment(Alignment::Center);
        f.render_widget(paragraph, *area);
        return;
    }

    let paragraph = Paragraph::new(app.logs.join("\n"));
    f.render_widget(paragraph, *area);
}
