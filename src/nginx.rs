use crate::config::Config;

pub fn generate_nginx_config(config: &Config) -> String {
    let mut servers = Vec::new();

    for route in &config.routes {
        let target_container = config.containers.iter().find(|c| c.name == route.target);

        if let Some(container) = target_container {
            let internal_port = container.port.unwrap_or(8000);
            let host_port = route.host_port;
            let target = &container.name;

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

pub fn generate_dockerfile(host_ports: &[u16]) -> String {
    let ports_str = host_ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"FROM nginx:stable-alpine
COPY nginx.conf /etc/nginx/nginx.conf
EXPOSE {ports_str}
CMD ["nginx", "-g", "daemon off;"]
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ContainerConfig, RouteConfig};

    #[test]
    fn test_generate_nginx_config() {
        let mut config = Config::default();
        config.containers.push(ContainerConfig {
            name: "app1".to_string(),
            label: None,
            port: Some(8080),
            network: None,
        });
        config.routes.push(RouteConfig {
            host_port: 80,
            target: "app1".to_string(),
        });

        let nginx_conf = generate_nginx_config(&config);
        assert!(nginx_conf.contains("listen 80;"));
        assert!(nginx_conf.contains("set $backend_addr app1:8080;"));
    }

    #[test]
    fn test_generate_dockerfile() {
        let dockerfile = generate_dockerfile(&[80, 443]);
        assert!(dockerfile.contains("EXPOSE 80 443"));
    }
}
