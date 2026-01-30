use serde::{Deserialize, Serialize};

use crate::paths::DEFAULT_PORT;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteConfig {
    pub host_port: u16,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    #[serde(default)]
    pub containers: Vec<ContainerConfig>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default = "default_proxy_name")]
    pub proxy_name: String,
    #[serde(default = "default_network")]
    pub network: String,
}

fn default_proxy_name() -> String {
    "proxy-manager".to_string()
}

fn default_network() -> String {
    "proxy-net".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            containers: Vec::new(),
            routes: Vec::new(),
            proxy_name: default_proxy_name(),
            network: default_network(),
        }
    }
}

impl ContainerConfig {
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }
}

pub fn find_container<'a>(config: &'a AppConfig, identifier: &str) -> Option<&'a ContainerConfig> {
    config
        .containers
        .iter()
        .find(|c| c.name == identifier || c.label.as_deref() == Some(identifier))
}

pub fn find_route<'a>(config: &'a AppConfig, host_port: u16) -> Option<&'a RouteConfig> {
    config.routes.iter().find(|r| r.host_port == host_port)
}

pub fn host_ports(config: &AppConfig) -> Vec<u16> {
    if config.routes.is_empty() {
        vec![DEFAULT_PORT]
    } else {
        config.routes.iter().map(|r| r.host_port).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = AppConfig::default();
        assert_eq!(config.proxy_name, "proxy-manager");
        assert_eq!(config.network, "proxy-net");
        assert!(config.containers.is_empty());
        assert!(config.routes.is_empty());
    }

    #[test]
    fn find_container_by_label_or_name() {
        let config = AppConfig {
            containers: vec![ContainerConfig {
                name: "svc-a".to_string(),
                label: Some("A".to_string()),
                port: None,
                network: None,
            }],
            ..AppConfig::default()
        };

        assert!(find_container(&config, "svc-a").is_some());
        assert!(find_container(&config, "A").is_some());
        assert!(find_container(&config, "missing").is_none());
    }

    #[test]
    fn find_route_by_host_port() {
        let config = AppConfig {
            routes: vec![RouteConfig {
                host_port: 9000,
                target: "svc".to_string(),
            }],
            ..AppConfig::default()
        };
        assert!(find_route(&config, 9000).is_some());
        assert!(find_route(&config, 8000).is_none());
    }

    #[test]
    fn host_ports_defaults_to_8000() {
        let config = AppConfig::default();
        assert_eq!(host_ports(&config), vec![DEFAULT_PORT]);
    }

    #[test]
    fn host_ports_matches_routes() {
        let config = AppConfig {
            routes: vec![
                RouteConfig {
                    host_port: 7000,
                    target: "svc-a".to_string(),
                },
                RouteConfig {
                    host_port: 7001,
                    target: "svc-b".to_string(),
                },
            ],
            ..AppConfig::default()
        };
        assert_eq!(host_ports(&config), vec![7000, 7001]);
    }

    #[test]
    fn container_port_defaults() {
        let container = ContainerConfig {
            name: "svc".to_string(),
            label: None,
            port: None,
            network: None,
        };
        assert_eq!(container.port_or_default(), DEFAULT_PORT);
    }
}
