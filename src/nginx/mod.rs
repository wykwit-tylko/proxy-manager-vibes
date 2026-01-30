use crate::config::Config;

pub fn generate_nginx_config(config: &Config) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        let target = &route.target;
        let target_container = config.find_container(target);

        if target_container.is_none() {
            continue;
        }

        let target_container = target_container.unwrap();
        let internal_port = config.get_internal_port(Some(target_container));
        let host_port = route.host_port;

        let server_block = format!(
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

        servers.push(server_block);
    }

    let servers_str = servers.join("\n\n");

    format!(
        r#"events {{}}

http {{
    resolver 127.0.0.11 valid=30s;
{servers_str}
}}"#
    )
}

pub fn generate_dockerfile(config: &Config) -> String {
    let host_ports = config.get_all_host_ports();
    let expose_ports = host_ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"FROM nginx:stable-alpine
COPY nginx.conf /etc/nginx/nginx.conf
EXPOSE {expose_ports}
CMD ["nginx", "-g", "daemon off;"]"#
    )
}

pub async fn build_proxy(
    docker: &crate::docker::DockerClient,
    config: &Config,
) -> anyhow::Result<()> {
    if config.containers.is_empty() {
        return Err(anyhow::anyhow!(
            "No containers configured. Use 'add' command first."
        ));
    }

    let build_dir = Config::build_dir();
    tokio::fs::create_dir_all(&build_dir).await?;

    // Generate nginx config
    let nginx_conf = generate_nginx_config(config);
    let nginx_path = build_dir.join("nginx.conf");
    tokio::fs::write(&nginx_path, nginx_conf).await?;

    // Generate Dockerfile
    let dockerfile = generate_dockerfile(config);
    let dockerfile_path = build_dir.join("Dockerfile");
    tokio::fs::write(&dockerfile_path, dockerfile).await?;

    // Build image
    println!("Building proxy image...");
    let proxy_image = config.get_proxy_image();
    docker.build_image(&build_dir, &proxy_image).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ContainerConfig, Route};

    #[test]
    fn test_generate_nginx_config_empty() {
        let config = Config::default();
        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("events {}"));
        assert!(nginx_conf.contains("http {"));
    }

    #[test]
    fn test_generate_nginx_config_with_routes() {
        let mut config = Config::default();
        config.containers.push(ContainerConfig {
            name: "app1".to_string(),
            label: None,
            port: Some(8080),
            network: None,
        });
        config.routes.push(Route {
            host_port: 8000,
            target: "app1".to_string(),
        });

        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("listen 8000"));
        assert!(nginx_conf.contains("set $backend_addr app1:8080"));
        assert!(nginx_conf.contains("proxy_pass http://$backend_addr"));
    }

    #[test]
    fn test_generate_dockerfile() {
        let mut config = Config::default();
        config.routes.push(Route {
            host_port: 8000,
            target: "app1".to_string(),
        });
        config.routes.push(Route {
            host_port: 8080,
            target: "app2".to_string(),
        });

        let dockerfile = generate_dockerfile(&config);
        assert!(dockerfile.contains("FROM nginx:stable-alpine"));
        assert!(dockerfile.contains("EXPOSE 8000 8080"));
        assert!(dockerfile.contains("COPY nginx.conf /etc/nginx/nginx.conf"));
    }

    #[test]
    fn test_generate_dockerfile_default_port() {
        let config = Config::default();
        let dockerfile = generate_dockerfile(&config);
        assert!(dockerfile.contains("EXPOSE 8000"));
    }
}
