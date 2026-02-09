use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use bollard::Docker;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, ListContainersOptions,
    RemoveContainerOptions, StopContainerOptions,
};
use bollard::image::BuildImageOptions;
use bollard::models::{ContainerSummary, HostConfig, Network, PortBinding};
use bollard::network::{CreateNetworkOptions, ListNetworksOptions};
use futures_util::StreamExt;

/// Information about a running Docker container.
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub name: String,
    pub status: String,
    pub networks: Vec<String>,
}

/// Information about a Docker network.
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub container_count: usize,
}

/// Create a Docker client connection.
pub fn create_client() -> Result<Docker> {
    Docker::connect_with_local_defaults().context("Failed to connect to Docker daemon")
}

/// List all containers, optionally filtered by name pattern.
pub async fn list_containers(
    docker: &Docker,
    filter_pattern: Option<&str>,
) -> Result<Vec<ContainerInfo>> {
    let options = ListContainersOptions::<String> {
        all: true,
        ..Default::default()
    };

    let containers = docker
        .list_containers(Some(options))
        .await
        .context("Failed to list containers")?;

    let mut result = Vec::new();
    for c in containers {
        let names = extract_container_names(&c);
        let status = c.status.clone().unwrap_or_default();
        let networks = extract_container_networks(&c);

        for name in names {
            if let Some(pattern) = filter_pattern
                && !name.to_lowercase().contains(&pattern.to_lowercase())
            {
                continue;
            }
            result.push(ContainerInfo {
                name,
                status: status.clone(),
                networks: networks.clone(),
            });
        }
    }

    Ok(result)
}

/// List all Docker networks.
pub async fn list_networks(docker: &Docker) -> Result<Vec<NetworkInfo>> {
    let networks = docker
        .list_networks(Some(ListNetworksOptions::<String> {
            ..Default::default()
        }))
        .await
        .context("Failed to list networks")?;

    Ok(networks.iter().map(network_to_info).collect())
}

fn network_to_info(net: &Network) -> NetworkInfo {
    NetworkInfo {
        name: net.name.clone().unwrap_or_default(),
        driver: net.driver.clone().unwrap_or_else(|| "unknown".to_string()),
        scope: net.scope.clone().unwrap_or_else(|| "local".to_string()),
        container_count: net.containers.as_ref().map_or(0, |c| c.len()),
    }
}

/// Get the network name for a specific container.
pub async fn get_container_network(docker: &Docker, container_name: &str) -> Option<String> {
    let info = docker.inspect_container(container_name, None).await.ok()?;
    let networks = info.network_settings?.networks?;
    networks.keys().next().cloned()
}

/// Ensure a Docker network exists, creating it if necessary.
pub async fn ensure_network(docker: &Docker, network_name: &str) -> Result<()> {
    let networks = docker
        .list_networks(Some(ListNetworksOptions::<String> {
            ..Default::default()
        }))
        .await
        .context("Failed to list networks")?;

    let exists = networks
        .iter()
        .any(|n| n.name.as_deref() == Some(network_name));

    if !exists {
        println!("Creating network: {network_name}");
        docker
            .create_network(CreateNetworkOptions {
                name: network_name,
                driver: "bridge",
                ..Default::default()
            })
            .await
            .with_context(|| format!("Failed to create network: {network_name}"))?;
    }

    Ok(())
}

/// Build a Docker image from a build directory.
pub async fn build_image(docker: &Docker, build_path: &Path, tag: &str) -> Result<()> {
    // Create a tar archive of the build directory
    let tar_data = create_tar_archive(build_path).with_context(|| {
        format!(
            "Failed to create build archive from {}",
            build_path.display()
        )
    })?;

    let options = BuildImageOptions {
        t: tag,
        rm: true,
        ..Default::default()
    };

    let mut stream = docker.build_image(options, None, Some(tar_data.into()));

    while let Some(result) = stream.next().await {
        let info = result.context("Build stream error")?;
        if let Some(error) = info.error {
            anyhow::bail!("Build failed: {error}");
        }
    }

    Ok(())
}

/// Create a tar archive from a directory.
fn create_tar_archive(dir: &Path) -> Result<Vec<u8>> {
    let buf = Vec::new();
    let mut archive = tar::Builder::new(buf);

    for entry in std::fs::read_dir(dir).context("Failed to read build directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let name = path
                .file_name()
                .context("Invalid file name")?
                .to_string_lossy();
            let mut file = std::fs::File::open(&path)?;
            archive.append_file(name.as_ref(), &mut file)?;
        }
    }

    let mut buf = archive
        .into_inner()
        .context("Failed to finalize tar archive")?;
    buf.flush()?;
    Ok(buf)
}

/// Get the status of a container by name.
pub async fn get_container_status(docker: &Docker, name: &str) -> Result<Option<String>> {
    match docker.inspect_container(name, None).await {
        Ok(info) => Ok(info
            .state
            .and_then(|s| s.status.map(|st| format!("{st:?}")))),
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(e) => Err(e).context("Failed to inspect container"),
    }
}

/// Check if a container exists.
pub async fn container_exists(docker: &Docker, name: &str) -> Result<bool> {
    match docker.inspect_container(name, None).await {
        Ok(_) => Ok(true),
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(false),
        Err(e) => Err(e).context("Failed to check container existence"),
    }
}

/// Start a container with the given configuration.
pub async fn start_container(
    docker: &Docker,
    name: &str,
    image: &str,
    network: &str,
    port_mappings: &HashMap<u16, u16>,
) -> Result<()> {
    let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
    let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();

    for (container_port, host_port) in port_mappings {
        let key = format!("{container_port}/tcp");
        exposed_ports.insert(key.clone(), HashMap::new());
        port_bindings.insert(
            key,
            Some(vec![PortBinding {
                host_ip: None,
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
        image: Some(image.to_string()),
        exposed_ports: Some(exposed_ports),
        host_config: Some(host_config),
        ..Default::default()
    };

    docker
        .create_container(
            Some(CreateContainerOptions {
                name,
                ..Default::default()
            }),
            config,
        )
        .await
        .with_context(|| format!("Failed to create container: {name}"))?;

    docker
        .start_container::<String>(name, None)
        .await
        .with_context(|| format!("Failed to start container: {name}"))?;

    Ok(())
}

/// Connect a container to an additional network.
pub async fn connect_to_network(docker: &Docker, container: &str, network: &str) -> Result<()> {
    docker
        .connect_network(
            network,
            bollard::network::ConnectNetworkOptions {
                container,
                ..Default::default()
            },
        )
        .await
        .with_context(|| format!("Failed to connect {container} to network {network}"))?;
    Ok(())
}

/// Stop and remove a container.
pub async fn stop_and_remove_container(docker: &Docker, name: &str) -> Result<bool> {
    match docker.inspect_container(name, None).await {
        Ok(_) => {
            // Stop the container (with a timeout)
            let _ = docker
                .stop_container(name, Some(StopContainerOptions { t: 10 }))
                .await;
            // Remove it
            docker
                .remove_container(
                    name,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await
                .with_context(|| format!("Failed to remove container: {name}"))?;
            Ok(true)
        }
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(false),
        Err(e) => Err(e).context("Failed to inspect container"),
    }
}

/// Get logs from a container.
pub async fn get_container_logs(docker: &Docker, name: &str, tail: usize) -> Result<Vec<String>> {
    use bollard::container::LogsOptions;

    let options = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        tail: tail.to_string(),
        ..Default::default()
    };

    let mut stream = docker.logs(name, Some(options));
    let mut lines = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(output) => {
                lines.push(output.to_string().trim_end().to_string());
            }
            Err(e) => {
                return Err(e).context("Failed to read container logs");
            }
        }
    }

    Ok(lines)
}

/// Extract container names from a container summary (strip leading `/`).
fn extract_container_names(c: &ContainerSummary) -> Vec<String> {
    c.names
        .as_ref()
        .map(|names| {
            names
                .iter()
                .map(|n| n.trim_start_matches('/').to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Extract network names from a container summary.
fn extract_container_networks(c: &ContainerSummary) -> Vec<String> {
    c.network_settings
        .as_ref()
        .and_then(|ns| ns.networks.as_ref())
        .map(|nets| nets.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_container_names() {
        let c = ContainerSummary {
            names: Some(vec!["/my-container".to_string(), "/other".to_string()]),
            ..Default::default()
        };
        let names = extract_container_names(&c);
        assert_eq!(names, vec!["my-container", "other"]);
    }

    #[test]
    fn test_extract_container_names_empty() {
        let c = ContainerSummary {
            names: None,
            ..Default::default()
        };
        let names = extract_container_names(&c);
        assert!(names.is_empty());
    }

    #[test]
    fn test_extract_container_networks() {
        let mut networks = HashMap::new();
        networks.insert(
            "bridge".to_string(),
            bollard::models::EndpointSettings::default(),
        );
        networks.insert(
            "custom-net".to_string(),
            bollard::models::EndpointSettings::default(),
        );

        let c = ContainerSummary {
            network_settings: Some(bollard::models::ContainerSummaryNetworkSettings {
                networks: Some(networks),
            }),
            ..Default::default()
        };
        let nets = extract_container_networks(&c);
        assert_eq!(nets.len(), 2);
        assert!(nets.contains(&"bridge".to_string()));
        assert!(nets.contains(&"custom-net".to_string()));
    }

    #[test]
    fn test_network_to_info() {
        let net = Network {
            name: Some("test-net".to_string()),
            driver: Some("bridge".to_string()),
            scope: Some("local".to_string()),
            containers: Some({
                let mut m = HashMap::new();
                m.insert(
                    "abc123".to_string(),
                    bollard::models::NetworkContainer {
                        ..Default::default()
                    },
                );
                m
            }),
            ..Default::default()
        };
        let info = network_to_info(&net);
        assert_eq!(info.name, "test-net");
        assert_eq!(info.driver, "bridge");
        assert_eq!(info.scope, "local");
        assert_eq!(info.container_count, 1);
    }

    #[test]
    fn test_network_to_info_defaults() {
        let net = Network {
            name: None,
            driver: None,
            scope: None,
            containers: None,
            ..Default::default()
        };
        let info = network_to_info(&net);
        assert_eq!(info.name, "");
        assert_eq!(info.driver, "unknown");
        assert_eq!(info.scope, "local");
        assert_eq!(info.container_count, 0);
    }

    #[test]
    fn test_create_tar_archive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();

        let archive = create_tar_archive(dir.path()).unwrap();
        assert!(!archive.is_empty());

        // Verify the archive contains our files
        let mut tar = tar::Archive::new(&archive[..]);
        let entries: Vec<String> = tar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(entries.contains(&"test.txt".to_string()));
        assert!(entries.contains(&"config.json".to_string()));
    }
}
