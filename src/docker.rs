use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::image::BuildImageOptions;
use bollard::network::{CreateNetworkOptions, ListNetworksOptions};
use bollard::Docker;
use bytes::Bytes;
use futures::StreamExt;
use std::collections::HashMap;

use crate::config::Config as ProxyConfig;

pub struct DockerClient {
    client: Docker,
}

impl DockerClient {
    pub fn new() -> anyhow::Result<Self> {
        let client = Docker::connect_with_local_defaults()?;
        Ok(Self { client })
    }

    pub async fn list_containers(&self, filter: Option<&str>) -> anyhow::Result<Vec<String>> {
        let mut filters = HashMap::new();
        filters.insert("status", vec!["running"]);

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.client.list_containers(Some(options)).await?;
        let container_names: Vec<String> = containers
            .iter()
            .filter(|c| {
                if let Some(filter) = filter {
                    c.names
                        .as_ref()
                        .map(|names| {
                            names.iter().any(|n| {
                                let name = n.trim_start_matches('/').to_lowercase();
                                name.contains(&filter.to_lowercase())
                            })
                        })
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .filter_map(|c| c.names.as_ref()?.first().cloned())
            .map(|n| n.trim_start_matches('/').to_string())
            .collect();

        Ok(container_names)
    }

    pub async fn list_networks(&self) -> anyhow::Result<Vec<NetworkInfo>> {
        let options = ListNetworksOptions::<String>::default();
        let networks = self.client.list_networks(Some(options)).await?;

        let network_infos: Vec<NetworkInfo> = networks
            .into_iter()
            .map(|n| {
                let containers = n.containers.map(|c| c.len()).unwrap_or(0);
                NetworkInfo {
                    name: n.name.unwrap_or_default(),
                    driver: n.driver.unwrap_or_default(),
                    scope: n.scope.unwrap_or_default(),
                    containers,
                }
            })
            .collect();

        Ok(network_infos)
    }

    pub async fn get_container_network(
        &self,
        container_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let container = self
            .client
            .inspect_container(container_name, None)
            .await
            .ok();
        let networks = container
            .and_then(|c| c.network_settings)
            .and_then(|ns| ns.networks);

        if let Some(networks) = networks {
            Ok(networks.keys().next().cloned())
        } else {
            Ok(None)
        }
    }

    pub async fn ensure_network(&self, network_name: &str) -> anyhow::Result<bool> {
        let mut filters = HashMap::new();
        filters.insert("name", vec![network_name]);

        let options = ListNetworksOptions { filters };

        let networks = self.client.list_networks(Some(options)).await?;
        if networks.is_empty() {
            let options = CreateNetworkOptions {
                name: network_name.to_string(),
                driver: "bridge".to_string(),
                ..Default::default()
            };
            self.client.create_network(options).await?;
            println!("Created network: {}", network_name);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn get_container_status(
        &self,
        container_name: &str,
    ) -> anyhow::Result<Option<String>> {
        match self.client.inspect_container(container_name, None).await {
            Ok(info) => {
                let status = info.state.map(|s| format!("{:?}", s.status));
                Ok(status)
            }
            Err(_) => Ok(None),
        }
    }

    pub async fn container_exists(&self, container_name: &str) -> bool {
        self.client
            .inspect_container(container_name, None)
            .await
            .is_ok()
    }

    pub async fn stop_container(&self, container_name: &str) -> anyhow::Result<()> {
        let options = StopContainerOptions { t: 10 };
        self.client
            .stop_container(container_name, Some(options))
            .await?;
        Ok(())
    }

    pub async fn remove_container(&self, container_name: &str) -> anyhow::Result<()> {
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        self.client
            .remove_container(container_name, Some(options))
            .await?;
        Ok(())
    }

    pub async fn build_proxy_image(
        &self,
        config: &ProxyConfig,
        nginx_conf: &str,
    ) -> anyhow::Result<()> {
        let build_dir = ProxyConfig::build_dir()?;
        std::fs::create_dir_all(&build_dir)?;

        let nginx_conf_path = build_dir.join("nginx.conf");
        std::fs::write(&nginx_conf_path, nginx_conf)?;

        let dockerfile = format!(
            "FROM nginx:stable-alpine\n\
             COPY nginx.conf /etc/nginx/nginx.conf\n\
             EXPOSE {}\n\
             CMD [\"nginx\", \"-g\", \"daemon off;\"]\n\
             ",
            config
                .get_all_host_ports()
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        );

        let dockerfile_path = build_dir.join("Dockerfile");
        let dockerfile_content = dockerfile.clone();
        std::fs::write(&dockerfile_path, &dockerfile)?;

        let proxy_image = config.get_proxy_image();
        let options = BuildImageOptions {
            dockerfile: "Dockerfile",
            t: proxy_image.as_str(),
            rm: true,
            ..Default::default()
        };

        let mut stream =
            self.client
                .build_image(options, None, Some(Bytes::from(dockerfile_content)));
        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(stream) = info.stream {
                        print!("{}", stream);
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Build failed: {}", e)),
            }
        }

        Ok(())
    }

    pub async fn start_proxy(&self, config: &ProxyConfig) -> anyhow::Result<()> {
        let proxy_name = config.proxy_name.clone();
        let proxy_image = config.get_proxy_image();

        let mut port_bindings: HashMap<String, Option<Vec<bollard::service::PortBinding>>> =
            HashMap::new();
        for port in config.get_all_host_ports() {
            port_bindings.insert(
                format!("{}/tcp", port),
                Some(vec![bollard::service::PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(port.to_string()),
                }]),
            );
        }

        let host_config = bollard::service::HostConfig {
            port_bindings: Some(port_bindings),
            network_mode: Some(config.network.clone()),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: proxy_name.clone(),
            platform: None,
        };

        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();
        for port in config.get_all_host_ports() {
            exposed_ports.insert(format!("{}/tcp", port), HashMap::new());
        }

        let container_config = Config {
            image: Some(proxy_image),
            exposed_ports: Some(exposed_ports),
            host_config: Some(host_config),
            cmd: Some(vec![
                "nginx".to_string(),
                "-g".to_string(),
                "daemon off;".to_string(),
            ]),
            ..Default::default()
        };

        match self
            .client
            .create_container(Some(options), container_config)
            .await
        {
            Ok(_) => {
                self.client
                    .start_container(&proxy_name, None::<StartContainerOptions<String>>)
                    .await?;
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("conflict") {
                    println!("Proxy already running: {}", proxy_name);
                } else {
                    return Err(anyhow::anyhow!("Failed to start proxy: {}", e));
                }
            }
        }

        Ok(())
    }

    pub async fn stop_proxy(&self, proxy_name: &str) -> anyhow::Result<bool> {
        match self.client.inspect_container(proxy_name, None).await {
            Ok(_) => {
                println!("Stopping proxy: {}", proxy_name);
                let _ = self
                    .client
                    .stop_container(proxy_name, Some(StopContainerOptions { t: 10 }))
                    .await;
                let _ = self
                    .client
                    .remove_container(
                        proxy_name,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                println!("Proxy stopped");
                Ok(true)
            }
            Err(_) => {
                println!("Proxy not running");
                Ok(false)
            }
        }
    }

    pub async fn logs(
        &self,
        container_name: &str,
        follow: bool,
        tail: usize,
    ) -> anyhow::Result<()> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            timestamps: true,
            follow,
            ..Default::default()
        };

        let mut stream = self.client.logs(container_name, Some(options));
        while let Some(result) = stream.next().await {
            match result {
                Ok(log) => {
                    print!("{}", log);
                }
                Err(e) => {
                    eprintln!("Error reading logs: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    pub async fn is_proxy_running(&self, proxy_name: &str) -> bool {
        if let Ok(container) = self.client.inspect_container(proxy_name, None).await {
            container
                .state
                .map(|s| format!("{:?}", s.status).contains("Running"))
                .unwrap_or(false)
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub containers: usize,
}

pub fn generate_nginx_config(config: &ProxyConfig) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        if let Some(container) = config.find_container(&route.target) {
            let internal_port = config.get_internal_port(container);
            let host_port = route.host_port;

            servers.push(format!(
                "    server {{\n\
                        listen {};\n\
                 \n\
                        set $backend_addr {}:{};\n\
                        location / {{\n\
                            proxy_pass http://$backend_addr;\n\
                            proxy_set_header Host $host;\n\
                            proxy_set_header X-Real-IP $remote_addr;\n\
                            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n\
                            resolver 127.0.0.11 valid=30s;\n\
                            proxy_next_upstream error timeout http_502 http_503 http_504;\n\
                            proxy_intercept_errors on;\n\
                            error_page 502 503 504 =503 /fallback_{};\n\
                        }}\n\
                 \n\
                        location = /fallback_{} {{\n\
                            default_type text/plain;\n\
                            return 503 'Service temporarily unavailable - container {} is not running';\n\
                        }}\n\
                 }}\n",
                host_port, route.target, internal_port, host_port, host_port, route.target
            ));
        }
    }

    let servers_str = servers.join("\n");

    format!(
        "events {{}}\n\nhttp {{\n    resolver 127.0.0.11 valid=30s;\n{}\n}}\n",
        servers_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_nginx_config_empty() {
        let config = ProxyConfig::default();
        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("events {}"));
        assert!(nginx_conf.contains("http {"));
    }

    #[test]
    fn test_generate_nginx_config_with_route() {
        let mut config = ProxyConfig::default();
        config.containers.push(crate::config::Container {
            name: "my-app".to_string(),
            label: Some("My App".to_string()),
            port: Some(8080),
            network: None,
        });
        config.routes.push(crate::config::Route {
            host_port: 8000,
            target: "my-app".to_string(),
        });

        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("listen 8000;"));
        assert!(nginx_conf.contains("set $backend_addr my-app:8080;"));
    }

    #[test]
    fn test_generate_nginx_config_multiple_routes() {
        let mut config = ProxyConfig::default();
        config.containers.push(crate::config::Container {
            name: "app1".to_string(),
            label: None,
            port: Some(8001),
            network: None,
        });
        config.containers.push(crate::config::Container {
            name: "app2".to_string(),
            label: None,
            port: Some(8002),
            network: None,
        });
        config.routes.push(crate::config::Route {
            host_port: 8000,
            target: "app1".to_string(),
        });
        config.routes.push(crate::config::Route {
            host_port: 8001,
            target: "app2".to_string(),
        });

        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("listen 8000;"));
        assert!(nginx_conf.contains("listen 8001;"));
    }
}
