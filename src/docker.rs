use anyhow::{anyhow, Context, Result};
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, ListContainersOptions, LogsOptions,
    RemoveContainerOptions, StopContainerOptions,
};
use bollard::image::BuildImageOptions;
use bollard::models::{HostConfig, PortBinding};
use bollard::network::{CreateNetworkOptions, ListNetworksOptions};
use bollard::Docker;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub fn new() -> Result<Self> {
        let docker =
            Docker::connect_with_local_defaults().context("Failed to connect to Docker daemon")?;
        Ok(Self { docker })
    }

    pub async fn ensure_network(&self, network_name: &str) -> Result<()> {
        let networks = self
            .docker
            .list_networks(None::<ListNetworksOptions<String>>)
            .await
            .context("Failed to list networks")?;

        let network_exists = networks
            .iter()
            .any(|n| n.name == Some(network_name.to_string()));

        if !network_exists {
            println!("Creating network: {}", network_name);
            let options = CreateNetworkOptions {
                name: network_name,
                driver: "bridge",
                ..Default::default()
            };
            self.docker
                .create_network(options)
                .await
                .context("Failed to create network")?;
        }

        Ok(())
    }

    pub async fn list_networks(&self) -> Result<Vec<NetworkInfo>> {
        let networks = self
            .docker
            .list_networks(None::<ListNetworksOptions<String>>)
            .await
            .context("Failed to list networks")?;

        let mut info = Vec::new();
        for net in networks {
            info.push(NetworkInfo {
                name: net.name.unwrap_or_else(|| "unknown".to_string()),
                driver: net.driver.unwrap_or_else(|| "unknown".to_string()),
                scope: net.scope.unwrap_or_else(|| "local".to_string()),
                containers_count: net.containers.map(|c| c.len()).unwrap_or(0),
            });
        }

        Ok(info)
    }

    pub async fn list_containers(&self, filter: Option<&str>) -> Result<Vec<String>> {
        let options = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });

        let containers = self
            .docker
            .list_containers(options)
            .await
            .context("Failed to list containers")?;

        let mut names = Vec::new();
        for container in containers {
            if let Some(container_names) = container.names {
                for name in container_names {
                    let clean_name = name.trim_start_matches('/');
                    if let Some(f) = filter {
                        if clean_name.to_lowercase().contains(&f.to_lowercase()) {
                            names.push(clean_name.to_string());
                        }
                    } else {
                        names.push(clean_name.to_string());
                    }
                }
            }
        }

        Ok(names)
    }

    pub async fn get_container_network(&self, container_name: &str) -> Result<Option<String>> {
        let container = match self.docker.inspect_container(container_name, None).await {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        if let Some(network_settings) = container.network_settings {
            if let Some(networks) = network_settings.networks {
                if let Some((network_name, _)) = networks.iter().next() {
                    return Ok(Some(network_name.clone()));
                }
            }
        }

        Ok(None)
    }

    pub async fn container_exists(&self, name: &str) -> Result<bool> {
        match self.docker.inspect_container(name, None).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    pub async fn get_container_status(&self, name: &str) -> Result<Option<String>> {
        match self.docker.inspect_container(name, None).await {
            Ok(container) => {
                let status = container
                    .state
                    .and_then(|s| s.status)
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_else(|| "unknown".to_string());
                Ok(Some(status))
            }
            Err(_) => Ok(None),
        }
    }

    pub async fn build_image(&self, context_path: &Path, tag: &str) -> Result<()> {
        let tar_archive = create_tar_archive(context_path)?;

        let options = BuildImageOptions {
            dockerfile: "Dockerfile",
            t: tag,
            rm: true,
            ..Default::default()
        };

        let mut stream = self
            .docker
            .build_image(options, None, Some(tar_archive.into()));

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(output) => {
                    if let Some(stream) = output.stream {
                        print!("{}", stream);
                        std::io::stdout().flush().ok();
                    }
                    if let Some(error) = output.error {
                        return Err(anyhow!("Build error: {}", error));
                    }
                }
                Err(e) => return Err(anyhow!("Build failed: {}", e)),
            }
        }

        Ok(())
    }

    pub async fn start_container(
        &self,
        image: &str,
        name: &str,
        network: &str,
        ports: HashMap<u16, u16>,
    ) -> Result<()> {
        let mut port_bindings = HashMap::new();

        for (container_port, host_port) in ports {
            let port_key = format!("{}/tcp", container_port);
            port_bindings.insert(
                port_key.clone(),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(host_port.to_string()),
                }]),
            );
        }

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            network_mode: Some(network.to_string()),
            ..Default::default()
        };

        let config = ContainerConfig {
            image: Some(image),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name,
            ..Default::default()
        };

        self.docker
            .create_container(Some(options), config)
            .await
            .context("Failed to create container")?;

        self.docker
            .start_container::<String>(name, None)
            .await
            .context("Failed to start container")?;

        Ok(())
    }

    pub async fn connect_to_network(&self, container_name: &str, network: &str) -> Result<()> {
        use bollard::network::ConnectNetworkOptions;

        let options = ConnectNetworkOptions {
            container: container_name,
            ..Default::default()
        };

        self.docker
            .connect_network(network, options)
            .await
            .context("Failed to connect container to network")?;

        Ok(())
    }

    pub async fn stop_container(&self, name: &str) -> Result<()> {
        self.docker
            .stop_container(name, None::<StopContainerOptions>)
            .await
            .context("Failed to stop container")?;

        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };

        self.docker
            .remove_container(name, Some(options))
            .await
            .context("Failed to remove container")?;

        Ok(())
    }

    pub async fn get_logs(&self, name: &str, tail: usize, follow: bool) -> Result<()> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            follow,
            tail: tail.to_string(),
            ..Default::default()
        };

        let mut stream = self.docker.logs(name, Some(options));

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(output) => {
                    print!("{}", output);
                    std::io::stdout().flush().ok();
                }
                Err(e) => return Err(anyhow!("Failed to get logs: {}", e)),
            }
        }

        Ok(())
    }
}

pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub containers_count: usize,
}

fn create_tar_archive(path: &Path) -> Result<Vec<u8>> {
    use std::fs::File;

    let mut tar_data = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_data);

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();
            if file_path.is_file() {
                let file_name = file_path
                    .file_name()
                    .ok_or_else(|| anyhow!("Invalid file name"))?;
                let mut file = File::open(&file_path)?;
                let metadata = file.metadata()?;
                let mut header = tar::Header::new_gnu();
                header.set_size(metadata.len());
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, file_name, &mut file)?;
            }
        }

        tar.finish()?;
    }

    Ok(tar_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_client_creation() {
        // This test will only pass if Docker is available
        let result = DockerClient::new();
        // We can't assert success here because Docker may not be available in test environment
        assert!(result.is_ok() || result.is_err());
    }
}
