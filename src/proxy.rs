use crate::config::{
    get_all_host_ports, get_build_dir, get_proxy_image, get_proxy_name, load_config, save_config,
    Config, Container, Route,
};
use crate::docker::{ensure_network, DockerClient};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;

const DEFAULT_PORT: u16 = 8000;

fn get_internal_port(container: Option<&Container>) -> u16 {
    container.and_then(|c| c.port).unwrap_or(DEFAULT_PORT)
}

fn generate_nginx_config(config: &Config) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        let target = route.target.clone();
        let target_container = config.containers.iter().find(|c| c.name == *target);
        if target_container.is_none() {
            continue;
        }
        let container = target_container.unwrap();
        let internal_port = get_internal_port(Some(container));
        let host_port = route.host_port;

        let server = format!(
            r#"    server {{
        listen {host_port};

        set $backend_addr {target}:{internal_port};
        location / {{
            proxy_pass http://$backend_addr;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            resolver 127.0.0.11 valid=30s;
            proxy_next_upstream error timeout http_502 http_503 http_504;
            proxy_intercept_errors on;
            error_page 502 503 504 =503 /fallback_{host_port};
        }}

        location = /fallback_{host_port} {{
            default_type text/plain;
            return 503 'Service temporarily unavailable - container {target} is not running';
        }}
    }}"#
        );
        servers.push(server);
    }

    let servers_str = servers.join("\n\n");

    format!(
        r#"events {{}}

http {{
    resolver 127.0.0.11 valid=30s;
{servers_str}
}}
"#
    )
}

async fn build_proxy(config: &Config) -> Result<bool> {
    if config.containers.is_empty() {
        println!("Error: No containers configured. Use 'add' command first.");
        return Ok(false);
    }

    let build_dir = get_build_dir();
    if let Some(parent) = build_dir.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create build directory: {}", parent.display()))?;
    }

    let nginx_conf = generate_nginx_config(config);

    let nginx_conf_path = build_dir.join("nginx.conf");
    fs::write(&nginx_conf_path, nginx_conf)
        .with_context(|| format!("Failed to write nginx.conf: {}", nginx_conf_path.display()))?;

    let host_ports = get_all_host_ports(Some(config));
    let exposed_ports_str: Vec<String> = host_ports.iter().map(|p| p.to_string()).collect();
    let exposed_ports_str = exposed_ports_str.join(" ");

    let dockerfile = format!(
        r#"FROM nginx:stable-alpine
COPY nginx.conf /etc/nginx/nginx.conf
EXPOSE {exposed_ports_str}
CMD ["nginx", "-g", "daemon off;"]
"#
    );

    let dockerfile_path = build_dir.join("Dockerfile");
    fs::write(&dockerfile_path, dockerfile)
        .with_context(|| format!("Failed to write Dockerfile: {}", dockerfile_path.display()))?;

    println!("Building proxy image...");
    let proxy_image = get_proxy_image(Some(config));

    match DockerClient::new()?
        .build_image(&build_dir, &proxy_image)
        .await
    {
        Ok(_) => Ok(true),
        Err(e) => {
            println!("Build failed: {}", e);
            Ok(false)
        }
    }
}

pub async fn start_proxy() -> Result<bool> {
    let config = load_config()?;

    if config.containers.is_empty() {
        println!("Error: No containers configured. Use 'add' command first.");
        return Ok(false);
    }

    if config.routes.is_empty() {
        println!("Error: No routes configured. Use 'switch' command first.");
        return Ok(false);
    }

    let mut networks = std::collections::HashSet::new();
    networks.insert(config.network.clone());
    for c in &config.containers {
        if let Some(net) = &c.network {
            networks.insert(net.clone());
        }
    }

    for network in &networks {
        ensure_network(network).await?;
    }

    let proxy_name = get_proxy_name(Some(&config));
    let docker = DockerClient::new()?;

    if docker.container_exists(&proxy_name).await {
        println!("Proxy already running: {}", proxy_name);
        return Ok(true);
    }

    if !build_proxy(&config).await? {
        return Ok(false);
    }

    let host_ports = get_all_host_ports(Some(&config));
    let mut ports_mapping = HashMap::new();
    for port in &host_ports {
        ports_mapping.insert(format!("{}/tcp", port), *port);
    }

    println!("Starting proxy: {}", proxy_name);
    docker
        .start_container(
            &proxy_name,
            &get_proxy_image(Some(&config)),
            &config.network,
            &ports_mapping,
        )
        .await
        .context("Failed to start proxy container")?;

    let port_str = host_ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    println!("Proxy started on port(s): {}", port_str);

    Ok(true)
}

pub async fn stop_proxy() -> Result<bool> {
    let config = load_config()?;
    let proxy_name = get_proxy_name(Some(&config));

    let docker = DockerClient::new()?;

    if docker.container_exists(&proxy_name).await {
        println!("Stopping proxy: {}", proxy_name);
        docker.stop_container(&proxy_name).await?;
        println!("Proxy stopped");
        Ok(true)
    } else {
        println!("Proxy not running");
        Ok(true)
    }
}

pub async fn stop_port(host_port: u16) -> Result<bool> {
    let mut config = load_config()?;

    let route = config.routes.iter().find(|r| r.host_port == host_port);
    if route.is_none() {
        println!("Error: No route found for port {}", host_port);
        return Ok(false);
    }

    config.routes.retain(|r| r.host_port != host_port);
    save_config(&config)?;
    println!("Removed route: port {}", host_port);

    if config.routes.is_empty() {
        stop_proxy().await
    } else {
        reload_proxy().await
    }
}

pub async fn reload_proxy() -> Result<bool> {
    let config = load_config()?;

    if config.containers.is_empty() {
        println!("Error: No containers configured.");
        return Ok(false);
    }

    if config.routes.is_empty() {
        println!("Error: No routes configured.");
        return Ok(false);
    }

    println!("Reloading proxy...");
    stop_proxy().await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    start_proxy().await
}

pub async fn switch_target(identifier: &str, host_port: Option<u16>) -> Result<bool> {
    let mut config = load_config()?;

    let identifier_str = identifier.to_string();
    let container = config
        .containers
        .iter()
        .find(|c| c.name == identifier_str || c.label.as_ref() == Some(&identifier_str));

    if container.is_none() {
        println!("Error: Container '{}' not found in config", identifier);
        return Ok(false);
    }

    let container = container.unwrap();
    let host_port = host_port.unwrap_or(DEFAULT_PORT);

    let existing_route = config.routes.iter_mut().find(|r| r.host_port == host_port);
    if let Some(route) = existing_route {
        route.target = container.name.clone();
        save_config(&config)?;
        println!("Switching route: {} -> {}", host_port, container.name);
    } else {
        config.routes.push(Route {
            host_port,
            target: container.name.clone(),
        });
        config.routes.sort_by_key(|r| r.host_port);
        save_config(&config)?;
        println!("Adding route: {} -> {}", host_port, container.name);
    }

    reload_proxy().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_nginx_config_empty() {
        let config = Config::default();
        let result = generate_nginx_config(&config);
        assert!(result.contains("events"));
        assert!(result.contains("http"));
    }

    #[test]
    fn test_generate_nginx_config_with_routes() {
        let mut config = Config::default();
        config.containers.push(Container {
            name: "test-container".to_string(),
            label: Some("Test".to_string()),
            port: Some(8080),
            network: None,
        });
        config.routes.push(Route {
            host_port: 8000,
            target: "test-container".to_string(),
        });

        let result = generate_nginx_config(&config);
        assert!(result.contains("listen 8000"));
        assert!(result.contains("test-container:8080"));
    }

    #[test]
    fn test_get_internal_port() {
        let container = Container {
            name: "test".to_string(),
            label: None,
            port: Some(9000),
            network: None,
        };
        assert_eq!(get_internal_port(Some(&container)), 9000);
        assert_eq!(get_internal_port(None), DEFAULT_PORT);
    }
}
