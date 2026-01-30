use anyhow::{Context, Result};
use async_trait::async_trait;
use bollard::Docker;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::errors::Error as BollardError;
use bollard::image::BuildImageOptions;
use bollard::models::{HostConfig, PortBinding};
use bollard::network::{ConnectNetworkOptions, CreateNetworkOptions, ListNetworksOptions};
use futures_util::stream::TryStreamExt;
use std::collections::HashMap;
use std::io::Cursor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkSummary {
    pub name: String,
    pub driver: String,
    pub containers: usize,
    pub scope: String,
}

#[async_trait]
pub trait DockerApi: Send + Sync {
    async fn list_containers(&self, all: bool) -> Result<Vec<String>>;
    async fn list_networks(&self) -> Result<Vec<NetworkSummary>>;
    async fn ensure_network(&self, network: &str) -> Result<()>;
    async fn container_primary_network(&self, container: &str) -> Result<Option<String>>;
    async fn container_exists(&self, name: &str) -> Result<bool>;
    async fn container_status(&self, name: &str) -> Result<Option<String>>;
    async fn stop_and_remove_container(&self, name: &str) -> Result<bool>;
    async fn build_image_from_tar(&self, tag: &str, tar_bytes: Vec<u8>) -> Result<()>;
    async fn run_container_with_ports(
        &self,
        name: &str,
        image: &str,
        network: &str,
        ports: &[u16],
    ) -> Result<()>;
    async fn connect_container_to_network(&self, container: &str, network: &str) -> Result<()>;
    async fn stream_logs(&self, name: &str, follow: bool, tail: usize) -> Result<Vec<String>>;
}

#[derive(Clone)]
pub struct BollardDocker {
    docker: Docker,
}

impl BollardDocker {
    pub fn connect_local() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    fn is_not_found(err: &BollardError) -> bool {
        matches!(err, BollardError::DockerResponseServerError { status_code, .. } if *status_code == 404)
    }
}

#[derive(Clone, Default)]
pub struct NoopDocker;

#[async_trait]
impl DockerApi for NoopDocker {
    async fn list_containers(&self, _all: bool) -> Result<Vec<String>> {
        Err(anyhow::anyhow!("Docker not available for this command"))
    }

    async fn list_networks(&self) -> Result<Vec<NetworkSummary>> {
        Err(anyhow::anyhow!("Docker not available for this command"))
    }

    async fn ensure_network(&self, _network: &str) -> Result<()> {
        Err(anyhow::anyhow!("Docker not available for this command"))
    }

    async fn container_primary_network(&self, _container: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn container_exists(&self, _name: &str) -> Result<bool> {
        Ok(false)
    }

    async fn container_status(&self, _name: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn stop_and_remove_container(&self, _name: &str) -> Result<bool> {
        Ok(false)
    }

    async fn build_image_from_tar(&self, _tag: &str, _tar_bytes: Vec<u8>) -> Result<()> {
        Err(anyhow::anyhow!("Docker not available for this command"))
    }

    async fn run_container_with_ports(
        &self,
        _name: &str,
        _image: &str,
        _network: &str,
        _ports: &[u16],
    ) -> Result<()> {
        Err(anyhow::anyhow!("Docker not available for this command"))
    }

    async fn connect_container_to_network(&self, _container: &str, _network: &str) -> Result<()> {
        Err(anyhow::anyhow!("Docker not available for this command"))
    }

    async fn stream_logs(&self, _name: &str, _follow: bool, _tail: usize) -> Result<Vec<String>> {
        Err(anyhow::anyhow!("Docker not available for this command"))
    }
}

#[async_trait]
impl DockerApi for BollardDocker {
    async fn list_containers(&self, all: bool) -> Result<Vec<String>> {
        let mut opts = bollard::container::ListContainersOptions::<String> {
            all,
            ..Default::default()
        };
        // Avoid huge payload by not requesting sizes.
        opts.size = false;

        let containers = self
            .docker
            .list_containers(Some(opts))
            .await
            .context("list docker containers")?;

        let mut names = Vec::new();
        for c in containers {
            if let Some(raw_names) = c.names {
                for n in raw_names {
                    let trimmed = n.trim_start_matches('/');
                    if !trimmed.is_empty() {
                        names.push(trimmed.to_string());
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    async fn list_networks(&self) -> Result<Vec<NetworkSummary>> {
        let nets = self
            .docker
            .list_networks(Some(ListNetworksOptions::<String> {
                ..Default::default()
            }))
            .await
            .context("list docker networks")?;

        let mut out = Vec::new();
        for n in nets {
            let name = n.name.unwrap_or_else(|| "<unknown>".to_string());
            let driver = n.driver.unwrap_or_else(|| "unknown".to_string());
            let scope = n.scope.unwrap_or_else(|| "local".to_string());
            let containers = n.containers.as_ref().map(|m| m.len()).unwrap_or(0);
            out.push(NetworkSummary {
                name,
                driver,
                containers,
                scope,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn ensure_network(&self, network: &str) -> Result<()> {
        let existing = self
            .docker
            .list_networks(Some(ListNetworksOptions::<String> {
                filters: {
                    let mut m = HashMap::new();
                    m.insert("name".to_string(), vec![network.to_string()]);
                    m
                },
            }))
            .await
            .context("list docker networks")?;

        if existing.iter().any(|n| n.name.as_deref() == Some(network)) {
            return Ok(());
        }

        self.docker
            .create_network(CreateNetworkOptions {
                name: network,
                check_duplicate: true,
                driver: "bridge",
                ..Default::default()
            })
            .await
            .with_context(|| format!("create docker network '{network}'"))?;

        Ok(())
    }

    async fn container_primary_network(&self, container: &str) -> Result<Option<String>> {
        let inspected = match self.docker.inspect_container(container, None).await {
            Ok(v) => v,
            Err(e) if Self::is_not_found(&e) => return Ok(None),
            Err(e) => return Err(e).context("inspect docker container"),
        };

        let networks = inspected
            .network_settings
            .and_then(|ns| ns.networks)
            .unwrap_or_default();

        Ok(networks.keys().next().cloned())
    }

    async fn container_exists(&self, name: &str) -> Result<bool> {
        match self.docker.inspect_container(name, None).await {
            Ok(_) => Ok(true),
            Err(e) if Self::is_not_found(&e) => Ok(false),
            Err(e) => Err(e).context("inspect docker container"),
        }
    }

    async fn container_status(&self, name: &str) -> Result<Option<String>> {
        let inspected = match self.docker.inspect_container(name, None).await {
            Ok(v) => v,
            Err(e) if Self::is_not_found(&e) => return Ok(None),
            Err(e) => return Err(e).context("inspect docker container"),
        };
        Ok(inspected
            .state
            .and_then(|s| s.status)
            .map(|s| s.to_string()))
    }

    async fn stop_and_remove_container(&self, name: &str) -> Result<bool> {
        let exists = self.container_exists(name).await?;
        if !exists {
            return Ok(false);
        }

        let _ = self
            .docker
            .stop_container(name, Some(StopContainerOptions { t: 10 }))
            .await;

        self.docker
            .remove_container(
                name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .with_context(|| format!("remove container '{name}'"))?;

        Ok(true)
    }

    async fn build_image_from_tar(&self, tag: &str, tar_bytes: Vec<u8>) -> Result<()> {
        let opts = BuildImageOptions::<String> {
            t: tag.to_string(),
            rm: true,
            ..Default::default()
        };

        // Drain build output to completion.
        let mut s = self
            .docker
            .build_image(opts, None, Some(bytes::Bytes::from(tar_bytes)));
        while let Some(_msg) = s.try_next().await? {
            // Intentionally ignore build progress for now.
        }
        Ok(())
    }

    async fn run_container_with_ports(
        &self,
        name: &str,
        image: &str,
        network: &str,
        ports: &[u16],
    ) -> Result<()> {
        let mut exposed_ports = HashMap::new();
        let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();

        for port in ports {
            let key = format!("{port}/tcp");
            exposed_ports.insert(key.clone(), HashMap::new());
            port_bindings.insert(
                key,
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(port.to_string()),
                }]),
            );
        }

        let cfg = ContainerConfig {
            image: Some(image.to_string()),
            exposed_ports: Some(exposed_ports),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                network_mode: Some(network.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name,
                    platform: None,
                }),
                cfg,
            )
            .await
            .with_context(|| format!("create container '{name}'"))?;

        self.docker
            .start_container(name, None::<StartContainerOptions<String>>)
            .await
            .with_context(|| format!("start container '{name}'"))?;

        Ok(())
    }

    async fn connect_container_to_network(&self, container: &str, network: &str) -> Result<()> {
        self.docker
            .connect_network(
                network,
                ConnectNetworkOptions {
                    container,
                    endpoint_config: Default::default(),
                },
            )
            .await
            .with_context(|| format!("connect container '{container}' to network '{network}'"))?;
        Ok(())
    }

    async fn stream_logs(&self, name: &str, follow: bool, tail: usize) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut stream = self.docker.logs(
            name,
            Some(LogsOptions::<String> {
                follow,
                stdout: true,
                stderr: true,
                tail: tail.to_string(),
                ..Default::default()
            }),
        );

        while let Some(chunk) = stream.try_next().await? {
            let bytes = chunk.into_bytes();
            let s = String::from_utf8_lossy(&bytes);
            for line in s.lines() {
                out.push(line.to_string());
            }
            if !follow {
                // When not following, bollard should finish naturally; keep collecting.
            }
        }

        Ok(out)
    }
}

pub fn tar_directory_with_dockerfile_and_conf(
    dockerfile: &str,
    nginx_conf: &str,
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();

    {
        let mut builder = tar::Builder::new(&mut buf);

        let mut dockerfile_header = tar::Header::new_gnu();
        dockerfile_header.set_mode(0o644);
        dockerfile_header.set_size(dockerfile.len() as u64);
        dockerfile_header.set_cksum();
        builder
            .append_data(
                &mut dockerfile_header,
                "Dockerfile",
                Cursor::new(dockerfile.as_bytes()),
            )
            .context("add Dockerfile to tar")?;

        let mut nginx_header = tar::Header::new_gnu();
        nginx_header.set_mode(0o644);
        nginx_header.set_size(nginx_conf.len() as u64);
        nginx_header.set_cksum();
        builder
            .append_data(
                &mut nginx_header,
                "nginx.conf",
                Cursor::new(nginx_conf.as_bytes()),
            )
            .context("add nginx.conf to tar")?;
        builder.finish().context("finish tar")?;
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tar_builder_includes_expected_files() {
        let tar_bytes =
            tar_directory_with_dockerfile_and_conf("FROM nginx\n", "events {}\n").unwrap();
        let mut ar = tar::Archive::new(Cursor::new(tar_bytes));
        let mut names = ar
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, vec!["Dockerfile", "nginx.conf"]);
    }
}
