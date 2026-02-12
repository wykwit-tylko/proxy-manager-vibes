use clap::{Parser, Subcommand};
use std::time::Duration;
use tokio::time::sleep;

use crate::config::{Config, Container, Route, DEFAULT_PORT};
use crate::docker::{generate_nginx_config, DockerClient};

#[derive(Parser)]
#[command(name = "proxy-manager")]
#[command(
    about = "Manage Nginx proxy to route multiple ports to different docker app containers.",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "Start the proxy with all configured routes")]
    Start,
    #[command(about = "Stop the proxy (or stop routing for specific port)")]
    Stop {
        #[arg(index = 1, help = "Optional: Stop routing for specific port")]
        port: Option<u16>,
    },
    #[command(about = "Stop and start the proxy")]
    Restart,
    #[command(about = "Apply config changes by rebuilding proxy")]
    Reload,
    #[command(about = "List all configured containers with settings")]
    List,
    #[command(about = "List all Docker networks with container counts")]
    Networks,
    #[command(about = "Show proxy status and all active routes")]
    Status,
    #[command(about = "Create hardlink in ~/.local/bin for global access")]
    Install,
    #[command(about = "Show config file path and contents")]
    Config,
    #[command(about = "Show Nginx proxy container logs")]
    Logs {
        #[arg(short, long, help = "Follow log output (like tail -f)")]
        follow: bool,
        #[arg(short, long, default_value = "100", help = "Number of lines to show")]
        tail: usize,
    },
    #[command(about = "Add or update a container to config")]
    Add {
        #[arg(index = 1, help = "Docker container name")]
        container: String,
        #[arg(index = 2, help = "Optional display label")]
        label: Option<String>,
        #[arg(short, long, help = "Port the container exposes")]
        port: Option<u16>,
        #[arg(short, long, help = "Network the container is on")]
        network: Option<String>,
    },
    #[command(about = "Remove a container from the config")]
    Remove {
        #[arg(index = 1, help = "Container name or label to remove")]
        identifier: String,
    },
    #[command(about = "Route a host port to a container")]
    Switch {
        #[arg(index = 1, help = "Container name or label to route to")]
        identifier: String,
        #[arg(index = 2, help = "Host port to route (default: 8000)")]
        port: Option<u16>,
    },
    #[command(about = "List all Docker containers (optionally filtered)")]
    Detect {
        #[arg(index = 1, help = "Filter results by name pattern (case-insensitive)")]
        filter: Option<String>,
    },
    #[command(about = "Start the interactive TUI")]
    Tui,
}

pub async fn run_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start => start_proxy().await,
        Commands::Stop { port } => stop_proxy(port).await,
        Commands::Restart => restart_proxy().await,
        Commands::Reload => reload_proxy().await,
        Commands::List => list_containers(),
        Commands::Networks => list_networks().await,
        Commands::Status => status().await,
        Commands::Install => install_cli(),
        Commands::Config => show_config(),
        Commands::Logs { follow, tail } => show_logs(follow, tail).await,
        Commands::Add {
            container,
            label,
            port,
            network,
        } => add_container(container, label, port, network).await,
        Commands::Remove { identifier } => remove_container(identifier),
        Commands::Switch { identifier, port } => switch_target(identifier, port).await,
        Commands::Detect { filter } => detect_containers(filter).await,
        Commands::Tui => {
            #[cfg(feature = "tui")]
            {
                crate::tui::run_tui().await
            }
            #[cfg(not(feature = "tui"))]
            {
                Err(anyhow::anyhow!(
                    "TUI support not compiled. Enable 'tui' feature."
                ))
            }
        }
    }
}

async fn start_proxy() -> anyhow::Result<()> {
    let config = Config::load()?;

    if config.containers.is_empty() {
        return Err(anyhow::anyhow!(
            "Error: No containers configured. Use 'add' command first."
        ));
    }

    if config.routes.is_empty() {
        return Err(anyhow::anyhow!(
            "Error: No routes configured. Use 'switch' command first."
        ));
    }

    let docker = DockerClient::new()?;

    let mut networks = std::collections::HashSet::new();
    networks.insert(config.network.clone());
    for c in &config.containers {
        if let Some(ref network) = c.network {
            networks.insert(network.clone());
        }
    }

    for network in networks {
        docker.ensure_network(&network).await?;
    }

    let nginx_conf = generate_nginx_config(&config);

    println!("Building proxy image...");
    docker.build_proxy_image(&config, &nginx_conf).await?;

    println!("Starting proxy: {}", config.proxy_name);
    docker.start_proxy(&config).await?;

    let port_str = config
        .get_all_host_ports()
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    println!("Proxy started on port(s): {}", port_str);

    Ok(())
}

async fn stop_proxy(port: Option<u16>) -> anyhow::Result<()> {
    let mut config = Config::load()?;

    if let Some(host_port) = port {
        let route = config.find_route(host_port);
        if route.is_none() {
            return Err(anyhow::anyhow!(
                "Error: No route found for port {}",
                host_port
            ));
        }

        config.routes.retain(|r| r.host_port != host_port);
        config.save()?;

        println!("Removed route: port {}", host_port);

        if config.routes.is_empty() {
            let docker = DockerClient::new()?;
            docker.stop_proxy(&config.proxy_name).await?;
        } else {
            reload_proxy().await?;
        }
    } else {
        let docker = DockerClient::new()?;
        docker.stop_proxy(&config.proxy_name).await?;
    }

    Ok(())
}

async fn restart_proxy() -> anyhow::Result<()> {
    let docker = DockerClient::new()?;
    let config = Config::load()?;
    docker.stop_proxy(&config.proxy_name).await?;
    sleep(Duration::from_secs(1)).await;
    start_proxy().await
}

async fn reload_proxy() -> anyhow::Result<()> {
    let config = Config::load()?;

    if config.containers.is_empty() {
        return Err(anyhow::anyhow!("Error: No containers configured."));
    }

    if config.routes.is_empty() {
        return Err(anyhow::anyhow!("Error: No routes configured."));
    }

    let docker = DockerClient::new()?;
    docker.stop_proxy(&config.proxy_name).await?;
    sleep(Duration::from_secs(1)).await;

    let docker = DockerClient::new()?;

    let mut networks = std::collections::HashSet::new();
    networks.insert(config.network.clone());
    for c in &config.containers {
        if let Some(ref network) = c.network {
            networks.insert(network.clone());
        }
    }

    for network in networks {
        docker.ensure_network(&network).await?;
    }

    let nginx_conf = generate_nginx_config(&config);
    docker.build_proxy_image(&config, &nginx_conf).await?;
    docker.start_proxy(&config).await?;

    println!("Proxy reloaded");
    Ok(())
}

fn list_containers() -> anyhow::Result<()> {
    let config = Config::load()?;
    let route_map: std::collections::HashMap<_, _> = config
        .routes
        .iter()
        .map(|r| (&r.target, r.host_port))
        .collect();

    if config.containers.is_empty() {
        println!("No containers configured");
        return Ok(());
    }

    println!("Configured containers:");
    for c in &config.containers {
        let marker = route_map
            .get(&c.name)
            .map(|p| format!(" (port {})", p))
            .unwrap_or_default();
        let label = c
            .label
            .as_ref()
            .map(|l| format!(" - {}", l))
            .unwrap_or_default();
        let port = c.port.unwrap_or(DEFAULT_PORT);
        let net = c.network.as_ref().unwrap_or(&config.network).clone();
        println!("  {}:{}@{}{}{}", c.name, port, net, label, marker);
    }

    Ok(())
}

async fn list_networks() -> anyhow::Result<()> {
    let docker = DockerClient::new()?;
    println!("Available Docker networks:");
    let networks = docker.list_networks().await?;
    for net in networks {
        println!(
            "  {:<25} driver={:<10} containers={:<4} scope={}",
            net.name, net.driver, net.containers, net.scope
        );
    }
    Ok(())
}

async fn status() -> anyhow::Result<()> {
    let config = Config::load()?;
    let docker = DockerClient::new()?;

    let is_running = docker.is_proxy_running(&config.proxy_name).await;

    if is_running {
        println!("Proxy: {} (running)", config.proxy_name);
    } else {
        println!("Proxy: {} (stopped)", config.proxy_name);
    }

    println!();
    println!("Active routes:");
    for route in &config.routes {
        let target_container = config.find_container(&route.target);
        if let Some(container) = target_container {
            let internal_port = config.get_internal_port(container);
            println!(
                "  {} -> {}:{}",
                route.host_port, route.target, internal_port
            );
        } else {
            println!(
                "  {} -> {} (container not found)",
                route.host_port, route.target
            );
        }
    }

    Ok(())
}

fn install_cli() -> anyhow::Result<()> {
    let script_path = std::env::current_exe()?;
    let user_bin = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
        .join(".local")
        .join("bin");

    std::fs::create_dir_all(&user_bin)?;

    let hardlink = user_bin.join("proxy-manager");

    if hardlink.exists() || hardlink.is_symlink() {
        std::fs::remove_file(&hardlink)?;
    }

    std::fs::hard_link(&script_path, &hardlink)?;
    println!(
        "Created hardlink: {} -> {}",
        hardlink.display(),
        script_path.display()
    );
    println!();
    println!("See 'proxy-manager --help' for a quick start guide.");

    let path_str = user_bin.to_string_lossy().to_string();
    if !std::env::var("PATH")
        .unwrap_or_default()
        .contains(&path_str)
    {
        println!("NOTE: Add ~/.local/bin to your PATH:");
        println!("  export PATH=\"{}:$PATH\"", path_str);
        println!("  # Add to ~/.bashrc or ~/.zshrc to persist");
    }

    Ok(())
}

fn show_config() -> anyhow::Result<()> {
    let config_file = Config::config_file()?;
    println!("Config file: {}", config_file.display());
    println!();

    if config_file.exists() {
        let content = std::fs::read_to_string(&config_file)?;
        println!("{}", content);
    } else {
        println!("Config file does not exist yet. Add containers to create it.");
    }

    Ok(())
}

async fn show_logs(follow: bool, tail: usize) -> anyhow::Result<()> {
    let config = Config::load()?;
    let docker = DockerClient::new()?;

    if !docker.is_proxy_running(&config.proxy_name).await {
        return Err(anyhow::anyhow!(
            "Proxy container '{}' is not running",
            config.proxy_name
        ));
    }

    println!("Logs for: {}", config.proxy_name);
    println!("{}", "-".repeat(50));

    docker.logs(&config.proxy_name, follow, tail).await
}

async fn add_container(
    container_name: String,
    label: Option<String>,
    port: Option<u16>,
    network: Option<String>,
) -> anyhow::Result<()> {
    let mut config = Config::load()?;
    let docker = DockerClient::new()?;

    let network = if let Some(net) = network {
        Some(net)
    } else if let Ok(Some(detected_network)) = docker.get_container_network(&container_name).await {
        println!("Auto-detected network: {}", detected_network);
        Some(detected_network)
    } else {
        None
    };

    if let Some(existing) = config.find_container_mut(&container_name) {
        if label.is_some() {
            existing.label = label;
        }
        if port.is_some() {
            existing.port = port;
        }
        if network.is_some() {
            existing.network = network;
        }
        config.save()?;
        println!("Updated container: {}", container_name);
    } else {
        let container = Container {
            name: container_name.clone(),
            label,
            port,
            network,
        };
        config.containers.push(container);
        config.save()?;
        println!("Added container: {}", container_name);
    }

    Ok(())
}

fn remove_container(identifier: String) -> anyhow::Result<()> {
    let mut config = Config::load()?;

    let container = config
        .find_container(&identifier)
        .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?;

    let container_name = container.name.clone();
    config.containers.retain(|c| c.name != container_name);
    config.routes.retain(|r| r.target != container_name);
    config.save()?;

    println!("Removed container: {}", container_name);
    Ok(())
}

async fn switch_target(identifier: String, port: Option<u16>) -> anyhow::Result<()> {
    let mut config = Config::load()?;

    let container = config
        .find_container(&identifier)
        .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?;

    let container_name = container.name.clone();
    let host_port = port.unwrap_or(DEFAULT_PORT);

    if let Some(existing_route) = config.find_route_mut(host_port) {
        existing_route.target = container_name.clone();
        println!("Switching route: {} -> {}", host_port, container_name);
    } else {
        config.routes.push(Route {
            host_port,
            target: container_name.clone(),
        });
        config.routes.sort_by_key(|r| r.host_port);
        println!("Adding route: {} -> {}", host_port, container_name);
    }

    config.save()?;
    reload_proxy().await
}

async fn detect_containers(filter: Option<String>) -> anyhow::Result<()> {
    let docker = DockerClient::new()?;
    let containers = docker.list_containers(filter.as_deref()).await?;
    println!("Running containers:");
    for c in containers {
        println!("  {}", c);
    }
    Ok(())
}
