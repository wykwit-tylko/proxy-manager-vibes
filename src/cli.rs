use crate::ops::{RouteStatus, StatusInfo};

pub fn format_ports(ports: &[u16]) -> String {
    ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn format_status(status: &StatusInfo) -> String {
    if status.status.is_none() {
        return "Proxy not running".to_string();
    }

    let mut lines = Vec::new();
    let state = status.status.as_deref().unwrap_or("unknown");
    lines.push(format!("Proxy: {} ({})", status.proxy_name, state));
    lines.push("".to_string());
    lines.push("Active routes:".to_string());
    for route in &status.routes {
        lines.push(format_route(route));
    }
    lines.join("\n")
}

pub fn long_help() -> &'static str {
    "Quick Start:\n  proxy-manager add my-app-v1 Foo -p 8000\n  proxy-manager add my-app-v2 Bar -p 8080\n  proxy-manager switch my-app-v1 8000\n  proxy-manager switch my-app-v2 8001\n  proxy-manager start\n  proxy-manager status\n\nContainer Management:\n  proxy-manager add <name> [label]\n  proxy-manager add <name> -p 8080\n  proxy-manager add <name> -n custom-net\n  proxy-manager list\n  proxy-manager remove <name|label>\n\nRoute Management:\n  proxy-manager switch <container> [port]\n  proxy-manager stop [port]\n\nProxy Operations:\n  proxy-manager start\n  proxy-manager stop [port]\n  proxy-manager restart\n  proxy-manager reload\n  proxy-manager status\n\nLogging:\n  proxy-manager logs\n  proxy-manager logs -f\n  proxy-manager logs -n 50\n\nDiscovery:\n  proxy-manager detect [name]\n  proxy-manager networks\n\nConfiguration:\n  proxy-manager config\n\nInstallation:\n  proxy-manager install\n  "
}

fn format_route(route: &RouteStatus) -> String {
    if route.missing {
        format!(
            "  {} -> {} (container not found)",
            route.host_port, route.target
        )
    } else {
        format!(
            "  {} -> {}:{}",
            route.host_port, route.target, route.internal_port
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::StatusInfo;

    #[test]
    fn format_ports_joined() {
        assert_eq!(format_ports(&[8000, 8001]), "8000, 8001");
    }

    #[test]
    fn format_status_when_missing() {
        let status = StatusInfo {
            proxy_name: "proxy".to_string(),
            status: None,
            routes: vec![],
        };
        assert_eq!(format_status(&status), "Proxy not running");
    }

    #[test]
    fn long_help_contains_quick_start() {
        let help = long_help();
        assert!(help.contains("Quick Start"));
        assert!(help.contains("proxy-manager start"));
    }
}
