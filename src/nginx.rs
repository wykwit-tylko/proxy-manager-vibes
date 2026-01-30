use crate::config::{AppConfig, ContainerConfig};
use crate::paths::DEFAULT_PORT;

pub fn internal_port(target: Option<&ContainerConfig>) -> u16 {
    target.and_then(|c| c.port).unwrap_or(DEFAULT_PORT)
}

pub fn generate_nginx_config(config: &AppConfig) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        let target = config.containers.iter().find(|c| c.name == route.target);
        if target.is_none() {
            continue;
        }
        let port = internal_port(target);
        let host_port = route.host_port;
        let target_name = &route.target;

        let server_block = format!(
            "    server {{\n        listen {};\n\n        set $backend_addr {}:{};\n        location / {{\n            proxy_pass http://$backend_addr;\n            proxy_set_header Host $host;\n            proxy_set_header X-Real-IP $remote_addr;\n            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n            resolver 127.0.0.11 valid=30s;\n            proxy_next_upstream error timeout http_502 http_503 http_504;\n            proxy_intercept_errors on;\n            error_page 502 503 504 =503 /fallback_{};\n        }}\n\n        location = /fallback_{} {{\n            default_type text/plain;\n            return 503 'Service temporarily unavailable - container {} is not running';\n        }}\n    }}\n",
            host_port, target_name, port, host_port, host_port, target_name
        );
        servers.push(server_block);
    }

    let servers_str = servers.join("\n");
    format!("events {{}}\n\nhttp {{\n    resolver 127.0.0.11 valid=30s;\n{servers_str}\n}}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ContainerConfig, RouteConfig};

    #[test]
    fn generate_nginx_includes_routes() {
        let config = AppConfig {
            containers: vec![ContainerConfig {
                name: "svc".to_string(),
                label: None,
                port: Some(9000),
                network: None,
            }],
            routes: vec![RouteConfig {
                host_port: 8001,
                target: "svc".to_string(),
            }],
            ..AppConfig::default()
        };

        let conf = generate_nginx_config(&config);
        assert!(conf.contains("listen 8001"));
        assert!(conf.contains("set $backend_addr svc:9000"));
    }

    #[test]
    fn generate_nginx_skips_missing_container() {
        let config = AppConfig {
            containers: vec![],
            routes: vec![RouteConfig {
                host_port: 8001,
                target: "missing".to_string(),
            }],
            ..AppConfig::default()
        };

        let conf = generate_nginx_config(&config);
        assert!(!conf.contains("listen 8001"));
    }

    #[test]
    fn internal_port_defaults() {
        assert_eq!(internal_port(None), DEFAULT_PORT);
        let container = ContainerConfig {
            name: "svc".to_string(),
            label: None,
            port: None,
            network: None,
        };
        assert_eq!(internal_port(Some(&container)), DEFAULT_PORT);
    }
}
