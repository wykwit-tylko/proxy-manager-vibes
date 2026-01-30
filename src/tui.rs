use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute::command,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::{Config, ConfigManager};
use crate::containers::ContainerManager;
use crate::docker::DockerClient;
use crate::proxy::ProxyManager;
use crate::routes::RouteManager;

#[derive(Clone, PartialEq)]
enum MenuItem {
    Status,
    Containers,
    Routes,
    AddContainer,
    RemoveContainer,
    SwitchRoute,
    StartProxy,
    StopProxy,
    Logs,
    Quit,
}

impl MenuItem {
    fn to_string(&self) -> &str {
        match self {
            MenuItem::Status => "Status",
            MenuItem::Containers => "Containers",
            MenuItem::Routes => "Routes",
            MenuItem::AddContainer => "Add Container",
            MenuItem::RemoveContainer => "Remove Container",
            MenuItem::SwitchRoute => "Switch Route",
            MenuItem::StartProxy => "Start Proxy",
            MenuItem::StopProxy => "Stop Proxy",
            MenuItem::Logs => "Logs",
            MenuItem::Quit => "Quit",
        }
    }
}

struct App {
    menu_items: Vec<MenuItem>,
    selected_menu: usize,
    config: Config,
}

struct TuiState {
    app: App,
    input_mode: InputMode,
}

#[derive(PartialEq, Clone)]
enum InputMode {
    Menu,
    TextInput(String),
    Confirm,
}

impl TuiState {
    fn new(config: Config) -> Self {
        Self {
            app: App {
                menu_items: vec![
                    MenuItem::Status,
                    MenuItem::Containers,
                    MenuItem::Routes,
                    MenuItem::AddContainer,
                    MenuItem::RemoveContainer,
                    MenuItem::SwitchRoute,
                    MenuItem::StartProxy,
                    MenuItem::StopProxy,
                    MenuItem::Logs,
                    MenuItem::Quit,
                ],
                selected_menu: 0,
                config,
            },
            input_mode: InputMode::Menu,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let config_manager = ConfigManager::new()?;
    let config = config_manager.load()?;
    let docker = DockerClient::new().await?;

    let proxy_manager = Arc::new(Mutex::new(ProxyManager::new(
        config_manager.clone(),
        docker.clone(),
    )));
    let container_manager = Arc::new(ContainerManager::new(config_manager.clone(), docker));
    let route_manager = RouteManager::new(config_manager.clone(), {
        let pm = proxy_manager.lock().await;
        pm.clone()
    });

    let mut state = TuiState::new(config);

    let result = run_app(
        &mut terminal,
        &mut state,
        container_manager,
        route_manager,
    )
    .await;

    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut TuiState,
    container_manager: ContainerManager,
    route_manager: RouteManager,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, state))?;

        if let Event::Key(key) = event::read()? {
            match &mut state.input_mode {
                InputMode::Menu => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Up | KeyCode::Char('k') => {
                        if state.app.selected_menu > 0 {
                            state.app.selected_menu -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if state.app.selected_menu < state.app.menu_items.len() - 1 {
                            state.app.selected_menu += 1;
                        }
                    }
                    KeyCode::Enter => {
                        match state.app.menu_items[state.app.selected_menu] {
                            MenuItem::Status => {
                                state.app.config = container_manager.config_manager.load()?;
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::Containers => {
                                state.app.config = container_manager.config_manager.load()?;
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::Routes => {
                                state.app.config = container_manager.config_manager.load()?;
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::AddContainer => {
                                state.input_mode = InputMode::TextInput(String::new());
                            }
                            MenuItem::RemoveContainer => {
                                state.app.config = container_manager.config_manager.load()?;
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::SwitchRoute => {
                                state.app.config = container_manager.config_manager.load()?;
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::StartProxy => {
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::StopProxy => {
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::Logs => {
                                state.app.config = container_manager.config_manager.load()?;
                                state.input_mode = InputMode::Confirm;
                            }
                            MenuItem::Quit => return Ok(()),
                        }
                    }
                    _ => {}
                },
                InputMode::TextInput(ref input) => match key.code {
                    KeyCode::Enter => {
                        if !input.is_empty() {
                            if let Err(e) = container_manager
                                .add_container(input.clone(), None, None, None)
                                .await
                            {
                                eprintln!("Error adding container: {}", e);
                            }
                            state.input_mode = InputMode::Menu;
                            state.app.selected_menu = 0;
                        }
                    }
                    KeyCode::Char(c) => {
                        if c != '\n' && c != '\r' {
                            state.input_mode = InputMode::TextInput(format!("{}{}", input, c));
                        }
                    }
                    KeyCode::Backspace => {
                        if !input.is_empty() {
                            let new_input = input.chars().take(input.len().saturating_sub(1)).collect();
                            state.input_mode = InputMode::TextInput(new_input);
                        }
                    }
                    KeyCode::Esc => {
                        state.input_mode = InputMode::Menu;
                        state.app.selected_menu = 0;
                    }
                    _ => {}
                },
                InputMode::Confirm => match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                        match state.app.menu_items[state.app.selected_menu] {
                            MenuItem::Status => {
                                let _ = show_status(&state.app.config);
                            }
                            MenuItem::Containers => {
                                let _ = container_manager.list_containers();
                            }
                            MenuItem::Routes => {
                                show_routes(&state.app.config);
                            }
                            MenuItem::StartProxy => {
                                let pm = ProxyManager::new(
                                    container_manager.config_manager.clone(),
                                    container_manager.docker.clone(),
                                );
                                let _ = pm.start_proxy().await;
                            }
                            MenuItem::StopProxy => {
                                let pm = ProxyManager::new(
                                    container_manager.config_manager.clone(),
                                    container_manager.docker.clone(),
                                );
                                let _ = pm.stop_proxy().await;
                            }
                            MenuItem::Logs => {
                                let pm = ProxyManager::new(
                                    container_manager.config_manager.clone(),
                                    container_manager.docker.clone(),
                                );
                                let _ = pm.show_logs(true, 100).await;
                            }
                            _ => {}
                        }
                        state.input_mode = InputMode::Menu;
                        state.app.selected_menu = 0;
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        state.input_mode = InputMode::Menu;
                        state.app.selected_menu = 0;
                    }
                    KeyCode::Esc => {
                        state.input_mode = InputMode::Menu;
                        state.app.selected_menu = 0;
                    }
                    _ => {}
                },
            }
        }
    }
}

fn ui(f: &mut Frame<CrosstermBackend<std::io::Stdout>>, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(1),
            ]
            .as_ref(),
        )
        .split(f.area());

    match &state.input_mode {
        InputMode::Menu => {
            let title = Paragraph::new("Proxy Manager TUI")
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);

            f.render_widget(title, chunks[0]);

            let menu_items: Vec<ListItem> = state
                .app
                .menu_items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let style = if i == state.app.selected_menu {
                        Style::default().bg(Color::DarkGray).fg(Color::White)
                    } else {
                        Style::default()
                    };
                    ListItem::new(item.to_string()).style(style)
                })
                .collect();

            let list = List::new(menu_items)
                .block(Block::default().borders(Borders::ALL).title("Menu"))
                .style(Style::default().fg(Color::White));

            f.render_stateful_widget(list, chunks[1], &mut ListState::default());
        }
        InputMode::TextInput(ref input) => {
            let title = Paragraph::new(format!("Enter container name: {}", input))
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);

            f.render_widget(title, chunks[0]);

            let hint = Paragraph::new("Press Enter to add, Esc to cancel")
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center);

            f.render_widget(hint, chunks[1]);
        }
        InputMode::Confirm => {
            let title = Paragraph::new("Confirm action (y/n)?")
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);

            f.render_widget(title, chunks[0]);

            let hint = Paragraph::new("Press y to confirm, n to cancel")
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center);

            f.render_widget(hint, chunks[1]);
        }
    }
}

fn show_status(config: &Config) {
    println!("\n=== Proxy Status ===");
    println!("Containers: {}", config.containers.len());
    println!("Routes: {}", config.routes.len());
    println!("Proxy Name: {}", config.proxy_name);
    println!("Network: {}", config.network);
    println!();
}

fn show_routes(config: &Config) {
    println!("\n=== Active Routes ===");
    if config.routes.is_empty() {
        println!("No routes configured");
    } else {
        for route in &config.routes {
            println!("Port {} -> {}", route.host_port, route.target);
        }
    }
    println!();
}
