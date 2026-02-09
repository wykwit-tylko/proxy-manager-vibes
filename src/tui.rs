use std::io;

use anyhow::Result;
use bollard::Docker;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::config::{self, Config};
use crate::docker;
use crate::proxy;

/// Active panel/tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Containers,
    Routes,
    Status,
    Networks,
}

impl Tab {
    fn all() -> &'static [Tab] {
        &[Tab::Containers, Tab::Routes, Tab::Status, Tab::Networks]
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Containers => "Containers",
            Tab::Routes => "Routes",
            Tab::Status => "Status",
            Tab::Networks => "Networks",
        }
    }

    fn next(self) -> Tab {
        match self {
            Tab::Containers => Tab::Routes,
            Tab::Routes => Tab::Status,
            Tab::Status => Tab::Networks,
            Tab::Networks => Tab::Containers,
        }
    }

    fn prev(self) -> Tab {
        match self {
            Tab::Containers => Tab::Networks,
            Tab::Routes => Tab::Containers,
            Tab::Status => Tab::Routes,
            Tab::Networks => Tab::Status,
        }
    }
}

/// Modal dialog type.
#[derive(Debug, Clone)]
enum Modal {
    /// Confirm an action with a message.
    Confirm {
        message: String,
        action: ModalAction,
    },
    /// Display an informational/error message.
    Message { title: String, body: String },
}

/// Actions that can be confirmed via modal.
#[derive(Debug, Clone)]
enum ModalAction {
    RemoveContainer(String),
    RemoveRoute(u16),
    StopProxy,
    StartProxy,
    RestartProxy,
}

/// The TUI application state.
struct App {
    docker: Docker,
    config: Config,
    active_tab: Tab,
    container_list_state: ListState,
    route_list_state: ListState,
    network_list_state: ListState,
    proxy_status: String,
    network_infos: Vec<docker::NetworkInfo>,
    modal: Option<Modal>,
    status_lines: Vec<String>,
    should_quit: bool,
}

impl App {
    fn new(docker: Docker, config: Config) -> Self {
        let mut container_list_state = ListState::default();
        if !config.containers.is_empty() {
            container_list_state.select(Some(0));
        }
        let mut route_list_state = ListState::default();
        if !config.routes.is_empty() {
            route_list_state.select(Some(0));
        }

        Self {
            docker,
            config,
            active_tab: Tab::Containers,
            container_list_state,
            route_list_state,
            network_list_state: ListState::default(),
            proxy_status: "Unknown".to_string(),
            network_infos: Vec::new(),
            modal: None,
            status_lines: Vec::new(),
            should_quit: false,
        }
    }

    /// Refresh data from Docker and config.
    async fn refresh(&mut self) {
        // Reload config
        if let Ok(c) = config::load_config() {
            self.config = c;
        }

        // Update proxy status
        let proxy_name = self.config.proxy_name();
        self.proxy_status = match docker::get_container_status(&self.docker, proxy_name).await {
            Ok(Some(status)) => status,
            Ok(None) => "Not running".to_string(),
            Err(e) => format!("Error: {e}"),
        };

        // Build status lines
        self.status_lines = vec![
            format!("Proxy: {} ({})", proxy_name, self.proxy_status),
            String::new(),
        ];

        if self.config.routes.is_empty() {
            self.status_lines.push("No active routes".to_string());
        } else {
            self.status_lines.push("Active routes:".to_string());
            for route in &self.config.routes {
                let tc = self
                    .config
                    .containers
                    .iter()
                    .find(|c| c.name == route.target);
                if let Some(tc) = tc {
                    let port = Config::internal_port(tc);
                    self.status_lines.push(format!(
                        "  {} -> {}:{}",
                        route.host_port, route.target, port
                    ));
                } else {
                    self.status_lines.push(format!(
                        "  {} -> {} (not found)",
                        route.host_port, route.target
                    ));
                }
            }
        }

        // Update network list
        if let Ok(nets) = docker::list_networks(&self.docker).await {
            self.network_infos = nets;
            if !self.network_infos.is_empty() && self.network_list_state.selected().is_none() {
                self.network_list_state.select(Some(0));
            }
        }

        // Fix list selections
        self.fix_selections();
    }

    fn fix_selections(&mut self) {
        if self.config.containers.is_empty() {
            self.container_list_state.select(None);
        } else if self.container_list_state.selected().is_none() {
            self.container_list_state.select(Some(0));
        } else if let Some(i) = self.container_list_state.selected()
            && i >= self.config.containers.len()
        {
            self.container_list_state
                .select(Some(self.config.containers.len() - 1));
        }

        if self.config.routes.is_empty() {
            self.route_list_state.select(None);
        } else if self.route_list_state.selected().is_none() {
            self.route_list_state.select(Some(0));
        } else if let Some(i) = self.route_list_state.selected()
            && i >= self.config.routes.len()
        {
            self.route_list_state
                .select(Some(self.config.routes.len() - 1));
        }
    }

    fn move_selection_down(&mut self) {
        match self.active_tab {
            Tab::Containers => {
                let len = self.config.containers.len();
                if len == 0 {
                    return;
                }
                let i = self
                    .container_list_state
                    .selected()
                    .map(|i| (i + 1) % len)
                    .unwrap_or(0);
                self.container_list_state.select(Some(i));
            }
            Tab::Routes => {
                let len = self.config.routes.len();
                if len == 0 {
                    return;
                }
                let i = self
                    .route_list_state
                    .selected()
                    .map(|i| (i + 1) % len)
                    .unwrap_or(0);
                self.route_list_state.select(Some(i));
            }
            Tab::Networks => {
                let len = self.network_infos.len();
                if len == 0 {
                    return;
                }
                let i = self
                    .network_list_state
                    .selected()
                    .map(|i| (i + 1) % len)
                    .unwrap_or(0);
                self.network_list_state.select(Some(i));
            }
            Tab::Status => {}
        }
    }

    fn move_selection_up(&mut self) {
        match self.active_tab {
            Tab::Containers => {
                let len = self.config.containers.len();
                if len == 0 {
                    return;
                }
                let i = self
                    .container_list_state
                    .selected()
                    .map(|i| if i == 0 { len - 1 } else { i - 1 })
                    .unwrap_or(0);
                self.container_list_state.select(Some(i));
            }
            Tab::Routes => {
                let len = self.config.routes.len();
                if len == 0 {
                    return;
                }
                let i = self
                    .route_list_state
                    .selected()
                    .map(|i| if i == 0 { len - 1 } else { i - 1 })
                    .unwrap_or(0);
                self.route_list_state.select(Some(i));
            }
            Tab::Networks => {
                let len = self.network_infos.len();
                if len == 0 {
                    return;
                }
                let i = self
                    .network_list_state
                    .selected()
                    .map(|i| if i == 0 { len - 1 } else { i - 1 })
                    .unwrap_or(0);
                self.network_list_state.select(Some(i));
            }
            Tab::Status => {}
        }
    }

    /// Handle key events. Returns true if the event was consumed by a modal.
    async fn handle_key(&mut self, key: event::KeyEvent) {
        // Handle modal first
        if let Some(modal) = self.modal.take() {
            match modal {
                Modal::Confirm { action, .. } => {
                    if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
                        self.execute_action(action).await;
                    }
                    // Any other key dismisses
                }
                Modal::Message { .. } => {
                    // Any key dismisses
                }
            }
            return;
        }

        // Global keys
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return;
            }
            KeyCode::Tab => {
                self.active_tab = self.active_tab.next();
                return;
            }
            KeyCode::BackTab => {
                self.active_tab = self.active_tab.prev();
                return;
            }
            KeyCode::Char('r') => {
                self.refresh().await;
                return;
            }
            _ => {}
        }

        // Tab-specific keys
        match self.active_tab {
            Tab::Containers => self.handle_containers_key(key).await,
            Tab::Routes => self.handle_routes_key(key).await,
            Tab::Status => self.handle_status_key(key).await,
            Tab::Networks => {}
        }
    }

    async fn handle_containers_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(),
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(idx) = self.container_list_state.selected()
                    && let Some(c) = self.config.containers.get(idx)
                {
                    let name = c.name.clone();
                    self.modal = Some(Modal::Confirm {
                        message: format!("Remove container '{name}' from config?"),
                        action: ModalAction::RemoveContainer(name),
                    });
                }
            }
            _ => {}
        }
    }

    async fn handle_routes_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(),
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(idx) = self.route_list_state.selected()
                    && let Some(r) = self.config.routes.get(idx)
                {
                    let port = r.host_port;
                    self.modal = Some(Modal::Confirm {
                        message: format!("Remove route for port {port}?"),
                        action: ModalAction::RemoveRoute(port),
                    });
                }
            }
            _ => {}
        }
    }

    async fn handle_status_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Char('s') => {
                self.modal = Some(Modal::Confirm {
                    message: "Start the proxy?".to_string(),
                    action: ModalAction::StartProxy,
                });
            }
            KeyCode::Char('x') => {
                self.modal = Some(Modal::Confirm {
                    message: "Stop the proxy?".to_string(),
                    action: ModalAction::StopProxy,
                });
            }
            KeyCode::Char('R') => {
                self.modal = Some(Modal::Confirm {
                    message: "Restart the proxy?".to_string(),
                    action: ModalAction::RestartProxy,
                });
            }
            _ => {}
        }
    }

    async fn execute_action(&mut self, action: ModalAction) {
        let result = match action {
            ModalAction::RemoveContainer(ref name) => {
                proxy::remove_container(&mut self.config, name)
            }
            ModalAction::RemoveRoute(port) => {
                proxy::stop_port(&self.docker, &mut self.config, port).await
            }
            ModalAction::StopProxy => proxy::stop_proxy(&self.docker, &self.config)
                .await
                .map(|_| ()),
            ModalAction::StartProxy => proxy::start_proxy(&self.docker, &self.config).await,
            ModalAction::RestartProxy => {
                let _ = proxy::stop_proxy(&self.docker, &self.config).await;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                proxy::start_proxy(&self.docker, &self.config).await
            }
        };

        match result {
            Ok(()) => {
                self.refresh().await;
            }
            Err(e) => {
                self.modal = Some(Modal::Message {
                    title: "Error".to_string(),
                    body: format!("{e:#}"),
                });
                self.refresh().await;
            }
        }
    }
}

/// Draw the TUI.
fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tabs
            Constraint::Min(10),   // Content
            Constraint::Length(3), // Help
        ])
        .split(frame.area());

    draw_tabs(frame, app, chunks[0]);
    draw_content(frame, app, chunks[1]);
    draw_help(frame, app, chunks[2]);

    if let Some(ref modal) = app.modal {
        draw_modal(frame, modal);
    }
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Span> = Tab::all()
        .iter()
        .map(|t| {
            let style = if *t == app.active_tab {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Span::styled(format!(" {} ", t.label()), style)
        })
        .collect();

    let tabs_line = Line::from(titles);
    let tabs = Paragraph::new(tabs_line).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Proxy Manager"),
    );
    frame.render_widget(tabs, area);
}

fn draw_content(frame: &mut Frame, app: &mut App, area: Rect) {
    match app.active_tab {
        Tab::Containers => draw_containers(frame, app, area),
        Tab::Routes => draw_routes(frame, app, area),
        Tab::Status => draw_status(frame, app, area),
        Tab::Networks => draw_networks(frame, app, area),
    }
}

fn draw_containers(frame: &mut Frame, app: &mut App, area: Rect) {
    let route_map: std::collections::HashMap<&str, u16> = app
        .config
        .routes
        .iter()
        .map(|r| (r.target.as_str(), r.host_port))
        .collect();

    let items: Vec<ListItem> = app
        .config
        .containers
        .iter()
        .map(|c| {
            let port = Config::internal_port(c);
            let net = c.network.as_deref().unwrap_or(app.config.network_name());
            let label = c
                .label
                .as_ref()
                .map(|l| format!(" - {l}"))
                .unwrap_or_default();
            let routed = route_map
                .get(c.name.as_str())
                .map(|p| format!(" -> port {p}"))
                .unwrap_or_default();

            let line = format!("{}:{port}@{net}{label}{routed}", c.name);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Containers"))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.container_list_state);
}

fn draw_routes(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .config
        .routes
        .iter()
        .map(|r| {
            let tc = app.config.containers.iter().find(|c| c.name == r.target);
            let detail = if let Some(tc) = tc {
                let port = Config::internal_port(tc);
                format!("{} -> {}:{port}", r.host_port, r.target)
            } else {
                format!("{} -> {} (missing)", r.host_port, r.target)
            };
            ListItem::new(detail)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Routes"))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.route_list_state);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let text: Vec<Line> = app
        .status_lines
        .iter()
        .map(|l| Line::from(l.as_str()))
        .collect();

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Proxy Status"))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn draw_networks(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .network_infos
        .iter()
        .map(|n| {
            let line = format!(
                "{:<25} driver={:<10} containers={:<4} scope={}",
                n.name, n.driver, n.container_count, n.scope
            );
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Networks"))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.network_list_state);
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let help_text = match app.modal {
        Some(Modal::Confirm { .. }) => "y: Confirm | Any other key: Cancel",
        Some(Modal::Message { .. }) => "Press any key to dismiss",
        None => match app.active_tab {
            Tab::Containers => {
                "Tab/Shift+Tab: Switch tab | j/k: Navigate | d: Remove | r: Refresh | q: Quit"
            }
            Tab::Routes => {
                "Tab/Shift+Tab: Switch tab | j/k: Navigate | d: Remove | r: Refresh | q: Quit"
            }
            Tab::Status => {
                "Tab/Shift+Tab: Switch tab | s: Start | x: Stop | R: Restart | r: Refresh | q: Quit"
            }
            Tab::Networks => "Tab/Shift+Tab: Switch tab | j/k: Navigate | r: Refresh | q: Quit",
        },
    };

    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(help, area);
}

fn draw_modal(frame: &mut Frame, modal: &Modal) {
    let area = centered_rect(60, 30, frame.area());

    frame.render_widget(Clear, area);

    let (title, body) = match modal {
        Modal::Confirm { message, .. } => ("Confirm", format!("{message}\n\n[y] Yes  [n] No")),
        Modal::Message { title, body } => (title.as_str(), body.clone()),
    };

    let paragraph = Paragraph::new(body)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Create a centered rectangle.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Run the TUI application.
pub async fn run() -> Result<()> {
    let docker = docker::create_client()?;
    let config = config::load_config()?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(docker, config);
    app.refresh().await;

    loop {
        terminal.draw(|f| draw(f, &mut app))?;

        if event::poll(std::time::Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key).await;
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_next() {
        assert_eq!(Tab::Containers.next(), Tab::Routes);
        assert_eq!(Tab::Routes.next(), Tab::Status);
        assert_eq!(Tab::Status.next(), Tab::Networks);
        assert_eq!(Tab::Networks.next(), Tab::Containers);
    }

    #[test]
    fn test_tab_prev() {
        assert_eq!(Tab::Containers.prev(), Tab::Networks);
        assert_eq!(Tab::Routes.prev(), Tab::Containers);
        assert_eq!(Tab::Status.prev(), Tab::Routes);
        assert_eq!(Tab::Networks.prev(), Tab::Status);
    }

    #[test]
    fn test_tab_labels() {
        assert_eq!(Tab::Containers.label(), "Containers");
        assert_eq!(Tab::Routes.label(), "Routes");
        assert_eq!(Tab::Status.label(), "Status");
        assert_eq!(Tab::Networks.label(), "Networks");
    }

    #[test]
    fn test_tab_all() {
        let all = Tab::all();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0], Tab::Containers);
        assert_eq!(all[3], Tab::Networks);
    }

    #[test]
    fn test_centered_rect() {
        let area = Rect::new(0, 0, 100, 50);
        let rect = centered_rect(60, 30, area);
        // The centered rect should be within the area
        assert!(rect.x >= area.x);
        assert!(rect.y >= area.y);
        assert!(rect.right() <= area.right());
        assert!(rect.bottom() <= area.bottom());
    }

    #[test]
    fn test_app_new_empty_config() {
        let docker = Docker::connect_with_local_defaults().unwrap();
        let config = Config::default();
        let app = App::new(docker, config);
        assert_eq!(app.active_tab, Tab::Containers);
        assert!(app.container_list_state.selected().is_none());
        assert!(app.route_list_state.selected().is_none());
        assert!(!app.should_quit);
    }

    #[test]
    fn test_app_new_with_containers() {
        let docker = Docker::connect_with_local_defaults().unwrap();
        let config = Config {
            containers: vec![config::Container {
                name: "test".to_string(),
                label: None,
                port: None,
                network: None,
            }],
            routes: vec![config::Route {
                host_port: 8000,
                target: "test".to_string(),
            }],
            ..Config::default()
        };
        let app = App::new(docker, config);
        assert_eq!(app.container_list_state.selected(), Some(0));
        assert_eq!(app.route_list_state.selected(), Some(0));
    }

    #[test]
    fn test_app_fix_selections_empty() {
        let docker = Docker::connect_with_local_defaults().unwrap();
        let mut app = App::new(docker, Config::default());
        app.container_list_state.select(Some(5));
        app.fix_selections();
        assert!(app.container_list_state.selected().is_none());
    }

    #[test]
    fn test_app_fix_selections_out_of_bounds() {
        let docker = Docker::connect_with_local_defaults().unwrap();
        let config = Config {
            containers: vec![config::Container {
                name: "test".to_string(),
                label: None,
                port: None,
                network: None,
            }],
            ..Config::default()
        };
        let mut app = App::new(docker, config);
        app.container_list_state.select(Some(5));
        app.fix_selections();
        assert_eq!(app.container_list_state.selected(), Some(0));
    }

    #[test]
    fn test_move_selection_down_containers() {
        let docker = Docker::connect_with_local_defaults().unwrap();
        let config = Config {
            containers: vec![
                config::Container {
                    name: "a".to_string(),
                    label: None,
                    port: None,
                    network: None,
                },
                config::Container {
                    name: "b".to_string(),
                    label: None,
                    port: None,
                    network: None,
                },
            ],
            ..Config::default()
        };
        let mut app = App::new(docker, config);
        assert_eq!(app.container_list_state.selected(), Some(0));
        app.move_selection_down();
        assert_eq!(app.container_list_state.selected(), Some(1));
        app.move_selection_down();
        assert_eq!(app.container_list_state.selected(), Some(0)); // wraps
    }

    #[test]
    fn test_move_selection_up_containers() {
        let docker = Docker::connect_with_local_defaults().unwrap();
        let config = Config {
            containers: vec![
                config::Container {
                    name: "a".to_string(),
                    label: None,
                    port: None,
                    network: None,
                },
                config::Container {
                    name: "b".to_string(),
                    label: None,
                    port: None,
                    network: None,
                },
            ],
            ..Config::default()
        };
        let mut app = App::new(docker, config);
        assert_eq!(app.container_list_state.selected(), Some(0));
        app.move_selection_up();
        assert_eq!(app.container_list_state.selected(), Some(1)); // wraps to end
        app.move_selection_up();
        assert_eq!(app.container_list_state.selected(), Some(0));
    }
}
