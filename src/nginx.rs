use crate::config::{Config, Container, Route};

pub fn generate_nginx_config(config: &Config) -> String {
    let servers: Vec<String> = config
        .routes
        .iter()
        .filter_map(|route| {
            let target_container = config.find_container(&route.target)?;
            let internal_port = config.get_internal_port(target_container);
            let host_port = route.host_port;

            Some(format!(
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
                host_port, route.target, internal_port, host_port, host_port, route.target
            ))
        })
        .collect();

    let servers_str = servers.join("\n");

    format!(
        r#"events {{}}

http {{
    resolver 127.0.0.11 valid=30s;
{}}}
 "#,
        servers_str
    )
}

pub fn generate_dockerfile(config: &Config) -> String {
    let host_ports = config.get_all_host_ports();
    let ports_str = host_ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"FROM nginx:stable-alpine
COPY nginx.conf /etc/nginx/nginx.conf
EXPOSE {}
CMD ["nginx", "-g", "daemon off;"]"#,
        ports_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> Config {
        let mut config = Config::default();
        config.containers.push(Container {
            name: "app1".to_string(),
            label: Some("App 1".to_string()),
            port: Some(8000),
            network: Some("network1".to_string()),
        });
        config.containers.push(Container {
            name: "app2".to_string(),
            label: Some("App 2".to_string()),
            port: None,
            network: None,
        });
        config.routes.push(Route {
            host_port: 8080,
            target: "app1".to_string(),
        });
        config.routes.push(Route {
            host_port: 8081,
            target: "app2".to_string(),
        });
        config
    }

    #[test]
    fn test_generate_nginx_config() {
        let config = create_test_config();
        let nginx_config = generate_nginx_config(&config);

        assert!(nginx_config.contains("listen 8080;"));
        assert!(nginx_config.contains("listen 8081;"));
        assert!(nginx_config.contains("set $backend_addr app1:8000;"));
        assert!(nginx_config.contains("set $backend_addr app2:8000;"));
        assert!(nginx_config.contains("proxy_pass http://$backend_addr;"));
    }

    #[test]
    fn test_generate_dockerfile() {
        let config = create_test_config();
        let dockerfile = generate_dockerfile(&config);

        assert!(dockerfile.contains("FROM nginx:stable-alpine"));
        assert!(dockerfile.contains("COPY nginx.conf /etc/nginx/nginx.conf"));
        assert!(dockerfile.contains("EXPOSE 8080 8081"));
        assert!(dockerfile.contains(r#"CMD ["nginx", "-g", "daemon off;"]"#));
    }

    #[test]
    fn test_generate_nginx_config_empty() {
        let config = Config::default();
        let nginx_config = generate_nginx_config(&config);

        assert!(!nginx_config.contains("server {"));
        assert!(nginx_config.contains("events {}"));
        assert!(nginx_config.contains("http {"));
    }
}
