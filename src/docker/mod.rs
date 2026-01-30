use bollard::Docker;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, ListContainersOptions, LogsOptions,
    StopContainerOptions,
};
use bollard::image::BuildImageOptions;
use bollard::network::CreateNetworkOptions;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;

pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub fn new() -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub async fn ping(&self) -> anyhow::Result<()> {
        self.docker.ping().await?;
        Ok(())
    }

    pub async fn list_containers(
        &self,
        all: bool,
        filter_pattern: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        let options = ListContainersOptions::<String> {
            all,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;
        let names: Vec<String> = containers
            .into_iter()
            .filter_map(|c| c.names?.into_iter().next())
            .map(|name| name.trim_start_matches('/').to_string())
            .filter(|name| {
                if let Some(pattern) = filter_pattern {
                    name.to_lowercase().contains(&pattern.to_lowercase())
                } else {
                    true
                }
            })
            .collect();

        Ok(names)
    }

    pub async fn get_container_network(
        &self,
        container_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let options = ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;
        for container in containers {
            if let Some(names) = &container.names {
                for name in names {
                    let clean_name = name.trim_start_matches('/');
                    if clean_name == container_name
                        && let Some(network_settings) = &container.network_settings
                        && let Some(networks) = &network_settings.networks
                    {
                        return Ok(networks.keys().next().cloned());
                    }
                }
            }
        }

        Ok(None)
    }

    pub async fn list_networks(&self) -> anyhow::Result<Vec<NetworkInfo>> {
        let networks = self.docker.list_networks::<String>(None).await?;
        let mut infos = Vec::new();

        for network in networks {
            let container_count = network.containers.as_ref().map(|c| c.len()).unwrap_or(0);

            infos.push(NetworkInfo {
                name: network.name.unwrap_or_default(),
                driver: network.driver.unwrap_or_default(),
                scope: network.scope.unwrap_or_default(),
                container_count,
            });
        }

        Ok(infos)
    }

    pub async fn ensure_network(&self, network_name: &str) -> anyhow::Result<()> {
        let networks = self.docker.list_networks::<String>(None).await?;
        let exists = networks
            .iter()
            .any(|n| n.name.as_deref() == Some(network_name));

        if !exists {
            println!("Creating network: {}", network_name);
            let options = CreateNetworkOptions {
                name: network_name,
                driver: "bridge",
                ..Default::default()
            };
            self.docker.create_network(options).await?;
        }

        Ok(())
    }

    pub async fn connect_container_to_network(
        &self,
        container_id: &str,
        network_name: &str,
    ) -> anyhow::Result<()> {
        self.docker
            .connect_network(
                network_name,
                bollard::network::ConnectNetworkOptions {
                    container: container_id,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn build_image(&self, build_dir: &Path, tag: &str) -> anyhow::Result<()> {
        let dockerfile_path = build_dir.join("Dockerfile");
        let nginx_conf_path = build_dir.join("nginx.conf");

        // Create a tarball of the build context
        let mut tar = tar::Builder::new(Vec::new());
        tar.append_path_with_name(&dockerfile_path, "Dockerfile")?;
        tar.append_path_with_name(&nginx_conf_path, "nginx.conf")?;
        let tar_bytes = tar.into_inner()?;

        let options = BuildImageOptions {
            t: tag,
            rm: true,
            ..Default::default()
        };

        // Collect all build output
        let output = self
            .docker
            .build_image(options, None, Some(tar_bytes.into()));
        let results: Vec<_> = output.collect().await;

        for result in results {
            match result {
                Ok(build_info) => {
                    if let Some(stream) = build_info.stream {
                        print!("{}", stream);
                    }
                    if let Some(error) = build_info.error {
                        return Err(anyhow::anyhow!("Build error: {}", error));
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Build failed: {}", e)),
            }
        }

        Ok(())
    }

    pub async fn start_proxy_container(&self, config: &Config) -> anyhow::Result<()> {
        let proxy_name = config.get_proxy_name();
        let proxy_image = config.get_proxy_image();
        let default_network = config.get_network_name();

        // Check if already running
        if self
            .docker
            .inspect_container(proxy_name, None)
            .await
            .is_ok()
        {
            println!("Proxy already running: {}", proxy_name);
            return Ok(());
        }

        // Collect all networks
        let mut networks: std::collections::HashSet<String> = std::collections::HashSet::new();
        networks.insert(default_network.to_string());
        for c in &config.containers {
            if let Some(net) = &c.network {
                networks.insert(net.clone());
            }
        }

        // Ensure all networks exist
        for network in &networks {
            self.ensure_network(network).await?;
        }

        // Build port bindings
        let host_ports = config.get_all_host_ports();
        let mut port_bindings = HashMap::new();
        for port in &host_ports {
            let key = format!("{}/tcp", port);
            port_bindings.insert(
                key,
                Some(vec![bollard::models::PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(port.to_string()),
                }]),
            );
        }

        // Create host config with port bindings
        let host_config = bollard::models::HostConfig {
            port_bindings: Some(port_bindings),
            ..Default::default()
        };

        // Create container config
        let container_config = ContainerConfig {
            image: Some(proxy_image),
            host_config: Some(host_config),
            ..Default::default()
        };

        // Create container
        let options = CreateContainerOptions {
            name: proxy_name,
            ..Default::default()
        };

        let container = self
            .docker
            .create_container(Some(options), container_config)
            .await?;

        // Connect to default network
        self.docker
            .connect_network(
                default_network,
                bollard::network::ConnectNetworkOptions {
                    container: container.id.clone(),
                    ..Default::default()
                },
            )
            .await?;

        // Connect to other networks
        for network in networks {
            if network != default_network {
                match self
                    .docker
                    .connect_network(
                        &network,
                        bollard::network::ConnectNetworkOptions {
                            container: container.id.clone(),
                            ..Default::default()
                        },
                    )
                    .await
                {
                    Ok(_) => println!("Connected proxy to network: {}", network),
                    Err(e) => println!("Warning: Could not connect to network {}: {}", network, e),
                }
            }
        }

        // Start container
        self.docker
            .start_container::<String>(proxy_name, None)
            .await?;

        let port_str = host_ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("Proxy started on port(s): {}", port_str);

        Ok(())
    }

    pub async fn stop_proxy_container(&self, proxy_name: &str) -> anyhow::Result<bool> {
        match self.docker.inspect_container(proxy_name, None).await {
            Ok(_) => {
                println!("Stopping proxy: {}", proxy_name);
                let options = StopContainerOptions { t: 10 };
                self.docker
                    .stop_container(proxy_name, Some(options))
                    .await?;
                self.docker.remove_container(proxy_name, None).await?;
                println!("Proxy stopped");
                Ok(true)
            }
            Err(_) => {
                println!("Proxy not running");
                Ok(false)
            }
        }
    }

    pub async fn get_proxy_status(&self, proxy_name: &str) -> anyhow::Result<Option<String>> {
        match self.docker.inspect_container(proxy_name, None).await {
            Ok(info) => {
                let status = info
                    .state
                    .and_then(|s| s.status)
                    .map(|s| format!("{:?}", s));
                Ok(status)
            }
            Err(_) => Ok(None),
        }
    }

    pub async fn get_container_logs(
        &self,
        proxy_name: &str,
        tail: usize,
        follow: bool,
    ) -> anyhow::Result<Vec<String>> {
        match self.docker.inspect_container(proxy_name, None).await {
            Ok(_) => {
                let options = LogsOptions {
                    tail: tail.to_string(),
                    follow,
                    stdout: true,
                    stderr: true,
                    ..Default::default()
                };

                let mut logs = Vec::new();
                let output = self.docker.logs(proxy_name, Some(options));
                let results: Vec<_> = output.collect().await;

                for result in results {
                    match result {
                        Ok(log) => {
                            let line = String::from_utf8_lossy(log.as_ref());
                            logs.push(line.to_string());
                        }
                        Err(e) => return Err(anyhow::anyhow!("Error reading logs: {}", e)),
                    }
                }

                Ok(logs)
            }
            Err(_) => Err(anyhow::anyhow!(
                "Proxy container '{}' not running",
                proxy_name
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub container_count: usize,
}
