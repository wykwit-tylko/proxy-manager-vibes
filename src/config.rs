use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8000;
pub const DEFAULT_PROXY_NAME: &str = "proxy-manager";
pub const DEFAULT_NETWORK: &str = "proxy-net";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

fn default_proxy_name() -> String {
    DEFAULT_PROXY_NAME.to_string()
}

fn default_network() -> String {
    DEFAULT_NETWORK.to_string()
}

pub fn get_config_file() -> PathBuf {
    let mut config_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.push("proxy-manager");
    config_dir.push("proxy-config.json");
    config_dir
}

pub fn load_config() -> Result<Config> {
    let config_file = get_config_file();

    if config_file.exists() {
        let content = fs::read_to_string(&config_file)
            .with_context(|| format!("Failed to read config file: {}", config_file.display()))?;
        serde_json::from_str(&content).with_context(|| "Failed to parse config file")
    } else {
        Ok(Config::default())
    }
}

pub fn save_config(config: &Config) -> Result<()> {
    let config_file = get_config_file();

    if let Some(parent) = config_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    let content =
        serde_json::to_string_pretty(config).with_context(|| "Failed to serialize config")?;

    fs::write(&config_file, content)
        .with_context(|| format!("Failed to write config file: {}", config_file.display()))
}

pub fn get_proxy_name(config: Option<&Config>) -> String {
    match config {
        Some(c) => c.proxy_name.clone(),
        None => DEFAULT_PROXY_NAME.to_string(),
    }
}

pub fn get_proxy_image(config: Option<&Config>) -> String {
    let name = get_proxy_name(config);
    format!("{}:latest", name)
}

pub fn get_network_name(config: Option<&Config>) -> String {
    match config {
        Some(c) => c.network.clone(),
        None => DEFAULT_NETWORK.to_string(),
    }
}

pub fn get_all_host_ports(config: Option<&Config>) -> Vec<u16> {
    match config {
        Some(c) if !c.routes.is_empty() => c.routes.iter().map(|r| r.host_port).collect(),
        _ => vec![DEFAULT_PORT],
    }
}

pub fn get_data_dir() -> PathBuf {
    let mut data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.push("proxy-manager");
    data_dir
}

pub fn get_build_dir() -> PathBuf {
    let mut build_dir = get_data_dir();
    build_dir.push("build");
    build_dir
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
    fn test_container_serialization() {
        let container = Container {
            name: "test-container".to_string(),
            label: Some("Test Label".to_string()),
            port: Some(8080),
            network: Some("test-net".to_string()),
        };

        let json = serde_json::to_string(&container).unwrap();
        let deserialized: Container = serde_json::from_str(&json).unwrap();

        assert_eq!(container.name, deserialized.name);
        assert_eq!(container.label, deserialized.label);
        assert_eq!(container.port, deserialized.port);
        assert_eq!(container.network, deserialized.network);
    }

    #[test]
    fn test_route_serialization() {
        let route = Route {
            host_port: 8000,
            target: "my-container".to_string(),
        };

        let json = serde_json::to_string(&route).unwrap();
        let deserialized: Route = serde_json::from_str(&json).unwrap();

        assert_eq!(route.host_port, deserialized.host_port);
        assert_eq!(route.target, deserialized.target);
    }

    #[test]
    fn test_get_all_host_ports_empty() {
        let config = Config::default();
        let ports = get_all_host_ports(Some(&config));
        assert_eq!(ports, vec![DEFAULT_PORT]);
    }

    #[test]
    fn test_get_all_host_ports_with_routes() {
        let config = Config {
            routes: vec![
                Route {
                    host_port: 8000,
                    target: "container1".to_string(),
                },
                Route {
                    host_port: 8001,
                    target: "container2".to_string(),
                },
            ],
            ..Default::default()
        };

        let ports = get_all_host_ports(Some(&config));
        assert_eq!(ports, vec![8000, 8001]);
    }
}
