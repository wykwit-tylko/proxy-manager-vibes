use crate::config::{Config, Container};

/// Generate the nginx.conf content from the current configuration.
pub fn generate_nginx_config(config: &Config) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        let target_container: Option<&Container> =
            config.containers.iter().find(|c| c.name == route.target);

        let Some(target_container) = target_container else {
            continue;
        };

        let internal_port = Config::internal_port(target_container);
        let host_port = route.host_port;
        let target = &route.target;

        servers.push(format!(
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
        ));
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

/// Generate the Dockerfile content for the proxy.
pub fn generate_dockerfile(host_ports: &[u16]) -> String {
    let expose = host_ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"FROM nginx:stable-alpine
COPY nginx.conf /etc/nginx/nginx.conf
EXPOSE {expose}
CMD ["nginx", "-g", "daemon off;"]"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Container, Route};

    fn test_config() -> Config {
        Config {
            containers: vec![
                Container {
                    name: "app-v1".to_string(),
                    label: Some("Version 1".to_string()),
                    port: Some(8080),
                    network: None,
                },
                Container {
                    name: "app-v2".to_string(),
                    label: None,
                    port: None,
                    network: Some("custom-net".to_string()),
                },
            ],
            routes: vec![
                Route {
                    host_port: 8000,
                    target: "app-v1".to_string(),
                },
                Route {
                    host_port: 9000,
                    target: "app-v2".to_string(),
                },
            ],
            proxy_name: "proxy-manager".to_string(),
            network: "proxy-net".to_string(),
        }
    }

    #[test]
    fn test_generate_nginx_config_basic() {
        let config = test_config();
        let nginx_conf = generate_nginx_config(&config);

        // Should contain events block
        assert!(nginx_conf.contains("events {}"));
        // Should contain http block
        assert!(nginx_conf.contains("http {"));
        // Should contain resolver
        assert!(nginx_conf.contains("resolver 127.0.0.11 valid=30s;"));
        // Should contain server blocks for both routes
        assert!(nginx_conf.contains("listen 8000;"));
        assert!(nginx_conf.contains("listen 9000;"));
        // Should have correct backend addresses
        assert!(nginx_conf.contains("set $backend_addr app-v1:8080;"));
        assert!(nginx_conf.contains("set $backend_addr app-v2:8000;")); // default port
        // Should have fallback locations
        assert!(nginx_conf.contains("/fallback_8000"));
        assert!(nginx_conf.contains("/fallback_9000"));
        // Should contain error messages with container names
        assert!(nginx_conf.contains("container app-v1 is not running"));
        assert!(nginx_conf.contains("container app-v2 is not running"));
    }

    #[test]
    fn test_generate_nginx_config_no_routes() {
        let config = Config::default();
        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("events {}"));
        assert!(nginx_conf.contains("http {"));
        // No server blocks
        assert!(!nginx_conf.contains("server {"));
    }

    #[test]
    fn test_generate_nginx_config_missing_container() {
        let config = Config {
            containers: vec![],
            routes: vec![Route {
                host_port: 8000,
                target: "nonexistent".to_string(),
            }],
            proxy_name: "test".to_string(),
            network: "test-net".to_string(),
        };
        let nginx_conf = generate_nginx_config(&config);
        // Route with missing container should be skipped
        assert!(!nginx_conf.contains("server {"));
    }

    #[test]
    fn test_generate_dockerfile() {
        let dockerfile = generate_dockerfile(&[8000, 9000]);
        assert!(dockerfile.contains("FROM nginx:stable-alpine"));
        assert!(dockerfile.contains("EXPOSE 8000 9000"));
        assert!(dockerfile.contains("COPY nginx.conf /etc/nginx/nginx.conf"));
        assert!(dockerfile.contains("CMD [\"nginx\", \"-g\", \"daemon off;\"]"));
    }

    #[test]
    fn test_generate_dockerfile_single_port() {
        let dockerfile = generate_dockerfile(&[3000]);
        assert!(dockerfile.contains("EXPOSE 3000"));
    }

    #[test]
    fn test_proxy_headers() {
        let config = test_config();
        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("proxy_set_header Host $host;"));
        assert!(nginx_conf.contains("proxy_set_header X-Real-IP $remote_addr;"));
        assert!(
            nginx_conf.contains("proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;")
        );
    }

    #[test]
    fn test_error_handling_directives() {
        let config = test_config();
        let nginx_conf = generate_nginx_config(&config);
        assert!(
            nginx_conf.contains("proxy_next_upstream error timeout http_502 http_503 http_504;")
        );
        assert!(nginx_conf.contains("proxy_intercept_errors on;"));
    }
}
