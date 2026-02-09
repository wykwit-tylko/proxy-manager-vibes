mod commands;
mod config;
mod docker;
mod nginx;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "proxy-manager",
    version = "0.1.0",
    about = "Manage Nginx proxy to route multiple ports to different docker app containers.",
    long_about = None,
    after_help = r#"QUICK START:
  # 1. Add containers
  proxy-manager add my-app-v1 "Foo" -p 8000
  proxy-manager add my-app-v2 "Bar" -p 8080

  # 2. Switch ports to containers (adds routes)
  proxy-manager switch my-app-v1 8000
  proxy-manager switch my-app-v2 8001

  # 3. Start the proxy (routes multiple ports)
  proxy-manager start

  # 4. View status
  proxy-manager status

  # 5. Launch TUI
  proxy-manager tui

CONTAINER MANAGEMENT:
  proxy-manager add <name> [label]            # Add container (auto-detects network)
  proxy-manager add <name> -p 8080            # Specify custom port
  proxy-manager add <name> -n custom-net      # Specify custom network
  proxy-manager list                          # Show all configured containers
  proxy-manager remove <name|label>           # Remove container from config

ROUTE MANAGEMENT:
  proxy-manager switch <container> [port]     # Route host port to container (default: 8000)
  proxy-manager stop [port]                   # Stop routing for a port (removes route)

PROXY OPERATIONS:
  proxy-manager start                         # Start proxy with all configured routes
  proxy-manager stop [port]                   # Stop proxy (or stop routing for specific port)
  proxy-manager restart                       # Restart proxy
  proxy-manager reload                        # Apply config changes
  proxy-manager status                        # Show current status and all active routes

LOGGING:
  proxy-manager logs                          # Show proxy logs
  proxy-manager logs -f                       # Follow logs (tail -f mode)
  proxy-manager logs -n 50                    # Show last 50 lines

DISCOVERY:
  proxy-manager detect                        # List all Docker containers
  proxy-manager detect [name]                 # Filter containers by name
  proxy-manager networks                      # List all Docker networks

CONFIGURATION:
  proxy-manager config                        # View config file and contents

TUI:
  proxy-manager tui                           # Launch interactive text user interface
"#
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
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

    /// Restart the proxy
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
        #[arg(short = 'n', long, default_value = "100")]
        tail: usize,
    },

    /// Add or update a container to config
    Add {
        /// Docker container name
        container: String,

        /// Optional display label
        label: Option<String>,

        /// Port the container exposes (default: 8000)
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

    /// Launch interactive text user interface
    Tui,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Start) => {
            commands::start_proxy().await?;
        }
        Some(Commands::Stop { port }) => {
            if let Some(p) = port {
                commands::stop_port(p).await?;
            } else {
                commands::stop_proxy().await?;
            }
        }
        Some(Commands::Restart) => {
            commands::stop_proxy().await.ok();
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            commands::start_proxy().await?;
        }
        Some(Commands::Reload) => {
            commands::reload_proxy().await?;
        }
        Some(Commands::List) => {
            commands::list_containers().await?;
        }
        Some(Commands::Status) => {
            commands::status().await?;
        }
        Some(Commands::Add {
            container,
            label,
            port,
            network,
        }) => {
            commands::add_container(&container, label, port, network).await?;
        }
        Some(Commands::Networks) => {
            commands::list_networks().await?;
        }
        Some(Commands::Remove { identifier }) => {
            commands::remove_container(&identifier).await?;
        }
        Some(Commands::Switch { identifier, port }) => {
            commands::switch_target(&identifier, port).await?;
        }
        Some(Commands::Detect { filter }) => {
            commands::detect_containers(filter.as_deref()).await?;
        }
        Some(Commands::Config) => {
            commands::show_config().await?;
        }
        Some(Commands::Logs { follow, tail }) => {
            commands::show_logs(follow, tail).await?;
        }
        Some(Commands::Tui) => {
            tui::run_tui().await?;
        }
        None => {
            // Show help when no command is provided
            Cli::parse_from(["proxy-manager", "--help"]);
        }
    }

    Ok(())
}
