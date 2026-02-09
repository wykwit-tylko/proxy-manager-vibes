use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteEntry {
    pub host_port: u16,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub containers: Vec<ContainerEntry>,
    pub routes: Vec<RouteEntry>,
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

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub build_dir: PathBuf,
    pub user_bin: PathBuf,
}

impl Paths {
    pub fn from_home(home: PathBuf) -> Self {
        let config_dir = home.join(".local/share/proxy-manager");
        let config_file = config_dir.join("proxy-config.json");
        let build_dir = config_dir.join("build");
        let user_bin = home.join(".local/bin");
        Self {
            config_dir,
            config_file,
            build_dir,
            user_bin,
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(Self::from_home(home))
    }
}

impl Config {
    pub fn load(paths: &Paths) -> anyhow::Result<Self> {
        fs::create_dir_all(&paths.config_dir).context("Failed to create config directory")?;
        if !paths.config_file.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&paths.config_file)
            .with_context(|| format!("Failed reading {}", paths.config_file.display()))?;
        let mut cfg: Self = serde_json::from_str(&contents)
            .with_context(|| format!("Invalid JSON in {}", paths.config_file.display()))?;

        if cfg.proxy_name.is_empty() {
            cfg.proxy_name = default_proxy_name();
        }
        if cfg.network.is_empty() {
            cfg.network = default_network();
        }

        Ok(cfg)
    }

    pub fn save(&self, paths: &Paths) -> anyhow::Result<()> {
        fs::create_dir_all(&paths.config_dir).context("Failed to create config directory")?;
        let data = serde_json::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&paths.config_file, data)
            .with_context(|| format!("Failed writing {}", paths.config_file.display()))?;
        Ok(())
    }

    pub fn find_container(&self, identifier: &str) -> Option<&ContainerEntry> {
        self.containers
            .iter()
            .find(|c| c.name == identifier || c.label.as_deref() == Some(identifier))
    }

    pub fn find_container_mut(&mut self, name: &str) -> Option<&mut ContainerEntry> {
        self.containers.iter_mut().find(|c| c.name == name)
    }

    pub fn find_route_mut(&mut self, host_port: u16) -> Option<&mut RouteEntry> {
        self.routes.iter_mut().find(|r| r.host_port == host_port)
    }

    pub fn host_ports(&self) -> Vec<u16> {
        if self.routes.is_empty() {
            vec![DEFAULT_PORT]
        } else {
            self.routes.iter().map(|r| r.host_port).collect()
        }
    }

    pub fn proxy_image(&self) -> String {
        format!("{}:latest", self.proxy_name)
    }
}

fn default_proxy_name() -> String {
    "proxy-manager".to_string()
}

fn default_network() -> String {
    "proxy-net".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_default_when_config_missing() {
        let dir = tempdir().expect("tempdir");
        let paths = Paths::from_home(dir.path().to_path_buf());
        let cfg = Config::load(&paths).expect("load");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn saves_and_loads_config_round_trip() {
        let dir = tempdir().expect("tempdir");
        let paths = Paths::from_home(dir.path().to_path_buf());
        let cfg = Config {
            containers: vec![ContainerEntry {
                name: "app".to_string(),
                label: Some("Foo".to_string()),
                port: Some(8080),
                network: Some("app-net".to_string()),
            }],
            routes: vec![RouteEntry {
                host_port: 8001,
                target: "app".to_string(),
            }],
            proxy_name: "proxy".to_string(),
            network: "proxy-net".to_string(),
        };

        cfg.save(&paths).expect("save");
        let loaded = Config::load(&paths).expect("load");
        assert_eq!(cfg, loaded);
    }

    #[test]
    fn finds_container_by_name_or_label() {
        let cfg = Config {
            containers: vec![ContainerEntry {
                name: "app-v1".to_string(),
                label: Some("Blue".to_string()),
                port: None,
                network: None,
            }],
            ..Config::default()
        };

        assert_eq!(
            cfg.find_container("app-v1").map(|c| c.name.as_str()),
            Some("app-v1")
        );
        assert_eq!(
            cfg.find_container("Blue").map(|c| c.name.as_str()),
            Some("app-v1")
        );
        assert!(cfg.find_container("unknown").is_none());
    }
}
