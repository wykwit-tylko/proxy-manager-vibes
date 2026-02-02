use crate::config::Config;

pub struct NginxConfigGenerator;

impl NginxConfigGenerator {
    pub fn generate(config: &Config) -> String {
        let mut servers = Vec::new();

        for route in &config.routes {
            let target = &route.target;
            let target_container = config.find_container(target);

            if target_container.is_none() {
                continue;
            }

            let target_container = target_container.unwrap();
            let internal_port = target_container.get_port();
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
    }}"#,
                host_port = host_port,
                target = target,
                internal_port = internal_port
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
CMD ["nginx", "-g", "daemon off;"]"#,
            expose_ports = expose_ports
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Container, Route};

    #[test]
    fn test_generate_nginx_config_empty() {
        let config = Config::default();
        let nginx_conf = NginxConfigGenerator::generate(&config);

        assert!(nginx_conf.contains("events {}"));
        assert!(nginx_conf.contains("http {"));
        assert!(!nginx_conf.contains("server {"));
    }

    #[test]
    fn test_generate_nginx_config_with_routes() {
        let mut config = Config::default();
        config
            .containers
            .push(Container::new("app1").with_port(8080));
        config.routes.push(Route::new(8000, "app1"));

        let nginx_conf = NginxConfigGenerator::generate(&config);

        assert!(nginx_conf.contains("listen 8000"));
        assert!(nginx_conf.contains("set $backend_addr app1:8080"));
        assert!(nginx_conf.contains("proxy_pass http://$backend_addr"));
        assert!(nginx_conf.contains("resolver 127.0.0.11 valid=30s"));
    }

    #[test]
    fn test_generate_nginx_config_multiple_routes() {
        let mut config = Config::default();
        config
            .containers
            .push(Container::new("app1").with_port(8080));
        config
            .containers
            .push(Container::new("app2").with_port(9090));
        config.routes.push(Route::new(8000, "app1"));
        config.routes.push(Route::new(8001, "app2"));

        let nginx_conf = NginxConfigGenerator::generate(&config);

        assert!(nginx_conf.contains("listen 8000"));
        assert!(nginx_conf.contains("listen 8001"));
        assert!(nginx_conf.contains("app1:8080"));
        assert!(nginx_conf.contains("app2:9090"));
    }

    #[test]
    fn test_generate_dockerfile() {
        let mut config = Config::default();
        config.routes.push(Route::new(8000, "app1"));
        config.routes.push(Route::new(8001, "app2"));

        let dockerfile = NginxConfigGenerator::generate_dockerfile(&config);

        assert!(dockerfile.contains("FROM nginx:stable-alpine"));
        assert!(dockerfile.contains("COPY nginx.conf /etc/nginx/nginx.conf"));
        assert!(dockerfile.contains("EXPOSE 8000 8001"));
        assert!(dockerfile.contains("CMD [\"nginx\", \"-g\", \"daemon off;\"]"));
    }

    #[test]
    fn test_generate_dockerfile_default_port() {
        let config = Config::default();
        let dockerfile = NginxConfigGenerator::generate_dockerfile(&config);

        assert!(dockerfile.contains("EXPOSE 8000"));
    }

    #[test]
    fn test_nginx_config_fallback_page() {
        let mut config = Config::default();
        config.containers.push(Container::new("app1"));
        config.routes.push(Route::new(8000, "app1"));

        let nginx_conf = NginxConfigGenerator::generate(&config);

        assert!(nginx_conf.contains("error_page 502 503 504 =503 /fallback_8000"));
        assert!(nginx_conf.contains("location = /fallback_8000"));
        assert!(nginx_conf.contains("Service temporarily unavailable"));
    }
}
