use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8000;
pub const DEFAULT_PROXY_NAME: &str = "proxy-manager";
pub const DEFAULT_NETWORK: &str = "proxy-net";

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

impl Container {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: None,
            port: None,
            network: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    pub fn get_port(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Route {
    pub host_port: u16,
    pub target: String,
}

impl Route {
    pub fn new(host_port: u16, target: impl Into<String>) -> Self {
        Self {
            host_port,
            target: target.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub containers: Vec<Container>,
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

    pub fn find_container(&self, identifier: &str) -> Option<&Container> {
        self.containers
            .iter()
            .find(|c| c.name == identifier || c.label.as_deref() == Some(identifier))
    }

    pub fn find_container_mut(&mut self, identifier: &str) -> Option<&mut Container> {
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

    pub fn add_or_update_container(&mut self, container: Container) {
        if let Some(existing) = self
            .containers
            .iter_mut()
            .find(|c| c.name == container.name)
        {
            if container.label.is_some() {
                existing.label = container.label;
            }
            if container.port.is_some() {
                existing.port = container.port;
            }
            if container.network.is_some() {
                existing.network = container.network;
            }
        } else {
            self.containers.push(container);
        }
    }

    pub fn remove_container(&mut self, identifier: &str) -> Option<Container> {
        let idx = self
            .containers
            .iter()
            .position(|c| c.name == identifier || c.label.as_deref() == Some(identifier))?;
        let container = self.containers.remove(idx);
        // Also remove associated routes
        self.routes.retain(|r| r.target != container.name);
        Some(container)
    }

    pub fn get_all_host_ports(&self) -> Vec<u16> {
        if self.routes.is_empty() {
            vec![DEFAULT_PORT]
        } else {
            self.routes.iter().map(|r| r.host_port).collect()
        }
    }

    pub fn get_proxy_image(&self) -> String {
        format!("{}:latest", self.proxy_name)
    }

    pub fn set_route(&mut self, host_port: u16, target: impl Into<String>) {
        let target = target.into();
        if let Some(route) = self.find_route_mut(host_port) {
            route.target = target;
        } else {
            self.routes.push(Route::new(host_port, target));
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

    #[test]
    fn test_container_new() {
        let container = Container::new("test-container");
        assert_eq!(container.name, "test-container");
        assert_eq!(container.label, None);
        assert_eq!(container.port, None);
        assert_eq!(container.network, None);
    }

    #[test]
    fn test_container_builder() {
        let container = Container::new("test")
            .with_label("Test Label")
            .with_port(8080)
            .with_network("custom-net");

        assert_eq!(container.name, "test");
        assert_eq!(container.label, Some("Test Label".to_string()));
        assert_eq!(container.port, Some(8080));
        assert_eq!(container.network, Some("custom-net".to_string()));
    }

    #[test]
    fn test_container_get_port() {
        let container1 = Container::new("test1");
        assert_eq!(container1.get_port(), DEFAULT_PORT);

        let container2 = Container::new("test2").with_port(9000);
        assert_eq!(container2.get_port(), 9000);
    }

    #[test]
    fn test_route_new() {
        let route = Route::new(8080, "my-container");
        assert_eq!(route.host_port, 8080);
        assert_eq!(route.target, "my-container");
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
        assert_eq!(config.proxy_name, DEFAULT_PROXY_NAME);
        assert_eq!(config.network, DEFAULT_NETWORK);
    }

    #[test]
    fn test_config_find_container() {
        let mut config = Config::default();
        config
            .containers
            .push(Container::new("container1").with_label("label1"));
        config.containers.push(Container::new("container2"));

        assert!(config.find_container("container1").is_some());
        assert!(config.find_container("label1").is_some());
        assert!(config.find_container("nonexistent").is_none());
    }

    #[test]
    fn test_config_find_route() {
        let mut config = Config::default();
        config.routes.push(Route::new(8080, "container1"));
        config.routes.push(Route::new(8081, "container2"));

        assert!(config.find_route(8080).is_some());
        assert!(config.find_route(9999).is_none());
    }

    #[test]
    fn test_config_add_or_update_container() {
        let mut config = Config::default();

        // Add new container
        config.add_or_update_container(Container::new("test").with_port(8080));
        assert_eq!(config.containers.len(), 1);
        assert_eq!(config.containers[0].port, Some(8080));

        // Update existing
        config.add_or_update_container(Container::new("test").with_label("updated"));
        assert_eq!(config.containers.len(), 1);
        assert_eq!(config.containers[0].port, Some(8080));
        assert_eq!(config.containers[0].label, Some("updated".to_string()));
    }

    #[test]
    fn test_config_remove_container() {
        let mut config = Config::default();
        config.containers.push(Container::new("test1"));
        config.containers.push(Container::new("test2"));
        config.routes.push(Route::new(8080, "test1"));

        let removed = config.remove_container("test1");
        assert!(removed.is_some());
        assert_eq!(config.containers.len(), 1);
        assert!(config.routes.is_empty()); // Route should be removed too
    }

    #[test]
    fn test_config_get_all_host_ports() {
        let mut config = Config::default();
        assert_eq!(config.get_all_host_ports(), vec![DEFAULT_PORT]);

        config.routes.push(Route::new(8080, "c1"));
        config.routes.push(Route::new(8081, "c2"));
        let ports = config.get_all_host_ports();
        assert!(ports.contains(&8080));
        assert!(ports.contains(&8081));
    }

    #[test]
    fn test_config_set_route() {
        let mut config = Config::default();

        config.set_route(8080, "container1");
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.routes[0].target, "container1");

        // Update existing
        config.set_route(8080, "container2");
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.routes[0].target, "container2");
    }

    #[test]
    fn test_config_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("test-config.json");

        // Create a config
        let mut config = Config::default();
        config.proxy_name = "test-proxy".to_string();
        config
            .containers
            .push(Container::new("test").with_port(8080));
        config.routes.push(Route::new(8080, "test"));

        // Save it
        let content = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&config_file, content).unwrap();

        // Load it
        let loaded_content = std::fs::read_to_string(&config_file).unwrap();
        let loaded: Config = serde_json::from_str(&loaded_content).unwrap();

        assert_eq!(loaded.proxy_name, "test-proxy");
        assert_eq!(loaded.containers.len(), 1);
        assert_eq!(loaded.routes.len(), 1);
    }
}
