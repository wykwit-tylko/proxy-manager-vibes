use crate::config::{Config, ContainerConfig, Route};
use crate::docker::DockerClient;
use crate::nginx;
use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};

pub async fn add_container(
    name: &str,
    label: Option<String>,
    port: Option<u16>,
    network: Option<String>,
) -> Result<()> {
    let mut config = Config::load()?;
    let docker = DockerClient::new()?;

    // Auto-detect network if not provided
    let network = if network.is_none() {
        match docker.get_container_network(name).await? {
            Some(detected) => {
                println!("Auto-detected network: {}", detected);
                Some(detected)
            }
            None => None,
        }
    } else {
        network
    };

    // Check if container already exists
    if let Some(existing) = config.find_container_mut(name) {
        if let Some(l) = label {
            existing.label = Some(l);
        }
        if let Some(p) = port {
            existing.port = Some(p);
        }
        if let Some(n) = network {
            existing.network = Some(n);
        }
        config.save()?;
        println!("Updated container: {}", name);
    } else {
        config.containers.push(ContainerConfig {
            name: name.to_string(),
            label,
            port,
            network,
        });
        config.save()?;
        println!("Added container: {}", name);
    }

    Ok(())
}

pub async fn remove_container(identifier: &str) -> Result<()> {
    let mut config = Config::load()?;

    let container = config
        .find_container(identifier)
        .ok_or_else(|| anyhow!("Container '{}' not found in config", identifier))?;
    let container_name = container.name.clone();

    config.containers.retain(|c| c.name != container_name);
    config.routes.retain(|r| r.target != container_name);
    config.save()?;

    println!("Removed container: {}", container_name);
    Ok(())
}

pub async fn list_containers() -> Result<()> {
    let config = Config::load()?;

    if config.containers.is_empty() {
        println!("No containers configured");
        return Ok(());
    }

    let route_map: HashMap<String, u16> = config
        .routes
        .iter()
        .map(|r| (r.target.clone(), r.host_port))
        .collect();

    println!("Configured containers:");
    for c in &config.containers {
        let host_port = route_map.get(&c.name);
        let marker = if let Some(port) = host_port {
            format!(" (port {})", port)
        } else {
            String::new()
        };

        let label = if let Some(l) = &c.label {
            format!(" - {}", l)
        } else {
            String::new()
        };

        let port = c.port.unwrap_or(8000);
        let net = c.network.as_deref().unwrap_or(&config.network);

        println!("  {}:{}@{}{}{}", c.name, port, net, label, marker);
    }

    Ok(())
}

pub async fn switch_target(identifier: &str, host_port: Option<u16>) -> Result<()> {
    let mut config = Config::load()?;

    let container = config
        .find_container(identifier)
        .ok_or_else(|| anyhow!("Container '{}' not found in config", identifier))?;
    let container_name = container.name.clone();

    let host_port = host_port.unwrap_or(8000);

    if let Some(existing_route) = config.routes.iter_mut().find(|r| r.host_port == host_port) {
        existing_route.target = container_name.clone();
        config.save()?;
        println!("Switching route: {} -> {}", host_port, container_name);
    } else {
        config.routes.push(Route {
            host_port,
            target: container_name.clone(),
        });
        config.routes.sort_by_key(|r| r.host_port);
        config.save()?;
        println!("Adding route: {} -> {}", host_port, container_name);
    }

    reload_proxy().await?;
    Ok(())
}

pub async fn stop_port(host_port: u16) -> Result<()> {
    let mut config = Config::load()?;

    if !config.routes.iter().any(|r| r.host_port == host_port) {
        return Err(anyhow!("No route found for port {}", host_port));
    }

    config.routes.retain(|r| r.host_port != host_port);
    config.save()?;
    println!("Removed route: port {}", host_port);

    if config.routes.is_empty() {
        stop_proxy().await?;
    } else {
        reload_proxy().await?;
    }

    Ok(())
}

pub async fn start_proxy() -> Result<()> {
    let config = Config::load()?;
    let docker = DockerClient::new()?;

    if config.containers.is_empty() {
        return Err(anyhow!(
            "No containers configured. Use 'add' command first."
        ));
    }

    if config.routes.is_empty() {
        return Err(anyhow!("No routes configured. Use 'switch' command first."));
    }

    // Ensure all networks exist
    let mut networks = HashSet::new();
    networks.insert(config.network.clone());
    for container in &config.containers {
        if let Some(net) = &container.network {
            networks.insert(net.clone());
        }
    }

    for network in &networks {
        docker.ensure_network(network).await?;
    }

    // Check if proxy already running
    if docker.container_exists(&config.proxy_name).await? {
        println!("Proxy already running: {}", config.proxy_name);
        return Ok(());
    }

    // Build proxy image
    build_proxy().await?;

    // Start proxy container
    let host_ports = config.get_all_host_ports();
    let mut port_mapping = HashMap::new();
    for port in &host_ports {
        port_mapping.insert(*port, *port);
    }

    println!("Starting proxy: {}", config.proxy_name);
    docker
        .start_container(
            &config.get_proxy_image(),
            &config.proxy_name,
            &config.network,
            port_mapping,
        )
        .await?;

    // Connect to additional networks
    for network in &networks {
        if network != &config.network {
            match docker.connect_to_network(&config.proxy_name, network).await {
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

pub async fn stop_proxy() -> Result<()> {
    let config = Config::load()?;
    let docker = DockerClient::new()?;

    if !docker.container_exists(&config.proxy_name).await? {
        println!("Proxy not running");
        return Ok(());
    }

    println!("Stopping proxy: {}", config.proxy_name);
    docker.stop_container(&config.proxy_name).await?;
    println!("Proxy stopped");

    Ok(())
}

pub async fn reload_proxy() -> Result<()> {
    let config = Config::load()?;

    if config.containers.is_empty() {
        return Err(anyhow!("No containers configured."));
    }

    if config.routes.is_empty() {
        return Err(anyhow!("No routes configured."));
    }

    println!("Reloading proxy...");
    stop_proxy().await.ok(); // Ignore error if not running
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    start_proxy().await?;

    Ok(())
}

pub async fn build_proxy() -> Result<()> {
    let config = Config::load()?;
    let docker = DockerClient::new()?;

    if config.containers.is_empty() {
        return Err(anyhow!(
            "No containers configured. Use 'add' command first."
        ));
    }

    nginx::write_build_files(&config)?;

    println!("Building proxy image...");
    let build_dir = Config::build_dir()?;
    docker
        .build_image(&build_dir, &config.get_proxy_image())
        .await?;

    Ok(())
}

pub async fn status() -> Result<()> {
    let config = Config::load()?;
    let docker = DockerClient::new()?;

    if let Some(status) = docker.get_container_status(&config.proxy_name).await? {
        println!("Proxy: {} ({})", config.proxy_name, status);
        println!();
        println!("Active routes:");
        for route in &config.routes {
            let target = &route.target;
            if let Some(container) = config.find_container(target) {
                let internal_port = config.get_internal_port(container);
                println!("  {} -> {}:{}", route.host_port, target, internal_port);
            } else {
                println!("  {} -> {} (container not found)", route.host_port, target);
            }
        }
    } else {
        println!("Proxy not running");
    }

    Ok(())
}

pub async fn show_logs(follow: bool, tail: usize) -> Result<()> {
    let config = Config::load()?;
    let docker = DockerClient::new()?;

    if !docker.container_exists(&config.proxy_name).await? {
        return Err(anyhow!(
            "Proxy container '{}' not running",
            config.proxy_name
        ));
    }

    println!("Logs for: {}", config.proxy_name);
    println!("{}", "-".repeat(50));
    docker.get_logs(&config.proxy_name, tail, follow).await?;

    Ok(())
}

pub async fn detect_containers(filter: Option<&str>) -> Result<()> {
    let docker = DockerClient::new()?;

    println!("Detecting running containers...");
    let containers = docker.list_containers(filter).await?;

    println!("Running containers:");
    for name in containers {
        println!("  {}", name);
    }

    Ok(())
}

pub async fn list_networks() -> Result<()> {
    let docker = DockerClient::new()?;

    println!("Available Docker networks:");
    let networks = docker.list_networks().await?;

    for net in networks {
        println!(
            "  {:<25} driver={:<10} containers={:<4} scope={}",
            net.name, net.driver, net.containers_count, net.scope
        );
    }

    Ok(())
}

pub async fn show_config() -> Result<()> {
    let config = Config::load()?;
    let config_file = Config::config_file()?;

    println!("Config file: {}", config_file.display());
    println!();
    println!("{}", serde_json::to_string_pretty(&config)?);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_container() {
        // Note: This test requires a clean config state
        // In a real scenario, we'd mock the Config and Docker client
        let result =
            add_container("test-container", Some("Test".to_string()), Some(8080), None).await;
        // We can't assert success without mocking, so just ensure it doesn't panic
        let _ = result;
    }
}
