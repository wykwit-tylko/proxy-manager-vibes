use crate::config::ConfigManager;
use anyhow::Result;

pub struct RouteManager {
    config_manager: ConfigManager,
    proxy_manager: crate::proxy::ProxyManager,
}

impl RouteManager {
    pub fn new(config_manager: ConfigManager, proxy_manager: crate::proxy::ProxyManager) -> Self {
        Self {
            config_manager,
            proxy_manager,
        }
    }

    pub async fn switch_target(&self, identifier: String, host_port: Option<u16>) -> Result<()> {
        let mut config = self.config_manager.load()?;

        let container = config
            .find_container(&identifier)
            .ok_or_else(|| anyhow::anyhow!("Container '{}' not found in config", identifier))?;

        let host_port = host_port.unwrap_or(crate::config::DEFAULT_PORT);
        let container_name = container.name.clone();
        let has_route = config.find_route(host_port).is_some();

        if has_route {
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.host_port == host_port)
                .unwrap();
            route.target = container_name.clone();
            self.config_manager.save(&config)?;
            println!("Switching route: {} -> {}", host_port, container_name);
        } else {
            config.routes.push(crate::config::Route {
                host_port,
                target: container_name.clone(),
            });
            config.routes.sort_by_key(|r| r.host_port);
            self.config_manager.save(&config)?;
            println!("Adding route: {} -> {}", host_port, container_name);
        }

        self.proxy_manager.reload_proxy().await?;

        Ok(())
    }
}
