use clap::{Parser, Subcommand};
use anyhow::Result;

mod config;
mod docker;
mod nginx;
mod proxy;
mod containers;
mod routes;

use config::{ConfigManager, DEFAULT_PORT};
use docker::DockerClient;
use proxy::ProxyManager;
use containers::ContainerManager;
use routes::RouteManager;

#[derive(Parser)]
#[command(
    name = "proxy-manager",
    about = "Manage Nginx proxy to route multiple ports to different docker app containers.",
    long_about = None,
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Start the proxy with all configured routes")]
    Start,

    #[command(about = "Stop the proxy (or stop routing for specific port)")]
    Stop {
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

        #[arg(short = 'n', long, default_value = "100", help = "Number of lines to show")]
        tail: i32,
    },

    #[command(about = "Add or update a container to config")]
    Add {
        #[arg(help = "Docker container name")]
        container: String,

        #[arg(help = "Optional display label")]
        label: Option<String>,

        #[arg(short = 'p', long, help = "Port the container exposes (default: 8000)")]
        port: Option<u16>,

        #[arg(short = 'n', long, help = "Network the container is on (default: auto-detects from container or uses config's network)")]
        network: Option<String>,
    },

    #[command(about = "Remove a container from the config")]
    Remove {
        #[arg(help = "Container name or label to remove")]
        identifier: String,
    },

    #[command(about = "Route a host port to a container")]
    Switch {
        #[arg(help = "Container name or label to route to")]
        identifier: String,

        #[arg(help = "Host port to route (default: 8000)")]
        port: Option<u16>,
    },

    #[command(about = "List all Docker containers (optionally filtered)")]
    Detect {
        #[arg(help = "Filter results by name pattern (case-insensitive)")]
        filter: Option<String>,
    },

    #[command(about = "Launch TUI interface")]
    Tui,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let config_manager = ConfigManager::new()?;
    let docker = DockerClient::new().await?;
    let proxy_manager = ProxyManager::new(config_manager.clone(), docker.clone());
    let container_manager = ContainerManager::new(config_manager.clone(), docker.clone());
    let route_manager = RouteManager::new(config_manager.clone(), proxy_manager.clone());

    match cli.command {
        Commands::Start => {
            proxy_manager.start_proxy().await?;
        }
        Commands::Stop { port } => {
            if let Some(port) = port {
                proxy_manager.stop_port(port).await?;
            } else {
                proxy_manager.stop_proxy().await?;
            }
        }
        Commands::Restart => {
            proxy_manager.stop_proxy().await?;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            proxy_manager.start_proxy().await?;
        }
        Commands::Reload => {
            proxy_manager.reload_proxy().await?;
        }
        Commands::List => {
            container_manager.list_containers()?;
        }
        Commands::Networks => {
            container_manager.list_networks().await?;
        }
        Commands::Status => {
            proxy_manager.status().await?;
        }
        Commands::Install => {
            install_cli()?;
        }
        Commands::Config => {
            show_config(&config_manager)?;
        }
        Commands::Logs { follow, tail } => {
            proxy_manager.show_logs(follow, tail).await?;
        }
        Commands::Add { container, label, port, network } => {
            container_manager.add_container(container, label, port, network).await?;
        }
        Commands::Remove { identifier } => {
            container_manager.remove_container(identifier).await?;
        }
        Commands::Switch { identifier, port } => {
            route_manager.switch_target(identifier, port).await?;
        }
        Commands::Detect { filter } => {
            let containers = container_manager.detect_containers(filter).await?;
            println!("Running containers:");
            for c in containers {
                println!("  {}", c);
            }
        }
        Commands::Tui => {
            run_tui().await?;
        }
    }

    Ok(())
}

fn install_cli() -> Result<()> {
    use std::os::unix::fs::hard_link;

    let script_path = std::env::current_exe()?;
    let user_bin = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
        .join(".local")
        .join("bin");
    let hardlink = user_bin.join("proxy-manager");

    std::fs::create_dir_all(&user_bin)?;

    if hardlink.exists() {
        std::fs::remove_file(&hardlink)?;
    }

    hard_link(&script_path, &hardlink)?;
    println!("Created hardlink: {} -> {}", hardlink.display(), script_path.display());
    println!();
    println!("See 'proxy-manager --help' for a quick start guide.");

    let paths = std::env::var("PATH").unwrap_or_default();
    if !paths.split(':').any(|p| p == user_bin.to_str().unwrap_or("")) {
        println!("NOTE: Add {} to your PATH:", user_bin.display());
        println!("  export PATH=\"{}:$PATH\"", user_bin.display());
        println!("  # Add to ~/.bashrc or ~/.zshrc to persist");
    }

    Ok(())
}

fn show_config(config_manager: &ConfigManager) -> Result<()> {
    let config = config_manager.load()?;
    println!("Config file: {}", config_manager.config_file_path().display());
    println!();
    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

async fn run_tui() -> Result<()> {
    println!("TUI mode is not yet implemented. Use CLI commands instead.");
    Ok(())
}
