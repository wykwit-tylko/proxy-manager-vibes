use crate::docker::DockerApi;
use crate::manager::ProxyManager;
use anyhow::Context;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use std::io;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TuiState {
    pub entries: Vec<String>,
    pub selected: usize,
    pub status: String,
}

impl TuiState {
    pub fn new(entries: Vec<String>) -> Self {
        Self {
            entries,
            selected: 0,
            status: "q:quit  r:refresh  s:start  x:stop  e:reload".to_string(),
        }
    }

    pub fn next(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.entries.len();
    }

    pub fn previous(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.entries.len() - 1
        } else {
            self.selected - 1
        };
    }
}

pub fn run_tui<D: DockerApi>(manager: &ProxyManager<D>) -> anyhow::Result<()> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;

    let result = tui_loop(&mut terminal, manager);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}

fn tui_loop<D: DockerApi>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    manager: &ProxyManager<D>,
) -> anyhow::Result<()> {
    let mut state = TuiState::new(build_entries(manager)?);

    loop {
        terminal
            .draw(|f| draw_ui(f, &state))
            .context("failed drawing ui")?;

        if !event::poll(Duration::from_millis(200)).context("event poll failed")? {
            continue;
        }

        if let Event::Key(key) = event::read().context("event read failed")? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => state.next(),
                KeyCode::Up | KeyCode::Char('k') => state.previous(),
                KeyCode::Char('r') => {
                    state.entries = build_entries(manager)?;
                    state.status = "Refreshed".to_string();
                }
                KeyCode::Char('s') => {
                    state.status = manager.start_proxy().unwrap_or_else(|e| e.to_string());
                }
                KeyCode::Char('x') => {
                    state.status = manager.stop_proxy().unwrap_or_else(|e| e.to_string());
                }
                KeyCode::Char('e') => {
                    state.status = manager.reload_proxy().unwrap_or_else(|e| e.to_string());
                }
                _ => {}
            }
        }
    }
}

fn build_entries<D: DockerApi>(manager: &ProxyManager<D>) -> anyhow::Result<Vec<String>> {
    let cfg = manager.load_config()?;
    let routes = cfg.routes;
    let mut out = Vec::new();
    for container in cfg.containers {
        let port = container.port.unwrap_or(crate::config::DEFAULT_PORT);
        let route = routes
            .iter()
            .find(|r| r.target == container.name)
            .map(|r| format!("host {}", r.host_port))
            .unwrap_or_else(|| "unrouted".to_string());
        let label = container.label.unwrap_or_default();
        out.push(format!("{}:{} {} {}", container.name, port, route, label));
    }
    if out.is_empty() {
        out.push("No containers configured".to_string());
    }
    Ok(out)
}

fn draw_ui(frame: &mut Frame, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());

    let items: Vec<ListItem> = state.entries.iter().cloned().map(ListItem::new).collect();
    let list = List::new(items)
        .block(
            Block::default()
                .title("proxy-manager")
                .borders(Borders::ALL),
        )
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    let mut list_state = ratatui::widgets::ListState::default();
    if !state.entries.is_empty() {
        list_state.select(Some(state.selected));
    }

    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    let footer = Paragraph::new(state.status.as_str())
        .block(Block::default().borders(Borders::ALL).title("controls"));
    frame.render_widget(footer, chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::TuiState;

    #[test]
    fn next_wraps_at_end() {
        let mut state = TuiState::new(vec!["a".to_string(), "b".to_string()]);
        state.selected = 1;
        state.next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn previous_wraps_to_last() {
        let mut state = TuiState::new(vec!["a".to_string(), "b".to_string()]);
        state.selected = 0;
        state.previous();
        assert_eq!(state.selected, 1);
    }
}
