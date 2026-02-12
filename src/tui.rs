#![cfg(feature = "tui")]

use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};

use crate::config::{Config, View};

pub async fn run_tui() -> Result<()> {
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    terminal.clear()?;

    let res = run_app(&mut terminal).await;

    terminal.show_cursor()?;
    terminal.clear()?;

    res
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut config = Config::load().unwrap_or_default();
    let mut current_view = View::Containers;
    let mut selected_container = 0usize;
    let mut selected_route = 0usize;
    let mut message = String::new();

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(3),
                ])
                .split(f.size());

            let title = match current_view {
                View::Containers => "Containers",
                View::Routes => "Routes",
                View::Status => "Status",
                View::Logs => "Logs",
            };

            let help_text = "[Tab] Switch View | [q] Quit | [a] Add | [d] Delete";

            let header = Paragraph::new(Line::from(vec![
                " Proxy Manager ".into(),
                " | ".into(),
                title.into(),
            ]))
            .style(Style::default().fg(Color::Cyan))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Proxy Manager"),
            );

            f.render_widget(header, chunks[0]);

            match current_view {
                View::Containers => {
                    let items: Vec<ListItem> = config
                        .containers
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            let label = c
                                .label
                                .as_ref()
                                .map(|l| format!(" ({})", l))
                                .unwrap_or_default();

                            let port = c.port.unwrap_or(8000);
                            let net = c.network.as_ref().unwrap_or(&config.network);

                            let content = format!(
                                "{}{} :{}@{}{}",
                                if i == selected_container { "> " } else { "  " },
                                c.name,
                                port,
                                net,
                                label
                            );

                            ListItem::new(content).style(if i == selected_container {
                                Style::default().fg(Color::Yellow)
                            } else {
                                Style::default()
                            })
                        })
                        .collect();

                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title("Containers"))
                        .style(Style::default().fg(Color::White));

                    f.render_widget(list, chunks[1]);
                }
                View::Routes => {
                    let items: Vec<ListItem> = config
                        .routes
                        .iter()
                        .enumerate()
                        .map(|(i, r)| {
                            let content = format!(
                                "{}{} -> {}",
                                if i == selected_route { "> " } else { "  " },
                                r.host_port,
                                r.target
                            );

                            ListItem::new(content).style(if i == selected_route {
                                Style::default().fg(Color::Yellow)
                            } else {
                                Style::default()
                            })
                        })
                        .collect();

                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title("Routes"))
                        .style(Style::default().fg(Color::White));

                    f.render_widget(list, chunks[1]);
                }
                View::Status => {
                    let status_text = format!(
                        "Proxy: {}\nNetwork: {}\nContainers: {}\nRoutes: {}\n",
                        config.proxy_name,
                        config.network,
                        config.containers.len(),
                        config.routes.len()
                    );

                    let status = Paragraph::new(status_text)
                        .block(Block::default().borders(Borders::ALL).title("Status"))
                        .style(Style::default().fg(Color::White));

                    f.render_widget(status, chunks[1]);
                }
                View::Logs => {
                    let logs_text =
                        "Logs view not yet implemented.\nUse 'proxy-manager logs' command.";
                    let logs = Paragraph::new(logs_text)
                        .block(Block::default().borders(Borders::ALL).title("Logs"))
                        .style(Style::default().fg(Color::White));

                    f.render_widget(logs, chunks[1]);
                }
            }

            let msg = Paragraph::new(if message.is_empty() {
                help_text.to_string()
            } else {
                message.clone()
            })
            .style(Style::default().fg(if message.is_empty() {
                Color::DarkGray
            } else {
                Color::Green
            }))
            .block(Block::default().borders(Borders::ALL));

            f.render_widget(msg, chunks[2]);
        })?;

        let event = crossterm::event::read()?;
        if let crossterm::event::Event::Key(key) = event {
            match key.code {
                crossterm::event::KeyCode::Char('q') => {
                    break;
                }
                crossterm::event::KeyCode::Tab => {
                    current_view = match current_view {
                        View::Containers => View::Routes,
                        View::Routes => View::Status,
                        View::Status => View::Logs,
                        View::Logs => View::Containers,
                    };
                    selected_container = 0;
                    selected_route = 0;
                }
                crossterm::event::KeyCode::Down => match current_view {
                    View::Containers => {
                        if selected_container < config.containers.len().saturating_sub(1) {
                            selected_container += 1;
                        }
                    }
                    View::Routes => {
                        if selected_route < config.routes.len().saturating_sub(1) {
                            selected_route += 1;
                        }
                    }
                    _ => {}
                },
                crossterm::event::KeyCode::Up => match current_view {
                    View::Containers => {
                        if selected_container > 0 {
                            selected_container = selected_container.saturating_sub(1);
                        }
                    }
                    View::Routes => {
                        if selected_route > 0 {
                            selected_route = selected_route.saturating_sub(1);
                        }
                    }
                    _ => {}
                },
                crossterm::event::KeyCode::Char('a') => {
                    message = "Use CLI to add containers".to_string();
                }
                crossterm::event::KeyCode::Char('d') => match current_view {
                    View::Containers => {
                        if selected_container < config.containers.len() {
                            let container_name = config.containers[selected_container].name.clone();
                            config.containers.remove(selected_container);
                            config.routes.retain(|r| r.target != container_name);
                            if config.save().is_ok() {
                                message = format!("Removed container: {}", container_name);
                            }
                            selected_container = selected_container.saturating_sub(1);
                        }
                    }
                    View::Routes => {
                        if selected_route < config.routes.len() {
                            config.routes.remove(selected_route);
                            if config.save().is_ok() {
                                message = "Removed route".to_string();
                            }
                            selected_route = selected_route.saturating_sub(1);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
