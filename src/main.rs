mod cli;
mod config;
mod docker;
mod nginx;
mod ops;
mod paths;
mod storage;
mod tui;

use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

use crate::docker::CliDocker;

#[derive(Parser)]
#[command(name = "proxy-manager")]
#[command(about = "Manage Nginx proxy routes for Docker containers", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Start,
    Stop {
        port: Option<u16>,
    },
    Restart,
    Reload,
    List,
    Networks,
    Status,
    Logs {
        #[arg(short = 'f', long = "follow")]
        follow: bool,
        #[arg(short = 'n', long = "tail", default_value_t = 100)]
        tail: usize,
    },
    Add {
        container: String,
        label: Option<String>,
        #[arg(short = 'p', long = "port")]
        port: Option<u16>,
        #[arg(short = 'n', long = "network")]
        network: Option<String>,
    },
    Remove {
        identifier: String,
    },
    Switch {
        identifier: String,
        port: Option<u16>,
    },
    Detect {
        filter: Option<String>,
    },
    Config,
    Install,
    Tui,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let runtime = CliDocker;

    match args.command {
        Some(Command::Start) => {
            let config = storage::load_config()?;
            let ports = ops::start_proxy(&runtime, &config)?;
            println!("Proxy started on port(s): {}", cli::format_ports(&ports));
        }
        Some(Command::Stop { port }) => {
            let mut config = storage::load_config()?;
            if let Some(port) = port {
                let target = ops::find_route_name(&config, port);
                if ops::stop_port(&mut config, port)? {
                    if config.routes.is_empty() {
                        let stopped = ops::stop_proxy(&runtime, &config)?;
                        if stopped {
                            println!("Proxy stopped");
                        } else {
                            println!("Proxy not running");
                        }
                    } else {
                        let ports = ops::reload_proxy(&runtime, &config)?;
                        println!("Proxy reloaded on port(s): {}", cli::format_ports(&ports));
                    }
                    if let Some(target) = target {
                        println!("Removed route: port {} (was {})", port, target);
                    } else {
                        println!("Removed route: port {}", port);
                    }
                } else {
                    println!("Error: No route found for port {}", port);
                }
            } else {
                let stopped = ops::stop_proxy(&runtime, &config)?;
                if stopped {
                    println!("Proxy stopped");
                } else {
                    println!("Proxy not running");
                }
            }
        }
        Some(Command::Restart) => {
            let config = storage::load_config()?;
            let _ = ops::stop_proxy(&runtime, &config)?;
            sleep(Duration::from_secs(1));
            let ports = ops::start_proxy(&runtime, &config)?;
            println!("Proxy started on port(s): {}", cli::format_ports(&ports));
        }
        Some(Command::Reload) => {
            let config = storage::load_config()?;
            let ports = ops::reload_proxy(&runtime, &config)?;
            println!("Proxy reloaded on port(s): {}", cli::format_ports(&ports));
        }
        Some(Command::List) => {
            let config = storage::load_config()?;
            let lines = ops::list_containers(&config);
            if lines.is_empty() {
                println!("No containers configured");
            } else {
                println!("Configured containers:");
                for line in lines {
                    println!("  {}", line);
                }
            }
        }
        Some(Command::Networks) => {
            let networks = ops::list_networks(&runtime)?;
            if networks.is_empty() {
                println!("No Docker networks found");
            } else {
                println!("Available Docker networks:");
                for net in networks {
                    println!(
                        "  {:<25} driver={:<10} containers={:<4} scope={}",
                        net.name, net.driver, net.containers, net.scope
                    );
                }
            }
        }
        Some(Command::Status) => {
            let config = storage::load_config()?;
            let status = ops::build_status_info(&runtime, &config)?;
            println!("{}", cli::format_status(&status));
        }
        Some(Command::Logs { follow, tail }) => {
            let config = storage::load_config()?;
            let lines = ops::proxy_logs(&runtime, &config, tail, follow)?;
            if lines.is_empty() {
                println!("No logs available");
            } else {
                println!("Logs for: {}", ops::proxy_name(&config));
                println!("{}", "-".repeat(50));
                for line in lines {
                    println!("{}", line);
                }
            }
        }
        Some(Command::Add {
            container,
            label,
            port,
            network,
        }) => {
            let mut config = storage::load_config()?;
            let outcome =
                ops::add_container(&runtime, &mut config, &container, label, port, network)?;
            storage::save_config(&config)?;
            if let Some(network) = outcome.detected_network {
                println!("Auto-detected network: {}", network);
            }
            if outcome.updated {
                println!("Updated container: {}", container);
            } else {
                println!("Added container: {}", container);
            }
        }
        Some(Command::Remove { identifier }) => {
            let mut config = storage::load_config()?;
            if let Some(name) = ops::remove_container(&mut config, &identifier) {
                storage::save_config(&config)?;
                println!("Removed container: {}", name);
            } else {
                println!("Error: Container '{}' not found in config", identifier);
            }
        }
        Some(Command::Switch { identifier, port }) => {
            let mut config = storage::load_config()?;
            let container_name = config::find_container(&config, &identifier)
                .map(|c| c.name.clone())
                .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?;
            let result = ops::switch_route(&mut config, &identifier, port)?;
            storage::save_config(&config)?;
            let host_port = port.unwrap_or(paths::DEFAULT_PORT);
            match result {
                ops::SwitchResult::Added => {
                    println!("Adding route: {} -> {}", host_port, container_name);
                }
                ops::SwitchResult::Updated => {
                    println!("Switching route: {} -> {}", host_port, container_name);
                }
            }
            let ports = ops::reload_proxy(&runtime, &config)?;
            println!("Proxy reloaded on port(s): {}", cli::format_ports(&ports));
        }
        Some(Command::Detect { filter }) => {
            let containers = ops::detect_containers(&runtime, filter.as_deref())?;
            println!("Running containers:");
            for name in containers {
                println!("  {}", name);
            }
        }
        Some(Command::Config) => {
            let config = storage::load_config()?;
            let path = paths::config_file();
            println!("Config file: {}", path.display());
            println!();
            let json = serde_json::to_string_pretty(&config)?;
            println!("{}", json);
        }
        Some(Command::Install) => {
            let path = install_cli()?;
            println!("Created hardlink: {}", path.display());
            println!();
            println!("See 'proxy-manager --help' for a quick start guide.");
        }
        Some(Command::Tui) => {
            tui::run_tui(&runtime)?;
        }
        None => {
            let mut cmd = Args::command();
            cmd.print_help()?;
            println!();
        }
    }

    Ok(())
}

fn install_cli() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let target_dir = PathBuf::from(home).join(".local").join("bin");
    std::fs::create_dir_all(&target_dir)?;
    let target = target_dir.join("proxy-manager");
    if target.exists() {
        std::fs::remove_file(&target)?;
    }
    std::fs::hard_link(&exe, &target)?;
    Ok(target)
}
