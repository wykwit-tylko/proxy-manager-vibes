use anyhow::{Context, Result};
use bollard::Docker;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, ListContainersOptions, LogsOptions,
    RemoveContainerOptions, StopContainerOptions,
};
use bollard::image::BuildImageOptions;
use bollard::models::{ContainerSummary, Network, PortBinding};
use bollard::network::{ConnectNetworkOptions, CreateNetworkOptions, ListNetworksOptions};
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

use crate::config::Config;

pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub fn new() -> Result<Self> {
        let docker =
            Docker::connect_with_local_defaults().context("Failed to connect to Docker daemon")?;
        Ok(Self { docker })
    }

    pub fn new_with_docker(docker: Docker) -> Self {
        Self { docker }
    }

    pub async fn list_containers(&self, all: bool) -> Result<Vec<ContainerSummary>> {
        let options: ListContainersOptions<String> = ListContainersOptions {
            all,
            ..Default::default()
        };
        self.docker
            .list_containers(Some(options))
            .await
            .context("Failed to list containers")
    }

    pub async fn list_container_names(&self, filter_pattern: Option<&str>) -> Result<Vec<String>> {
        let containers = self.list_containers(true).await?;
        let names: Vec<String> = containers
            .into_iter()
            .filter_map(|c| {
                c.names.as_ref().and_then(|names| {
                    names.first().and_then(|n| {
                        let name = n.trim_start_matches('/').to_string();
                        match filter_pattern {
                            Some(pattern)
                                if name.to_lowercase().contains(&pattern.to_lowercase()) =>
                            {
                                Some(name)
                            }
                            Some(_) => None,
                            None => Some(name),
                        }
                    })
                })
            })
            .collect();
        Ok(names)
    }

    pub async fn list_networks(&self) -> Result<Vec<Network>> {
        let options: ListNetworksOptions<String> = ListNetworksOptions::default();
        self.docker
            .list_networks(Some(options))
            .await
            .context("Failed to list networks")
    }

    pub async fn get_container_network(&self, container_name: &str) -> Result<Option<String>> {
        let containers = self.list_containers(true).await?;

        for container in containers {
            if let Some(names) = &container.names
                && names
                    .iter()
                    .any(|n| n.trim_start_matches('/') == container_name)
                    && let Some(networks) = container.network_settings
                        && let Some(networks_map) = networks.networks {
                            return Ok(networks_map.keys().next().cloned());
                        }
        }

        Ok(None)
    }

    pub async fn ensure_network(&self, network_name: &str) -> Result<()> {
        let networks = self.list_networks().await?;

        if networks
            .iter()
            .any(|n| n.name.as_deref() == Some(network_name))
        {
            return Ok(());
        }

        let options = CreateNetworkOptions {
            name: network_name,
            driver: "bridge",
            ..Default::default()
        };

        self.docker
            .create_network(options)
            .await
            .context("Failed to create network")?;

        Ok(())
    }

    pub async fn build_proxy_image(&self, config: &Config, build_dir: &Path) -> Result<()> {
        // Ensure build directory exists
        fs::create_dir_all(build_dir).await?;

        let image_name = config.get_proxy_image();
        let options = BuildImageOptions {
            t: image_name.as_str(),
            rm: true,
            ..Default::default()
        };

        // Build context tarball would be created here
        // For simplicity, we'll use a different approach
        let dockerfile_path = build_dir.join("Dockerfile");
        let nginx_conf_path = build_dir.join("nginx.conf");

        // Check if files exist
        if !dockerfile_path.exists() || !nginx_conf_path.exists() {
            return Err(anyhow::anyhow!(
                "Build files not found. Run 'build' command first."
            ));
        }

        // Create build context
        let mut tar = tar::Builder::new(Vec::new());

        // Add Dockerfile
        let dockerfile_content = fs::read(&dockerfile_path).await?;
        let mut header = tar::Header::new_gnu();
        header.set_path("Dockerfile")?;
        header.set_size(dockerfile_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, dockerfile_content.as_slice())?;

        // Add nginx.conf
        let nginx_conf_content = fs::read(&nginx_conf_path).await?;
        let mut header = tar::Header::new_gnu();
        header.set_path("nginx.conf")?;
        header.set_size(nginx_conf_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, nginx_conf_content.as_slice())?;

        let tar_bytes = tar.into_inner()?;

        let stream = self
            .docker
            .build_image(options, None, Some(tar_bytes.into()));
        tokio::pin!(stream);

        while let Some(result) = stream.as_mut().next().await {
            match result {
                Ok(output) => {
                    if let Some(stream) = output.stream {
                        print!("{}", stream);
                    }
                    if let Some(error) = output.error {
                        return Err(anyhow::anyhow!("Build error: {}", error));
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }

        Ok(())
    }

    pub async fn start_proxy(&self, config: &Config) -> Result<()> {
        let proxy_name = config.proxy_name.clone();
        let proxy_image = config.get_proxy_image();
        let default_network = config.network.clone();

        // Check if already running
        if let Ok(container) = self.docker.inspect_container(&proxy_name, None).await
            && container.state.and_then(|s| s.running).unwrap_or(false) {
                println!("Proxy already running: {}", proxy_name);
                return Ok(());
            }

        // Ensure all networks exist
        let mut networks = std::collections::HashSet::new();
        networks.insert(default_network.as_str());

        for container in &config.containers {
            if let Some(network) = &container.network {
                networks.insert(network);
            }
        }

        for network in &networks {
            self.ensure_network(network).await?;
        }

        // Build image if needed
        let build_dir = Config::build_dir();
        self.build_proxy_image(config, &build_dir).await?;

        // Create port bindings
        let host_ports = config.get_all_host_ports();
        let mut port_bindings = HashMap::new();
        let mut exposed_ports = HashMap::new();

        for port in &host_ports {
            let port_str = format!("{}/tcp", port);
            port_bindings.insert(
                port_str.clone(),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(port.to_string()),
                }]),
            );
            exposed_ports.insert(port_str, HashMap::new());
        }

        // Create container
        let container_config = ContainerConfig {
            image: Some(proxy_image),
            host_config: Some(bollard::models::HostConfig {
                port_bindings: Some(port_bindings),
                ..Default::default()
            }),
            exposed_ports: Some(exposed_ports),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: proxy_name.as_str(),
            ..Default::default()
        };

        let container = self
            .docker
            .create_container(Some(options), container_config)
            .await
            .context("Failed to create proxy container")?;

        // Connect to default network first
        self.docker
            .connect_network(
                &default_network,
                ConnectNetworkOptions {
                    container: container.id.clone(),
                    ..Default::default()
                },
            )
            .await?;

        // Connect to other networks
        for network in networks {
            if network != default_network
                && let Err(e) = self
                    .docker
                    .connect_network(
                        network,
                        ConnectNetworkOptions {
                            container: container.id.clone(),
                            ..Default::default()
                        },
                    )
                    .await
                {
                    eprintln!("Warning: Could not connect to network {}: {}", network, e);
                }
        }

        // Start container
        self.docker
            .start_container::<String>(&container.id, None)
            .await
            .context("Failed to start proxy container")?;

        let port_str = host_ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("Proxy started on port(s): {}", port_str);

        Ok(())
    }

    pub async fn stop_proxy(&self, proxy_name: &str) -> Result<bool> {
        match self.docker.inspect_container(proxy_name, None).await {
            Ok(_) => {
                self.docker
                    .stop_container(proxy_name, Some(StopContainerOptions { t: 10 }))
                    .await?;
                self.docker
                    .remove_container(
                        proxy_name,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await?;
                println!("Proxy stopped");
                Ok(true)
            }
            Err(_) => {
                println!("Proxy not running");
                Ok(false)
            }
        }
    }

    pub async fn get_proxy_logs(
        &self,
        proxy_name: &str,
        tail: usize,
        follow: bool,
    ) -> Result<Vec<String>> {
        let options = LogsOptions {
            tail: tail.to_string(),
            follow,
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut logs = Vec::new();
        let stream = self.docker.logs(proxy_name, Some(options));
        tokio::pin!(stream);

        while let Some(result) = stream.as_mut().next().await {
            match result {
                Ok(log) => {
                    let line = String::from_utf8_lossy(log.as_ref());
                    logs.push(line.to_string());
                }
                Err(e) => return Err(e.into()),
            }
        }

        Ok(logs)
    }

    pub async fn get_container_status(&self, container_name: &str) -> Result<Option<String>> {
        match self.docker.inspect_container(container_name, None).await {
            Ok(info) => Ok(info
                .state
                .and_then(|s| s.status)
                .map(|status| format!("{:?}", status).to_lowercase())),
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a Docker daemon to be running
    // They are marked as ignored by default

    #[tokio::test]
    #[ignore]
    async fn test_docker_client_new() {
        let client = DockerClient::new();
        assert!(client.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_list_containers() {
        let client = DockerClient::new().unwrap();
        let containers = client.list_containers(true).await;
        assert!(containers.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_list_networks() {
        let client = DockerClient::new().unwrap();
        let networks = client.list_networks().await;
        assert!(networks.is_ok());
    }
}
