use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContainerConfig {
    pub name: String,
    pub label: Option<String>,
    pub port: Option<u16>,
    pub network: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteConfig {
    pub host_port: u16,
    pub target: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub containers: Vec<ContainerConfig>,
    pub routes: Vec<RouteConfig>,
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

pub fn get_config_dir() -> PathBuf {
    if let Some(proj_dirs) = ProjectDirs::from("", "", "proxy-manager") {
        proj_dirs.data_dir().to_path_buf()
    } else {
        // Fallback to a relative path if we can't get the project dirs
        PathBuf::from(".proxy-manager")
    }
}

pub fn get_config_file() -> PathBuf {
    let mut path = get_config_dir();
    path.push("proxy-config.json");
    path
}

pub fn load_config() -> Result<Config> {
    let config_file = get_config_file();
    if config_file.exists() {
        let content = fs::read_to_string(config_file)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    } else {
        Ok(Config::default())
    }
}

pub fn save_config(config: &Config) -> Result<()> {
    let config_dir = get_config_dir();
    fs::create_dir_all(&config_dir)?;
    let config_file = get_config_file();
    let content = serde_json::to_string_pretty(config)?;
    fs::write(config_file, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.proxy_name, "proxy-manager");
        assert_eq!(config.network, "proxy-net");
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
    }

    #[test]
    fn test_config_serialization() {
        let mut config = Config::default();
        config.containers.push(ContainerConfig {
            name: "test".to_string(),
            label: Some("Test".to_string()),
            port: Some(8080),
            network: Some("test-net".to_string()),
        });

        let json = serde_json::to_string(&config).unwrap();
        let decoded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.containers.len(), 1);
        assert_eq!(decoded.containers[0].name, "test");
    }
}
