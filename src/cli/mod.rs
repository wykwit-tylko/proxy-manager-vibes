use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::{Config, Container, DEFAULT_PORT};
use crate::docker::DockerClient;
use crate::nginx::NginxConfigGenerator;

#[derive(Parser)]
#[command(name = "proxy-manager")]
#[command(about = "Manage Nginx proxy to route multiple ports to different docker app containers")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
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
}

pub struct CliHandler {
    docker: DockerClient,
}

impl CliHandler {
    pub async fn new() -> Result<Self> {
        let docker = DockerClient::new()?;
        Ok(Self { docker })
    }

    pub async fn run(&self, cli: Cli) -> Result<()> {
        match cli.command {
            Commands::Start => self.cmd_start().await,
            Commands::Stop { port } => {
                if let Some(p) = port {
                    self.cmd_stop_port(p).await
                } else {
                    self.cmd_stop().await
                }
            }
            Commands::Restart => self.cmd_restart().await,
            Commands::Reload => self.cmd_reload().await,
            Commands::List => self.cmd_list(),
            Commands::Networks => self.cmd_networks().await,
            Commands::Status => self.cmd_status().await,
            Commands::Install => self.cmd_install(),
            Commands::Config => self.cmd_config(),
            Commands::Logs { follow, tail } => self.cmd_logs(follow, tail).await,
            Commands::Add {
                container,
                label,
                port,
                network,
            } => self.cmd_add(container, label, port, network).await,
            Commands::Remove { identifier } => self.cmd_remove(identifier),
            Commands::Switch { identifier, port } => self.cmd_switch(identifier, port).await,
            Commands::Detect { filter } => self.cmd_detect(filter).await,
        }
    }

    async fn cmd_start(&self) -> Result<()> {
        let config = Config::load()?;

        if config.containers.is_empty() {
            anyhow::bail!("Error: No containers configured. Use 'add' command first.");
        }

        if config.routes.is_empty() {
            anyhow::bail!("Error: No routes configured. Use 'switch' command first.");
        }

        // Write build files first
        self.write_build_files(&config).await?;

        self.docker.start_proxy(&config).await
    }

    async fn cmd_stop(&self) -> Result<()> {
        let config = Config::load()?;
        self.docker.stop_proxy(&config.proxy_name).await?;
        Ok(())
    }

    async fn cmd_stop_port(&self, host_port: u16) -> Result<()> {
        let mut config = Config::load()?;

        if config.find_route(host_port).is_none() {
            anyhow::bail!("Error: No route found for port {}", host_port);
        }

        config.remove_route(host_port);
        config.save()?;
        println!("Removed route: port {}", host_port);

        if config.routes.is_empty() {
            self.docker.stop_proxy(&config.proxy_name).await?;
        } else {
            self.cmd_reload().await?;
        }

        Ok(())
    }

    async fn cmd_restart(&self) -> Result<()> {
        self.cmd_stop().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        self.cmd_start().await
    }

    async fn cmd_reload(&self) -> Result<()> {
        let config = Config::load()?;

        if config.containers.is_empty() {
            anyhow::bail!("Error: No containers configured.");
        }

        if config.routes.is_empty() {
            anyhow::bail!("Error: No routes configured.");
        }

        println!("Reloading proxy...");
        self.cmd_stop().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        self.cmd_start().await
    }

    fn cmd_list(&self) -> Result<()> {
        let config = Config::load()?;
        let route_map: std::collections::HashMap<&str, u16> = config
            .routes
            .iter()
            .map(|r| (r.target.as_str(), r.host_port))
            .collect();

        if config.containers.is_empty() {
            println!("No containers configured");
            return Ok(());
        }

        println!("Configured containers:");
        for c in &config.containers {
            let marker = route_map
                .get(c.name.as_str())
                .map(|p| format!(" (port {})", p))
                .unwrap_or_default();
            let label = c
                .label
                .as_ref()
                .map(|l| format!(" - {}", l))
                .unwrap_or_default();
            let port = c.get_port();
            let net = c.network.as_ref().unwrap_or(&config.network);
            println!("  {}:{port}@{net}{label}{marker}", c.name);
        }

        Ok(())
    }

    async fn cmd_networks(&self) -> Result<()> {
        let networks = self.docker.list_networks().await?;

        println!("Available Docker networks:");
        for net in networks {
            let name = net.name.as_deref().unwrap_or("unknown");
            let driver = net.driver.as_deref().unwrap_or("unknown");
            let scope = net.scope.as_deref().unwrap_or("local");
            let containers_count = net.containers.as_ref().map(|c| c.len()).unwrap_or(0);
            println!(
                "  {:<25} driver={:<10} containers={:<4} scope={}",
                name, driver, containers_count, scope
            );
        }

        Ok(())
    }

    async fn cmd_status(&self) -> Result<()> {
        let config = Config::load()?;
        let proxy_name = &config.proxy_name;

        match self.docker.get_container_status(proxy_name).await? {
            Some(status) => {
                println!("Proxy: {} ({})", proxy_name, status);
                println!();
                println!("Active routes:");
                for route in &config.routes {
                    let host_port = route.host_port;
                    let target = &route.target;
                    match config.find_container(target) {
                        Some(container) => {
                            let internal_port = container.get_port();
                            println!("  {} -> {}:{}", host_port, target, internal_port);
                        }
                        None => {
                            println!("  {} -> {} (container not found)", host_port, target);
                        }
                    }
                }
            }
            None => {
                println!("Proxy not running");
            }
        }

        Ok(())
    }

    fn cmd_install(&self) -> Result<()> {
        

        let current_exe = std::env::current_exe()?;
        let user_bin = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".local")
            .join("bin");
        let link_path = user_bin.join("proxy-manager");

        std::fs::create_dir_all(&user_bin)?;

        // Remove existing link if it exists
        if link_path.exists() || link_path.is_symlink() {
            std::fs::remove_file(&link_path)?;
        }

        // Create hardlink
        std::fs::hard_link(&current_exe, &link_path)?;
        println!(
            "Created hardlink: {} -> {}",
            link_path.display(),
            current_exe.display()
        );
        println!();
        println!("See 'proxy-manager --help' for a quick start guide.");

        // Check PATH
        let path = std::env::var("PATH").unwrap_or_default();
        if !path.contains(user_bin.to_str().unwrap_or("")) {
            println!("NOTE: Add ~/.local/bin to your PATH:");
            println!("  export PATH=\"{}:$PATH\"", user_bin.display());
            println!("  # Add to ~/.bashrc or ~/.zshrc to persist");
        }

        Ok(())
    }

    fn cmd_config(&self) -> Result<()> {
        let config_file = Config::config_file();
        println!("Config file: {}", config_file.display());
        println!();

        let config = Config::load()?;
        println!("{}", serde_json::to_string_pretty(&config)?);

        Ok(())
    }

    async fn cmd_logs(&self, follow: bool, tail: usize) -> Result<()> {
        let config = Config::load()?;
        let proxy_name = &config.proxy_name;

        if follow {
            println!("Logs for: {}", proxy_name);
            println!("{}", "-".repeat(50));
            let logs = self.docker.get_proxy_logs(proxy_name, tail, true).await?;
            for line in logs {
                print!("{}", line);
            }
        } else {
            println!("Logs for: {}", proxy_name);
            println!("{}", "-".repeat(50));
            let logs = self.docker.get_proxy_logs(proxy_name, tail, false).await?;
            for line in logs {
                print!("{}", line);
            }
        }

        Ok(())
    }

    async fn cmd_add(
        &self,
        container_name: String,
        label: Option<String>,
        port: Option<u16>,
        network: Option<String>,
    ) -> Result<()> {
        let mut config = Config::load()?;

        // Auto-detect network if not provided
        let network = if network.is_none() {
            match self.docker.get_container_network(&container_name).await? {
                Some(net) => {
                    println!("Auto-detected network: {}", net);
                    Some(net)
                }
                None => None,
            }
        } else {
            network
        };

        let container = {
            let mut c = Container::new(&container_name);
            if let Some(l) = label {
                c = c.with_label(l);
            }
            if let Some(p) = port {
                c = c.with_port(p);
            }
            if let Some(n) = network {
                c = c.with_network(n);
            }
            c
        };

        let is_update = config.find_container(&container_name).is_some();
        config.add_or_update_container(container);
        config.save()?;

        if is_update {
            println!("Updated container: {}", container_name);
        } else {
            println!("Added container: {}", container_name);
        }

        Ok(())
    }

    fn cmd_remove(&self, identifier: String) -> Result<()> {
        let mut config = Config::load()?;

        match config.remove_container(&identifier) {
            Some(container) => {
                config.save()?;
                println!("Removed container: {}", container.name);
                Ok(())
            }
            None => {
                anyhow::bail!("Error: Container '{}' not found in config", identifier)
            }
        }
    }

    async fn cmd_switch(&self, identifier: String, port: Option<u16>) -> Result<()> {
        let mut config = Config::load()?;
        let host_port = port.unwrap_or(DEFAULT_PORT);

        // Verify container exists
        if config.find_container(&identifier).is_none() {
            anyhow::bail!("Error: Container '{}' not found in config", identifier);
        }

        let is_update = config.find_route(host_port).is_some();
        config.set_route(host_port, &identifier);
        config.save()?;

        if is_update {
            println!("Switching route: {} -> {}", host_port, identifier);
        } else {
            println!("Adding route: {} -> {}", host_port, identifier);
        }

        self.cmd_reload().await
    }

    async fn cmd_detect(&self, filter: Option<String>) -> Result<()> {
        println!("Detecting running containers...");
        let containers = self.docker.list_container_names(filter.as_deref()).await?;

        println!("Running containers:");
        for name in containers {
            println!("  {}", name);
        }

        Ok(())
    }

    async fn write_build_files(&self, config: &Config) -> Result<()> {
        let build_dir = Config::build_dir();
        tokio::fs::create_dir_all(&build_dir).await?;

        let nginx_conf = NginxConfigGenerator::generate(config);
        let dockerfile = NginxConfigGenerator::generate_dockerfile(config);

        tokio::fs::write(build_dir.join("nginx.conf"), nginx_conf).await?;
        tokio::fs::write(build_dir.join("Dockerfile"), dockerfile).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parser() {
        // Test that CLI parses correctly
        let cli = Cli::parse_from(["proxy-manager", "list"]);
        matches!(cli.command, Commands::List);
    }
}
