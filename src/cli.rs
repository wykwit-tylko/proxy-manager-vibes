use crate::config::{load_config, save_config, Container, DEFAULT_PORT};
use crate::docker::{detect_containers, list_networks, DockerClient};
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub fn list_containers() {
    let config = load_config().unwrap_or_default();
    let route_map: std::collections::HashMap<String, u16> = config
        .routes
        .iter()
        .map(|r| (r.target.clone(), r.host_port))
        .collect();

    if config.containers.is_empty() {
        println!("No containers configured");
        return;
    }

    println!("Configured containers:");
    for c in &config.containers {
        let host_port = route_map.get(&c.name);
        let marker = if let Some(port) = host_port {
            format!(" (port {})", port)
        } else {
            String::new()
        };
        let label = c
            .label
            .as_ref()
            .map(|l| format!(" - {}", l))
            .unwrap_or_default();
        let port = c.port.unwrap_or(DEFAULT_PORT);
        let net = c.network.as_ref().unwrap_or(&config.network);
        println!("  {}:{}@{}{}{}", c.name, port, net, label, marker);
    }
}

pub async fn cli_list_networks() {
    println!("Available Docker networks:");
    match list_networks().await {
        Ok(networks) => {
            for net in networks {
                println!(
                    "  {:<25} driver={:<10} containers={:<4} scope={}",
                    net.name, net.driver, net.containers_count, net.scope
                );
            }
        }
        Err(e) => println!("Error listing networks: {}", e),
    }
}

pub async fn status() {
    let config = load_config().unwrap_or_default();
    let proxy_name = crate::config::get_proxy_name(Some(&config));
    let docker = match DockerClient::new() {
        Ok(d) => d,
        Err(e) => {
            println!("Error connecting to Docker: {}", e);
            return;
        }
    };

    match docker.get_container_status(&proxy_name).await {
        Ok(Some(status)) => {
            println!("Proxy: {} ({})", proxy_name, status);
            println!();
            println!("Active routes:");
            for route in &config.routes {
                let host_port = route.host_port;
                let target = route.target.clone();
                let target_container = config.containers.iter().find(|c| c.name == target);

                if let Some(container) = target_container {
                    let internal_port = container.port.unwrap_or(DEFAULT_PORT);
                    println!("  {} -> {}:{}", host_port, target, internal_port);
                } else {
                    println!("  {} -> {} (container not found)", host_port, target);
                }
            }
        }
        _ => {
            println!("Proxy not running");
        }
    }
}

pub fn install_cli() {
    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("proxy-manager");
    let user_bin = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/bin");
    let hardlink = user_bin.join("proxy-manager");

    if let Err(e) = fs::create_dir_all(&user_bin) {
        println!("Error creating directory: {}", e);
        return;
    }

    if hardlink.exists() || hardlink.is_symlink() {
        if let Err(e) = fs::remove_file(&hardlink) {
            println!("Error removing existing link: {}", e);
            return;
        }
    }

    if let Err(e) = fs::hard_link(&script_path, &hardlink) {
        println!("Error creating hardlink: {}", e);
        return;
    }

    println!(
        "Created hardlink: {} -> {}",
        hardlink.display(),
        script_path.display()
    );
    println!();
    println!("See 'proxy-manager --help' for a quick start guide.");

    let path_str = user_bin.to_string_lossy();
    let path_string = path_str.to_string();
    if !std::env::var("PATH")
        .unwrap_or_default()
        .contains(&path_string)
    {
        println!("NOTE: Add ~/.local/bin to your PATH:");
        println!("  export PATH=\"{}:$PATH\"", path_str);
        println!("  # Add to ~/.bashrc or ~/.zshrc to persist");
    }
}

pub fn show_config() {
    let config = load_config().unwrap_or_default();
    let config_file = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("proxy-manager")
        .join("proxy-config.json");

    println!("Config file: {}", config_file.display());
    println!();
    println!(
        "{}",
        serde_json::to_string_pretty(&config).unwrap_or_default()
    );
}

pub async fn show_logs(follow: bool, tail: usize) -> Result<()> {
    let config = load_config().unwrap_or_default();
    let proxy_name = crate::config::get_proxy_name(Some(&config));

    let docker = match DockerClient::new() {
        Ok(d) => d,
        Err(e) => {
            println!("Error connecting to Docker: {}", e);
            return Ok(());
        }
    };

    match docker.get_container_status(&proxy_name).await {
        Ok(Some(_)) => {
            println!("Logs for: {}", proxy_name);
            println!("{}", "-".repeat(50));

            match docker.get_container_logs(&proxy_name, follow, tail).await {
                Ok(logs) => {
                    for log_line in logs {
                        print!("{}", log_line);
                    }
                }
                Err(e) => {
                    println!("Error getting logs: {}", e);
                }
            }
        }
        Ok(None) => {
            println!("Proxy container '{}' not running", proxy_name);
        }
        Err(e) => {
            println!("Error checking proxy status: {}", e);
        }
    }

    Ok(())
}

pub async fn add_container(
    container_name: &str,
    label: Option<&str>,
    port: Option<u16>,
    network: Option<&str>,
) -> Result<()> {
    let mut config = load_config()?;

    let network = if let Some(net) = network {
        Some(net.to_string())
    } else {
        let docker = DockerClient::new()?;
        if let Ok(Some(detected)) = docker.get_container_network(container_name).await {
            println!("Auto-detected network: {}", detected);
            Some(detected)
        } else {
            None
        }
    };

    let existing = config
        .containers
        .iter_mut()
        .find(|c| c.name == container_name);
    if let Some(existing) = existing {
        if let Some(l) = label {
            existing.label = Some(l.to_string());
        }
        if let Some(p) = port {
            existing.port = Some(p);
        }
        if let Some(n) = network {
            existing.network = Some(n);
        }
        save_config(&config)?;
        println!("Updated container: {}", container_name);
    } else {
        let entry = Container {
            name: container_name.to_string(),
            label: label.map(|l| l.to_string()),
            port,
            network,
        };
        config.containers.push(entry);
        save_config(&config)?;
        println!("Added container: {}", container_name);
    }

    Ok(())
}

pub async fn remove_container(identifier: &str) -> Result<bool> {
    let mut config = load_config()?;

    let identifier_str = identifier.to_string();
    let container_name = config
        .containers
        .iter()
        .find(|c| c.name == identifier_str || c.label.as_ref() == Some(&identifier_str))
        .map(|c| c.name.clone());

    if container_name.is_none() {
        println!("Error: Container '{}' not found in config", identifier);
        return Ok(false);
    }

    let container_name = container_name.unwrap();
    config.containers.retain(|c| c.name != container_name);
    config.routes.retain(|r| r.target != container_name);
    save_config(&config)?;
    println!("Removed container: {}", container_name);

    Ok(true)
}

pub async fn switch_target(identifier: &str, port: Option<u16>) -> Result<bool> {
    crate::proxy::switch_target(identifier, port).await
}

pub async fn cli_detect_containers(filter: Option<&str>) {
    match detect_containers(filter).await {
        Ok(containers) => {
            println!("Running containers:");
            for c in containers {
                println!("  {}", c);
            }
        }
        Err(e) => println!("Error detecting containers: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Container;

    #[test]
    fn test_container_display() {
        let container = Container {
            name: "test".to_string(),
            label: Some("Test Label".to_string()),
            port: Some(8080),
            network: Some("test-net".to_string()),
        };
        assert_eq!(container.name, "test");
        assert_eq!(container.label, Some("Test Label".to_string()));
    }
}
