use anyhow::{Context, Result};
use bollard::{
    container::{
        Config, CreateContainerOptions, ListContainersOptions, LogsOptions, NetworkingConfig,
        StopContainerOptions,
    },
    image::BuildImageOptions,
    network::{ConnectNetworkOptions, CreateNetworkOptions},
    Docker,
};
use futures_util::StreamExt;
use std::collections::HashMap;

#[derive(Clone)]
pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub async fn new() -> Result<Self> {
        let docker =
            Docker::connect_with_defaults().context("Failed to connect to Docker daemon")?;
        Ok(Self { docker })
    }

    pub async fn list_containers(
        &self,
        all: bool,
    ) -> Result<Vec<bollard::models::ContainerSummary>> {
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
            .list_networks::<String>(None)
            .await
            .context("Failed to list networks")
    }

    pub async fn inspect_container(
        &self,
        name: &str,
    ) -> Result<bollard::models::ContainerInspectResponse> {
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
        let network_names: Vec<String> = networks.iter().filter_map(|n| n.name.clone()).collect();

        if !network_names.contains(&network_name.to_string()) {
            tracing::info!("Creating network: {}", network_name);
            self.docker
                .create_network::<String>(CreateNetworkOptions {
                    name: network_name.to_string(),
                    driver: "bridge".to_string(),
                    ..Default::default()
                })
                .await
                .context("Failed to create network")?;
        }
        Ok(())
    }

    pub async fn build_image(&self, build_dir: &std::path::Path, tag: &str) -> Result<()> {
        use tokio::io::AsyncReadExt;

        let tar = tokio::fs::File::open(build_dir).await?;
        let metadata = tar.metadata().await?;
        let mut reader = tokio::io::BufReader::new(tar);
        let mut buffer = vec![0; metadata.len().try_into().unwrap_or(1024 * 1024)];
        reader.read_to_end(&mut buffer).await?;

        let options = BuildImageOptions {
            dockerfile: "Dockerfile".to_string(),
            t: tag.to_string(),
            rm: true,
            ..Default::default()
        };

        let mut stream = self.docker.build_image(options, None, Some(buffer.into()));

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
        let mut pb_map: HashMap<String, Option<Vec<bollard::models::PortBinding>>> = HashMap::new();

        for p in port_bindings.keys() {
            let port = p.split('/').next().unwrap_or("");
            pb_map.insert(
                p.clone(),
                Some(vec![bollard::models::PortBinding {
                    host_ip: None,
                    host_port: Some(port.to_string()),
                }]),
            );
        }

        let exposed_ports: HashMap<String, HashMap<(), ()>> = port_bindings
            .keys()
            .map(|p| (p.clone(), HashMap::new()))
            .collect();

        let config = Config {
            image: Some(image.to_string()),
            exposed_ports: Some(exposed_ports),
            host_config: Some(bollard::models::HostConfig {
                port_bindings: Some(pb_map),
                ..Default::default()
            }),
            networking_config: Some(NetworkingConfig {
                endpoints_config: {
                    let mut map = HashMap::new();
                    map.insert(
                        network.to_string(),
                        bollard::models::EndpointSettings {
                            network_id: None,
                            gateway: None,
                            ip_address: None,
                            ip_prefix_len: None,
                            ipv6_gateway: None,
                            global_ipv6_address: None,
                            global_ipv6_prefix_len: None,
                            mac_address: None,
                            driver_opts: None,
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
                Some(CreateContainerOptions {
                    name: name.to_string(),
                    platform: None,
                }),
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
            .stop_container(name, Some(StopContainerOptions { t: 10i64 }))
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
            .connect_network(
                network,
                ConnectNetworkOptions {
                    container: name.to_string(),
                    endpoint_config: bollard::models::EndpointSettings {
                        network_id: None,
                        gateway: None,
                        ip_address: None,
                        ip_prefix_len: None,
                        ipv6_gateway: None,
                        global_ipv6_address: None,
                        global_ipv6_prefix_len: None,
                        mac_address: None,
                        driver_opts: None,
                        links: None,
                        aliases: None,
                        endpoint_id: None,
                        dns_names: None,
                        ipam_config: None,
                    },
                },
            )
            .await
            .context("Failed to connect container to network")?;

        Ok(())
    }

    pub async fn get_container(
        &self,
        name: &str,
    ) -> Result<bollard::models::ContainerInspectResponse> {
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
    ) -> Result<
        impl futures_util::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>>,
    > {
        let options = LogsOptions::<String> {
            follow,
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };

        Ok(self.docker.logs(name, Some(options)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_docker_connection() {
        let client = DockerClient::new().await;
        assert!(client.is_ok());
    }
}
