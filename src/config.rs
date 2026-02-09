use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default port used when none is specified.
pub const DEFAULT_PORT: u16 = 8000;

/// Default proxy container name.
const DEFAULT_PROXY_NAME: &str = "proxy-manager";

/// Default Docker network name.
const DEFAULT_NETWORK: &str = "proxy-net";

/// A registered container in the proxy configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Container {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

/// A route mapping a host port to a target container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Route {
    pub host_port: u16,
    pub target: String,
}

/// The top-level proxy manager configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub containers: Vec<Container>,
    #[serde(default)]
    pub routes: Vec<Route>,
    #[serde(default = "default_proxy_name")]
    pub proxy_name: String,
    #[serde(default = "default_network")]
    pub network: String,
}

fn default_proxy_name() -> String {
    DEFAULT_PROXY_NAME.to_string()
}

fn default_network() -> String {
    DEFAULT_NETWORK.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            containers: Vec::new(),
            routes: Vec::new(),
            proxy_name: default_proxy_name(),
            network: default_network(),
        }
    }
}

impl Config {
    /// Returns the proxy container name.
    pub fn proxy_name(&self) -> &str {
        &self.proxy_name
    }

    /// Returns the proxy Docker image tag.
    pub fn proxy_image(&self) -> String {
        format!("{}:latest", self.proxy_name)
    }

    /// Returns the default network name.
    pub fn network_name(&self) -> &str {
        &self.network
    }

    /// Returns the internal port for a given container, defaulting to `DEFAULT_PORT`.
    pub fn internal_port(container: &Container) -> u16 {
        container.port.unwrap_or(DEFAULT_PORT)
    }

    /// Returns all host ports from configured routes, or `[DEFAULT_PORT]` if none.
    pub fn all_host_ports(&self) -> Vec<u16> {
        if self.routes.is_empty() {
            vec![DEFAULT_PORT]
        } else {
            self.routes.iter().map(|r| r.host_port).collect()
        }
    }

    /// Find a container by name or label.
    pub fn find_container(&self, identifier: &str) -> Option<&Container> {
        self.containers
            .iter()
            .find(|c| c.name == identifier || c.label.as_deref() == Some(identifier))
    }

    /// Find a route by host port.
    pub fn find_route(&self, host_port: u16) -> Option<&Route> {
        self.routes.iter().find(|r| r.host_port == host_port)
    }

    /// Find a route by host port (mutable).
    pub fn find_route_mut(&mut self, host_port: u16) -> Option<&mut Route> {
        self.routes.iter_mut().find(|r| r.host_port == host_port)
    }

    /// Add or update a container in the configuration.
    /// Returns `true` if it was an update, `false` if newly added.
    pub fn add_container(
        &mut self,
        name: &str,
        label: Option<&str>,
        port: Option<u16>,
        network: Option<&str>,
    ) -> bool {
        if let Some(existing) = self.containers.iter_mut().find(|c| c.name == name) {
            if let Some(l) = label {
                existing.label = Some(l.to_string());
            }
            if let Some(p) = port {
                existing.port = Some(p);
            }
            if let Some(n) = network {
                existing.network = Some(n.to_string());
            }
            true
        } else {
            self.containers.push(Container {
                name: name.to_string(),
                label: label.map(|s| s.to_string()),
                port,
                network: network.map(|s| s.to_string()),
            });
            false
        }
    }

    /// Remove a container (by name or label) and any routes targeting it.
    /// Returns the removed container's name, or `None` if not found.
    pub fn remove_container(&mut self, identifier: &str) -> Option<String> {
        let container_name = self.find_container(identifier)?.name.clone();
        self.containers.retain(|c| c.name != container_name);
        self.routes.retain(|r| r.target != container_name);
        Some(container_name)
    }

    /// Set or update a route for the given host port to the given container.
    /// Returns `true` if an existing route was updated, `false` if a new one was added.
    pub fn set_route(&mut self, host_port: u16, target: &str) -> bool {
        if let Some(route) = self.find_route_mut(host_port) {
            route.target = target.to_string();
            true
        } else {
            self.routes.push(Route {
                host_port,
                target: target.to_string(),
            });
            self.routes.sort_by_key(|r| r.host_port);
            false
        }
    }

    /// Remove a route by host port. Returns the removed route, or `None`.
    pub fn remove_route(&mut self, host_port: u16) -> Option<Route> {
        if let Some(idx) = self.routes.iter().position(|r| r.host_port == host_port) {
            Some(self.routes.remove(idx))
        } else {
            None
        }
    }

    /// Collect all unique networks that containers belong to (including the default).
    pub fn all_networks(&self) -> Vec<String> {
        let mut nets = std::collections::BTreeSet::new();
        nets.insert(self.network.clone());
        for c in &self.containers {
            if let Some(n) = &c.network {
                nets.insert(n.clone());
            }
        }
        nets.into_iter().collect()
    }
}

/// Returns the configuration directory path.
pub fn config_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("proxy-manager")
}

/// Returns the configuration file path.
pub fn config_file() -> PathBuf {
    config_dir().join("proxy-config.json")
}

/// Returns the build directory path.
pub fn build_dir() -> PathBuf {
    config_dir().join("build")
}

/// Load configuration from disk, returning defaults if the file doesn't exist.
pub fn load_config() -> Result<Config> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;

    let path = config_file();
    if path.exists() {
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: Config = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(config)
    } else {
        Ok(Config::default())
    }
}

/// Save configuration to disk.
pub fn save_config(config: &Config) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;

    let path = config_file();
    let data = serde_json::to_string_pretty(config).context("Failed to serialize config")?;
    std::fs::write(&path, data)
        .with_context(|| format!("Failed to write config file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> Config {
        Config {
            containers: vec![
                Container {
                    name: "app-v1".to_string(),
                    label: Some("Version 1".to_string()),
                    port: Some(8080),
                    network: None,
                },
                Container {
                    name: "app-v2".to_string(),
                    label: None,
                    port: None,
                    network: Some("custom-net".to_string()),
                },
            ],
            routes: vec![Route {
                host_port: 8000,
                target: "app-v1".to_string(),
            }],
            proxy_name: "my-proxy".to_string(),
            network: "proxy-net".to_string(),
        }
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
        assert_eq!(config.proxy_name, "proxy-manager");
        assert_eq!(config.network, "proxy-net");
    }

    #[test]
    fn test_proxy_image() {
        let config = sample_config();
        assert_eq!(config.proxy_image(), "my-proxy:latest");
    }

    #[test]
    fn test_internal_port_with_port() {
        let c = Container {
            name: "test".to_string(),
            label: None,
            port: Some(9090),
            network: None,
        };
        assert_eq!(Config::internal_port(&c), 9090);
    }

    #[test]
    fn test_internal_port_default() {
        let c = Container {
            name: "test".to_string(),
            label: None,
            port: None,
            network: None,
        };
        assert_eq!(Config::internal_port(&c), DEFAULT_PORT);
    }

    #[test]
    fn test_all_host_ports_empty_routes() {
        let config = Config::default();
        assert_eq!(config.all_host_ports(), vec![DEFAULT_PORT]);
    }

    #[test]
    fn test_all_host_ports_with_routes() {
        let config = sample_config();
        assert_eq!(config.all_host_ports(), vec![8000]);
    }

    #[test]
    fn test_find_container_by_name() {
        let config = sample_config();
        let c = config.find_container("app-v1").unwrap();
        assert_eq!(c.name, "app-v1");
    }

    #[test]
    fn test_find_container_by_label() {
        let config = sample_config();
        let c = config.find_container("Version 1").unwrap();
        assert_eq!(c.name, "app-v1");
    }

    #[test]
    fn test_find_container_not_found() {
        let config = sample_config();
        assert!(config.find_container("nonexistent").is_none());
    }

    #[test]
    fn test_find_route() {
        let config = sample_config();
        let r = config.find_route(8000).unwrap();
        assert_eq!(r.target, "app-v1");
        assert!(config.find_route(9999).is_none());
    }

    #[test]
    fn test_add_container_new() {
        let mut config = Config::default();
        let was_update = config.add_container("new-app", Some("New App"), Some(3000), None);
        assert!(!was_update);
        assert_eq!(config.containers.len(), 1);
        assert_eq!(config.containers[0].name, "new-app");
        assert_eq!(config.containers[0].label.as_deref(), Some("New App"));
        assert_eq!(config.containers[0].port, Some(3000));
    }

    #[test]
    fn test_add_container_update() {
        let mut config = sample_config();
        let was_update = config.add_container("app-v1", Some("Updated"), Some(9999), None);
        assert!(was_update);
        let c = config.find_container("app-v1").unwrap();
        assert_eq!(c.label.as_deref(), Some("Updated"));
        assert_eq!(c.port, Some(9999));
    }

    #[test]
    fn test_remove_container() {
        let mut config = sample_config();
        let removed = config.remove_container("app-v1");
        assert_eq!(removed, Some("app-v1".to_string()));
        assert!(config.find_container("app-v1").is_none());
        // Route targeting app-v1 should also be removed
        assert!(config.find_route(8000).is_none());
    }

    #[test]
    fn test_remove_container_by_label() {
        let mut config = sample_config();
        let removed = config.remove_container("Version 1");
        assert_eq!(removed, Some("app-v1".to_string()));
    }

    #[test]
    fn test_remove_container_not_found() {
        let mut config = sample_config();
        assert!(config.remove_container("nonexistent").is_none());
    }

    #[test]
    fn test_set_route_new() {
        let mut config = sample_config();
        let was_update = config.set_route(9000, "app-v2");
        assert!(!was_update);
        assert_eq!(config.routes.len(), 2);
        // Routes should be sorted
        assert_eq!(config.routes[0].host_port, 8000);
        assert_eq!(config.routes[1].host_port, 9000);
    }

    #[test]
    fn test_set_route_update() {
        let mut config = sample_config();
        let was_update = config.set_route(8000, "app-v2");
        assert!(was_update);
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.routes[0].target, "app-v2");
    }

    #[test]
    fn test_remove_route() {
        let mut config = sample_config();
        let removed = config.remove_route(8000);
        assert!(removed.is_some());
        assert!(config.routes.is_empty());
    }

    #[test]
    fn test_remove_route_not_found() {
        let mut config = sample_config();
        assert!(config.remove_route(9999).is_none());
    }

    #[test]
    fn test_all_networks() {
        let config = sample_config();
        let nets = config.all_networks();
        assert!(nets.contains(&"proxy-net".to_string()));
        assert!(nets.contains(&"custom-net".to_string()));
        assert_eq!(nets.len(), 2);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = sample_config();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_deserialize_empty_json() {
        let json = "{}";
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
        assert_eq!(config.proxy_name, "proxy-manager");
        assert_eq!(config.network, "proxy-net");
    }
}
