use anyhow::{Result, bail};

use crate::config::{AppConfig, ContainerConfig, RouteConfig, find_container};
use crate::paths::DEFAULT_PORT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchResult {
    Added,
    Updated,
}

pub fn upsert_container(
    config: &mut AppConfig,
    name: &str,
    label: Option<String>,
    port: Option<u16>,
    network: Option<String>,
) -> bool {
    if let Some(existing) = config.containers.iter_mut().find(|c| c.name == name) {
        if let Some(label) = label {
            existing.label = Some(label);
        }
        if let Some(port) = port {
            existing.port = Some(port);
        }
        if let Some(network) = network {
            existing.network = Some(network);
        }
        return true;
    }

    let entry = ContainerConfig {
        name: name.to_string(),
        label,
        port,
        network,
    };
    config.containers.push(entry);
    false
}

pub fn remove_container(config: &mut AppConfig, identifier: &str) -> Option<String> {
    let container = find_container(config, identifier)?.clone();
    let name = container.name.clone();
    config.containers.retain(|c| c.name != name);
    config.routes.retain(|r| r.target != name);
    Some(name)
}

pub fn switch_route(
    config: &mut AppConfig,
    identifier: &str,
    host_port: Option<u16>,
) -> Result<SwitchResult> {
    let container_name = find_container(config, identifier)
        .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?
        .name
        .clone();
    let host_port = host_port.unwrap_or(DEFAULT_PORT);

    if let Some(route) = config.routes.iter_mut().find(|r| r.host_port == host_port) {
        route.target = container_name.clone();
        return Ok(SwitchResult::Updated);
    }

    config.routes.push(RouteConfig {
        host_port,
        target: container_name,
    });
    config.routes.sort_by_key(|r| r.host_port);
    Ok(SwitchResult::Added)
}

pub fn list_containers(config: &AppConfig) -> Vec<String> {
    let mut route_map = std::collections::HashMap::new();
    for route in &config.routes {
        route_map.insert(route.target.clone(), route.host_port);
    }

    config
        .containers
        .iter()
        .map(|c| {
            let host_port = route_map.get(&c.name).copied();
            let port = c.port.unwrap_or(DEFAULT_PORT);
            let network = c.network.as_deref().unwrap_or(config.network.as_str());
            let mut line = format!("{}:{}@{}", c.name, port, network);
            if let Some(label) = &c.label {
                line.push_str(&format!(" - {}", label));
            }
            if let Some(host_port) = host_port {
                line.push_str(&format!(" (port {})", host_port));
            }
            line
        })
        .collect()
}

pub fn ensure_has_containers(config: &AppConfig) -> Result<()> {
    if config.containers.is_empty() {
        bail!("No containers configured. Use 'add' command first.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn upsert_adds_new_container() {
        let mut config = AppConfig::default();
        let existed = upsert_container(&mut config, "svc", Some("Label".to_string()), None, None);
        assert!(!existed);
        assert_eq!(config.containers.len(), 1);
    }

    #[test]
    fn upsert_updates_existing() {
        let mut config = AppConfig::default();
        upsert_container(&mut config, "svc", None, None, None);
        let existed = upsert_container(
            &mut config,
            "svc",
            Some("Label".to_string()),
            Some(9000),
            None,
        );
        assert!(existed);
        assert_eq!(config.containers[0].label.as_deref(), Some("Label"));
        assert_eq!(config.containers[0].port, Some(9000));
    }

    #[test]
    fn remove_container_deletes_routes() {
        let mut config = AppConfig::default();
        upsert_container(&mut config, "svc", None, None, None);
        config.routes.push(RouteConfig {
            host_port: 8000,
            target: "svc".to_string(),
        });
        let removed = remove_container(&mut config, "svc");
        assert_eq!(removed.as_deref(), Some("svc"));
        assert!(config.routes.is_empty());
    }

    #[test]
    fn switch_route_adds_and_updates() {
        let mut config = AppConfig::default();
        upsert_container(&mut config, "svc", None, None, None);
        let added = switch_route(&mut config, "svc", Some(8001)).unwrap();
        assert_eq!(added, SwitchResult::Added);
        let updated = switch_route(&mut config, "svc", Some(8001)).unwrap();
        assert_eq!(updated, SwitchResult::Updated);
    }

    #[test]
    fn list_containers_renders_info() {
        let mut config = AppConfig::default();
        upsert_container(
            &mut config,
            "svc",
            Some("Label".to_string()),
            Some(9000),
            None,
        );
        config.routes.push(RouteConfig {
            host_port: 8001,
            target: "svc".to_string(),
        });
        let lines = list_containers(&config);
        assert!(lines[0].contains("svc:9000@"));
        assert!(lines[0].contains("Label"));
        assert!(lines[0].contains("port 8001"));
    }
}
