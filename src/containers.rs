use crate::config::{ConfigManager, Container};
use anyhow::Result;

pub struct ContainerManager {
    config_manager: ConfigManager,
    docker: crate::docker::DockerClient,
}

impl ContainerManager {
    pub fn new(config_manager: ConfigManager, docker: crate::docker::DockerClient) -> Self {
        Self {
            config_manager,
            docker,
        }
    }

    pub async fn add_container(
        &self,
        container_name: String,
        label: Option<String>,
        port: Option<u16>,
        network: Option<String>,
    ) -> Result<()> {
        let mut config = self.config_manager.load()?;

        let network = if network.is_none() {
            if let Ok(Some(detected)) = self.docker.get_container_network(&container_name).await {
                println!("Auto-detected network: {}", detected);
                Some(detected)
            } else {
                None
            }
        } else {
            network
        };

        if let Some(_existing) = config.find_container(&container_name) {
            let mut updated = false;

            if let Some(label) = label {
                let container = config
                    .containers
                    .iter_mut()
                    .find(|c| c.name == container_name)
                    .unwrap();
                container.label = Some(label);
                updated = true;
            }

            if port.is_some() {
                let container = config
                    .containers
                    .iter_mut()
                    .find(|c| c.name == container_name)
                    .unwrap();
                container.port = port;
                updated = true;
            }

            if network.is_some() {
                let container = config
                    .containers
                    .iter_mut()
                    .find(|c| c.name == container_name)
                    .unwrap();
                container.network = network;
                updated = true;
            }

            if updated {
                self.config_manager.save(&config)?;
                println!("Updated container: {}", container_name);
            }
        } else {
            let entry = Container {
                name: container_name.clone(),
                label,
                port,
                network,
            };
            config.containers.push(entry);
            self.config_manager.save(&config)?;
            println!("Added container: {}", container_name);
        }

        Ok(())
    }

    pub async fn remove_container(&self, identifier: String) -> Result<bool> {
        let mut config = self.config_manager.load()?;

        let container = config.find_container(&identifier);

        if container.is_none() {
            println!("Error: Container '{}' not found in config", identifier);
            return Ok(false);
        }

        let container_name = container.unwrap().name.clone();
        config.containers.retain(|c| c.name != container_name.as_str());
        config.routes.retain(|r| r.target != container_name.as_str());
        self.config_manager.save(&config)?;
        println!("Removed container: {}", container_name);

        Ok(true)
    }

    pub fn list_containers(&self) -> Result<()> {
        let config = self.config_manager.load();

        if config
            .as_ref()
            .map(|c| c.containers.is_empty())
            .unwrap_or(true)
        {
            println!("No containers configured");
            return Ok(());
        }

        let config = config?;
        let route_map: std::collections::HashMap<String, u16> = config
            .routes
            .iter()
            .map(|r| (r.target.clone(), r.host_port))
            .collect();

        println!("Configured containers:");
        for c in &config.containers {
            let host_port = route_map.get(&c.name);
            let marker = if let Some(host_port) = host_port {
                format!(" (port {})", host_port)
            } else {
                String::new()
            };
            let label = if let Some(label) = &c.label {
                format!(" - {}", label)
            } else {
                String::new()
            };
            let port = c.port.unwrap_or(crate::config::DEFAULT_PORT);
            let net = c.network.as_ref().unwrap_or(&config.network);
            println!("  {}:{}@{}{}{}", c.name, port, net, label, marker);
        }

        Ok(())
    }

    pub async fn detect_containers(&self, filter: Option<String>) -> Result<Vec<String>> {
        println!("Detecting running containers...");
        let containers = self.docker.list_containers(true).await?;

        let names: Vec<String> = containers
            .into_iter()
            .filter_map(|c| c.names)
            .flatten()
            .map(|n| n.trim_start_matches('/').to_string())
            .filter(|n| {
                if let Some(filter) = &filter {
                    n.to_lowercase().contains(&filter.to_lowercase())
                } else {
                    true
                }
            })
            .collect();

        Ok(names)
    }

    pub async fn list_networks(&self) -> Result<()> {
        println!("Available Docker networks:");
        let networks = self.docker.list_networks().await?;

        for net in networks {
            let driver = net.driver.unwrap_or_else(|| "unknown".to_string());
            let containers_count = net.containers.as_ref().map_or(0, |c| c.len());
            let scope = net.scope.unwrap_or_else(|| "local".to_string());

            println!(
                "  {:<25} driver={:<10} containers={:<4} scope={}",
                net.name.unwrap_or_else(|| "unnamed".to_string()),
                driver,
                containers_count,
                scope
            );
        }

        Ok(())
    }
}
