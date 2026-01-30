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
}
