pub mod handler;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "proxy-manager")]
#[command(about = "Manage Nginx proxy to route multiple ports to different docker app containers.")]
#[command(version = "0.1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
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
    /// Create hardlink in ~/.local/bin for global access
    Install,
    /// Show config file path and contents
    Config,
    /// Show Nginx proxy container logs
    Logs {
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show (default: 100)
        #[arg(short, long, default_value = "100")]
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
        /// Network the container is on (default: auto-detects from container or uses config's network)
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
    /// Launch interactive TUI
    Tui,
}
