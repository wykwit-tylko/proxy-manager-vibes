use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8000;
pub const DEFAULT_PROXY_NAME: &str = "proxy-manager";
pub const DEFAULT_NETWORK: &str = "proxy-net";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Containers,
    Routes,
    Status,
    Logs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn default_proxy_name() -> String {
    DEFAULT_PROXY_NAME.to_string()
}

fn default_network() -> String {
    DEFAULT_NETWORK.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    pub host_port: u16,
    pub target: String,
}

impl Config {
    pub fn config_dir() -> anyhow::Result<PathBuf> {
        let config_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
            .join("proxy-manager");
        Ok(config_dir)
    }

    pub fn config_file() -> anyhow::Result<PathBuf> {
        Ok(Self::config_dir()?.join("proxy-config.json"))
    }

    pub fn build_dir() -> anyhow::Result<PathBuf> {
        Ok(Self::config_dir()?.join("build"))
    }

    pub fn load() -> anyhow::Result<Config> {
        let config_file = Self::config_file()?;
        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)?;
            let mut config: Config = serde_json::from_str(&content)?;
            if config.containers.is_empty() {
                config.containers = Vec::new();
            }
            if config.routes.is_empty() {
                config.routes = Vec::new();
            }
            Ok(config)
        } else {
            Ok(Config {
                containers: Vec::new(),
                routes: Vec::new(),
                proxy_name: DEFAULT_PROXY_NAME.to_string(),
                network: DEFAULT_NETWORK.to_string(),
            })
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let config_dir = Self::config_dir()?;
        std::fs::create_dir_all(&config_dir)?;
        let config_file = Self::config_file()?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(config_file, content)?;
        Ok(())
    }

    pub fn get_proxy_image(&self) -> String {
        format!("{}:latest", self.proxy_name)
    }

    pub fn get_internal_port(&self, container: &Container) -> u16 {
        container.port.unwrap_or(DEFAULT_PORT)
    }

    pub fn get_all_host_ports(&self) -> Vec<u16> {
        if self.routes.is_empty() {
            vec![DEFAULT_PORT]
        } else {
            self.routes.iter().map(|r| r.host_port).collect()
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.proxy_name, DEFAULT_PROXY_NAME);
        assert_eq!(config.network, DEFAULT_NETWORK);
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
    }

    #[test]
    fn test_get_all_host_ports_empty() {
        let config = Config::default();
        let ports = config.get_all_host_ports();
        assert_eq!(ports, vec![DEFAULT_PORT]);
    }

    #[test]
    fn test_get_all_host_ports_with_routes() {
        let mut config = Config::default();
        config.routes.push(Route {
            host_port: 8000,
            target: "app1".to_string(),
        });
        config.routes.push(Route {
            host_port: 8001,
            target: "app2".to_string(),
        });
        let ports = config.get_all_host_ports();
        assert_eq!(ports, vec![8000, 8001]);
    }

    #[test]
    fn test_get_internal_port() {
        let container = Container {
            name: "test".to_string(),
            label: None,
            port: Some(8080),
            network: None,
        };
        let config = Config::default();
        assert_eq!(config.get_internal_port(&container), 8080);
    }

    #[test]
    fn test_get_internal_port_default() {
        let container = Container {
            name: "test".to_string(),
            label: None,
            port: None,
            network: None,
        };
        let config = Config::default();
        assert_eq!(config.get_internal_port(&container), DEFAULT_PORT);
    }

    #[test]
    fn test_find_container_by_name() {
        let mut config = Config::default();
        config.containers.push(Container {
            name: "my-app".to_string(),
            label: Some("My App".to_string()),
            port: Some(8000),
            network: Some("custom-net".to_string()),
        });

        let found = config.find_container("my-app");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "my-app");
    }

    #[test]
    fn test_find_container_by_label() {
        let mut config = Config::default();
        config.containers.push(Container {
            name: "my-app".to_string(),
            label: Some("My App".to_string()),
            port: Some(8000),
            network: None,
        });

        let found = config.find_container("My App");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "my-app");
    }

    #[test]
    fn test_find_route() {
        let mut config = Config::default();
        config.routes.push(Route {
            host_port: 8000,
            target: "app1".to_string(),
        });

        let found = config.find_route(8000);
        assert!(found.is_some());
        assert_eq!(found.unwrap().target, "app1");
    }
}
