use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{
        Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, TableState, Tabs,
    },
};
use std::io;
use std::time::{Duration, Instant};

use crate::config::{Config, Container};
use crate::docker::DockerClient;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    Containers,
    Routes,
    Status,
    Logs,
}

impl Tab {
    fn as_str(&self) -> &'static str {
        match self {
            Tab::Containers => "Containers",
            Tab::Routes => "Routes",
            Tab::Status => "Status",
            Tab::Logs => "Logs",
        }
    }

    fn next(&self) -> Self {
        match self {
            Tab::Containers => Tab::Routes,
            Tab::Routes => Tab::Status,
            Tab::Status => Tab::Logs,
            Tab::Logs => Tab::Containers,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Tab::Containers => Tab::Logs,
            Tab::Routes => Tab::Containers,
            Tab::Status => Tab::Routes,
            Tab::Logs => Tab::Status,
        }
    }
}

pub struct TuiApp {
    docker: DockerClient,
    config: Config,
    current_tab: Tab,
    container_table_state: TableState,
    route_table_state: TableState,
    logs_scroll: usize,
    logs_content: Vec<String>,
    last_update: Instant,
    proxy_status: Option<String>,
    show_popup: Option<Popup>,
    input_buffer: String,
    input_mode: InputMode,
}

#[derive(Debug, Clone)]
enum Popup {
    AddContainer,
    RemoveContainer(String),
    AddRoute,
    Confirm(String, Box<Popup>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum InputMode {
    Normal,
    Input,
}

impl TuiApp {
    pub async fn new() -> anyhow::Result<Self> {
        let docker = DockerClient::new()?;
        let config = Config::load()?;
        let proxy_status = docker
            .get_container_status(&config.proxy_name)
            .await
            .ok()
            .flatten();

        Ok(Self {
            docker,
            config,
            current_tab: Tab::Containers,
            container_table_state: TableState::default().with_selected(0),
            route_table_state: TableState::default().with_selected(0),
            logs_scroll: 0,
            logs_content: Vec::new(),
            last_update: Instant::now(),
            proxy_status,
            show_popup: None,
            input_buffer: String::new(),
            input_mode: InputMode::Normal,
        })
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut last_tick = Instant::now();
        let tick_rate = Duration::from_millis(250);

        loop {
            terminal.draw(|f| self.draw(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if crossterm::event::poll(timeout)?
                && let Event::Key(key) = event::read()?
                    && key.kind == KeyEventKind::Press
                        && self.handle_key(key.code).await? {
                            break;
                        }

            if last_tick.elapsed() >= tick_rate {
                self.on_tick().await?;
                last_tick = Instant::now();
            }
        }

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    async fn on_tick(&mut self) -> anyhow::Result<()> {
        // Update data every 5 seconds
        if self.last_update.elapsed() >= Duration::from_secs(5) {
            self.config = Config::load()?;
            self.proxy_status = self
                .docker
                .get_container_status(&self.config.proxy_name)
                .await
                .ok()
                .flatten();

            if self.current_tab == Tab::Logs {
                self.update_logs().await?;
            }

            self.last_update = Instant::now();
        }
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyCode) -> anyhow::Result<bool> {
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key).await,
            InputMode::Input => self.handle_input_key(key).await,
        }
    }

    async fn handle_normal_key(&mut self, key: KeyCode) -> anyhow::Result<bool> {
        match key {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Tab => {
                self.current_tab = self.current_tab.next();
            }
            KeyCode::BackTab => {
                self.current_tab = self.current_tab.prev();
            }
            KeyCode::Right => {
                self.current_tab = self.current_tab.next();
            }
            KeyCode::Left => {
                self.current_tab = self.current_tab.prev();
            }
            _ => match self.current_tab {
                Tab::Containers => self.handle_containers_key(key).await?,
                Tab::Routes => self.handle_routes_key(key).await?,
                Tab::Status => self.handle_status_key(key).await?,
                Tab::Logs => self.handle_logs_key(key)?,
            },
        }
        Ok(false)
    }

    async fn handle_containers_key(&mut self, key: KeyCode) -> anyhow::Result<()> {
        match key {
            KeyCode::Down => {
                let len = self.config.containers.len();
                if len > 0 {
                    let i = self
                        .container_table_state
                        .selected()
                        .map(|i| (i + 1) % len)
                        .unwrap_or(0);
                    self.container_table_state.select(Some(i));
                }
            }
            KeyCode::Up => {
                let len = self.config.containers.len();
                if len > 0 {
                    let i = self
                        .container_table_state
                        .selected()
                        .map(|i| if i == 0 { len - 1 } else { i - 1 })
                        .unwrap_or(0);
                    self.container_table_state.select(Some(i));
                }
            }
            KeyCode::Char('a') => {
                self.show_popup = Some(Popup::AddContainer);
                self.input_mode = InputMode::Input;
                self.input_buffer.clear();
            }
            KeyCode::Char('d') => {
                if let Some(idx) = self.container_table_state.selected()
                    && let Some(container) = self.config.containers.get(idx) {
                        let name = container.name.clone();
                        self.show_popup = Some(Popup::RemoveContainer(name));
                    }
            }
            KeyCode::Char('r') => {
                self.reload_proxy().await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_routes_key(&mut self, key: KeyCode) -> anyhow::Result<()> {
        match key {
            KeyCode::Down => {
                let len = self.config.routes.len();
                if len > 0 {
                    let i = self
                        .route_table_state
                        .selected()
                        .map(|i| (i + 1) % len)
                        .unwrap_or(0);
                    self.route_table_state.select(Some(i));
                }
            }
            KeyCode::Up => {
                let len = self.config.routes.len();
                if len > 0 {
                    let i = self
                        .route_table_state
                        .selected()
                        .map(|i| if i == 0 { len - 1 } else { i - 1 })
                        .unwrap_or(0);
                    self.route_table_state.select(Some(i));
                }
            }
            KeyCode::Char('a') => {
                self.show_popup = Some(Popup::AddRoute);
                self.input_mode = InputMode::Input;
                self.input_buffer.clear();
            }
            KeyCode::Char('d') => {
                if let Some(idx) = self.route_table_state.selected()
                    && let Some(route) = self.config.routes.get(idx) {
                        let port = route.host_port;
                        self.config.remove_route(port);
                        self.config.save()?;
                    }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_status_key(&mut self, key: KeyCode) -> anyhow::Result<()> {
        match key {
            KeyCode::Char('s') => {
                self.start_proxy().await?;
            }
            KeyCode::Char('x') => {
                self.stop_proxy().await?;
            }
            KeyCode::Char('r') => {
                self.reload_proxy().await?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_logs_key(&mut self, key: KeyCode) -> anyhow::Result<()> {
        match key {
            KeyCode::Down => {
                self.logs_scroll = self.logs_scroll.saturating_add(1);
            }
            KeyCode::Up => {
                self.logs_scroll = self.logs_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.logs_scroll = self.logs_scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.logs_scroll = self.logs_scroll.saturating_sub(10);
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_input_key(&mut self, key: KeyCode) -> anyhow::Result<bool> {
        match key {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.show_popup = None;
                self.input_buffer.clear();
            }
            KeyCode::Enter => {
                self.process_input().await?;
                self.input_mode = InputMode::Normal;
                self.show_popup = None;
                self.input_buffer.clear();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            _ => {}
        }
        Ok(false)
    }

    async fn process_input(&mut self) -> anyhow::Result<()> {
        match self.show_popup {
            Some(Popup::AddContainer) => {
                if !self.input_buffer.is_empty() {
                    let name = self.input_buffer.clone();
                    let network = self
                        .docker
                        .get_container_network(&name)
                        .await
                        .ok()
                        .flatten();
                    let container = Container::new(name).with_network(network.unwrap_or_default());
                    self.config.add_or_update_container(container);
                    self.config.save()?;
                }
            }
            Some(Popup::AddRoute) => {
                // Format: "port:container_name"
                if let Some((port_str, target)) = self.input_buffer.split_once(':')
                    && let Ok(port) = port_str.parse::<u16>() {
                        self.config.set_route(port, target);
                        self.config.save()?;
                    }
            }
            Some(Popup::RemoveContainer(ref name)) => {
                if self.input_buffer.to_lowercase() == "y" {
                    self.config.remove_container(name);
                    self.config.save()?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn start_proxy(&mut self) -> anyhow::Result<()> {
        if self.config.containers.is_empty() {
            return Ok(());
        }
        if self.config.routes.is_empty() {
            return Ok(());
        }

        // Write build files
        let build_dir = Config::build_dir();
        tokio::fs::create_dir_all(&build_dir).await?;

        let nginx_conf = crate::nginx::NginxConfigGenerator::generate(&self.config);
        let dockerfile = crate::nginx::NginxConfigGenerator::generate_dockerfile(&self.config);

        tokio::fs::write(build_dir.join("nginx.conf"), nginx_conf).await?;
        tokio::fs::write(build_dir.join("Dockerfile"), dockerfile).await?;

        self.docker.start_proxy(&self.config).await?;
        self.proxy_status = Some("running".to_string());
        Ok(())
    }

    async fn stop_proxy(&mut self) -> anyhow::Result<()> {
        self.docker.stop_proxy(&self.config.proxy_name).await?;
        self.proxy_status = None;
        Ok(())
    }

    async fn reload_proxy(&mut self) -> anyhow::Result<()> {
        self.stop_proxy().await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.start_proxy().await
    }

    async fn update_logs(&mut self) -> anyhow::Result<()> {
        self.logs_content = self
            .docker
            .get_proxy_logs(&self.config.proxy_name, 100, false)
            .await?;
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(frame.area());

        self.render_tabs(frame, main_layout[0]);
        self.render_content(frame, main_layout[1]);

        if let Some(ref popup) = self.show_popup {
            self.render_popup(frame, popup.clone());
        }
    }

    fn render_tabs(&self, frame: &mut Frame, area: Rect) {
        let tabs = [Tab::Containers, Tab::Routes, Tab::Status, Tab::Logs];

        let tab_titles: Vec<Line> = tabs
            .iter()
            .map(|t| {
                let style = if *t == self.current_tab {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(t.as_str()).style(style)
            })
            .collect();

        let tabs_widget = Tabs::new(tab_titles)
            .block(
                Block::default()
                    .title(" Proxy Manager ")
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::default().fg(Color::Yellow))
            .select(match self.current_tab {
                Tab::Containers => 0,
                Tab::Routes => 1,
                Tab::Status => 2,
                Tab::Logs => 3,
            });

        frame.render_widget(tabs_widget, area);
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect) {
        match self.current_tab {
            Tab::Containers => self.render_containers(frame, area),
            Tab::Routes => self.render_routes(frame, area),
            Tab::Status => self.render_status(frame, area),
            Tab::Logs => self.render_logs(frame, area),
        }
    }

    fn render_containers(&mut self, frame: &mut Frame, area: Rect) {
        let header = ["Name", "Label", "Port", "Network"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(Style::default().add_modifier(Modifier::BOLD))
            .height(1);

        let rows: Vec<Row> = self
            .config
            .containers
            .iter()
            .map(|c| {
                Row::new(vec![
                    Cell::from(c.name.clone()),
                    Cell::from(c.label.clone().unwrap_or_default()),
                    Cell::from(c.get_port().to_string()),
                    Cell::from(
                        c.network
                            .clone()
                            .unwrap_or_else(|| self.config.network.clone()),
                    ),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(30),
                Constraint::Percentage(30),
                Constraint::Percentage(15),
                Constraint::Percentage(25),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .title(" Containers [a:add d:remove r:reload] ")
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_spacing(HighlightSpacing::Always);

        frame.render_stateful_widget(table, area, &mut self.container_table_state);
    }

    fn render_routes(&mut self, frame: &mut Frame, area: Rect) {
        let header = ["Host Port", "Target Container", "Target Port"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(Style::default().add_modifier(Modifier::BOLD))
            .height(1);

        let rows: Vec<Row> = self
            .config
            .routes
            .iter()
            .map(|r| {
                let target_port = self
                    .config
                    .find_container(&r.target)
                    .map(|c| c.get_port().to_string())
                    .unwrap_or_else(|| "?".to_string());

                Row::new(vec![
                    Cell::from(r.host_port.to_string()),
                    Cell::from(r.target.clone()),
                    Cell::from(target_port),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(25),
                Constraint::Percentage(40),
                Constraint::Percentage(35),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .title(" Routes [a:add d:remove] ")
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_spacing(HighlightSpacing::Always);

        frame.render_stateful_widget(table, area, &mut self.route_table_state);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let status_text = match &self.proxy_status {
            Some(s) => format!("Status: {}", s),
            None => "Status: not running".to_string(),
        };

        let route_info: String = self
            .config
            .routes
            .iter()
            .map(|r| {
                let target_port = self
                    .config
                    .find_container(&r.target)
                    .map(|c| c.get_port().to_string())
                    .unwrap_or_else(|| "?".to_string());
                format!("  {} -> {}:{}", r.host_port, r.target, target_port)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let text = format!(
            "{}\n\nActive Routes:\n{}",
            status_text,
            if route_info.is_empty() {
                "  (none)".to_string()
            } else {
                route_info
            }
        );

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Status [s:start x:stop r:reload] ")
                    .borders(Borders::ALL),
            )
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    fn render_logs(&self, frame: &mut Frame, area: Rect) {
        let logs = self.logs_content.join("");
        let paragraph = Paragraph::new(logs)
            .block(
                Block::default()
                    .title(" Logs [↑↓ scroll] ")
                    .borders(Borders::ALL),
            )
            .scroll((self.logs_scroll as u16, 0))
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    fn render_popup(&self, frame: &mut Frame, popup: Popup) {
        let area = frame.area();
        let popup_area = Self::centered_rect(60, 20, area);

        frame.render_widget(Clear, popup_area);

        let (title, content) = match popup {
            Popup::AddContainer => (
                " Add Container ",
                format!("Container name:\n{}", self.input_buffer),
            ),
            Popup::AddRoute => (
                " Add Route ",
                format!("Format: port:container\n{}", self.input_buffer),
            ),
            Popup::RemoveContainer(ref name) => (
                " Remove Container ",
                format!("Remove '{}'? (y/n)\n{}", name, self.input_buffer),
            ),
            Popup::Confirm(ref msg, _) => (" Confirm ", format!("{}\n{}", msg, self.input_buffer)),
        };

        let paragraph = Paragraph::new(content)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(paragraph, popup_area);
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
}
