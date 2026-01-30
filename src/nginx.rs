use crate::config::{Config, DEFAULT_PORT};

pub fn generate_nginx_config(config: &Config) -> String {
    let mut servers = String::new();

    for route in &config.routes {
        let target = &route.target;
        let target_container = config.containers.iter().find(|c| c.name == *target);
        let Some(container) = target_container else {
            continue;
        };

        let internal_port = container.port.unwrap_or(DEFAULT_PORT);
        let host_port = route.host_port;

        servers.push_str("    server {\n");
        servers.push_str(&format!("        listen {host_port};\n\n"));
        servers.push_str(&format!(
            "        set $backend_addr {target}:{internal_port};\n"
        ));
        servers.push_str("        location / {\n");
        servers.push_str("            proxy_pass http://$backend_addr;\n");
        servers.push_str("            proxy_set_header Host $host;\n");
        servers.push_str("            proxy_set_header X-Real-IP $remote_addr;\n");
        servers
            .push_str("            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n");
        servers.push_str("            resolver 127.0.0.11 valid=30s;\n");
        servers.push_str(
            "            proxy_next_upstream error timeout http_502 http_503 http_504;\n",
        );
        servers.push_str("            proxy_intercept_errors on;\n");
        servers.push_str(&format!(
            "            error_page 502 503 504 =503 /fallback_{host_port};\n"
        ));
        servers.push_str("        }\n\n");
        servers.push_str(&format!("        location = /fallback_{host_port} {{\n"));
        servers.push_str("            default_type text/plain;\n");
        servers.push_str(&format!(
            "            return 503 'Service temporarily unavailable - container {target} is not running';\n"
        ));
        servers.push_str("        }\n");
        servers.push_str("    }\n\n");
    }

    format!("events {{}}\n\nhttp {{\n    resolver 127.0.0.11 valid=30s;\n{servers}}}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ContainerConfig, Route};

    #[test]
    fn nginx_config_includes_server_blocks_for_existing_targets_only() {
        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "app".to_string(),
                label: None,
                port: Some(9000),
                network: None,
            }],
            routes: vec![
                Route {
                    host_port: 8000,
                    target: "app".to_string(),
                },
                Route {
                    host_port: 8001,
                    target: "missing".to_string(),
                },
            ],
            ..Config::default()
        };

        let conf = generate_nginx_config(&cfg);
        assert!(conf.contains("listen 8000;"));
        assert!(conf.contains("set $backend_addr app:9000;"));
        assert!(!conf.contains("listen 8001;"));
        assert!(conf.contains("events {}"));
        assert!(conf.contains("http {"));
    }
}
