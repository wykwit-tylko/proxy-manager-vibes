use anyhow::Result;
use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::docker::DockerRuntime;
use crate::ops;
use crate::storage;

pub fn run_tui(runtime: &impl DockerRuntime) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = tui_loop(runtime, &mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn tui_loop(
    runtime: &impl DockerRuntime,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(250);

    loop {
        let config = storage::load_config()?;
        let status = ops::build_status_info(runtime, &config)?;
        let containers = ops::list_containers(&config);

        terminal.draw(|frame| {
            let size = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(size);

            let title = Paragraph::new("proxy-manager TUI")
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().add_modifier(Modifier::BOLD));
            frame.render_widget(title, chunks[0]);

            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);

            let status_text = crate::cli::format_status(&status);
            let status_block = Paragraph::new(status_text)
                .block(Block::default().title("Status").borders(Borders::ALL));
            frame.render_widget(status_block, body_chunks[0]);

            let items: Vec<ListItem> = if containers.is_empty() {
                vec![ListItem::new("No containers configured")]
            } else {
                containers.into_iter().map(ListItem::new).collect()
            };
            let list =
                List::new(items).block(Block::default().title("Containers").borders(Borders::ALL));
            frame.render_widget(list, body_chunks[1]);

            let help = Paragraph::new("q: quit").block(Block::default().borders(Borders::ALL));
            frame.render_widget(help, chunks[2]);
        })?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && let KeyCode::Char('q') = key.code
        {
            return Ok(());
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}
