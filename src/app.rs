use crate::config::{Config, ContainerConfig, DEFAULT_PORT, Route};
use crate::docker::{DockerApi, tar_directory_with_dockerfile_and_conf};
use crate::nginx;
use crate::store::Store;
use anyhow::{Context, Result, anyhow};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tokio::time::{Duration, sleep};

#[derive(Clone, Copy, Debug)]
pub struct Timings {
    pub restart_delay: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            restart_delay: Duration::from_secs(1),
        }
    }
}

pub struct App<D: DockerApi> {
    pub store: Store,
    pub docker: D,
    timings: Timings,
}

impl<D: DockerApi> App<D> {
    pub fn new(store: Store, docker: D) -> Self {
        Self {
            store,
            docker,
            timings: Timings::default(),
        }
    }

    pub fn new_with_timings(store: Store, docker: D, timings: Timings) -> Self {
        Self {
            store,
            docker,
            timings,
        }
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
        sleep(self.timings.restart_delay).await;
        out.extend(self.start_proxy().await?);
        Ok(out)
    }

    pub async fn restart_proxy(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        out.extend(self.stop_proxy().await?);
        sleep(self.timings.restart_delay).await;
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
        let app = App::new_with_timings(
            store.clone(),
            docker,
            Timings {
                restart_delay: Duration::from_millis(0),
            },
        );

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
        let app = App::new_with_timings(
            store.clone(),
            docker,
            Timings {
                restart_delay: Duration::from_millis(0),
            },
        );

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

    type RunCall = (String, String, String, Vec<u16>);

    #[derive(Clone, Default)]
    struct RecordingDocker {
        exists: Arc<Mutex<bool>>,
        stop_removed: Arc<Mutex<bool>>,
        ensured: Arc<Mutex<Vec<String>>>,
        built: Arc<Mutex<Vec<String>>>,
        ran: Arc<Mutex<Vec<RunCall>>>,
        connected: Arc<Mutex<Vec<(String, String)>>>,
        fail_connect_network: Option<String>,
    }

    impl RecordingDocker {
        fn set_exists(&self, v: bool) {
            *self.exists.lock().unwrap() = v;
        }

        fn set_stop_removed(&self, v: bool) {
            *self.stop_removed.lock().unwrap() = v;
        }

        fn ensured(&self) -> Vec<String> {
            self.ensured.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl DockerApi for RecordingDocker {
        async fn list_containers(&self, _all: bool) -> Result<Vec<String>> {
            Ok(vec![])
        }

        async fn list_networks(&self) -> Result<Vec<NetworkSummary>> {
            Ok(vec![])
        }

        async fn ensure_network(&self, network: &str) -> Result<()> {
            self.ensured.lock().unwrap().push(network.to_string());
            Ok(())
        }

        async fn container_primary_network(&self, _container: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn container_exists(&self, _name: &str) -> Result<bool> {
            Ok(*self.exists.lock().unwrap())
        }

        async fn container_status(&self, _name: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn stop_and_remove_container(&self, _name: &str) -> Result<bool> {
            let removed = *self.stop_removed.lock().unwrap();
            if removed {
                self.set_exists(false);
            }
            Ok(removed)
        }

        async fn build_image_from_tar(&self, tag: &str, _tar_bytes: Vec<u8>) -> Result<()> {
            self.built.lock().unwrap().push(tag.to_string());
            Ok(())
        }

        async fn run_container_with_ports(
            &self,
            name: &str,
            image: &str,
            network: &str,
            ports: &[u16],
        ) -> Result<()> {
            self.ran.lock().unwrap().push((
                name.to_string(),
                image.to_string(),
                network.to_string(),
                ports.to_vec(),
            ));
            self.set_exists(true);
            Ok(())
        }

        async fn connect_container_to_network(&self, container: &str, network: &str) -> Result<()> {
            if self
                .fail_connect_network
                .as_deref()
                .is_some_and(|n| n == network)
            {
                return Err(anyhow!("connect failed"));
            }
            self.connected
                .lock()
                .unwrap()
                .push((container.to_string(), network.to_string()));
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

    struct TestApp {
        _td: tempfile::TempDir,
        app: App<RecordingDocker>,
    }

    fn app_with_cfg_and_docker(cfg: Config, docker: RecordingDocker) -> TestApp {
        let td = tempfile::tempdir().unwrap();
        let store = Store::new_with_config_dir(td.path());
        store.save(&cfg).unwrap();
        let app = App::new_with_timings(
            store,
            docker,
            Timings {
                restart_delay: Duration::from_millis(0),
            },
        );
        TestApp { _td: td, app }
    }

    #[tokio::test]
    async fn start_proxy_ensures_networks_and_runs_with_ports_and_connections() {
        let docker = RecordingDocker::default();
        docker.set_exists(false);

        let cfg = Config {
            containers: vec![
                ContainerConfig {
                    name: "a".to_string(),
                    label: None,
                    port: Some(9000),
                    network: None,
                },
                ContainerConfig {
                    name: "b".to_string(),
                    label: None,
                    port: Some(9001),
                    network: Some("net2".to_string()),
                },
            ],
            routes: vec![
                Route {
                    host_port: 8001,
                    target: "a".to_string(),
                },
                Route {
                    host_port: 8002,
                    target: "b".to_string(),
                },
            ],
            proxy_name: "proxy-manager".to_string(),
            network: "proxy-net".to_string(),
        };

        let test_app = app_with_cfg_and_docker(cfg.clone(), docker.clone());
        let out = test_app.app.start_proxy().await.unwrap();

        let ensured = docker.ensured();
        assert!(ensured.contains(&"proxy-net".to_string()));
        assert!(ensured.contains(&"net2".to_string()));

        let ran = docker.ran.lock().unwrap().clone();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].0, cfg.proxy_name);
        assert_eq!(ran[0].1, cfg.proxy_image());
        assert_eq!(ran[0].2, cfg.network);
        assert_eq!(ran[0].3, vec![8001, 8002]);

        let connected = docker.connected.lock().unwrap().clone();
        assert_eq!(
            connected,
            vec![("proxy-manager".to_string(), "net2".to_string())]
        );

        assert!(
            out.iter()
                .any(|l| l.contains("Proxy started on port(s): 8001, 8002"))
        );
    }

    #[tokio::test]
    async fn start_proxy_skips_build_and_run_when_proxy_already_exists() {
        let docker = RecordingDocker::default();
        docker.set_exists(true);

        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "a".to_string(),
                label: None,
                port: None,
                network: None,
            }],
            routes: vec![Route {
                host_port: 8000,
                target: "a".to_string(),
            }],
            ..Config::default()
        };

        let test_app = app_with_cfg_and_docker(cfg, docker.clone());
        let out = test_app.app.start_proxy().await.unwrap();

        assert!(out.iter().any(|l| l.contains("Proxy already running")));
        assert!(docker.built.lock().unwrap().is_empty());
        assert!(docker.ran.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn start_proxy_includes_warning_if_network_connect_fails() {
        let docker = RecordingDocker {
            fail_connect_network: Some("net2".to_string()),
            ..Default::default()
        };
        docker.set_exists(false);

        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "a".to_string(),
                label: None,
                port: None,
                network: Some("net2".to_string()),
            }],
            routes: vec![Route {
                host_port: 8000,
                target: "a".to_string(),
            }],
            ..Config::default()
        };

        let test_app = app_with_cfg_and_docker(cfg, docker);
        let out = test_app.app.start_proxy().await.unwrap();
        assert!(
            out.iter()
                .any(|l| l.contains("Warning: Could not connect to network net2"))
        );
    }

    #[tokio::test]
    async fn stop_proxy_prints_stopped_when_container_removed() {
        let docker = RecordingDocker::default();
        docker.set_exists(true);
        docker.set_stop_removed(true);

        let cfg = Config {
            proxy_name: "proxy-manager".to_string(),
            ..Config::default()
        };

        let test_app = app_with_cfg_and_docker(cfg, docker);
        let out = test_app.app.stop_proxy().await.unwrap();
        assert!(
            out.iter()
                .any(|l| l.contains("Stopping proxy: proxy-manager"))
        );
        assert!(out.iter().any(|l| l.contains("Proxy stopped")));
    }

    #[tokio::test]
    async fn reload_proxy_stops_then_starts() {
        let docker = RecordingDocker::default();
        docker.set_exists(true);
        docker.set_stop_removed(true);

        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "a".to_string(),
                label: None,
                port: None,
                network: None,
            }],
            routes: vec![Route {
                host_port: 8000,
                target: "a".to_string(),
            }],
            ..Config::default()
        };

        let test_app = app_with_cfg_and_docker(cfg, docker.clone());
        let out = test_app.app.reload_proxy().await.unwrap();
        assert!(
            out.first()
                .is_some_and(|l| l.contains("Reloading proxy..."))
        );
        assert!(out.iter().any(|l| l.contains("Stopping proxy:")));
        assert!(out.iter().any(|l| l.contains("Starting proxy:")));

        assert_eq!(docker.ran.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn stop_port_with_remaining_routes_triggers_reload() {
        let docker = RecordingDocker::default();
        docker.set_exists(true);
        docker.set_stop_removed(true);

        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "a".to_string(),
                label: None,
                port: None,
                network: None,
            }],
            routes: vec![
                Route {
                    host_port: 8000,
                    target: "a".to_string(),
                },
                Route {
                    host_port: 8001,
                    target: "a".to_string(),
                },
            ],
            ..Config::default()
        };

        let test_app = app_with_cfg_and_docker(cfg, docker.clone());
        let out = test_app.app.stop_port(8001).await.unwrap();
        assert!(out.iter().any(|l| l.contains("Removed route: port 8001")));
        assert!(out.iter().any(|l| l.contains("Reloading proxy...")));

        let new_cfg = test_app.app.store.load().unwrap();
        assert_eq!(new_cfg.routes.len(), 1);
        assert_eq!(new_cfg.routes[0].host_port, 8000);

        assert_eq!(docker.ran.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn stop_port_last_route_triggers_stop() {
        let docker = RecordingDocker::default();
        docker.set_exists(true);
        docker.set_stop_removed(true);

        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "a".to_string(),
                label: None,
                port: None,
                network: None,
            }],
            routes: vec![Route {
                host_port: 8000,
                target: "a".to_string(),
            }],
            ..Config::default()
        };

        let test_app = app_with_cfg_and_docker(cfg, docker.clone());
        let out = test_app.app.stop_port(8000).await.unwrap();
        assert!(out.iter().any(|l| l.contains("Removed route: port 8000")));
        assert!(out.iter().any(|l| l.contains("Stopping proxy:")));
        assert!(out.iter().any(|l| l.contains("Proxy stopped")));

        let new_cfg = test_app.app.store.load().unwrap();
        assert!(new_cfg.routes.is_empty());
        assert!(docker.ran.lock().unwrap().is_empty());
    }
}
