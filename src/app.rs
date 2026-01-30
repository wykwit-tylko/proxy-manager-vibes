use crate::config::{Config, ContainerConfig, DEFAULT_PORT, Route};
use crate::docker::{DockerApi, tar_directory_with_dockerfile_and_conf};
use crate::nginx;
use crate::store::Store;
use anyhow::{Context, Result, anyhow};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tokio::time::{Duration, sleep};

pub struct App<D: DockerApi> {
    pub store: Store,
    pub docker: D,
}

impl<D: DockerApi> App<D> {
    pub fn new(store: Store, docker: D) -> Self {
        Self { store, docker }
    }

    pub fn config_file_path(&self) -> &Path {
        &self.store.config_file
    }

    pub async fn add_container(
        &self,
        container_name: String,
        label: Option<String>,
        port: Option<u16>,
        network: Option<String>,
    ) -> Result<Vec<String>> {
        let mut cfg = self.store.load()?;
        let mut out = Vec::new();

        let mut effective_network = network;
        if effective_network.is_none()
            && let Some(detected) = self
                .docker
                .container_primary_network(&container_name)
                .await?
        {
            out.push(format!("Auto-detected network: {detected}"));
            effective_network = Some(detected);
        }

        if let Some(existing) = cfg.find_container_mut_by_name(&container_name) {
            if label.is_some() {
                existing.label = label;
            }
            if port.is_some() {
                existing.port = port;
            }
            if effective_network.is_some() {
                existing.network = effective_network;
            }
            self.store.save(&cfg)?;
            out.push(format!("Updated container: {container_name}"));
            return Ok(out);
        }

        let entry = ContainerConfig {
            name: container_name.clone(),
            label,
            port,
            network: effective_network,
        };
        cfg.containers.push(entry);
        self.store.save(&cfg)?;
        out.push(format!("Added container: {container_name}"));
        Ok(out)
    }

    pub async fn remove_container(&self, identifier: String) -> Result<Vec<String>> {
        let mut cfg = self.store.load()?;
        let Some(container) = cfg.find_container_by_name_or_label(&identifier).cloned() else {
            return Ok(vec![format!(
                "Error: Container '{identifier}' not found in config"
            )]);
        };

        cfg.containers.retain(|c| c.name != container.name);
        cfg.routes.retain(|r| r.target != container.name);
        self.store.save(&cfg)?;
        Ok(vec![format!("Removed container: {}", container.name)])
    }

    pub fn list_configured_containers(&self) -> Result<Vec<String>> {
        let cfg = self.store.load()?;
        if cfg.containers.is_empty() {
            return Ok(vec!["No containers configured".to_string()]);
        }

        let mut route_map: BTreeMap<String, u16> = BTreeMap::new();
        for r in &cfg.routes {
            route_map.insert(r.target.clone(), r.host_port);
        }

        let mut out = Vec::new();
        out.push("Configured containers:".to_string());
        for c in &cfg.containers {
            let host_port = route_map.get(&c.name).copied();
            let marker = host_port
                .map(|p| format!(" (port {p})"))
                .unwrap_or_default();
            let label = c
                .label
                .as_ref()
                .map(|l| format!(" - {l}"))
                .unwrap_or_default();
            let port = c.port.unwrap_or(DEFAULT_PORT);
            let net = c.network.as_deref().unwrap_or(cfg.network.as_str());
            out.push(format!("  {}:{port}@{net}{label}{marker}", c.name));
        }
        Ok(out)
    }

    pub async fn detect_containers(&self, filter: Option<String>) -> Result<Vec<String>> {
        let mut containers = self.docker.list_containers(true).await?;
        if let Some(f) = filter {
            let f = f.to_lowercase();
            containers.retain(|c| c.to_lowercase().contains(&f));
        }

        let mut out = Vec::new();
        out.push("Running containers:".to_string());
        for c in containers {
            out.push(format!("  {c}"));
        }
        Ok(out)
    }

    pub async fn list_networks(&self) -> Result<Vec<String>> {
        let networks = self.docker.list_networks().await?;
        let mut out = Vec::new();
        out.push("Available Docker networks:".to_string());
        for n in networks {
            out.push(format!(
                "  {:<25} driver={:<10} containers={:<4} scope={}",
                n.name, n.driver, n.containers, n.scope
            ));
        }
        Ok(out)
    }

    pub fn show_config(&self) -> Result<Vec<String>> {
        let cfg = self.store.load()?;
        let mut out = Vec::new();
        out.push(format!("Config file: {}", self.store.config_file.display()));
        out.push("".to_string());
        out.push(serde_json::to_string_pretty(&cfg)?);
        Ok(out)
    }

    pub async fn stop_proxy(&self) -> Result<Vec<String>> {
        let cfg = self.store.load()?;
        let proxy_name = cfg.proxy_name;
        let removed = self.docker.stop_and_remove_container(&proxy_name).await?;
        if removed {
            Ok(vec![
                format!("Stopping proxy: {proxy_name}"),
                "Proxy stopped".to_string(),
            ])
        } else {
            Ok(vec!["Proxy not running".to_string()])
        }
    }

    pub async fn build_proxy_image(&self, cfg: &Config) -> Result<()> {
        if cfg.containers.is_empty() {
            return Err(anyhow!(
                "No containers configured. Use 'add' command first."
            ));
        }

        fs::create_dir_all(&self.store.build_dir)?;

        let nginx_conf = nginx::generate_nginx_config(cfg);
        fs::write(self.store.build_dir.join("nginx.conf"), &nginx_conf)
            .context("write nginx.conf")?;

        let host_ports = cfg.host_ports();
        let expose = host_ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let dockerfile = format!(
            "FROM nginx:stable-alpine\nCOPY nginx.conf /etc/nginx/nginx.conf\nEXPOSE {expose}\nCMD [\"nginx\", \"-g\", \"daemon off;\"]\n"
        );
        fs::write(self.store.build_dir.join("Dockerfile"), &dockerfile)
            .context("write Dockerfile")?;

        let tar_bytes = tar_directory_with_dockerfile_and_conf(&dockerfile, &nginx_conf)?;
        self.docker
            .build_image_from_tar(&cfg.proxy_image(), tar_bytes)
            .await
            .context("docker build image")?;

        Ok(())
    }

    pub async fn start_proxy(&self) -> Result<Vec<String>> {
        let cfg = self.store.load()?;

        if cfg.containers.is_empty() {
            return Ok(vec![
                "Error: No containers configured. Use 'add' command first.".to_string(),
            ]);
        }
        if cfg.routes.is_empty() {
            return Ok(vec![
                "Error: No routes configured. Use 'switch' command first.".to_string(),
            ]);
        }

        let proxy_name = cfg.proxy_name.clone();
        let proxy_image = cfg.proxy_image();
        let default_network = cfg.network.clone();

        let mut networks: BTreeSet<String> = BTreeSet::new();
        networks.insert(default_network.clone());
        for c in &cfg.containers {
            if let Some(n) = &c.network {
                networks.insert(n.clone());
            }
        }

        let mut out = Vec::new();
        for n in &networks {
            self.docker.ensure_network(n).await?;
        }

        if self.docker.container_exists(&proxy_name).await? {
            out.push(format!("Proxy already running: {proxy_name}"));
            return Ok(out);
        }

        out.push("Building proxy image...".to_string());
        self.build_proxy_image(&cfg).await?;

        let host_ports = cfg.host_ports();
        out.push(format!("Starting proxy: {proxy_name}"));
        self.docker
            .run_container_with_ports(&proxy_name, &proxy_image, &default_network, &host_ports)
            .await?;

        for n in &networks {
            if n != &default_network {
                if let Err(e) = self
                    .docker
                    .connect_container_to_network(&proxy_name, n)
                    .await
                {
                    out.push(format!("Warning: Could not connect to network {n}: {e}"));
                } else {
                    out.push(format!("Connected proxy to network: {n}"));
                }
            }
        }

        let port_str = host_ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!("Proxy started on port(s): {port_str}"));
        Ok(out)
    }

    pub async fn reload_proxy(&self) -> Result<Vec<String>> {
        let cfg = self.store.load()?;
        if cfg.containers.is_empty() {
            return Ok(vec!["Error: No containers configured.".to_string()]);
        }
        if cfg.routes.is_empty() {
            return Ok(vec!["Error: No routes configured.".to_string()]);
        }

        let mut out = Vec::new();
        out.push("Reloading proxy...".to_string());
        out.extend(self.stop_proxy().await?);
        sleep(Duration::from_secs(1)).await;
        out.extend(self.start_proxy().await?);
        Ok(out)
    }

    pub async fn restart_proxy(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        out.extend(self.stop_proxy().await?);
        sleep(Duration::from_secs(1)).await;
        out.extend(self.start_proxy().await?);
        Ok(out)
    }

    pub async fn stop_port(&self, host_port: u16) -> Result<Vec<String>> {
        let mut cfg = self.store.load()?;
        if cfg.find_route_by_port(host_port).is_none() {
            return Ok(vec![format!("Error: No route found for port {host_port}")]);
        }

        cfg.routes.retain(|r| r.host_port != host_port);
        self.store.save(&cfg)?;

        let mut out = Vec::new();
        out.push(format!("Removed route: port {host_port}"));
        if cfg.routes.is_empty() {
            out.extend(self.stop_proxy().await?);
        } else {
            out.extend(self.reload_proxy().await?);
        }
        Ok(out)
    }

    pub async fn switch_target(
        &self,
        identifier: String,
        host_port: Option<u16>,
    ) -> Result<Vec<String>> {
        let mut cfg = self.store.load()?;
        let Some(container) = cfg.find_container_by_name_or_label(&identifier).cloned() else {
            return Ok(vec![format!(
                "Error: Container '{identifier}' not found in config"
            )]);
        };

        let host_port = host_port.unwrap_or(DEFAULT_PORT);
        let mut out = Vec::new();
        if let Some(r) = cfg.find_route_mut_by_port(host_port) {
            r.target = container.name.clone();
            self.store.save(&cfg)?;
            out.push(format!(
                "Switching route: {host_port} -> {}",
                container.name
            ));
        } else {
            cfg.routes.push(Route {
                host_port,
                target: container.name.clone(),
            });
            cfg.sort_routes();
            self.store.save(&cfg)?;
            out.push(format!("Adding route: {host_port} -> {}", container.name));
        }
        out.extend(self.reload_proxy().await?);
        Ok(out)
    }

    pub async fn status(&self) -> Result<Vec<String>> {
        let cfg = self.store.load()?;
        let proxy_name = cfg.proxy_name.clone();
        let Some(status) = self.docker.container_status(&proxy_name).await? else {
            return Ok(vec!["Proxy not running".to_string()]);
        };

        let mut out = Vec::new();
        out.push(format!("Proxy: {proxy_name} ({status})"));
        out.push("".to_string());
        out.push("Active routes:".to_string());
        for route in &cfg.routes {
            let host_port = route.host_port;
            let target = &route.target;
            let target_container = cfg.containers.iter().find(|c| c.name == *target);
            if let Some(c) = target_container {
                let internal_port = c.port.unwrap_or(DEFAULT_PORT);
                out.push(format!("  {host_port} -> {target}:{internal_port}"));
            } else {
                out.push(format!("  {host_port} -> {target} (container not found)"));
            }
        }
        Ok(out)
    }

    pub async fn logs(&self, follow: bool, tail: usize) -> Result<Vec<String>> {
        let cfg = self.store.load()?;
        let proxy_name = cfg.proxy_name;
        if !self.docker.container_exists(&proxy_name).await? {
            return Ok(vec![format!("Proxy container '{proxy_name}' not running")]);
        }
        let mut out = Vec::new();
        out.push(format!("Logs for: {proxy_name}"));
        out.push("-".repeat(50));
        out.extend(self.docker.stream_logs(&proxy_name, follow, tail).await?);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::DockerApi;
    use crate::docker::NetworkSummary;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeDocker {
        detected_network: Option<String>,
        exists: Arc<Mutex<bool>>,
        ensured_networks: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl DockerApi for FakeDocker {
        async fn list_containers(&self, _all: bool) -> Result<Vec<String>> {
            Ok(vec!["app".to_string()])
        }

        async fn list_networks(&self) -> Result<Vec<NetworkSummary>> {
            Ok(vec![])
        }

        async fn ensure_network(&self, network: &str) -> Result<()> {
            self.ensured_networks
                .lock()
                .unwrap()
                .push(network.to_string());
            Ok(())
        }

        async fn container_primary_network(&self, _container: &str) -> Result<Option<String>> {
            Ok(self.detected_network.clone())
        }

        async fn container_exists(&self, _name: &str) -> Result<bool> {
            Ok(*self.exists.lock().unwrap())
        }

        async fn container_status(&self, _name: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn stop_and_remove_container(&self, _name: &str) -> Result<bool> {
            Ok(false)
        }

        async fn build_image_from_tar(&self, _tag: &str, _tar_bytes: Vec<u8>) -> Result<()> {
            Ok(())
        }

        async fn run_container_with_ports(
            &self,
            _name: &str,
            _image: &str,
            _network: &str,
            _ports: &[u16],
        ) -> Result<()> {
            Ok(())
        }

        async fn connect_container_to_network(
            &self,
            _container: &str,
            _network: &str,
        ) -> Result<()> {
            Ok(())
        }

        async fn stream_logs(
            &self,
            _name: &str,
            _follow: bool,
            _tail: usize,
        ) -> Result<Vec<String>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn add_container_auto_detects_network_when_missing() {
        let td = tempfile::tempdir().unwrap();
        let store = Store::new_with_config_dir(td.path());
        let docker = FakeDocker {
            detected_network: Some("net1".to_string()),
            ..Default::default()
        };
        let app = App::new(store.clone(), docker);

        let out = app
            .add_container("app".to_string(), None, None, None)
            .await
            .unwrap();
        assert!(
            out.iter()
                .any(|l| l.contains("Auto-detected network: net1"))
        );

        let cfg = store.load().unwrap();
        assert_eq!(cfg.containers.len(), 1);
        assert_eq!(cfg.containers[0].network.as_deref(), Some("net1"));
    }

    #[tokio::test]
    async fn switch_target_adds_route_and_sorts() {
        let td = tempfile::tempdir().unwrap();
        let store = Store::new_with_config_dir(td.path());
        let docker = FakeDocker::default();
        let app = App::new(store.clone(), docker);

        let _ = app
            .add_container("a".to_string(), None, None, Some("n".to_string()))
            .await
            .unwrap();
        let _ = app
            .add_container("b".to_string(), None, None, Some("n".to_string()))
            .await
            .unwrap();

        // Add routes out of order.
        let _ = app
            .switch_target("b".to_string(), Some(8002))
            .await
            .unwrap();
        let _ = app
            .switch_target("a".to_string(), Some(8001))
            .await
            .unwrap();

        let cfg = store.load().unwrap();
        assert_eq!(cfg.routes.len(), 2);
        assert_eq!(cfg.routes[0].host_port, 8001);
        assert_eq!(cfg.routes[1].host_port, 8002);
    }
}
