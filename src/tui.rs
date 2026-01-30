use crate::config::{load_config, Config};
use crate::docker::DockerManager;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

pub async fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let docker_manager = DockerManager::new()?;
    let app = App::new(docker_manager).await?;
    let res = run_app(&mut terminal, app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

struct App {
    docker_manager: DockerManager,
    config: Config,
    containers_state: ListState,
    routes_state: ListState,
    active_tab: Tab,
}

#[derive(PartialEq)]
enum Tab {
    Containers,
    Routes,
}

impl App {
    async fn new(docker_manager: DockerManager) -> Result<Self> {
        let config = load_config()?;
        let mut containers_state = ListState::default();
        containers_state.select(Some(0));
        let mut routes_state = ListState::default();
        routes_state.select(Some(0));

        Ok(Self {
            docker_manager,
            config,
            containers_state,
            routes_state,
            active_tab: Tab::Containers,
        })
    }

    fn next_container(&mut self) {
        let i = match self.containers_state.selected() {
            Some(i) => {
                if i >= self.config.containers.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.containers_state.select(Some(i));
    }

    fn previous_container(&mut self) {
        let i = match self.containers_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.config.containers.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.containers_state.select(Some(i));
    }

    fn next_route(&mut self) {
        if self.config.routes.is_empty() {
            return;
        }
        let i = match self.routes_state.selected() {
            Some(i) => {
                if i >= self.config.routes.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.routes_state.select(Some(i));
    }

    fn previous_route(&mut self) {
        if self.config.routes.is_empty() {
            return;
        }
        let i = match self.routes_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.config.routes.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.routes_state.select(Some(i));
    }
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Tab => {
                        app.active_tab = if app.active_tab == Tab::Containers {
                            Tab::Routes
                        } else {
                            Tab::Containers
                        };
                    }
                    KeyCode::Down => {
                        if app.active_tab == Tab::Containers {
                            app.next_container();
                        } else {
                            app.next_route();
                        }
                    }
                    KeyCode::Up => {
                        if app.active_tab == Tab::Containers {
                            app.previous_container();
                        } else {
                            app.previous_route();
                        }
                    }
                    KeyCode::Char('s') => {
                        // Start/Switch logic
                        let _ = app.docker_manager.start_proxy(&app.config).await;
                    }
                    KeyCode::Char('x') => {
                        // Stop logic
                        let _ = app.docker_manager.stop_proxy(&app.config.proxy_name).await;
                    }
                    KeyCode::Char('r') => {
                        // Reload logic
                        app.config = load_config().unwrap_or_default();
                    }
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(f.size());

    let title = Paragraph::new("Proxy Manager TUI").block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[1]);

    let containers: Vec<ListItem> = app
        .config
        .containers
        .iter()
        .map(|c| {
            ListItem::new(format!(
                "{} ({})",
                c.name,
                c.label.as_deref().unwrap_or("no label")
            ))
        })
        .collect();

    let containers_list = List::new(containers)
        .block(Block::default().borders(Borders::ALL).title("Containers"))
        .highlight_style(if app.active_tab == Tab::Containers {
            ratatui::style::Style::default()
                .add_modifier(ratatui::style::Modifier::BOLD)
                .fg(ratatui::style::Color::Yellow)
        } else {
            ratatui::style::Style::default()
        })
        .highlight_symbol(">> ");
    f.render_stateful_widget(containers_list, body_chunks[0], &mut app.containers_state);

    let routes: Vec<ListItem> = app
        .config
        .routes
        .iter()
        .map(|r| ListItem::new(format!("Port {} -> {}", r.host_port, r.target)))
        .collect();

    let routes_list = List::new(routes)
        .block(Block::default().borders(Borders::ALL).title("Routes"))
        .highlight_style(if app.active_tab == Tab::Routes {
            ratatui::style::Style::default()
                .add_modifier(ratatui::style::Modifier::BOLD)
                .fg(ratatui::style::Color::Yellow)
        } else {
            ratatui::style::Style::default()
        })
        .highlight_symbol(">> ");
    f.render_stateful_widget(routes_list, body_chunks[1], &mut app.routes_state);

    let help =
        Paragraph::new("Tab: Switch View | Arrows: Navigate | s: Start/Reload | x: Stop | q: Quit")
            .block(Block::default().borders(Borders::ALL));
    f.render_widget(help, chunks[2]);
}
