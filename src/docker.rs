use crate::config::Config as AppConfig;
use crate::nginx::{generate_dockerfile, generate_nginx_config};
use anyhow::{anyhow, Result};
use bollard::container::LogOutput;
use bollard::container::{
    InspectContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::image::BuildImageOptions;
use bollard::models::Network;
use bollard::network::{ConnectNetworkOptions, CreateNetworkOptions, ListNetworksOptions};
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use tokio::fs;

pub struct DockerManager {
    docker: Docker,
}

impl DockerManager {
    pub fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub async fn list_containers(&self, filter: Option<&str>) -> Result<Vec<String>> {
        let mut filters = HashMap::new();
        if let Some(pattern) = filter {
            filters.insert("name".to_string(), vec![pattern.to_string()]);
        }

        let options = ListContainersOptions::<String> {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;
        Ok(containers
            .into_iter()
            .flat_map(|c| c.names)
            .flatten()
            .map(|n| n.trim_start_matches('/').to_string())
            .collect())
    }

    pub async fn list_networks(&self) -> Result<Vec<Network>> {
        let networks = self
            .docker
            .list_networks(None::<ListNetworksOptions<String>>)
            .await?;
        Ok(networks)
    }

    pub async fn ensure_network(&self, network_name: &str) -> Result<()> {
        let networks = self.list_networks().await?;
        if !networks
            .iter()
            .any(|n| n.name.as_deref() == Some(network_name))
        {
            let options = CreateNetworkOptions {
                name: network_name,
                driver: "bridge",
                ..Default::default()
            };
            self.docker.create_network(options).await?;
        }
        Ok(())
    }

    pub async fn build_proxy(&self, config: &AppConfig) -> Result<()> {
        let config_dir = crate::config::get_config_dir();
        let build_dir = config_dir.join("build");
        fs::create_dir_all(&build_dir).await?;

        let nginx_conf = generate_nginx_config(config);
        fs::write(build_dir.join("nginx.conf"), nginx_conf).await?;

        let host_ports: Vec<u16> = config.routes.iter().map(|r| r.host_port).collect();
        let dockerfile = generate_dockerfile(&host_ports);
        fs::write(build_dir.join("Dockerfile"), dockerfile).await?;

        let mut header = tar::Header::new_gnu();

        let mut tar = tar::Builder::new(Vec::new());

        let nginx_conf_content = fs::read(build_dir.join("nginx.conf")).await?;
        header.set_size(nginx_conf_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "nginx.conf", &nginx_conf_content[..])?;

        let dockerfile_content = fs::read(build_dir.join("Dockerfile")).await?;
        header.set_size(dockerfile_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "Dockerfile", &dockerfile_content[..])?;

        tar.finish()?;
        let tar_data = tar.into_inner()?;

        let image_name = format!("{}:latest", config.proxy_name);
        let options = BuildImageOptions {
            t: image_name,
            rm: true,
            ..Default::default()
        };

        let mut build_stream = self
            .docker
            .build_image(options, None, Some(tar_data.into()));
        while let Some(chunk) = build_stream.next().await {
            match chunk {
                Ok(c) => {
                    if let Some(error) = c.error {
                        return Err(anyhow!("Build error: {}", error));
                    }
                    if let Some(stream) = c.stream {
                        print!("{}", stream);
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }

        Ok(())
    }

    pub async fn start_proxy(&self, config: &AppConfig) -> Result<()> {
        self.ensure_network(&config.network).await?;

        let mut networks = std::collections::HashSet::new();
        networks.insert(config.network.clone());
        for c in &config.containers {
            if let Some(net) = &c.network {
                networks.insert(net.clone());
            }
        }

        for net in &networks {
            self.ensure_network(net).await?;
        }

        if self
            .docker
            .inspect_container(&config.proxy_name, None::<InspectContainerOptions>)
            .await
            .is_ok()
        {
            println!("Proxy already running: {}", config.proxy_name);
            return Ok(());
        }

        self.build_proxy(config).await?;

        let mut port_bindings = HashMap::new();
        for route in &config.routes {
            let port_str = format!("{}/tcp", route.host_port);
            port_bindings.insert(
                port_str,
                Some(vec![bollard::models::PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(route.host_port.to_string()),
                }]),
            );
        }

        let host_config = bollard::models::HostConfig {
            port_bindings: Some(port_bindings),
            network_mode: Some(config.network.clone()),
            ..Default::default()
        };

        let image_name = format!("{}:latest", config.proxy_name);
        let options = bollard::container::CreateContainerOptions {
            name: config.proxy_name.clone(),
            ..Default::default()
        };

        let container_config = bollard::container::Config {
            image: Some(image_name),
            host_config: Some(host_config),
            ..Default::default()
        };

        self.docker
            .create_container(Some(options), container_config)
            .await?;
        self.docker
            .start_container(&config.proxy_name, None::<StartContainerOptions<String>>)
            .await?;

        // Connect to other networks
        for net in networks {
            if net != config.network {
                let options = ConnectNetworkOptions {
                    container: config.proxy_name.clone(),
                    endpoint_config: Default::default(),
                };
                if let Err(e) = self.docker.connect_network(&net, options).await {
                    println!("Warning: Could not connect to network {}: {}", net, e);
                }
            }
        }

        Ok(())
    }

    pub async fn stop_proxy(&self, proxy_name: &str) -> Result<()> {
        match self
            .docker
            .stop_container(proxy_name, None::<StopContainerOptions>)
            .await
        {
            Ok(_) => {
                self.docker
                    .remove_container(proxy_name, None::<RemoveContainerOptions>)
                    .await?;
                println!("Proxy stopped");
            }
            Err(_) => {
                println!("Proxy not running or could not be stopped");
            }
        }
        Ok(())
    }

    pub async fn get_container_logs(
        &self,
        proxy_name: &str,
        tail: usize,
        follow: bool,
    ) -> Result<()> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            follow,
            ..Default::default()
        };

        let mut stream = self.docker.logs(proxy_name, Some(options));
        while let Some(log) = stream.next().await {
            match log {
                Ok(output) => match output {
                    LogOutput::StdOut { message } => {
                        print!("{}", String::from_utf8_lossy(&message))
                    }
                    LogOutput::StdErr { message } => {
                        eprint!("{}", String::from_utf8_lossy(&message))
                    }
                    LogOutput::Console { message } => {
                        print!("{}", String::from_utf8_lossy(&message))
                    }
                    LogOutput::StdIn { .. } => {}
                },
                Err(e) => return Err(anyhow!("Error reading logs: {}", e)),
            }
        }
        Ok(())
    }

    pub async fn get_container_network(&self, container_name: &str) -> Result<Option<String>> {
        match self
            .docker
            .inspect_container(container_name, None::<InspectContainerOptions>)
            .await
        {
            Ok(inspect) => {
                if let Some(network_settings) = inspect.network_settings {
                    if let Some(networks) = network_settings.networks {
                        return Ok(networks.keys().next().cloned());
                    }
                }
                Ok(None)
            }
            Err(_) => Ok(None),
        }
    }
}
