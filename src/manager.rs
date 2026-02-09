use crate::config::{Config, ContainerEntry, DEFAULT_PORT, Paths, RouteEntry};
use crate::docker::DockerApi;
use anyhow::{Context, bail};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::thread;
use std::time::Duration;

pub struct ProxyManager<D: DockerApi> {
    docker: D,
    paths: Paths,
}

impl<D: DockerApi> ProxyManager<D> {
    pub fn new(docker: D, paths: Paths) -> Self {
        Self { docker, paths }
    }

    pub fn load_config(&self) -> anyhow::Result<Config> {
        Config::load(&self.paths)
    }

    pub fn add_container(
        &self,
        container_name: &str,
        label: Option<&str>,
        port: Option<u16>,
        network: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut cfg = self.load_config()?;
        let mut selected_network = network.map(ToOwned::to_owned);
        if selected_network.is_none() {
            selected_network = self.docker.inspect_container_network(container_name)?;
        }

        if let Some(existing) = cfg.find_container_mut(container_name) {
            if let Some(label_value) = label {
                existing.label = Some(label_value.to_string());
            }
            if let Some(port_value) = port {
                existing.port = Some(port_value);
            }
            if let Some(network_value) = selected_network {
                existing.network = Some(network_value);
            }
            cfg.save(&self.paths)?;
            return Ok(format!("Updated container: {container_name}"));
        }

        cfg.containers.push(ContainerEntry {
            name: container_name.to_string(),
            label: label.map(ToOwned::to_owned),
            port,
            network: selected_network,
        });
        cfg.save(&self.paths)?;
        Ok(format!("Added container: {container_name}"))
    }

    pub fn remove_container(&self, identifier: &str) -> anyhow::Result<String> {
        let mut cfg = self.load_config()?;
        let container = cfg
            .find_container(identifier)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Container '{identifier}' not found in config"))?;

        cfg.containers.retain(|c| c.name != container.name);
        cfg.routes.retain(|r| r.target != container.name);
        cfg.save(&self.paths)?;
        Ok(format!("Removed container: {}", container.name))
    }

    pub fn switch_target(
        &self,
        identifier: &str,
        host_port: Option<u16>,
    ) -> anyhow::Result<String> {
        let mut cfg = self.load_config()?;
        let target_name = cfg
            .find_container(identifier)
            .map(|c| c.name.clone())
            .ok_or_else(|| anyhow::anyhow!("Container '{identifier}' not found in config"))?;

        let target_port = host_port.unwrap_or(DEFAULT_PORT);

        let message = if let Some(route) = cfg.find_route_mut(target_port) {
            route.target = target_name.clone();
            format!("Switching route: {target_port} -> {target_name}")
        } else {
            cfg.routes.push(RouteEntry {
                host_port: target_port,
                target: target_name.clone(),
            });
            cfg.routes.sort_by_key(|r| r.host_port);
            format!("Adding route: {target_port} -> {target_name}")
        };
        cfg.save(&self.paths)?;
        self.reload_proxy()?;
        Ok(message)
    }

    pub fn stop_port(&self, host_port: u16) -> anyhow::Result<String> {
        let mut cfg = self.load_config()?;
        if !cfg.routes.iter().any(|r| r.host_port == host_port) {
            bail!("No route found for port {host_port}");
        }

        cfg.routes.retain(|r| r.host_port != host_port);
        cfg.save(&self.paths)?;

        if cfg.routes.is_empty() {
            let _ = self.stop_proxy()?;
        } else {
            self.reload_proxy()?;
        }

        Ok(format!("Removed route: port {host_port}"))
    }

    pub fn list_containers_output(&self) -> anyhow::Result<String> {
        let cfg = self.load_config()?;
        if cfg.containers.is_empty() {
            return Ok("No containers configured".to_string());
        }

        let route_map: HashMap<&str, u16> = cfg
            .routes
            .iter()
            .map(|r| (r.target.as_str(), r.host_port))
            .collect();

        let mut lines = vec!["Configured containers:".to_string()];
        for c in &cfg.containers {
            let marker = route_map
                .get(c.name.as_str())
                .map(|port| format!(" (port {port})"))
                .unwrap_or_default();
            let label = c
                .label
                .as_ref()
                .map(|l| format!(" - {l}"))
                .unwrap_or_default();
            let port = c.port.unwrap_or(DEFAULT_PORT);
            let network = c.network.clone().unwrap_or_else(|| cfg.network.clone());
            lines.push(format!("  {}:{port}@{network}{label}{marker}", c.name));
        }

        Ok(lines.join("\n"))
    }

    pub fn detect_containers_output(&self, filter: Option<&str>) -> anyhow::Result<String> {
        let items = self.docker.list_containers(filter)?;
        let mut out = vec!["Running containers:".to_string()];
        for item in items {
            out.push(format!("  {item}"));
        }
        Ok(out.join("\n"))
    }

    pub fn list_networks_output(&self) -> anyhow::Result<String> {
        let nets = self.docker.list_networks()?;
        let mut lines = vec!["Available Docker networks:".to_string()];
        for net in nets {
            lines.push(format!(
                "  {:<25} driver={:<10} containers={:<4} scope={}",
                net.name, net.driver, net.containers, net.scope
            ));
        }
        Ok(lines.join("\n"))
    }

    pub fn build_proxy(&self) -> anyhow::Result<()> {
        let cfg = self.load_config()?;
        if cfg.containers.is_empty() {
            bail!("No containers configured. Use 'add' command first.");
        }

        fs::create_dir_all(&self.paths.build_dir)
            .with_context(|| format!("Failed to create {}", self.paths.build_dir.display()))?;

        fs::write(
            self.paths.build_dir.join("nginx.conf"),
            generate_nginx_config(&cfg),
        )
        .context("Failed to write nginx.conf")?;
        fs::write(
            self.paths.build_dir.join("Dockerfile"),
            generate_dockerfile(&cfg.host_ports()),
        )
        .context("Failed to write Dockerfile")?;

        self.docker.build_image(
            self.paths.build_dir.to_string_lossy().as_ref(),
            &cfg.proxy_image(),
        )
    }

    pub fn start_proxy(&self) -> anyhow::Result<String> {
        let cfg = self.load_config()?;
        if cfg.containers.is_empty() {
            bail!("No containers configured. Use 'add' command first.");
        }
        if cfg.routes.is_empty() {
            bail!("No routes configured. Use 'switch' command first.");
        }

        let mut networks = BTreeSet::new();
        networks.insert(cfg.network.clone());
        for c in &cfg.containers {
            if let Some(net) = &c.network {
                networks.insert(net.clone());
            }
        }

        for net in &networks {
            self.ensure_network(net)?;
        }

        if self.docker.container_exists(&cfg.proxy_name)? {
            return Ok(format!("Proxy already running: {}", cfg.proxy_name));
        }

        self.build_proxy()?;
        let host_ports = cfg.host_ports();
        self.docker.run_proxy(
            &cfg.proxy_image(),
            &cfg.proxy_name,
            &cfg.network,
            &host_ports,
        )?;

        for net in &networks {
            if net != &cfg.network {
                let _ = self.docker.connect_network(net, &cfg.proxy_name);
            }
        }

        Ok(format!(
            "Proxy started on port(s): {}",
            host_ports
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    pub fn stop_proxy(&self) -> anyhow::Result<String> {
        let cfg = self.load_config()?;
        if self.docker.stop_and_remove(&cfg.proxy_name)? {
            Ok("Proxy stopped".to_string())
        } else {
            Ok("Proxy not running".to_string())
        }
    }

    pub fn reload_proxy(&self) -> anyhow::Result<String> {
        let cfg = self.load_config()?;
        if cfg.containers.is_empty() {
            bail!("No containers configured.");
        }
        if cfg.routes.is_empty() {
            bail!("No routes configured.");
        }
        let _ = self.stop_proxy()?;
        thread::sleep(Duration::from_secs(1));
        self.start_proxy()
    }

    pub fn restart_proxy(&self) -> anyhow::Result<String> {
        let _ = self.stop_proxy()?;
        thread::sleep(Duration::from_secs(1));
        self.start_proxy()
    }

    pub fn status_output(&self) -> anyhow::Result<String> {
        let cfg = self.load_config()?;
        let status = self.docker.inspect_status(&cfg.proxy_name)?;
        let Some(state) = status else {
            return Ok("Proxy not running".to_string());
        };

        let mut lines = vec![
            format!("Proxy: {} ({state})", cfg.proxy_name),
            String::new(),
        ];
        lines.push("Active routes:".to_string());
        for route in &cfg.routes {
            match cfg.containers.iter().find(|c| c.name == route.target) {
                Some(container) => {
                    lines.push(format!(
                        "  {} -> {}:{}",
                        route.host_port,
                        route.target,
                        container.port.unwrap_or(DEFAULT_PORT)
                    ));
                }
                None => {
                    lines.push(format!(
                        "  {} -> {} (container not found)",
                        route.host_port, route.target
                    ));
                }
            }
        }
        Ok(lines.join("\n"))
    }

    pub fn show_config_output(&self) -> anyhow::Result<String> {
        let cfg = self.load_config()?;
        Ok(format!(
            "Config file: {}\n\n{}",
            self.paths.config_file.display(),
            serde_json::to_string_pretty(&cfg)?
        ))
    }

    pub fn show_logs(&self, follow: bool, tail: usize) -> anyhow::Result<()> {
        let cfg = self.load_config()?;
        self.docker.logs(&cfg.proxy_name, follow, tail)
    }

    pub fn install_cli(&self, exe_path: &std::path::Path) -> anyhow::Result<String> {
        fs::create_dir_all(&self.paths.user_bin)
            .with_context(|| format!("Failed to create {}", self.paths.user_bin.display()))?;
        let link = self.paths.user_bin.join("proxy-manager");
        if link.exists() {
            fs::remove_file(&link)
                .with_context(|| format!("Failed removing {}", link.display()))?;
        }
        fs::hard_link(exe_path, &link)
            .with_context(|| format!("Failed linking {}", link.display()))?;
        Ok(format!(
            "Created hardlink: {} -> {}",
            link.display(),
            exe_path.display()
        ))
    }

    fn ensure_network(&self, network_name: &str) -> anyhow::Result<()> {
        let existing = self.docker.list_network_names()?;
        if !existing.iter().any(|n| n == network_name) {
            self.docker.create_network(network_name)?;
        }
        Ok(())
    }
}

pub fn generate_nginx_config(config: &Config) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        let target = &route.target;
        let target_container = config.containers.iter().find(|c| c.name == *target);
        let Some(container) = target_container else {
            continue;
        };

        let internal_port = container.port.unwrap_or(DEFAULT_PORT);
        let host_port = route.host_port;
        servers.push(format!(
            "    server {{\n        listen {host_port};\n\n        set $backend_addr {target}:{internal_port};\n        location / {{\n            proxy_pass http://$backend_addr;\n            proxy_set_header Host $host;\n            proxy_set_header X-Real-IP $remote_addr;\n            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n            resolver 127.0.0.11 valid=30s;\n            proxy_next_upstream error timeout http_502 http_503 http_504;\n            proxy_intercept_errors on;\n            error_page 502 503 504 =503 /fallback_{host_port};\n        }}\n\n        location = /fallback_{host_port} {{\n            default_type text/plain;\n            return 503 'Service temporarily unavailable - container {target} is not running';\n        }}\n    }}\n"
        ));
    }

    format!(
        "events {{}}\n\nhttp {{\n    resolver 127.0.0.11 valid=30s;\n{}\n}}\n",
        servers.join("\n")
    )
}

pub fn generate_dockerfile(host_ports: &[u16]) -> String {
    let ports = host_ports
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "FROM nginx:stable-alpine\nCOPY nginx.conf /etc/nginx/nginx.conf\nEXPOSE {ports}\nCMD [\"nginx\", \"-g\", \"daemon off;\"]\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::NetworkInfo;
    use std::cell::RefCell;
    use tempfile::tempdir;

    #[derive(Default)]
    struct MockDocker {
        network_names: Vec<String>,
        container_exists: bool,
        status: Option<String>,
        runs: RefCell<usize>,
    }

    impl DockerApi for MockDocker {
        fn list_containers(&self, _filter: Option<&str>) -> anyhow::Result<Vec<String>> {
            Ok(vec!["app-v1".to_string(), "app-v2".to_string()])
        }

        fn list_network_names(&self) -> anyhow::Result<Vec<String>> {
            Ok(self.network_names.clone())
        }

        fn create_network(&self, _name: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn inspect_container_network(&self, _name: &str) -> anyhow::Result<Option<String>> {
            Ok(Some("auto-net".to_string()))
        }

        fn list_networks(&self) -> anyhow::Result<Vec<NetworkInfo>> {
            Ok(vec![NetworkInfo {
                name: "bridge".to_string(),
                driver: "bridge".to_string(),
                containers: 2,
                scope: "local".to_string(),
            }])
        }

        fn build_image(&self, _path: &str, _tag: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn container_exists(&self, _name: &str) -> anyhow::Result<bool> {
            Ok(self.container_exists)
        }

        fn run_proxy(
            &self,
            _image: &str,
            _name: &str,
            _network: &str,
            _host_ports: &[u16],
        ) -> anyhow::Result<()> {
            *self.runs.borrow_mut() += 1;
            Ok(())
        }

        fn connect_network(&self, _network: &str, _container: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop_and_remove(&self, _name: &str) -> anyhow::Result<bool> {
            Ok(true)
        }

        fn inspect_status(&self, _name: &str) -> anyhow::Result<Option<String>> {
            Ok(self.status.clone())
        }

        fn logs(&self, _name: &str, _follow: bool, _tail: usize) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn test_manager(docker: MockDocker) -> ProxyManager<MockDocker> {
        let temp = tempdir().expect("tempdir");
        let paths = Paths::from_home(temp.path().to_path_buf());
        ProxyManager::new(docker, paths)
    }

    #[test]
    fn adds_container_with_autodetected_network() {
        let mgr = test_manager(MockDocker::default());
        mgr.add_container("app", Some("Blue"), Some(8080), None)
            .expect("add container");
        let cfg = mgr.load_config().expect("load config");
        assert_eq!(cfg.containers[0].network.as_deref(), Some("auto-net"));
        assert_eq!(cfg.containers[0].port, Some(8080));
    }

    #[test]
    fn switch_target_adds_sorted_route() {
        let mgr = test_manager(MockDocker::default());
        mgr.add_container("b", None, None, Some("n"))
            .expect("add b");
        mgr.add_container("a", None, None, Some("n"))
            .expect("add a");
        mgr.switch_target("b", Some(8002)).expect("switch b");
        mgr.switch_target("a", Some(8001)).expect("switch a");
        let cfg = mgr.load_config().expect("load config");
        assert_eq!(cfg.routes[0].host_port, 8001);
        assert_eq!(cfg.routes[1].host_port, 8002);
    }

    #[test]
    fn start_proxy_runs_when_config_valid() {
        let mgr = test_manager(MockDocker {
            network_names: vec!["proxy-net".to_string()],
            ..MockDocker::default()
        });
        mgr.add_container("app", None, None, Some("proxy-net"))
            .expect("add app");
        mgr.switch_target("app", Some(8000)).expect("switch app");
        let out = mgr.start_proxy().expect("start");
        assert!(out.contains("Proxy started"));
    }

    #[test]
    fn status_output_handles_not_running() {
        let mgr = test_manager(MockDocker::default());
        let out = mgr.status_output().expect("status");
        assert_eq!(out, "Proxy not running");
    }

    #[test]
    fn nginx_config_contains_fallback_route() {
        let cfg = Config {
            containers: vec![ContainerEntry {
                name: "app".to_string(),
                label: None,
                port: Some(9000),
                network: None,
            }],
            routes: vec![RouteEntry {
                host_port: 8001,
                target: "app".to_string(),
            }],
            ..Config::default()
        };
        let text = generate_nginx_config(&cfg);
        assert!(text.contains("listen 8001;"));
        assert!(text.contains("/fallback_8001"));
        assert!(text.contains("app:9000"));
    }

    #[test]
    fn dockerfile_exposes_all_ports() {
        let text = generate_dockerfile(&[8000, 8001]);
        assert!(text.contains("EXPOSE 8000 8001"));
    }
}
