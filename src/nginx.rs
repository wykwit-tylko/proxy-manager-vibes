use crate::config::Config;
use anyhow::{Context, Result};
use std::fs;

pub fn generate_nginx_config(config: &Config) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        let target = &route.target;
        let target_container = config.containers.iter().find(|c| c.name == *target);

        if target_container.is_none() {
            continue;
        }

        let internal_port = config.get_internal_port(target_container.unwrap());
        let host_port = route.host_port;

        let server_block = format!(
            r#"    server {{
        listen {};

        set $backend_addr {}:{};
        location / {{
            proxy_pass http://$backend_addr;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            resolver 127.0.0.11 valid=30s;
            proxy_next_upstream error timeout http_502 http_503 http_504;
            proxy_intercept_errors on;
            error_page 502 503 504 =503 /fallback_{};
        }}

        location = /fallback_{} {{
            default_type text/plain;
            return 503 'Service temporarily unavailable - container {} is not running';
        }}
    }}"#,
            host_port, target, internal_port, host_port, host_port, target
        );

        servers.push(server_block);
    }

    let servers_str = servers.join("\n\n");

    format!(
        r#"events {{}}

http {{
    resolver 127.0.0.11 valid=30s;
{}
}}
"#,
        servers_str
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
EXPOSE {}
CMD ["nginx", "-g", "daemon off;"]
"#,
        expose_ports
    )
}

pub fn write_build_files(config: &Config) -> Result<()> {
    let build_dir = Config::build_dir()?;
    fs::create_dir_all(&build_dir).context("Failed to create build directory")?;

    let nginx_conf = generate_nginx_config(config);
    let nginx_conf_path = build_dir.join("nginx.conf");
    fs::write(&nginx_conf_path, nginx_conf).context("Failed to write nginx.conf")?;

    let dockerfile = generate_dockerfile(config);
    let dockerfile_path = build_dir.join("Dockerfile");
    fs::write(&dockerfile_path, dockerfile).context("Failed to write Dockerfile")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ContainerConfig, Route};

    #[test]
    fn test_generate_nginx_config() {
        let mut config = Config::default();
        config.containers.push(ContainerConfig {
            name: "app1".to_string(),
            label: Some("App 1".to_string()),
            port: Some(8080),
            network: None,
        });
        config.routes.push(Route {
            host_port: 8000,
            target: "app1".to_string(),
        });

        let nginx_conf = generate_nginx_config(&config);

        assert!(nginx_conf.contains("listen 8000"));
        assert!(nginx_conf.contains("app1:8080"));
        assert!(nginx_conf.contains("fallback_8000"));
    }

    #[test]
    fn test_generate_dockerfile() {
        let mut config = Config::default();
        config.routes.push(Route {
            host_port: 8000,
            target: "app1".to_string(),
        });
        config.routes.push(Route {
            host_port: 8001,
            target: "app2".to_string(),
        });

        let dockerfile = generate_dockerfile(&config);

        assert!(dockerfile.contains("FROM nginx:stable-alpine"));
        assert!(dockerfile.contains("EXPOSE 8000 8001"));
        assert!(dockerfile.contains("CMD"));
    }

    #[test]
    fn test_empty_routes() {
        let config = Config::default();
        let nginx_conf = generate_nginx_config(&config);

        // Should still have valid nginx structure even with no routes
        assert!(nginx_conf.contains("events {}"));
        assert!(nginx_conf.contains("http {"));
    }
}
