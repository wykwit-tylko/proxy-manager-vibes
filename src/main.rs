mod config;
mod docker;
mod nginx;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::{load_config, save_config, ContainerConfig, RouteConfig};
use docker::DockerManager;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Parser)]
#[command(name = "proxy-manager")]
#[command(about = "Manage Nginx proxy to route multiple ports to different docker app containers.", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy with all configured routes
    Start,
    /// Stop the proxy (or stop routing for specific port)
    Stop {
        /// Optional: Stop routing for specific port
        port: Option<u16>,
    },
    /// Stop and start the proxy
    Restart,
    /// Apply config changes by rebuilding proxy
    Reload,
    /// List all configured containers with settings
    List,
    /// List all Docker networks with container counts
    Networks,
    /// Show proxy status and all active routes
    Status,
    /// Show config file path and contents
    Config,
    /// Show Nginx proxy container logs
    Logs {
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show (default: 100)
        #[arg(short, long, default_value_t = 100)]
        tail: usize,
    },
    /// Create hardlink in ~/.local/bin for global access
    Install,
    /// Add or update a container to config
    Add {
        /// Docker container name
        container: String,
        /// Optional display label
        label: Option<String>,
        /// Port the container exposes
        #[arg(short, long)]
        port: Option<u16>,
        /// Network the container is on
        #[arg(short, long)]
        network: Option<String>,
    },
    /// Remove a container from the config
    Remove {
        /// Container name or label to remove
        identifier: String,
    },
    /// Route a host port to a container
    Switch {
        /// Container name or label to route to
        identifier: String,
        /// Host port to route (default: 8000)
        port: Option<u16>,
    },
    /// List all Docker containers (optionally filtered)
    Detect {
        /// Filter results by name pattern (case-insensitive)
        filter: Option<String>,
    },
    /// Open the TUI
    Tui,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let docker_manager = DockerManager::new()?;

    match cli.command {
        Commands::Start => {
            let config = load_config()?;
            docker_manager.start_proxy(&config).await?;
        }
        Commands::Stop { port } => {
            let mut config = load_config()?;
            if let Some(p) = port {
                config.routes.retain(|r| r.host_port != p);
                save_config(&config)?;
                println!("Removed route: port {}", p);
                if config.routes.is_empty() {
                    docker_manager.stop_proxy(&config.proxy_name).await?;
                } else {
                    docker_manager.stop_proxy(&config.proxy_name).await?;
                    sleep(Duration::from_secs(1)).await;
                    docker_manager.start_proxy(&config).await?;
                }
            } else {
                let config = load_config()?;
                docker_manager.stop_proxy(&config.proxy_name).await?;
            }
        }
        Commands::Restart => {
            let config = load_config()?;
            docker_manager.stop_proxy(&config.proxy_name).await?;
            sleep(Duration::from_secs(1)).await;
            docker_manager.start_proxy(&config).await?;
        }
        Commands::Reload => {
            let config = load_config()?;
            docker_manager.stop_proxy(&config.proxy_name).await?;
            sleep(Duration::from_secs(1)).await;
            docker_manager.start_proxy(&config).await?;
        }
        Commands::List => {
            let config = load_config()?;
            if config.containers.is_empty() {
                println!("No containers configured");
            } else {
                println!("Configured containers:");
                for c in &config.containers {
                    let route = config.routes.iter().find(|r| r.target == c.name);
                    let marker = if let Some(r) = route {
                        format!(" (port {})", r.host_port)
                    } else {
                        "".to_string()
                    };
                    let label = c
                        .label
                        .as_ref()
                        .map(|l| format!(" - {}", l))
                        .unwrap_or_default();
                    let port = c.port.unwrap_or(8000);
                    let net = c.network.as_deref().unwrap_or(&config.network);
                    println!("  {}:{}@{}{}{}", c.name, port, net, label, marker);
                }
            }
        }
        Commands::Networks => {
            let networks = docker_manager.list_networks().await?;
            println!("Available Docker networks:");
            for net in networks {
                let name = net.name.unwrap_or_else(|| "unknown".to_string());
                let driver = net.driver.unwrap_or_else(|| "unknown".to_string());
                let containers_count = net.containers.map(|c| c.len()).unwrap_or(0);
                let scope = net.scope.unwrap_or_else(|| "local".to_string());
                println!(
                    "  {:<25} driver={:<10} containers={:<4} scope={}",
                    name, driver, containers_count, scope
                );
            }
        }
        Commands::Status => {
            let config = load_config()?;
            println!("Proxy: {}", config.proxy_name);
            println!();
            println!("Active routes:");
            for route in &config.routes {
                let target_container = config.containers.iter().find(|c| c.name == route.target);
                if let Some(c) = target_container {
                    let port = c.port.unwrap_or(8000);
                    println!("  {} -> {}:{}", route.host_port, c.name, port);
                } else {
                    println!(
                        "  {} -> {} (container not found)",
                        route.host_port, route.target
                    );
                }
            }
        }
        Commands::Config => {
            let config = load_config()?;
            println!("Config file: {:?}", config::get_config_file());
            println!();
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        Commands::Logs { follow, tail } => {
            let config = load_config()?;
            docker_manager
                .get_container_logs(&config.proxy_name, tail, follow)
                .await?;
        }
        Commands::Install => {
            let exe_path = std::env::current_exe()?;
            let home = std::env::var("HOME")?;
            let user_bin = std::path::PathBuf::from(home).join(".local").join("bin");
            let target_path = user_bin.join("proxy-manager");

            std::fs::create_dir_all(&user_bin)?;

            if target_path.exists() {
                std::fs::remove_file(&target_path)?;
            }

            std::fs::hard_link(&exe_path, &target_path)?;
            println!("Created hardlink: {:?} -> {:?}", target_path, exe_path);
            println!();
            println!("See 'proxy-manager --help' for a quick start guide.");

            let path_env = std::env::var("PATH").unwrap_or_default();
            if !path_env.contains(user_bin.to_str().unwrap_or_default()) {
                println!("NOTE: Add ~/.local/bin to your PATH:");
                println!("  export PATH=\"{:?}:$PATH\"", user_bin);
                println!("  # Add to ~/.bashrc or ~/.zshrc to persist");
            }
        }
        Commands::Add {
            container,
            label,
            port,
            network,
        } => {
            let mut config = load_config()?;
            let mut network = network;
            if network.is_none() {
                network = docker_manager.get_container_network(&container).await?;
                if let Some(ref net) = network {
                    println!("Auto-detected network: {}", net);
                }
            }

            if let Some(existing) = config.containers.iter_mut().find(|c| c.name == container) {
                if label.is_some() {
                    existing.label = label;
                }
                if port.is_some() {
                    existing.port = port;
                }
                if network.is_some() {
                    existing.network = network;
                }
                println!("Updated container: {}", container);
            } else {
                config.containers.push(ContainerConfig {
                    name: container.clone(),
                    label,
                    port,
                    network,
                });
                println!("Added container: {}", container);
            }
            save_config(&config)?;
        }
        Commands::Remove { identifier } => {
            let mut config = load_config()?;
            let container_name = config
                .containers
                .iter()
                .find(|c| c.name == identifier || c.label.as_deref() == Some(&identifier))
                .map(|c| c.name.clone());

            if let Some(name) = container_name {
                config.containers.retain(|c| c.name != name);
                config.routes.retain(|r| r.target != name);
                save_config(&config)?;
                println!("Removed container: {}", name);
            } else {
                println!("Error: Container '{}' not found in config", identifier);
            }
        }
        Commands::Switch { identifier, port } => {
            let mut config = load_config()?;
            let container_name = config
                .containers
                .iter()
                .find(|c| c.name == identifier || c.label.as_deref() == Some(&identifier))
                .map(|c| c.name.clone());

            if let Some(name) = container_name {
                let host_port = port.unwrap_or(8000);
                if let Some(route) = config.routes.iter_mut().find(|r| r.host_port == host_port) {
                    route.target = name.clone();
                    println!("Switching route: {} -> {}", host_port, name);
                } else {
                    config.routes.push(RouteConfig {
                        host_port,
                        target: name.clone(),
                    });
                    config.routes.sort_by_key(|r| r.host_port);
                    println!("Adding route: {} -> {}", host_port, name);
                }
                save_config(&config)?;

                // Reload proxy
                docker_manager.stop_proxy(&config.proxy_name).await?;
                sleep(Duration::from_secs(1)).await;
                docker_manager.start_proxy(&config).await?;
            } else {
                println!("Error: Container '{}' not found in config", identifier);
            }
        }
        Commands::Detect { filter } => {
            let containers = docker_manager.list_containers(filter.as_deref()).await?;
            println!("Running containers:");
            for c in containers {
                println!("  {}", c);
            }
        }
        Commands::Tui => {
            tui::run_tui().await?;
        }
    }

    Ok(())
}
