use clap::{Parser, Subcommand};

/// Manage Nginx proxy to route multiple ports to different Docker app containers.
#[derive(Parser, Debug)]
#[command(
    name = "proxy-manager",
    version,
    about,
    after_help = r#"Quick Start:
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
"#
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
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
        /// Number of lines to show
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

        /// Network the container is on (auto-detects if not specified)
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

    /// Launch the interactive TUI
    Tui,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_no_command() {
        let cli = Cli::parse_from(["proxy-manager"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_parse_start() {
        let cli = Cli::parse_from(["proxy-manager", "start"]);
        assert!(matches!(cli.command, Some(Command::Start)));
    }

    #[test]
    fn test_parse_stop_no_port() {
        let cli = Cli::parse_from(["proxy-manager", "stop"]);
        match cli.command {
            Some(Command::Stop { port }) => assert!(port.is_none()),
            _ => panic!("Expected Stop command"),
        }
    }

    #[test]
    fn test_parse_stop_with_port() {
        let cli = Cli::parse_from(["proxy-manager", "stop", "8080"]);
        match cli.command {
            Some(Command::Stop { port }) => assert_eq!(port, Some(8080)),
            _ => panic!("Expected Stop command"),
        }
    }

    #[test]
    fn test_parse_add_minimal() {
        let cli = Cli::parse_from(["proxy-manager", "add", "my-app"]);
        match cli.command {
            Some(Command::Add {
                container,
                label,
                port,
                network,
            }) => {
                assert_eq!(container, "my-app");
                assert!(label.is_none());
                assert!(port.is_none());
                assert!(network.is_none());
            }
            _ => panic!("Expected Add command"),
        }
    }

    #[test]
    fn test_parse_add_full() {
        let cli = Cli::parse_from([
            "proxy-manager",
            "add",
            "my-app",
            "My Label",
            "-p",
            "3000",
            "-n",
            "custom-net",
        ]);
        match cli.command {
            Some(Command::Add {
                container,
                label,
                port,
                network,
            }) => {
                assert_eq!(container, "my-app");
                assert_eq!(label.as_deref(), Some("My Label"));
                assert_eq!(port, Some(3000));
                assert_eq!(network.as_deref(), Some("custom-net"));
            }
            _ => panic!("Expected Add command"),
        }
    }

    #[test]
    fn test_parse_remove() {
        let cli = Cli::parse_from(["proxy-manager", "remove", "my-app"]);
        match cli.command {
            Some(Command::Remove { identifier }) => assert_eq!(identifier, "my-app"),
            _ => panic!("Expected Remove command"),
        }
    }

    #[test]
    fn test_parse_switch_no_port() {
        let cli = Cli::parse_from(["proxy-manager", "switch", "my-app"]);
        match cli.command {
            Some(Command::Switch { identifier, port }) => {
                assert_eq!(identifier, "my-app");
                assert!(port.is_none());
            }
            _ => panic!("Expected Switch command"),
        }
    }

    #[test]
    fn test_parse_switch_with_port() {
        let cli = Cli::parse_from(["proxy-manager", "switch", "my-app", "9000"]);
        match cli.command {
            Some(Command::Switch { identifier, port }) => {
                assert_eq!(identifier, "my-app");
                assert_eq!(port, Some(9000));
            }
            _ => panic!("Expected Switch command"),
        }
    }

    #[test]
    fn test_parse_logs_default_tail() {
        let cli = Cli::parse_from(["proxy-manager", "logs"]);
        match cli.command {
            Some(Command::Logs { tail }) => assert_eq!(tail, 100),
            _ => panic!("Expected Logs command"),
        }
    }

    #[test]
    fn test_parse_logs_custom_tail() {
        let cli = Cli::parse_from(["proxy-manager", "logs", "-n", "50"]);
        match cli.command {
            Some(Command::Logs { tail }) => assert_eq!(tail, 50),
            _ => panic!("Expected Logs command"),
        }
    }

    #[test]
    fn test_parse_detect_no_filter() {
        let cli = Cli::parse_from(["proxy-manager", "detect"]);
        match cli.command {
            Some(Command::Detect { filter }) => assert!(filter.is_none()),
            _ => panic!("Expected Detect command"),
        }
    }

    #[test]
    fn test_parse_detect_with_filter() {
        let cli = Cli::parse_from(["proxy-manager", "detect", "web"]);
        match cli.command {
            Some(Command::Detect { filter }) => assert_eq!(filter.as_deref(), Some("web")),
            _ => panic!("Expected Detect command"),
        }
    }

    #[test]
    fn test_parse_tui() {
        let cli = Cli::parse_from(["proxy-manager", "tui"]);
        assert!(matches!(cli.command, Some(Command::Tui)));
    }

    #[test]
    fn test_parse_restart() {
        let cli = Cli::parse_from(["proxy-manager", "restart"]);
        assert!(matches!(cli.command, Some(Command::Restart)));
    }

    #[test]
    fn test_parse_reload() {
        let cli = Cli::parse_from(["proxy-manager", "reload"]);
        assert!(matches!(cli.command, Some(Command::Reload)));
    }

    #[test]
    fn test_parse_config() {
        let cli = Cli::parse_from(["proxy-manager", "config"]);
        assert!(matches!(cli.command, Some(Command::Config)));
    }

    #[test]
    fn test_parse_status() {
        let cli = Cli::parse_from(["proxy-manager", "status"]);
        assert!(matches!(cli.command, Some(Command::Status)));
    }
}
