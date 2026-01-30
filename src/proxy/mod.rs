use crate::config::Config;
use crate::docker::DockerClient;
use crate::nginx::build_proxy;
use std::time::Duration;

pub struct ProxyManager {
    docker: DockerClient,
}

impl ProxyManager {
    pub fn new() -> anyhow::Result<Self> {
        let docker = DockerClient::new()?;
        Ok(Self { docker })
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let config = Config::load()?;

        if config.containers.is_empty() {
            return Err(anyhow::anyhow!(
                "No containers configured. Use 'add' command first."
            ));
        }

        if config.routes.is_empty() {
            return Err(anyhow::anyhow!(
                "No routes configured. Use 'switch' command first."
            ));
        }

        // Build and start
        build_proxy(&self.docker, &config).await?;
        self.docker.start_proxy_container(&config).await?;

        Ok(())
    }

    pub async fn stop(&self) -> anyhow::Result<bool> {
        let config = Config::load()?;
        let proxy_name = config.get_proxy_name();
        self.docker.stop_proxy_container(proxy_name).await
    }

    pub async fn restart(&self) -> anyhow::Result<()> {
        self.stop().await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.start().await
    }

    pub async fn reload(&self) -> anyhow::Result<()> {
        let config = Config::load()?;

        if config.containers.is_empty() {
            return Err(anyhow::anyhow!("No containers configured."));
        }

        if config.routes.is_empty() {
            return Err(anyhow::anyhow!("No routes configured."));
        }

        println!("Reloading proxy...");
        self.stop().await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.start().await
    }

    pub async fn stop_port(&self, host_port: u16) -> anyhow::Result<()> {
        let mut config = Config::load()?;

        if config.find_route(host_port).is_none() {
            return Err(anyhow::anyhow!("No route found for port {}", host_port));
        }

        config.remove_route(host_port);
        config.save()?;
        println!("Removed route: port {}", host_port);

        if config.routes.is_empty() {
            self.stop().await?;
        } else {
            self.reload().await?;
        }

        Ok(())
    }

    pub async fn switch_target(
        &self,
        identifier: &str,
        host_port: Option<u16>,
    ) -> anyhow::Result<()> {
        let mut config = Config::load()?;

        let container = config
            .find_container(identifier)
            .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?
            .clone();

        let host_port = host_port.unwrap_or(crate::config::DEFAULT_PORT);

        config.add_or_update_route(host_port, container.name.clone());
        config.save()?;

        if config.find_route(host_port).map(|r| &r.target) == Some(&container.name) {
            println!("Switching route: {} -> {}", host_port, container.name);
        } else {
            println!("Adding route: {} -> {}", host_port, container.name);
        }

        self.reload().await
    }

    pub async fn add_container(
        &self,
        name: String,
        label: Option<String>,
        port: Option<u16>,
        network: Option<String>,
    ) -> anyhow::Result<()> {
        let mut config = Config::load()?;

        // Auto-detect network if not provided
        let network = if network.is_none() {
            match self.docker.get_container_network(&name).await? {
                Some(detected) => {
                    println!("Auto-detected network: {}", detected);
                    Some(detected)
                }
                None => None,
            }
        } else {
            network
        };

        let is_new = config.add_or_update_container(name.clone(), label, port, network);
        config.save()?;

        if is_new {
            println!("Added container: {}", name);
        } else {
            println!("Updated container: {}", name);
        }

        Ok(())
    }

    pub async fn remove_container(&self, identifier: &str) -> anyhow::Result<()> {
        let mut config = Config::load()?;

        let removed = config
            .remove_container(identifier)
            .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?;

        config.save()?;
        println!("Removed container: {}", removed.name);

        Ok(())
    }

    pub async fn list_containers(&self) -> anyhow::Result<()> {
        let config = Config::load()?;

        if config.containers.is_empty() {
            println!("No containers configured");
            return Ok(());
        }

        let route_map: std::collections::HashMap<&str, u16> = config
            .routes
            .iter()
            .map(|r| (r.target.as_str(), r.host_port))
            .collect();

        println!("Configured containers:");
        for c in &config.containers {
            let marker = route_map
                .get(c.name.as_str())
                .map(|port| format!(" (port {})", port))
                .unwrap_or_default();
            let label = c
                .label
                .as_ref()
                .map(|l| format!(" - {}", l))
                .unwrap_or_default();
            let port = c.port.unwrap_or(crate::config::DEFAULT_PORT);
            let net = c.network.as_deref().unwrap_or(&config.network);
            println!("  {}:{}@{}{}{}", c.name, port, net, label, marker);
        }

        Ok(())
    }

    pub async fn status(&self) -> anyhow::Result<()> {
        let config = Config::load()?;
        let proxy_name = config.get_proxy_name();

        match self.docker.get_proxy_status(proxy_name).await? {
            Some(status) => {
                println!("Proxy: {} ({})", proxy_name, status);
                println!();
                println!("Active routes:");
                for route in &config.routes {
                    let host_port = route.host_port;
                    let target = &route.target;
                    if let Some(container) = config.find_container(target) {
                        let internal_port = config.get_internal_port(Some(container));
                        println!("  {} -> {}:{}", host_port, target, internal_port);
                    } else {
                        println!("  {} -> {} (container not found)", host_port, target);
                    }
                }
            }
            None => {
                println!("Proxy not running");
            }
        }

        Ok(())
    }

    pub async fn detect_containers(&self, filter: Option<&str>) -> anyhow::Result<()> {
        let containers = self.docker.list_containers(true, filter).await?;
        println!("Running containers:");
        for c in containers {
            println!("  {}", c);
        }
        Ok(())
    }

    pub async fn list_networks(&self) -> anyhow::Result<()> {
        let networks = self.docker.list_networks().await?;
        println!("Available Docker networks:");
        for net in networks {
            println!(
                "  {:<25} driver={:<10} containers={:<4} scope={}",
                net.name, net.driver, net.container_count, net.scope
            );
        }
        Ok(())
    }

    pub async fn show_logs(&self, tail: usize, follow: bool) -> anyhow::Result<()> {
        let config = Config::load()?;
        let proxy_name = config.get_proxy_name();

        let logs = self
            .docker
            .get_container_logs(proxy_name, tail, follow)
            .await?;
        println!("Logs for: {}", proxy_name);
        println!("{}", "-".repeat(50));
        for line in logs {
            print!("{}", line);
        }

        Ok(())
    }

    pub fn show_config() -> anyhow::Result<()> {
        let config = Config::load()?;
        println!("Config file: {}", Config::config_file().display());
        println!();
        println!("{}", serde_json::to_string_pretty(&config)?);
        Ok(())
    }
}
