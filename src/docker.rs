use anyhow::{Context, Result};
use bollard::{
    container::{ListContainersOptions, LogOutput},
    image::BuildImageOptions,
    network::ListNetworksOptions,
    Docker,
};
use futures_util::TryStreamExt;
use std::collections::HashMap;

pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_defaults()
            .context("Failed to connect to Docker daemon")?;
        Ok(Self { docker })
    }

    pub async fn list_containers(&self, all: bool) -> Result<Vec<bollard::models::Container>> {
        let options = if all {
            Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            })
        } else {
            None
        };

        self.docker
            .list_containers(options)
            .await
            .context("Failed to list containers")
    }

    pub async fn list_networks(&self) -> Result<Vec<bollard::models::Network>> {
        self.docker
            .list_networks(None::<ListNetworksOptions>)
            .await
            .context("Failed to list networks")
    }

    pub async fn inspect_container(&self, name: &str) -> Result<bollard::models::ContainerInspectResponse> {
        self.docker
            .inspect_container(name, None)
            .await
            .context("Failed to inspect container")
    }

    pub async fn get_container_network(&self, name: &str) -> Result<Option<String>> {
        match self.inspect_container(name).await {
            Ok(container) => {
                let networks = container
                    .network_settings
                    .and_then(|ns| ns.networks)
                    .unwrap_or_default();

                if networks.is_empty() {
                    Ok(None)
                } else {
                    Ok(networks.keys().next().cloned())
                }
            }
            Err(e) => {
                tracing::warn!("Failed to inspect container {}: {}", name, e);
                Ok(None)
            }
        }
    }

    pub async fn ensure_network(&self, network_name: &str) -> Result<()> {
        let networks = self.list_networks().await?;
        let network_names: Vec<String> = networks
            .iter()
            .filter_map(|n| n.name.clone())
            .collect();

        if !network_names.contains(&network_name.to_string()) {
            tracing::info!("Creating network: {}", network_name);
            self.docker
                .create_network::<String>(
                    bollard::network::CreateNetworkOptions {
                        name: network_name.to_string(),
                        driver: "bridge".to_string(),
                        ..Default::default()
                    },
                )
                .await
                .context("Failed to create network")?;
        }
        Ok(())
    }

    pub async fn build_image(
        &self,
        build_dir: &std::path::Path,
        tag: &str,
    ) -> Result<()> {
        use bollard::image::BuildImageOptions;

        let tar = tokio::fs::File::open(build_dir).await?;

        let options = BuildImageOptions {
            dockerfile: "Dockerfile",
            t: tag,
            rm: true,
            ..Default::default()
        };

        let mut stream = self.docker.build_image(options, None, None);

        while let Some(result) = stream.next().await {
            match result {
                Ok(output) => {
                    if let Some(error) = output.error {
                        anyhow::bail!("Build failed: {}", error);
                    }
                    if let Some(stream) = output.stream {
                        tracing::info!("{}", stream);
                    }
                }
                Err(e) => return Err(e).context("Build image error"),
            }
        }

        Ok(())
    }

    pub async fn run_container(
        &self,
        image: &str,
        name: &str,
        network: &str,
        port_bindings: HashMap<String, HashMap<(), ()>>,
    ) -> Result<()> {
        use bollard::container::{
            Config, CreateContainerOptions, HostConfig, NetworkingConfig, PortBinding,
        };

        let port_bindings: HashMap<String, Option<Vec<PortBinding>>> = port_bindings
            .keys()
            .map(|p| {
                (
                    p.clone(),
                    Some(vec![PortBinding {
                        host_ip: None,
                        host_port: Some(p.split('/').next().unwrap_or("").to_string()),
                    }]),
                )
            })
            .collect();

        let exposed_ports: HashMap<String, HashMap<(), ()>> = port_bindings
            .keys()
            .map(|p| (p.clone(), HashMap::new()))
            .collect();

        let config = Config {
            image: Some(image.to_string()),
            exposed_ports: Some(exposed_ports),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                ..Default::default()
            }),
            networking_config: Some(NetworkingConfig {
                endpoints_config: {
                    let mut map = HashMap::new();
                    map.insert(
                        network.to_string(),
                        bollard::models::EndpointSettings {
                            network_id: None,
                            endpoints_config: None,
                            gateway: None,
                            ip_address: None,
                            ip_prefix_len: None,
                            ipv6_gateway: None,
                            global_ipv6_address: None,
                            global_ipv6_prefix_len: None,
                            mac_address: None,
                            driver_opt: None,
                            links: None,
                            aliases: None,
                            endpoint_id: None,
                            dns_names: None,
                            ipam_config: None,
                        },
                    );
                    map
                },
            }),
            ..Default::default()
        };

        self.docker
            .create_container::<String, _>(
                Some(CreateContainerOptions { name: name.to_string() }),
                config,
            )
            .await
            .context("Failed to create container")?;

        self.docker
            .start_container::<String>(name, None)
            .await
            .context("Failed to start container")?;

        Ok(())
    }

    pub async fn stop_container(&self, name: &str) -> Result<()> {
        self.docker
            .stop_container(name, None::<u32>)
            .await
            .ok();

        self.docker
            .remove_container(name, None)
            .await
            .context("Failed to remove container")?;

        Ok(())
    }

    pub async fn connect_container_to_network(&self, name: &str, network: &str) -> Result<()> {
        self.docker
            .connect_network(network, bollard::models::EndpointSettings {
                container: Some(name.to_string()),
                ..Default::default()
            })
            .await
            .context("Failed to connect container to network")?;

        Ok(())
    }

    pub async fn get_container(&self, name: &str) -> Result<bollard::models::ContainerInspectResponse> {
        self.docker
            .inspect_container(name, None)
            .await
            .context("Failed to get container")
    }

    pub async fn container_exists(&self, name: &str) -> bool {
        self.get_container(name).await.is_ok()
    }

    pub async fn get_logs(
        &self,
        name: &str,
        tail: i32,
        follow: bool,
    ) -> Result<impl futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>>> {
        let options = bollard::container::LogsOptions::<String> {
            follow: Some(follow),
            stdout: Some(true),
            stderr: Some(true),
            tail: Some(tail.to_string()),
            ..Default::default()
        };

        Ok(self.docker.logs(name, Some(options)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Ignored because it requires Docker to be running
    async fn test_docker_connection() {
        let client = DockerClient::new().await;
        assert!(client.is_ok());
    }
}
