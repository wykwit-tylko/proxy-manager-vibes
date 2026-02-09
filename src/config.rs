use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const DEFAULT_PORT: u16 = 8000;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub containers: Vec<ContainerConfig>,
    pub routes: Vec<Route>,
    pub proxy_name: String,
    pub network: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            containers: Vec::new(),
            routes: Vec::new(),
            proxy_name: "proxy-manager".to_string(),
            network: "proxy-net".to_string(),
        }
    }
}

impl Config {
    pub fn config_dir() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("", "", "proxy-manager")
            .context("Failed to determine project directories")?;
        let config_dir = proj_dirs.data_dir().to_path_buf();
        Ok(config_dir)
    }

    pub fn config_file() -> Result<PathBuf> {
        let config_dir = Self::config_dir()?;
        Ok(config_dir.join("proxy-config.json"))
    }

    pub fn build_dir() -> Result<PathBuf> {
        let config_dir = Self::config_dir()?;
        Ok(config_dir.join("build"))
    }

    pub fn load() -> Result<Self> {
        let config_file = Self::config_file()?;

        if !config_file.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&config_file).context("Failed to read config file")?;

        let config: Config =
            serde_json::from_str(&contents).context("Failed to parse config file")?;

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

        let config_file = Self::config_file()?;
        let contents = serde_json::to_string_pretty(self).context("Failed to serialize config")?;

        fs::write(&config_file, contents).context("Failed to write config file")?;

        Ok(())
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

    #[allow(dead_code)]
    pub fn find_route(&self, host_port: u16) -> Option<&Route> {
        self.routes.iter().find(|r| r.host_port == host_port)
    }

    pub fn get_internal_port(&self, container: &ContainerConfig) -> u16 {
        container.port.unwrap_or(DEFAULT_PORT)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.containers.len(), 0);
        assert_eq!(config.routes.len(), 0);
        assert_eq!(config.proxy_name, "proxy-manager");
        assert_eq!(config.network, "proxy-net");
    }

    #[test]
    fn test_find_container_by_name() {
        let mut config = Config::default();
        config.containers.push(ContainerConfig {
            name: "test-container".to_string(),
            label: Some("test".to_string()),
            port: Some(8080),
            network: None,
        });

        assert!(config.find_container("test-container").is_some());
        assert!(config.find_container("test").is_some());
        assert!(config.find_container("nonexistent").is_none());
    }

    #[test]
    fn test_get_internal_port() {
        let config = Config::default();

        let container_with_port = ContainerConfig {
            name: "test".to_string(),
            label: None,
            port: Some(9000),
            network: None,
        };
        assert_eq!(config.get_internal_port(&container_with_port), 9000);

        let container_without_port = ContainerConfig {
            name: "test".to_string(),
            label: None,
            port: None,
            network: None,
        };
        assert_eq!(
            config.get_internal_port(&container_without_port),
            DEFAULT_PORT
        );
    }

    #[test]
    fn test_get_all_host_ports() {
        let mut config = Config::default();
        assert_eq!(config.get_all_host_ports(), vec![DEFAULT_PORT]);

        config.routes.push(Route {
            host_port: 8080,
            target: "test".to_string(),
        });
        config.routes.push(Route {
            host_port: 8081,
            target: "test2".to_string(),
        });
        assert_eq!(config.get_all_host_ports(), vec![8080, 8081]);
    }

    #[test]
    fn test_serialization() {
        let config = Config {
            containers: vec![ContainerConfig {
                name: "test".to_string(),
                label: Some("Test Container".to_string()),
                port: Some(8080),
                network: Some("custom-net".to_string()),
            }],
            routes: vec![Route {
                host_port: 8000,
                target: "test".to_string(),
            }],
            proxy_name: "my-proxy".to_string(),
            network: "my-net".to_string(),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(config.containers, deserialized.containers);
        assert_eq!(config.routes, deserialized.routes);
        assert_eq!(config.proxy_name, deserialized.proxy_name);
        assert_eq!(config.network, deserialized.network);
    }
}
