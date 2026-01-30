use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 8000;
pub const DEFAULT_PROXY_NAME: &str = "proxy-manager";
pub const DEFAULT_NETWORK: &str = "proxy-net";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub host_port: u16,
    pub target: String,
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

fn default_proxy_name() -> String {
    DEFAULT_PROXY_NAME.to_string()
}

fn default_network() -> String {
    DEFAULT_NETWORK.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            containers: vec![],
            routes: vec![],
            proxy_name: default_proxy_name(),
            network: default_network(),
        }
    }
}

impl Config {
    pub fn get_internal_port(&self, container: &Container) -> u16 {
        container.port.unwrap_or(DEFAULT_PORT)
    }

    pub fn get_proxy_image(&self) -> String {
        format!("{}:latest", self.proxy_name)
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

    pub fn find_route(&self, host_port: u16) -> Option<&Route> {
        self.routes.iter().find(|r| r.host_port == host_port)
    }
}

pub struct ConfigManager {
    config_dir: PathBuf,
    config_file: PathBuf,
    build_dir: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::home_dir()
            .context("Could not find home directory")?
            .join(".local")
            .join("share")
            .join("proxy-manager");

        let config_file = config_dir.join("proxy-config.json");
        let build_dir = config_dir.join("build");

        Ok(Self {
            config_dir,
            config_file,
            build_dir,
        })
    }

    pub fn ensure_config_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir).context("Failed to create config directory")?;
        Ok(())
    }

    pub fn load(&self) -> Result<Config> {
        self.ensure_config_dir()?;

        if self.config_file.exists() {
            let content =
                fs::read_to_string(&self.config_file).context("Failed to read config file")?;
            let config: Config =
                serde_json::from_str(&content).context("Failed to parse config file")?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        self.ensure_config_dir()?;

        let content = serde_json::to_string_pretty(config).context("Failed to serialize config")?;

        fs::write(&self.config_file, content).context("Failed to write config file")?;

        Ok(())
    }

    pub fn config_file_path(&self) -> &Path {
        &self.config_file
    }

    pub fn build_dir(&self) -> &Path {
        &self.build_dir
    }

    pub fn ensure_build_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.build_dir).context("Failed to create build directory")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
        assert_eq!(config.proxy_name, DEFAULT_PROXY_NAME);
        assert_eq!(config.network, DEFAULT_NETWORK);
    }

    #[test]
    fn test_find_container() {
        let mut config = Config::default();
        config.containers.push(Container {
            name: "test-container".to_string(),
            label: Some("Test".to_string()),
            port: None,
            network: None,
        });

        assert!(config.find_container("test-container").is_some());
        assert!(config.find_container("Test").is_some());
        assert!(config.find_container("not-found").is_none());
    }

    #[test]
    fn test_find_route() {
        let mut config = Config::default();
        config.routes.push(Route {
            host_port: 8000,
            target: "test-container".to_string(),
        });

        assert!(config.find_route(8000).is_some());
        assert!(config.find_route(9000).is_none());
    }

    #[test]
    fn test_get_all_host_ports() {
        let mut config = Config::default();
        assert_eq!(config.get_all_host_ports(), vec![DEFAULT_PORT]);

        config.routes.push(Route {
            host_port: 8000,
            target: "test".to_string(),
        });
        config.routes.push(Route {
            host_port: 8001,
            target: "test2".to_string(),
        });
        assert_eq!(config.get_all_host_ports(), vec![8000, 8001]);
    }

    #[test]
    fn test_get_internal_port() {
        let config = Config::default();

        let container_with_port = Container {
            name: "test".to_string(),
            label: None,
            port: Some(9000),
            network: None,
        };
        assert_eq!(config.get_internal_port(&container_with_port), 9000);

        let container_without_port = Container {
            name: "test2".to_string(),
            label: None,
            port: None,
            network: None,
        };
        assert_eq!(
            config.get_internal_port(&container_without_port),
            DEFAULT_PORT
        );
    }
}
