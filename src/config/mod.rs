use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8000;
pub const DEFAULT_PROXY_NAME: &str = "proxy-manager";
pub const DEFAULT_NETWORK: &str = "proxy-net";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Route {
    pub host_port: u16,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub containers: Vec<ContainerConfig>,
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
            proxy_name: DEFAULT_PROXY_NAME.to_string(),
            network: DEFAULT_NETWORK.to_string(),
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("proxy-manager")
    }

    pub fn config_file() -> PathBuf {
        Self::config_dir().join("proxy-config.json")
    }

    pub fn build_dir() -> PathBuf {
        Self::config_dir().join("build")
    }

    pub fn load() -> anyhow::Result<Self> {
        let config_file = Self::config_file();
        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)?;
            let config: Config = serde_json::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let config_dir = Self::config_dir();
        std::fs::create_dir_all(&config_dir)?;
        let config_file = Self::config_file();
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_file, content)?;
        Ok(())
    }

    pub fn get_proxy_name(&self) -> &str {
        &self.proxy_name
    }

    pub fn get_proxy_image(&self) -> String {
        format!("{}:latest", self.proxy_name)
    }

    pub fn get_network_name(&self) -> &str {
        &self.network
    }

    pub fn get_internal_port(&self, target_container: Option<&ContainerConfig>) -> u16 {
        target_container
            .and_then(|c| c.port)
            .unwrap_or(DEFAULT_PORT)
    }

    pub fn get_all_host_ports(&self) -> Vec<u16> {
        if self.routes.is_empty() {
            vec![DEFAULT_PORT]
        } else {
            self.routes.iter().map(|r| r.host_port).collect()
        }
    }

    pub fn find_container(&self, identifier: &str) -> Option<&ContainerConfig> {
        self.containers
            .iter()
            .find(|c| c.name == identifier || c.label.as_deref() == Some(identifier))
    }

    pub fn find_container_mut(&mut self, identifier: &str) -> Option<&mut ContainerConfig> {
        self.containers
            .iter_mut()
            .find(|c| c.name == identifier || c.label.as_deref() == Some(identifier))
    }

    pub fn find_route(&self, host_port: u16) -> Option<&Route> {
        self.routes.iter().find(|r| r.host_port == host_port)
    }

    pub fn find_route_mut(&mut self, host_port: u16) -> Option<&mut Route> {
        self.routes.iter_mut().find(|r| r.host_port == host_port)
    }

    pub fn add_or_update_container(
        &mut self,
        name: String,
        label: Option<String>,
        port: Option<u16>,
        network: Option<String>,
    ) -> bool {
        if let Some(existing) = self.containers.iter_mut().find(|c| c.name == name) {
            // Update existing
            if label.is_some() {
                existing.label = label;
            }
            if port.is_some() {
                existing.port = port;
            }
            if network.is_some() {
                existing.network = network;
            }
            false
        } else {
            // Add new
            self.containers.push(ContainerConfig {
                name,
                label,
                port,
                network,
            });
            true
        }
    }

    pub fn remove_container(&mut self, identifier: &str) -> Option<ContainerConfig> {
        let idx = self
            .containers
            .iter()
            .position(|c| c.name == identifier || c.label.as_deref() == Some(identifier))?;
        let container = self.containers.remove(idx);
        // Also remove routes pointing to this container
        self.routes.retain(|r| r.target != container.name);
        Some(container)
    }

    pub fn add_or_update_route(&mut self, host_port: u16, target: String) {
        if let Some(existing) = self.routes.iter_mut().find(|r| r.host_port == host_port) {
            existing.target = target;
        } else {
            self.routes.push(Route { host_port, target });
            self.routes.sort_by_key(|r| r.host_port);
        }
    }

    pub fn remove_route(&mut self, host_port: u16) -> Option<Route> {
        let idx = self.routes.iter().position(|r| r.host_port == host_port)?;
        Some(self.routes.remove(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_config() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("proxy-config.json");
        (temp_dir, config_path)
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
        assert_eq!(config.proxy_name, DEFAULT_PROXY_NAME);
        assert_eq!(config.network, DEFAULT_NETWORK);
    }

    #[test]
    fn test_find_container() {
        let mut config = Config::default();
        config.containers.push(ContainerConfig {
            name: "test-container".to_string(),
            label: Some("test-label".to_string()),
            port: Some(8080),
            network: None,
        });

        assert!(config.find_container("test-container").is_some());
        assert!(config.find_container("test-label").is_some());
        assert!(config.find_container("nonexistent").is_none());
    }

    #[test]
    fn test_add_or_update_container() {
        let mut config = Config::default();

        // Add new container
        let is_new = config.add_or_update_container(
            "container1".to_string(),
            Some("label1".to_string()),
            Some(8080),
            None,
        );
        assert!(is_new);
        assert_eq!(config.containers.len(), 1);

        // Update existing
        let is_new = config.add_or_update_container(
            "container1".to_string(),
            Some("new-label".to_string()),
            None,
            None,
        );
        assert!(!is_new);
        assert_eq!(config.containers.len(), 1);
        assert_eq!(config.containers[0].label, Some("new-label".to_string()));
    }

    #[test]
    fn test_remove_container() {
        let mut config = Config::default();
        config.containers.push(ContainerConfig {
            name: "container1".to_string(),
            label: None,
            port: None,
            network: None,
        });
        config.routes.push(Route {
            host_port: 8000,
            target: "container1".to_string(),
        });

        let removed = config.remove_container("container1");
        assert!(removed.is_some());
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty()); // Route should also be removed
    }

    #[test]
    fn test_add_or_update_route() {
        let mut config = Config::default();

        config.add_or_update_route(8000, "container1".to_string());
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.routes[0].host_port, 8000);

        // Update existing
        config.add_or_update_route(8000, "container2".to_string());
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.routes[0].target, "container2");
    }

    #[test]
    fn test_get_all_host_ports() {
        let mut config = Config::default();
        assert_eq!(config.get_all_host_ports(), vec![DEFAULT_PORT]);

        config.routes.push(Route {
            host_port: 8080,
            target: "c1".to_string(),
        });
        config.routes.push(Route {
            host_port: 9090,
            target: "c2".to_string(),
        });

        let ports = config.get_all_host_ports();
        assert_eq!(ports.len(), 2);
        assert!(ports.contains(&8080));
        assert!(ports.contains(&9090));
    }

    #[test]
    fn test_serialization() {
        let config = Config {
            containers: vec![ContainerConfig {
                name: "test".to_string(),
                label: Some("Test".to_string()),
                port: Some(8080),
                network: Some("test-net".to_string()),
            }],
            routes: vec![Route {
                host_port: 8000,
                target: "test".to_string(),
            }],
            proxy_name: "proxy-manager".to_string(),
            network: "proxy-net".to_string(),
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }
}
