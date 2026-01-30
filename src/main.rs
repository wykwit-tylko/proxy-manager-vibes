use anyhow::Result;
use clap::{Parser, Subcommand};
use proxy_manager::{cli, docker, proxy};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "proxy-manager")]
#[command(author = "Proxy Manager Team")]
#[command(version = VERSION)]
#[command(about = "Manage Nginx proxy to route multiple ports to different docker app containers", long_about = None)]
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
        #[arg(short, long, help = "Stop routing for specific port")]
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
        #[arg(help = "Docker container name")]
        container: String,

        #[arg(help = "Optional display label")]
        label: Option<String>,

        #[arg(short, long, help = "Port the container exposes (default: 8000)")]
        port: Option<u16>,

        #[arg(short, long, help = "Network the container is on")]
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

    #[command(about = "Launch the TUI interface")]
    Tui,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    match args.command {
        Commands::Start => {
            let _ = proxy::start_proxy().await?;
        }
        Commands::Stop { port } => {
            if let Some(p) = port {
                let _ = proxy::stop_port(p).await?;
            } else {
                let _ = proxy::stop_proxy().await?;
            }
        }
        Commands::Restart => {
            let _ = proxy::stop_proxy().await?;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let _ = proxy::start_proxy().await?;
        }
        Commands::Reload => {
            let _ = proxy::reload_proxy().await?;
        }
        Commands::List => cli::list_containers(),
        Commands::Networks => {
            let _ = docker::list_networks().await?;
        }
        Commands::Status => cli::status().await,
        Commands::Install => cli::install_cli(),
        Commands::Config => cli::show_config(),
        Commands::Logs { follow, tail } => {
            cli::show_logs(follow, tail).await?;
        }
        Commands::Add {
            container,
            label,
            port,
            network,
        } => {
            cli::add_container(&container, label.as_deref(), port, network.as_deref()).await?;
        }
        Commands::Remove { identifier } => {
            cli::remove_container(&identifier).await?;
        }
        Commands::Switch { identifier, port } => {
            cli::switch_target(&identifier, port).await?;
        }
        Commands::Detect { filter } => cli::cli_detect_containers(filter.as_deref()).await,
        Commands::Tui => {
            #[cfg(feature = "tui")]
            {
                proxy_manager::tui::run_tui().await?;
            }
            #[cfg(not(feature = "tui"))]
            {
                anyhow::bail!("TUI feature not enabled. Recompile with --features tui");
            }
        }
    }

    Ok(())
}
