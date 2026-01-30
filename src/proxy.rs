use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;

use crate::config::ConfigManager;
use crate::docker::DockerClient;
use crate::nginx::{generate_dockerfile, generate_nginx_config};
use futures_util::StreamExt;

#[derive(Clone)]
pub struct ProxyManager {
    config_manager: ConfigManager,
    docker: DockerClient,
}

impl ProxyManager {
    pub fn new(config_manager: ConfigManager, docker: DockerClient) -> Self {
        Self {
            config_manager,
            docker,
        }
    }

    pub fn config_manager(&self) -> &ConfigManager {
        &self.config_manager
    }

    pub fn docker(&self) -> &DockerClient {
        &self.docker
    }

    pub async fn build_proxy(&self) -> Result<()> {
        let config = self.config_manager.load()?;

        if config.containers.is_empty() {
            anyhow::bail!("No containers configured. Use 'add' command first.");
        }

        self.config_manager.ensure_build_dir()?;

        let nginx_conf = generate_nginx_config(&config);
        let nginx_conf_path = self.config_manager.build_dir().join("nginx.conf");
        fs::write(&nginx_conf_path, nginx_conf).context("Failed to write nginx.conf")?;

        let dockerfile = generate_dockerfile(&config);
        let dockerfile_path = self.config_manager.build_dir().join("Dockerfile");
        fs::write(&dockerfile_path, dockerfile).context("Failed to write Dockerfile")?;

        println!("Building proxy image...");

        let proxy_image = config.get_proxy_image();
        self.docker
            .build_image(self.config_manager.build_dir(), &proxy_image)
            .await?;

        Ok(())
    }

    pub async fn start_proxy(&self) -> Result<()> {
        let config = self.config_manager.load()?;
        let proxy_name = &config.proxy_name;
        let proxy_image = config.get_proxy_image();
        let default_network = &config.network;

        if config.containers.is_empty() {
            anyhow::bail!("No containers configured. Use 'add' command first.");
        }

        if config.routes.is_empty() {
            anyhow::bail!("No routes configured. Use 'switch' command first.");
        }

        let mut networks = std::collections::HashSet::new();
        networks.insert(default_network.clone());

        for container in &config.containers {
            if let Some(network) = &container.network {
                networks.insert(network.clone());
            }
        }

        for network in &networks {
            self.docker.ensure_network(network).await?;
        }

        if self.docker.container_exists(proxy_name).await {
            println!("Proxy already running: {}", proxy_name);
            return Ok(());
        }

        if !self.docker.container_exists(&proxy_image).await {
            self.build_proxy().await?;
        }

        let host_ports = config.get_all_host_ports();
        let mut port_bindings: HashMap<String, HashMap<(), ()>> = HashMap::new();

        for port in &host_ports {
            port_bindings.insert(format!("{}/tcp", port), HashMap::new());
        }

        println!("Starting proxy: {}", proxy_name);
        self.docker
            .run_container(&proxy_image, proxy_name, default_network, port_bindings)
            .await?;

        for network in networks {
            if network != *default_network {
                match self
                    .docker
                    .connect_container_to_network(proxy_name, &network)
                    .await
                {
                    Ok(_) => println!("Connected proxy to network: {}", network),
                    Err(e) => println!("Warning: Could not connect to network {}: {}", network, e),
                }
            }
        }

        let port_str = host_ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("Proxy started on port(s): {}", port_str);

        Ok(())
    }

    pub async fn stop_proxy(&self) -> Result<()> {
        let config = self.config_manager.load()?;
        let proxy_name = &config.proxy_name;

        if self.docker.container_exists(proxy_name).await {
            println!("Stopping proxy: {}", proxy_name);
            self.docker.stop_container(proxy_name).await?;
            println!("Proxy stopped");
        } else {
            println!("Proxy not running");
        }

        Ok(())
    }

    pub async fn reload_proxy(&self) -> Result<()> {
        let config = self.config_manager.load()?;

        if config.containers.is_empty() {
            anyhow::bail!("No containers configured.");
        }

        if config.routes.is_empty() {
            anyhow::bail!("No routes configured.");
        }

        println!("Reloading proxy...");
        self.stop_proxy().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        self.start_proxy().await?;

        Ok(())
    }

    pub async fn stop_port(&self, host_port: u16) -> Result<()> {
        let mut config = self.config_manager.load()?;

        if config.find_route(host_port).is_none() {
            anyhow::bail!("No route found for port {}", host_port);
        }

        config.routes.retain(|r| r.host_port != host_port);
        self.config_manager.save(&config)?;
        println!("Removed route: port {}", host_port);

        if config.routes.is_empty() {
            self.stop_proxy().await
        } else {
            self.reload_proxy().await
        }
    }

    pub async fn status(&self) -> Result<()> {
        let config = self.config_manager.load()?;
        let proxy_name = &config.proxy_name;

        if self.docker.container_exists(proxy_name).await {
            let container = self.docker.get_container(proxy_name).await?;
            let state = container
                .state
                .unwrap_or(bollard::models::ContainerState::default());
            let status_str = format!("{:?}", state).to_lowercase();
            println!("Proxy: {} ({})", proxy_name, status_str);
            println!();
            println!("Active routes:");

            for route in &config.routes {
                let host_port = route.host_port;
                let target = &route.target;

                if let Some(target_container) = config.find_container(target) {
                    let internal_port = config.get_internal_port(target_container);
                    println!("  {} -> {}:{}", host_port, target, internal_port);
                } else {
                    println!("  {} -> {} (container not found)", host_port, target);
                }
            }
        } else {
            println!("Proxy not running");
        }

        Ok(())
    }

    pub async fn show_logs(&self, follow: bool, tail: i32) -> Result<()> {
        let config = self.config_manager.load()?;
        let proxy_name = &config.proxy_name;

        if !self.docker.container_exists(proxy_name).await {
            anyhow::bail!("Proxy container '{}' not running", proxy_name);
        }

        println!("Logs for: {}", proxy_name);
        println!("{}", "-".repeat(50));

        let logs = self.docker.get_logs(proxy_name, tail, follow).await?;

        futures_util::pin_mut!(logs);

        while let Some(result) = logs.next().await {
            match result {
                Ok(log_output) => match log_output {
                    bollard::container::LogOutput::StdOut { message } => {
                        print!("{}", String::from_utf8_lossy(&message));
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        eprint!("{}", String::from_utf8_lossy(&message));
                    }
                    _ => {}
                },
                Err(e) => {
                    eprintln!("Error reading logs: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }
}
