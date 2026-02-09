use std::collections::HashMap;

use anyhow::{Result, bail};
use bollard::Docker;

use crate::config::{self, Config};
use crate::docker;
use crate::nginx;

/// Build the proxy Docker image from the current configuration.
pub async fn build_proxy(docker_client: &Docker, config: &Config) -> Result<()> {
    if config.containers.is_empty() {
        bail!("No containers configured. Use 'add' command first.");
    }

    let build_dir = config::build_dir();
    std::fs::create_dir_all(&build_dir)?;

    // Generate and write nginx.conf
    let nginx_conf = nginx::generate_nginx_config(config);
    std::fs::write(build_dir.join("nginx.conf"), nginx_conf)?;

    // Generate and write Dockerfile
    let host_ports = config.all_host_ports();
    let dockerfile = nginx::generate_dockerfile(&host_ports);
    std::fs::write(build_dir.join("Dockerfile"), dockerfile)?;

    // Build the Docker image
    println!("Building proxy image...");
    let proxy_image = config.proxy_image();
    docker::build_image(docker_client, &build_dir, &proxy_image).await?;

    Ok(())
}

/// Start the proxy container with all configured routes.
pub async fn start_proxy(docker_client: &Docker, config: &Config) -> Result<()> {
    let proxy_name = config.proxy_name();

    if config.containers.is_empty() {
        bail!("No containers configured. Use 'add' command first.");
    }
    if config.routes.is_empty() {
        bail!("No routes configured. Use 'switch' command first.");
    }

    // Ensure all networks exist
    for network in config.all_networks() {
        docker::ensure_network(docker_client, &network).await?;
    }

    // Check if already running
    if docker::container_exists(docker_client, proxy_name).await? {
        println!("Proxy already running: {proxy_name}");
        return Ok(());
    }

    // Build
    build_proxy(docker_client, config).await?;

    // Create port mappings
    let host_ports = config.all_host_ports();
    let port_mappings: HashMap<u16, u16> = host_ports.iter().map(|&p| (p, p)).collect();

    let default_network = config.network_name();
    let proxy_image = config.proxy_image();

    // Start the container
    println!("Starting proxy: {proxy_name}");
    docker::start_container(
        docker_client,
        proxy_name,
        &proxy_image,
        default_network,
        &port_mappings,
    )
    .await?;

    // Connect to additional networks
    for network in config.all_networks() {
        if network != default_network {
            match docker::connect_to_network(docker_client, proxy_name, &network).await {
                Ok(()) => println!("Connected proxy to network: {network}"),
                Err(e) => eprintln!("Warning: Could not connect to network {network}: {e}"),
            }
        }
    }

    let port_str = host_ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    println!("Proxy started on port(s): {port_str}");

    Ok(())
}

/// Stop and remove the proxy container.
pub async fn stop_proxy(docker_client: &Docker, config: &Config) -> Result<bool> {
    let proxy_name = config.proxy_name();
    let removed = docker::stop_and_remove_container(docker_client, proxy_name).await?;
    if removed {
        println!("Proxy stopped");
    } else {
        println!("Proxy not running");
    }
    Ok(removed)
}

/// Stop routing for a specific port.
pub async fn stop_port(docker_client: &Docker, config: &mut Config, host_port: u16) -> Result<()> {
    if config.find_route(host_port).is_none() {
        bail!("No route found for port {host_port}");
    }

    config.remove_route(host_port);
    config::save_config(config)?;
    println!("Removed route: port {host_port}");

    if config.routes.is_empty() {
        stop_proxy(docker_client, config).await?;
    } else {
        reload_proxy(docker_client, config).await?;
    }

    Ok(())
}

/// Reload the proxy by stopping and restarting it.
pub async fn reload_proxy(docker_client: &Docker, config: &Config) -> Result<()> {
    if config.containers.is_empty() {
        bail!("No containers configured.");
    }
    if config.routes.is_empty() {
        bail!("No routes configured.");
    }

    println!("Reloading proxy...");
    stop_proxy(docker_client, config).await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    start_proxy(docker_client, config).await?;

    Ok(())
}

/// Switch a host port to route to a specific container.
pub async fn switch_target(
    docker_client: &Docker,
    config: &mut Config,
    identifier: &str,
    host_port: Option<u16>,
) -> Result<()> {
    let container = config
        .find_container(identifier)
        .ok_or_else(|| anyhow::anyhow!("Container '{identifier}' not found in config"))?;
    let container_name = container.name.clone();

    let host_port = host_port.unwrap_or(config::DEFAULT_PORT);

    let was_update = config.set_route(host_port, &container_name);
    config::save_config(config)?;

    if was_update {
        println!("Switching route: {host_port} -> {container_name}");
    } else {
        println!("Adding route: {host_port} -> {container_name}");
    }

    reload_proxy(docker_client, config).await?;
    Ok(())
}

/// Add a container to the configuration, auto-detecting network if not specified.
pub async fn add_container(
    docker_client: &Docker,
    config: &mut Config,
    container_name: &str,
    label: Option<&str>,
    port: Option<u16>,
    network: Option<&str>,
) -> Result<()> {
    let network = if let Some(n) = network {
        Some(n.to_string())
    } else {
        let detected = docker::get_container_network(docker_client, container_name).await;
        if let Some(ref net) = detected {
            println!("Auto-detected network: {net}");
        }
        detected
    };

    let was_update = config.add_container(container_name, label, port, network.as_deref());
    config::save_config(config)?;

    if was_update {
        println!("Updated container: {container_name}");
    } else {
        println!("Added container: {container_name}");
    }

    Ok(())
}

/// Remove a container from the configuration.
pub fn remove_container(config: &mut Config, identifier: &str) -> Result<()> {
    match config.remove_container(identifier) {
        Some(name) => {
            config::save_config(config)?;
            println!("Removed container: {name}");
            Ok(())
        }
        None => bail!("Container '{identifier}' not found in config"),
    }
}

/// Display the list of configured containers.
pub fn list_containers(config: &Config) {
    if config.containers.is_empty() {
        println!("No containers configured");
        return;
    }

    let route_map: HashMap<&str, u16> = config
        .routes
        .iter()
        .map(|r| (r.target.as_str(), r.host_port))
        .collect();

    println!("Configured containers:");
    for c in &config.containers {
        let host_port = route_map.get(c.name.as_str());
        let marker = host_port
            .map(|p| format!(" (port {p})"))
            .unwrap_or_default();
        let label = c
            .label
            .as_ref()
            .map(|l| format!(" - {l}"))
            .unwrap_or_default();
        let port = Config::internal_port(c);
        let net = c.network.as_deref().unwrap_or(config.network_name());
        println!("  {name}:{port}@{net}{label}{marker}", name = c.name);
    }
}

/// Display the proxy status.
pub async fn show_status(docker_client: &Docker, config: &Config) -> Result<()> {
    let proxy_name = config.proxy_name();

    match docker::get_container_status(docker_client, proxy_name).await? {
        Some(status) => {
            println!("Proxy: {proxy_name} ({status})");
            println!();
            println!("Active routes:");
            for route in &config.routes {
                let target_container = config.containers.iter().find(|c| c.name == route.target);
                if let Some(tc) = target_container {
                    let internal_port = Config::internal_port(tc);
                    println!(
                        "  {} -> {}:{}",
                        route.host_port, route.target, internal_port
                    );
                } else {
                    println!(
                        "  {} -> {} (container not found)",
                        route.host_port, route.target
                    );
                }
            }
        }
        None => {
            println!("Proxy not running");
        }
    }

    Ok(())
}

/// Display the configuration.
pub fn show_config(config: &Config) -> Result<()> {
    let config_file = config::config_file();
    println!("Config file: {}", config_file.display());
    println!();
    println!("{}", serde_json::to_string_pretty(config)?);
    Ok(())
}

/// Display proxy logs.
pub async fn show_logs(docker_client: &Docker, config: &Config, tail: usize) -> Result<()> {
    let proxy_name = config.proxy_name();

    match docker::get_container_logs(docker_client, proxy_name, tail).await {
        Ok(lines) => {
            println!("Logs for: {proxy_name}");
            println!("{}", "-".repeat(50));
            for line in lines {
                println!("{line}");
            }
        }
        Err(_) => {
            println!("Proxy container '{proxy_name}' not running");
        }
    }

    Ok(())
}

/// List Docker networks.
pub async fn show_networks(docker_client: &Docker) -> Result<()> {
    let networks = docker::list_networks(docker_client).await?;
    println!("Available Docker networks:");
    for net in networks {
        println!(
            "  {:<25} driver={:<10} containers={:<4} scope={}",
            net.name, net.driver, net.container_count, net.scope
        );
    }
    Ok(())
}

/// Detect running containers.
pub async fn detect_containers(docker_client: &Docker, filter: Option<&str>) -> Result<()> {
    println!("Detecting running containers...");
    let containers = docker::list_containers(docker_client, filter).await?;
    println!("Running containers:");
    for c in containers {
        println!("  {:<30} {} [{}]", c.name, c.status, c.networks.join(", "));
    }
    Ok(())
}
