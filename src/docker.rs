use bollard::container::{Config, ListContainersOptions, StartContainerOptions};
use bollard::image::BuildImageOptions;
use bollard::network::CreateNetworkOptions;
use bollard::Docker;
use futures_util::StreamExt;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub containers_count: usize,
}

pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub fn new() -> Result<Self, anyhow::Error> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub async fn list_containers(
        &self,
        filter: Option<&str>,
    ) -> Result<Vec<String>, anyhow::Error> {
        let mut filters = HashMap::new();
        filters.insert("all", vec!["true"]);

        if let Some(f) = filter {
            filters.insert("name", vec![f]);
        }

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;

        Ok(containers
            .into_iter()
            .filter_map(|c| c.names.map(|n| n.join(",")).or(c.id))
            .collect())
    }

    pub async fn list_networks(&self) -> Result<Vec<NetworkInfo>, anyhow::Error> {
        let networks = self.docker.list_networks::<&str>(None).await?;

        Ok(networks
            .into_iter()
            .map(|net| NetworkInfo {
                name: net.name.unwrap_or_default(),
                driver: net.driver.unwrap_or_default(),
                scope: net.scope.unwrap_or_default(),
                containers_count: net.containers.as_ref().map_or(0, |c| c.len()),
            })
            .collect())
    }

    pub async fn get_container_network(
        &self,
        container_name: &str,
    ) -> Result<Option<String>, anyhow::Error> {
        let container = self.docker.inspect_container(container_name, None).await?;

        let networks = container.network_settings.and_then(|nw| nw.networks);

        if let Some(networks) = networks {
            if let Some((name, _)) = networks.into_iter().next() {
                return Ok(Some(name));
            }
        }

        Ok(None)
    }

    pub async fn container_exists(&self, name: &str) -> bool {
        self.docker.inspect_container(name, None).await.is_ok()
    }

    pub async fn get_container_status(&self, name: &str) -> Result<Option<String>, anyhow::Error> {
        match self.docker.inspect_container(name, None).await {
            Ok(container) => {
                let state = container.state;
                let status = state.and_then(|s| s.status).map(|s| s.to_string());
                Ok(status)
            }
            Err(_) => Ok(None),
        }
    }

    pub async fn create_network(&self, name: &str) -> Result<(), anyhow::Error> {
        let options = CreateNetworkOptions::<&str> {
            name,
            driver: "bridge",
            check_duplicate: true,
            ..Default::default()
        };

        self.docker.create_network(options).await?;
        Ok(())
    }

    pub async fn network_exists(&self, name: &str) -> bool {
        self.docker
            .inspect_network::<&str>(name, None)
            .await
            .is_ok()
    }

    pub async fn build_image(
        &self,
        build_dir: &std::path::Path,
        tag: &str,
    ) -> Result<(), anyhow::Error> {
        let _options = BuildImageOptions {
            t: tag,
            rm: true,
            ..Default::default()
        };

        let mut cmd = std::process::Command::new("docker");
        cmd.arg("build")
            .arg("-t")
            .arg(tag)
            .arg(build_dir.to_str().unwrap_or("."));

        let output = cmd
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute docker build: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Docker build failed: {}", stderr));
        }

        Ok(())
    }

    pub async fn start_container(
        &self,
        name: &str,
        image: &str,
        network: &str,
        ports: &HashMap<String, u16>,
    ) -> Result<(), anyhow::Error> {
        let port_bindings: HashMap<String, Option<Vec<bollard::models::PortBinding>>> = ports
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    Some(vec![bollard::models::PortBinding {
                        host_ip: None,
                        host_port: Some(v.to_string()),
                    }]),
                )
            })
            .collect();

        let config = Config {
            image: Some(image.to_string()),
            host_config: Some(bollard::models::HostConfig {
                port_bindings: Some(port_bindings),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker
            .create_container(
                Some(bollard::container::CreateContainerOptions {
                    name: name.to_string(),
                    ..Default::default()
                }),
                config,
            )
            .await?;

        self.docker
            .start_container::<&str>(name, Some(StartContainerOptions::default()))
            .await?;

        if network != "bridge" {
            self.connect_container_to_network(name, network).await?;
        }

        Ok(())
    }

    pub async fn stop_container(&self, name: &str) -> Result<(), anyhow::Error> {
        let _ = self.docker.stop_container(name, None).await;
        let _ = self.docker.remove_container(name, None).await;
        Ok(())
    }

    pub async fn connect_container_to_network(
        &self,
        container_name: &str,
        network: &str,
    ) -> Result<(), anyhow::Error> {
        self.docker
            .connect_network(
                network,
                bollard::network::ConnectNetworkOptions::<&str> {
                    container: container_name,
                    endpoint_config: Default::default(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn get_container_logs(
        &self,
        name: &str,
        follow: bool,
        tail: usize,
    ) -> Result<Vec<String>, anyhow::Error> {
        let tail_str = if tail == 0 {
            "all".to_string()
        } else {
            tail.to_string()
        };

        let options = bollard::container::LogsOptions {
            follow,
            tail: tail_str.as_str(),
            stdout: true,
            stderr: false,
            since: 0,
            until: 0,
            timestamps: false,
        };

        let log_stream = self.docker.logs(name, Some(options));

        let mut logs = Vec::new();

        tokio::pin!(log_stream);

        while let Some(result) = log_stream.next().await {
            match result {
                Ok(log_output) => logs.push(log_output.to_string()),
                Err(_) => break,
            }
        }

        Ok(logs)
    }
}

pub async fn ensure_network(network_name: &str) -> Result<String, anyhow::Error> {
    let docker = DockerClient::new()?;

    if !docker.network_exists(network_name).await {
        println!("Creating network: {}", network_name);
        docker.create_network(network_name).await?;
    }

    Ok(network_name.to_string())
}

pub async fn detect_containers(filter: Option<&str>) -> Result<Vec<String>, anyhow::Error> {
    let docker = DockerClient::new()?;
    docker.list_containers(filter).await
}

pub async fn list_networks() -> Result<Vec<NetworkInfo>, anyhow::Error> {
    let docker = DockerClient::new()?;
    docker.list_networks().await
}

pub async fn get_container_network(container_name: &str) -> Result<Option<String>, anyhow::Error> {
    let docker = DockerClient::new()?;
    docker.get_container_network(container_name).await
}

pub async fn container_exists(name: &str) -> bool {
    if let Ok(docker) = DockerClient::new() {
        docker.container_exists(name).await
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_info_default() {
        let info = NetworkInfo {
            name: "test".to_string(),
            driver: "bridge".to_string(),
            scope: "local".to_string(),
            containers_count: 0,
        };
        assert_eq!(info.name, "test");
    }

    #[test]
    fn test_network_info_partial() {
        let info = NetworkInfo {
            name: "test-net".to_string(),
            driver: "overlay".to_string(),
            scope: "swarm".to_string(),
            containers_count: 5,
        };
        assert_eq!(info.name, "test-net");
        assert_eq!(info.driver, "overlay");
        assert_eq!(info.scope, "swarm");
        assert_eq!(info.containers_count, 5);
    }
}
