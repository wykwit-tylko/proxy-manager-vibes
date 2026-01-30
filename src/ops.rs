use anyhow::{Result, bail};

use crate::config::{AppConfig, ContainerConfig, RouteConfig, find_container};
use crate::docker::{DockerRuntime, NetworkInfo};
use crate::nginx;
use crate::paths;
use crate::paths::DEFAULT_PORT;
use crate::storage;
use std::fs;
use std::path::PathBuf;

const PROXY_IMAGE_SUFFIX: &str = ":latest";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchResult {
    Added,
    Updated,
}

pub fn upsert_container(
    config: &mut AppConfig,
    name: &str,
    label: Option<String>,
    port: Option<u16>,
    network: Option<String>,
) -> bool {
    if let Some(existing) = config.containers.iter_mut().find(|c| c.name == name) {
        if let Some(label) = label {
            existing.label = Some(label);
        }
        if let Some(port) = port {
            existing.port = Some(port);
        }
        if let Some(network) = network {
            existing.network = Some(network);
        }
        return true;
    }

    let entry = ContainerConfig {
        name: name.to_string(),
        label,
        port,
        network,
    };
    config.containers.push(entry);
    false
}

pub fn remove_container(config: &mut AppConfig, identifier: &str) -> Option<String> {
    let container = find_container(config, identifier)?.clone();
    let name = container.name.clone();
    config.containers.retain(|c| c.name != name);
    config.routes.retain(|r| r.target != name);
    Some(name)
}

pub fn switch_route(
    config: &mut AppConfig,
    identifier: &str,
    host_port: Option<u16>,
) -> Result<SwitchResult> {
    let container_name = find_container(config, identifier)
        .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?
        .name
        .clone();
    let host_port = host_port.unwrap_or(DEFAULT_PORT);

    if let Some(route) = config.routes.iter_mut().find(|r| r.host_port == host_port) {
        route.target = container_name.clone();
        return Ok(SwitchResult::Updated);
    }

    config.routes.push(RouteConfig {
        host_port,
        target: container_name,
    });
    config.routes.sort_by_key(|r| r.host_port);
    Ok(SwitchResult::Added)
}

pub fn list_containers(config: &AppConfig) -> Vec<String> {
    let mut route_map = std::collections::HashMap::new();
    for route in &config.routes {
        route_map.insert(route.target.clone(), route.host_port);
    }

    config
        .containers
        .iter()
        .map(|c| {
            let host_port = route_map.get(&c.name).copied();
            let port = c.port.unwrap_or(DEFAULT_PORT);
            let network = c.network.as_deref().unwrap_or(config.network.as_str());
            let mut line = format!("{}:{}@{}", c.name, port, network);
            if let Some(label) = &c.label {
                line.push_str(&format!(" - {}", label));
            }
            if let Some(host_port) = host_port {
                line.push_str(&format!(" (port {})", host_port));
            }
            line
        })
        .collect()
}

pub fn ensure_has_containers(config: &AppConfig) -> Result<()> {
    if config.containers.is_empty() {
        bail!("No containers configured. Use 'add' command first.");
    }
    Ok(())
}

pub fn ensure_has_routes(config: &AppConfig) -> Result<()> {
    if config.routes.is_empty() {
        bail!("No routes configured. Use 'switch' command first.");
    }
    Ok(())
}

pub fn proxy_name(config: &AppConfig) -> &str {
    config.proxy_name.as_str()
}

pub fn proxy_image(config: &AppConfig) -> String {
    format!("{}{}", proxy_name(config), PROXY_IMAGE_SUFFIX)
}

pub fn host_ports(config: &AppConfig) -> Vec<u16> {
    crate::config::host_ports(config)
}

pub fn ensure_network(runtime: &impl DockerRuntime, network: &str) -> Result<()> {
    let existing = runtime.list_networks()?;
    let already = existing.iter().any(|n| n.name == network);
    if !already {
        runtime.create_network(network)?;
    }
    Ok(())
}

pub fn ensure_all_networks(
    runtime: &impl DockerRuntime,
    config: &AppConfig,
) -> Result<Vec<String>> {
    let mut networks = vec![config.network.clone()];
    for container in &config.containers {
        if let Some(net) = &container.network
            && !networks.contains(net)
        {
            networks.push(net.clone());
        }
    }
    for network in &networks {
        ensure_network(runtime, network)?;
    }
    Ok(networks)
}

pub fn detect_containers(
    runtime: &impl DockerRuntime,
    filter: Option<&str>,
) -> Result<Vec<String>> {
    let mut containers = runtime.list_containers(true)?;
    if let Some(filter) = filter {
        let filter = filter.to_lowercase();
        containers.retain(|name| name.to_lowercase().contains(&filter));
    }
    Ok(containers)
}

pub fn list_networks(runtime: &impl DockerRuntime) -> Result<Vec<NetworkInfo>> {
    runtime.list_networks()
}

pub fn build_proxy(runtime: &impl DockerRuntime, config: &AppConfig) -> Result<PathBuf> {
    ensure_has_containers(config)?;
    let build_dir = paths::build_dir();
    fs::create_dir_all(&build_dir)?;

    let nginx_conf = nginx::generate_nginx_config(config);
    fs::write(build_dir.join("nginx.conf"), nginx_conf)?;

    let ports = host_ports(config);
    let expose = ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let dockerfile = format!(
        "FROM nginx:stable-alpine\nCOPY nginx.conf /etc/nginx/nginx.conf\nEXPOSE {}\nCMD [\"nginx\", \"-g\", \"daemon off;\"]\n",
        expose
    );
    fs::write(build_dir.join("Dockerfile"), dockerfile)?;

    let image = proxy_image(config);
    runtime.build_image(&image, build_dir.to_str().unwrap_or("."))?;
    Ok(build_dir)
}

pub fn start_proxy(runtime: &impl DockerRuntime, config: &AppConfig) -> Result<Vec<u16>> {
    ensure_has_containers(config)?;
    ensure_has_routes(config)?;

    let networks = ensure_all_networks(runtime, config)?;
    let proxy_name = proxy_name(config).to_string();

    if runtime.container_exists(&proxy_name)? {
        return Ok(host_ports(config));
    }

    build_proxy(runtime, config)?;

    let ports = host_ports(config);
    runtime.run_container(&proxy_name, &proxy_image(config), &config.network, &ports)?;

    for network in networks {
        if network != config.network {
            let _ = runtime.connect_network(&proxy_name, &network);
        }
    }

    Ok(ports)
}

pub fn stop_proxy(runtime: &impl DockerRuntime, config: &AppConfig) -> Result<bool> {
    let proxy_name = proxy_name(config);
    if runtime.container_exists(proxy_name)? {
        runtime.stop_remove_container(proxy_name)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn reload_proxy(runtime: &impl DockerRuntime, config: &AppConfig) -> Result<Vec<u16>> {
    ensure_has_containers(config)?;
    ensure_has_routes(config)?;

    stop_proxy(runtime, config)?;
    start_proxy(runtime, config)
}

pub fn remove_route(config: &mut AppConfig, port: u16) -> bool {
    let before = config.routes.len();
    config.routes.retain(|r| r.host_port != port);
    config.routes.len() != before
}

pub fn find_route_name(config: &AppConfig, host_port: u16) -> Option<String> {
    config
        .routes
        .iter()
        .find(|r| r.host_port == host_port)
        .map(|r| r.target.clone())
}

pub fn stop_port(config: &mut AppConfig, port: u16) -> Result<bool> {
    if !remove_route(config, port) {
        return Ok(false);
    }
    storage::save_config(config)?;
    Ok(true)
}

pub fn proxy_logs(
    runtime: &impl DockerRuntime,
    config: &AppConfig,
    tail: usize,
    follow: bool,
) -> Result<Vec<String>> {
    runtime.container_logs(proxy_name(config), tail, follow)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddOutcome {
    pub updated: bool,
    pub detected_network: Option<String>,
}

pub fn add_container(
    runtime: &impl DockerRuntime,
    config: &mut AppConfig,
    name: &str,
    label: Option<String>,
    port: Option<u16>,
    network: Option<String>,
) -> Result<AddOutcome> {
    let mut resolved_network = network;
    let mut detected = None;
    if resolved_network.is_none()
        && let Some(net) = runtime.container_network(name)?
    {
        resolved_network = Some(net.clone());
        detected = Some(net);
    }

    let updated = upsert_container(config, name, label, port, resolved_network);
    Ok(AddOutcome {
        updated,
        detected_network: detected,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteStatus {
    pub host_port: u16,
    pub target: String,
    pub internal_port: u16,
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusInfo {
    pub proxy_name: String,
    pub status: Option<String>,
    pub routes: Vec<RouteStatus>,
}

pub fn build_status_info(runtime: &impl DockerRuntime, config: &AppConfig) -> Result<StatusInfo> {
    let proxy = proxy_name(config).to_string();
    let status = runtime.container_status(&proxy)?;
    let mut routes = Vec::new();
    for route in &config.routes {
        let target = config.containers.iter().find(|c| c.name == route.target);
        if let Some(container) = target {
            routes.push(RouteStatus {
                host_port: route.host_port,
                target: container.name.clone(),
                internal_port: container.port_or_default(),
                missing: false,
            });
        } else {
            routes.push(RouteStatus {
                host_port: route.host_port,
                target: route.target.clone(),
                internal_port: DEFAULT_PORT,
                missing: true,
            });
        }
    }
    Ok(StatusInfo {
        proxy_name: proxy,
        status,
        routes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::docker::NetworkInfo;

    #[derive(Default)]
    struct FakeDocker {
        containers: Vec<String>,
        networks: Vec<String>,
        logs: Vec<String>,
    }

    impl DockerRuntime for FakeDocker {
        fn list_containers(&self, _all: bool) -> Result<Vec<String>> {
            Ok(self.containers.clone())
        }

        fn list_networks(&self) -> Result<Vec<NetworkInfo>> {
            Ok(self
                .networks
                .iter()
                .map(|name| NetworkInfo {
                    name: name.clone(),
                    driver: "bridge".to_string(),
                    containers: 0,
                    scope: "local".to_string(),
                })
                .collect())
        }

        fn container_network(&self, _container: &str) -> Result<Option<String>> {
            Ok(None)
        }

        fn build_image(&self, _tag: &str, _build_dir: &str) -> Result<()> {
            Ok(())
        }

        fn container_exists(&self, name: &str) -> Result<bool> {
            Ok(self.containers.iter().any(|c| c == name))
        }

        fn container_status(&self, name: &str) -> Result<Option<String>> {
            if self.containers.iter().any(|c| c == name) {
                Ok(Some("running".to_string()))
            } else {
                Ok(None)
            }
        }

        fn run_container(
            &self,
            _name: &str,
            _image: &str,
            _network: &str,
            _ports: &[u16],
        ) -> Result<()> {
            Ok(())
        }

        fn stop_remove_container(&self, _name: &str) -> Result<()> {
            Ok(())
        }

        fn connect_network(&self, _name: &str, _network: &str) -> Result<()> {
            Ok(())
        }

        fn container_logs(&self, _name: &str, _tail: usize, _follow: bool) -> Result<Vec<String>> {
            Ok(self.logs.clone())
        }

        fn create_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn upsert_adds_new_container() {
        let mut config = AppConfig::default();
        let existed = upsert_container(&mut config, "svc", Some("Label".to_string()), None, None);
        assert!(!existed);
        assert_eq!(config.containers.len(), 1);
    }

    #[test]
    fn upsert_updates_existing() {
        let mut config = AppConfig::default();
        upsert_container(&mut config, "svc", None, None, None);
        let existed = upsert_container(
            &mut config,
            "svc",
            Some("Label".to_string()),
            Some(9000),
            None,
        );
        assert!(existed);
        assert_eq!(config.containers[0].label.as_deref(), Some("Label"));
        assert_eq!(config.containers[0].port, Some(9000));
    }

    #[test]
    fn remove_container_deletes_routes() {
        let mut config = AppConfig::default();
        upsert_container(&mut config, "svc", None, None, None);
        config.routes.push(RouteConfig {
            host_port: 8000,
            target: "svc".to_string(),
        });
        let removed = remove_container(&mut config, "svc");
        assert_eq!(removed.as_deref(), Some("svc"));
        assert!(config.routes.is_empty());
    }

    #[test]
    fn switch_route_adds_and_updates() {
        let mut config = AppConfig::default();
        upsert_container(&mut config, "svc", None, None, None);
        let added = switch_route(&mut config, "svc", Some(8001)).unwrap();
        assert_eq!(added, SwitchResult::Added);
        let updated = switch_route(&mut config, "svc", Some(8001)).unwrap();
        assert_eq!(updated, SwitchResult::Updated);
    }

    #[test]
    fn list_containers_renders_info() {
        let mut config = AppConfig::default();
        upsert_container(
            &mut config,
            "svc",
            Some("Label".to_string()),
            Some(9000),
            None,
        );
        config.routes.push(RouteConfig {
            host_port: 8001,
            target: "svc".to_string(),
        });
        let lines = list_containers(&config);
        assert!(lines[0].contains("svc:9000@"));
        assert!(lines[0].contains("Label"));
        assert!(lines[0].contains("port 8001"));
    }

    #[test]
    fn ensure_network_creates_if_missing() {
        let docker = FakeDocker::default();
        ensure_network(&docker, "proxy-net").unwrap();
    }

    #[test]
    fn stop_port_saves_when_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", dir.path());
        }
        let config_path = paths::config_file();

        let mut config = AppConfig::default();
        config.routes.push(RouteConfig {
            host_port: 8001,
            target: "svc".to_string(),
        });
        storage::save_config_to(&config_path, &config).unwrap();

        let removed = stop_port(&mut config, 8001).unwrap();
        assert!(removed);
        let loaded = storage::load_config_from(&config_path).unwrap();
        assert!(loaded.routes.is_empty());
        if let Some(value) = original {
            unsafe {
                std::env::set_var("HOME", value);
            }
        }
    }

    #[test]
    fn detect_filters_containers() {
        let docker = FakeDocker {
            containers: vec!["foo".to_string(), "bar".to_string()],
            ..Default::default()
        };
        let filtered = detect_containers(&docker, Some("fo")).unwrap();
        assert_eq!(filtered, vec!["foo".to_string()]);
    }

    #[test]
    fn remove_route_removes_route() {
        let mut config = AppConfig::default();
        config.routes.push(RouteConfig {
            host_port: 8001,
            target: "svc".to_string(),
        });
        let removed = remove_route(&mut config, 8001);
        assert!(removed);
        assert!(config.routes.is_empty());
    }

    #[test]
    fn proxy_logs_returns_lines() {
        let docker = FakeDocker {
            logs: vec!["line".to_string()],
            ..Default::default()
        };
        let config = AppConfig::default();
        let lines = proxy_logs(&docker, &config, 10, false).unwrap();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn add_container_detects_network() {
        struct NetworkDocker;

        impl DockerRuntime for NetworkDocker {
            fn list_containers(&self, _all: bool) -> Result<Vec<String>> {
                Ok(vec![])
            }
            fn list_networks(&self) -> Result<Vec<NetworkInfo>> {
                Ok(vec![])
            }
            fn container_network(&self, _container: &str) -> Result<Option<String>> {
                Ok(Some("net".to_string()))
            }
            fn build_image(&self, _tag: &str, _build_dir: &str) -> Result<()> {
                Ok(())
            }
            fn container_exists(&self, _name: &str) -> Result<bool> {
                Ok(false)
            }
            fn container_status(&self, _name: &str) -> Result<Option<String>> {
                Ok(None)
            }
            fn run_container(
                &self,
                _name: &str,
                _image: &str,
                _network: &str,
                _ports: &[u16],
            ) -> Result<()> {
                Ok(())
            }
            fn stop_remove_container(&self, _name: &str) -> Result<()> {
                Ok(())
            }
            fn connect_network(&self, _name: &str, _network: &str) -> Result<()> {
                Ok(())
            }
            fn container_logs(
                &self,
                _name: &str,
                _tail: usize,
                _follow: bool,
            ) -> Result<Vec<String>> {
                Ok(vec![])
            }
            fn create_network(&self, _name: &str) -> Result<()> {
                Ok(())
            }
        }

        let runtime = NetworkDocker;
        let mut config = AppConfig::default();
        let outcome = add_container(&runtime, &mut config, "svc", None, None, None).unwrap();
        assert!(!outcome.updated);
        assert_eq!(outcome.detected_network.as_deref(), Some("net"));
        assert_eq!(config.containers[0].network.as_deref(), Some("net"));
    }

    #[test]
    fn build_status_info_marks_missing() {
        let docker = FakeDocker::default();
        let config = AppConfig {
            routes: vec![RouteConfig {
                host_port: 8001,
                target: "missing".to_string(),
            }],
            ..AppConfig::default()
        };
        let status = build_status_info(&docker, &config).unwrap();
        assert_eq!(status.routes.len(), 1);
        assert!(status.routes[0].missing);
    }
}
