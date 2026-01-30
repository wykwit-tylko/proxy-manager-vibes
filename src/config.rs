use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 8000;
pub const DEFAULT_PROXY_NAME: &str = "proxy-manager";
pub const DEFAULT_NETWORK: &str = "proxy-net";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
            proxy_name: DEFAULT_PROXY_NAME.to_string(),
            network: DEFAULT_NETWORK.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub host_port: u16,
    pub target: String,
}

impl Config {
    pub fn proxy_image(&self) -> String {
        format!("{}:latest", self.proxy_name)
    }

    pub fn internal_port_for(&self, target: &str) -> u16 {
        self.containers
            .iter()
            .find(|c| c.name == target)
            .and_then(|c| c.port)
            .unwrap_or(DEFAULT_PORT)
    }

    pub fn host_ports(&self) -> Vec<u16> {
        if self.routes.is_empty() {
            vec![DEFAULT_PORT]
        } else {
            self.routes.iter().map(|r| r.host_port).collect()
        }
    }

    pub fn find_container_by_name_or_label(&self, identifier: &str) -> Option<&ContainerConfig> {
        self.containers.iter().find(|c| {
            c.name == identifier || c.label.as_deref().is_some_and(|label| label == identifier)
        })
    }

    pub fn find_container_mut_by_name(&mut self, name: &str) -> Option<&mut ContainerConfig> {
        self.containers.iter_mut().find(|c| c.name == name)
    }

    pub fn find_route_by_port(&self, host_port: u16) -> Option<&Route> {
        self.routes.iter().find(|r| r.host_port == host_port)
    }

    pub fn find_route_mut_by_port(&mut self, host_port: u16) -> Option<&mut Route> {
        self.routes.iter_mut().find(|r| r.host_port == host_port)
    }

    pub fn sort_routes(&mut self) {
        self.routes.sort_by_key(|r| r.host_port);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_match_python_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.proxy_name, DEFAULT_PROXY_NAME);
        assert_eq!(cfg.network, DEFAULT_NETWORK);
        assert!(cfg.containers.is_empty());
        assert!(cfg.routes.is_empty());
        assert_eq!(cfg.host_ports(), vec![DEFAULT_PORT]);
    }

    #[test]
    fn find_container_by_label_or_name() {
        let cfg = Config {
            containers: vec![
                ContainerConfig {
                    name: "my-app".to_string(),
                    label: Some("Foo".to_string()),
                    port: Some(8080),
                    network: None,
                },
                ContainerConfig {
                    name: "other".to_string(),
                    label: None,
                    port: None,
                    network: None,
                },
            ],
            routes: vec![],
            ..Config::default()
        };

        assert_eq!(
            cfg.find_container_by_name_or_label("my-app").unwrap().name,
            "my-app"
        );
        assert_eq!(
            cfg.find_container_by_name_or_label("Foo").unwrap().name,
            "my-app"
        );
        assert_eq!(
            cfg.find_container_by_name_or_label("other").unwrap().name,
            "other"
        );
        assert!(cfg.find_container_by_name_or_label("missing").is_none());
    }
}
