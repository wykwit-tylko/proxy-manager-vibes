use crate::app::App;
use crate::config::{Config, DEFAULT_PORT};
use crate::docker::DockerApi;
use crate::store::Store;
use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewModel {
    pub title: String,
    pub containers: Vec<String>,
    pub routes: Vec<String>,
}

pub fn build_view_model(cfg: &Config, proxy_status: Option<&str>) -> ViewModel {
    let title = match proxy_status {
        Some(s) => format!("Proxy: {} ({s})", cfg.proxy_name),
        None => format!("Proxy: {} (not running)", cfg.proxy_name),
    };

    let mut route_map = std::collections::BTreeMap::new();
    for r in &cfg.routes {
        route_map.insert(r.target.clone(), r.host_port);
    }

    let mut containers = Vec::new();
    for c in &cfg.containers {
        let port = c.port.unwrap_or(DEFAULT_PORT);
        let net = c.network.as_deref().unwrap_or(cfg.network.as_str());
        let label = c.label.as_deref().unwrap_or("");
        let marker = route_map
            .get(&c.name)
            .map(|p| format!("  [:{p}]"))
            .unwrap_or_default();
        let label_str = if label.is_empty() {
            "".to_string()
        } else {
            format!(" - {label}")
        };
        containers.push(format!("{}:{port}@{net}{label_str}{marker}", c.name));
    }

    let mut routes = Vec::new();
    if cfg.routes.is_empty() {
        routes.push("(no routes configured)".to_string());
    } else {
        for r in &cfg.routes {
            let internal = cfg.internal_port_for(&r.target);
            routes.push(format!("{} -> {}:{internal}", r.host_port, r.target));
        }
    }

    ViewModel {
        title,
        containers,
        routes,
    }
}

struct TuiState<D: DockerApi> {
    app: App<D>,
    cfg: Config,
    proxy_status: Option<String>,
    selected: usize,
    messages: VecDeque<String>,
    last_refresh: Instant,
}

impl<D: DockerApi> TuiState<D> {
    async fn new(app: App<D>) -> Result<Self> {
        let cfg = app.store.load()?;
        let proxy_status = app.docker.container_status(&cfg.proxy_name).await?;
        Ok(Self {
            app,
            cfg,
            proxy_status,
            selected: 0,
            messages: VecDeque::with_capacity(200),
            last_refresh: Instant::now(),
        })
    }

    async fn refresh(&mut self) -> Result<()> {
        self.cfg = self.app.store.load()?;
        self.proxy_status = self
            .app
            .docker
            .container_status(&self.cfg.proxy_name)
            .await?;
        self.last_refresh = Instant::now();
        if self.selected >= self.cfg.containers.len() {
            self.selected = self.cfg.containers.len().saturating_sub(1);
        }
        Ok(())
    }

    fn push_messages(&mut self, lines: Vec<String>) {
        for l in lines {
            if self.messages.len() == 200 {
                self.messages.pop_front();
            }
            self.messages.push_back(l);
        }
    }
}

pub async fn run<D: DockerApi>(store: Store, docker: D) -> Result<()> {
    let app = App::new(store, docker);
    let mut state = TuiState::new(app).await?;

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let tick = Duration::from_millis(200);
    let refresh_every = Duration::from_secs(2);

    let res = tui_loop(&mut terminal, &mut state, tick, refresh_every).await;

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

async fn tui_loop<D: DockerApi>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut TuiState<D>,
    tick: Duration,
    refresh_every: Duration,
) -> Result<()> {
    loop {
        if state.last_refresh.elapsed() >= refresh_every {
            let _ = state.refresh().await;
        }

        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(6),
                ])
                .split(size);

            draw_header(f, chunks[0], &state.cfg, state.proxy_status.as_deref());
            draw_body(
                f,
                chunks[1],
                &state.cfg,
                state.proxy_status.as_deref(),
                state.selected,
            );
            draw_footer(f, chunks[2], &state.messages);
        })?;

        if event::poll(tick)? && let Event::Key(key) = event::read()? {
            if handle_key(key, state).await? {
                return Ok(());
            }
        }
    }
}

async fn handle_key<D: DockerApi>(key: KeyEvent, state: &mut TuiState<D>) -> Result<bool> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => return Ok(true),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(true),
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            state.selected = state.selected.saturating_sub(1);
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            if !state.cfg.containers.is_empty() {
                state.selected = (state.selected + 1).min(state.cfg.containers.len() - 1);
            }
        }
        (KeyCode::Char('u'), _) => {
            state.refresh().await?;
            state.push_messages(vec!["Refreshed".to_string()]);
        }
        (KeyCode::Char('s'), _) => {
            let out = state.app.start_proxy().await?;
            state.push_messages(out);
            let _ = state.refresh().await;
        }
        (KeyCode::Char('t'), _) => {
            let out = state.app.stop_proxy().await?;
            state.push_messages(out);
            let _ = state.refresh().await;
        }
        (KeyCode::Char('r'), _) => {
            let out = state.app.reload_proxy().await?;
            state.push_messages(out);
            let _ = state.refresh().await;
        }
        _ => {}
    }

    Ok(false)
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, cfg: &Config, proxy_status: Option<&str>) {
    let vm = build_view_model(cfg, proxy_status);
    let help = "q quit | u refresh | s start | t stop | r reload | j/k move";
    let lines = vec![
        Line::from(vec![Span::styled(
            vm.title,
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(Span::styled(help, Style::default().fg(Color::DarkGray))),
    ];

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("proxy-manager"),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

fn draw_body(
    f: &mut ratatui::Frame,
    area: Rect,
    cfg: &Config,
    proxy_status: Option<&str>,
    selected: usize,
) {
    let vm = build_view_model(cfg, proxy_status);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items = vm
        .containers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == selected {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(s.clone(), style)))
        })
        .collect::<Vec<_>>();

    let containers =
        List::new(items).block(Block::default().borders(Borders::ALL).title("Containers"));
    f.render_widget(containers, cols[0]);

    let routes = List::new(vm.routes.into_iter().map(ListItem::new).collect::<Vec<_>>())
        .block(Block::default().borders(Borders::ALL).title("Routes"));
    f.render_widget(routes, cols[1]);
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, messages: &VecDeque<String>) {
    let mut lines = Vec::new();
    for msg in messages.iter().rev().take(5).rev() {
        lines.push(Line::from(msg.as_str()));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no messages)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Activity"))
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ContainerConfig, Route};

    #[test]
    fn view_model_formats_routes_and_title() {
        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "app".to_string(),
                label: Some("Foo".to_string()),
                port: Some(9000),
                network: Some("net".to_string()),
            }],
            routes: vec![Route {
                host_port: 8001,
                target: "app".to_string(),
            }],
            proxy_name: "proxy-manager".to_string(),
            network: "proxy-net".to_string(),
        };

        let vm = build_view_model(&cfg, Some("running"));
        assert!(vm.title.contains("proxy-manager"));
        assert!(vm.title.contains("running"));
        assert!(vm.routes.iter().any(|r| r.contains("8001")));
        assert!(vm.containers.iter().any(|c| c.contains("app:9000")));
    }
}
