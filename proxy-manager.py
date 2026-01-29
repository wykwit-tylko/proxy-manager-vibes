#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.8"
# dependencies = [
#     "docker>=7.0.0",
# ]
# ///

import argparse
import json
import os
import time
from pathlib import Path

import docker
from docker.errors import NotFound

DEFAULT_PORT = 8000

CONFIG_DIR = Path.home() / ".local" / "share" / "proxy-manager"
CONFIG_FILE = CONFIG_DIR / "proxy-config.json"
BUILD_DIR = CONFIG_DIR / "build"

DEFAULT_CONFIG = {
    "containers": [],
    "routes": [],
    "proxy_name": "proxy-manager",
    "network": "proxy-net",
}

client = docker.from_env()


def get_proxy_name(config=None):
    if config is None:
        config = load_config()
    return config.get("proxy_name", DEFAULT_CONFIG["proxy_name"])


def get_proxy_image(config=None):
    if config is None:
        config = load_config()
    proxy_name = get_proxy_name(config)
    return f"{proxy_name}:latest"


def get_internal_port(target_container=None):
    if target_container:
        return target_container.get("port") or DEFAULT_PORT
    return DEFAULT_PORT


def get_all_host_ports(config=None):
    if config is None:
        config = load_config()
    if config["routes"]:
        return [r["host_port"] for r in config["routes"]]
    return [DEFAULT_PORT]


def get_network_name(config=None):
    if config is None:
        config = load_config()
    return config.get("network", DEFAULT_CONFIG["network"])


def ensure_network(network_name=None):
    if network_name is None:
        config = load_config()
        network_name = get_network_name(config)
    networks = [n.name for n in client.networks.list()]
    if network_name not in networks:
        print(f"Creating network: {network_name}")
        client.networks.create(network_name, driver="bridge")
    return network_name


def load_config():
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    if CONFIG_FILE.exists():
        with open(CONFIG_FILE, "r") as f:
            config = json.load(f)
        if not config.get("containers"):
            config["containers"] = []
        if not config.get("routes"):
            config["routes"] = []
        return config
    return DEFAULT_CONFIG.copy()


def save_config(config):
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    with open(CONFIG_FILE, "w") as f:
        json.dump(config, f, indent=2)


def detect_containers(filter_pattern=None):
    print("Detecting running containers...")
    containers = []
    for c in client.containers.list(all=True):
        name = c.name
        if filter_pattern:
            if filter_pattern.lower() in name.lower():
                containers.append(name)
        else:
            containers.append(name)
    return containers


def list_networks():
    print("Available Docker networks:")
    networks = client.networks.list()
    for net in networks:
        driver = net.attrs.get("Driver", "unknown")
        containers_count = len(net.attrs.get("Containers", {}))
        scope = net.attrs.get("Scope", "local")
        print(
            f"  {net.name:<25} driver={driver:<10} containers={containers_count:<4} scope={scope}"
        )


def get_container_network(container_name):
    try:
        container = client.containers.get(container_name)
        networks = container.attrs.get("NetworkSettings", {}).get("Networks", {})
        if networks:
            return list(networks.keys())[0]
    except (NotFound, Exception):
        pass
    return None


def find_container(config, identifier):
    return next(
        (
            c
            for c in config["containers"]
            if c["name"] == identifier or c.get("label") == identifier
        ),
        None,
    )


def find_route(config, host_port):
    return next(
        (r for r in config["routes"] if r["host_port"] == host_port),
        None,
    )


def add_container(container_name, label=None, port=None, network=None):
    config = load_config()
    existing = next(
        (c for c in config["containers"] if c["name"] == container_name), None
    )

    if network is None:
        detected_network = get_container_network(container_name)
        if detected_network:
            network = detected_network
            print(f"Auto-detected network: {network}")

    if existing:
        if label:
            existing["label"] = label
        if port:
            existing["port"] = port
        if network:
            existing["network"] = network
        save_config(config)
        print(f"Updated container: {container_name}")
    else:
        entry = {"name": container_name}
        if label:
            entry["label"] = label
        if port:
            entry["port"] = port
        if network:
            entry["network"] = network
        config["containers"].append(entry)
        save_config(config)
        print(f"Added container: {container_name}")


def remove_container(identifier):
    config = load_config()
    container = find_container(config, identifier)

    if not container:
        print(f"Error: Container '{identifier}' not found in config")
        return False

    config["containers"] = [
        c for c in config["containers"] if c["name"] != container["name"]
    ]
    config["routes"] = [r for r in config["routes"] if r["target"] != container["name"]]
    save_config(config)
    print(f"Removed container: {container['name']}")
    return True


def generate_nginx_config(config):
    routes = config["routes"]
    servers = []

    for route in routes:
        target = route["target"]
        target_container = next(
            (c for c in config["containers"] if c["name"] == target), None
        )
        if not target_container:
            continue
        internal_port = get_internal_port(target_container)
        host_port = route["host_port"]

        servers.append(
            f"    server {{\n"
            f"        listen {host_port};\n"
            f"\n"
            f"        set $backend_addr {target}:{internal_port};\n"
            f"        location / {{\n"
            f"            proxy_pass http://$backend_addr;\n"
            f"            proxy_set_header Host $host;\n"
            f"            proxy_set_header X-Real-IP $remote_addr;\n"
            f"            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n"
            f"            resolver 127.0.0.11 valid=30s;\n"
            f"            proxy_next_upstream error timeout http_502 http_503 http_504;\n"
            f"            proxy_intercept_errors on;\n"
            f"            error_page 502 503 504 =503 /fallback_{host_port};\n"
            f"        }}\n"
            f"\n"
            f"        location = /fallback_{host_port} {{\n"
            f"            default_type text/plain;\n"
            f"            return 503 'Service temporarily unavailable - container {target} is not running';\n"
            f"        }}\n"
            f"    }}\n"
        )

    servers_str = "\n".join(servers)

    return f"""events {{}}

http {{
    resolver 127.0.0.11 valid=30s;
{servers_str}
}}
 """


def build_proxy():
    config = load_config()

    if not config["containers"]:
        print("Error: No containers configured. Use 'add' command first.")
        return False

    BUILD_DIR.mkdir(parents=True, exist_ok=True)

    nginx_conf = generate_nginx_config(config)

    with open(BUILD_DIR / "nginx.conf", "w") as f:
        f.write(nginx_conf)

    host_ports = get_all_host_ports(config)
    dockerfile = f"""FROM nginx:stable-alpine
 COPY nginx.conf /etc/nginx/nginx.conf
 EXPOSE {" ".join(str(p) for p in host_ports)}
 CMD ["nginx", "-g", "daemon off;"]
 """

    with open(BUILD_DIR / "Dockerfile", "w") as f:
        f.write(dockerfile)

    print("Building proxy image...")
    proxy_image = get_proxy_image(config)
    try:
        client.images.build(path=str(BUILD_DIR), tag=proxy_image, rm=True)
        return True
    except Exception as e:
        print(f"Build failed: {e}")
        return False


def start_proxy():
    config = load_config()
    proxy_name = get_proxy_name(config)
    proxy_image = get_proxy_image(config)
    default_network = get_network_name(config)

    if not config["containers"]:
        print("Error: No containers configured. Use 'add' command first.")
        return False

    if not config["routes"]:
        print("Error: No routes configured. Use 'switch' command first.")
        return False

    networks = set()
    networks.add(default_network)
    for c in config["containers"]:
        if c.get("network"):
            networks.add(c["network"])

    for network in networks:
        ensure_network(network)

    try:
        _ = client.containers.get(proxy_name)
        print(f"Proxy already running: {proxy_name}")
        return True
    except NotFound:
        pass

    if not build_proxy():
        return False

    host_ports = get_all_host_ports(config)
    ports_mapping = {f"{port}/tcp": port for port in host_ports}

    print(f"Starting proxy: {proxy_name}")
    client.containers.run(
        proxy_image,
        name=proxy_name,
        detach=True,
        network=default_network,
        ports=ports_mapping,
    )

    container = client.containers.get(proxy_name)
    for network in networks:
        if network != default_network:
            try:
                net = client.networks.get(network)
                net.connect(container)
                print(f"Connected proxy to network: {network}")
            except Exception as e:
                print(f"Warning: Could not connect to network {network}: {e}")

    port_str = ", ".join(str(p) for p in host_ports)
    print(f"Proxy started on port(s): {port_str}")
    return True


def stop_proxy():
    proxy_name = get_proxy_name()
    try:
        container = client.containers.get(proxy_name)
        print(f"Stopping proxy: {proxy_name}")
        container.stop()
        container.remove()
        print("Proxy stopped")
        return True
    except NotFound:
        print("Proxy not running")
        return False


def stop_port(host_port):
    config = load_config()
    route = find_route(config, host_port)

    if not route:
        print(f"Error: No route found for port {host_port}")
        return False

    config["routes"] = [r for r in config["routes"] if r["host_port"] != host_port]
    save_config(config)
    print(f"Removed route: port {host_port}")

    if config["routes"]:
        return reload_proxy()
    else:
        return stop_proxy()


def reload_proxy():
    config = load_config()

    if not config["containers"]:
        print("Error: No containers configured.")
        return False

    if not config["routes"]:
        print("Error: No routes configured.")
        return False

    print("Reloading proxy...")
    stop_proxy()
    time.sleep(1)
    return start_proxy()


def switch_target(identifier, host_port=None):
    config = load_config()
    container = find_container(config, identifier)

    if not container:
        print(f"Error: Container '{identifier}' not found in config")
        return False

    if host_port is None:
        host_port = DEFAULT_PORT

    existing_route = find_route(config, host_port)
    if existing_route:
        existing_route["target"] = container["name"]
        save_config(config)
        print(f"Switching route: {host_port} -> {container['name']}")
    else:
        config["routes"].append({"host_port": host_port, "target": container["name"]})
        config["routes"].sort(key=lambda r: r["host_port"])
        save_config(config)
        print(f"Adding route: {host_port} -> {container['name']}")
    return reload_proxy()


def list_containers():
    config = load_config()
    route_map = {r["target"]: r["host_port"] for r in config["routes"]}

    if not config["containers"]:
        print("No containers configured")
        return

    print("Configured containers:")
    for c in config["containers"]:
        host_port = route_map.get(c["name"])
        if host_port:
            marker = f" (port {host_port})"
        else:
            marker = ""
        label = f" - {c['label']}" if c.get("label") else ""
        port = c.get("port") or DEFAULT_PORT
        net = c.get("network") or config.get("network", "proxy-net")
        print(f"  {c['name']}:{port}@{net}{label}{marker}")


def status():
    proxy_name = get_proxy_name()
    try:
        container = client.containers.get(proxy_name)
        config = load_config()
        status = container.status
        print(f"Proxy: {proxy_name} ({status})")
        print("")
        print("Active routes:")
        for route in config["routes"]:
            host_port = route["host_port"]
            target = route["target"]
            target_container = next(
                (c for c in config["containers"] if c["name"] == target), None
            )
            if target_container:
                internal_port = get_internal_port(target_container)
                print(f"  {host_port} -> {target}:{internal_port}")
            else:
                print(f"  {host_port} -> {target} (container not found)")
    except NotFound:
        print("Proxy not running")


def show_config():
    config = load_config()
    print(f"Config file: {CONFIG_FILE}")
    print("")
    print(json.dumps(config, indent=2))


def show_logs(follow=False, tail=100):
    proxy_name = get_proxy_name()
    try:
        container = client.containers.get(proxy_name)
        print(f"Logs for: {proxy_name}")
        print("-" * 50)
        logs = container.logs(tail=tail, follow=follow, stream=True)
        for line in logs:
            print(line.decode("utf-8").rstrip())
    except NotFound:
        print(f"Proxy container '{proxy_name}' not running")


def install_cli():
    script_path = Path(__file__).absolute()
    user_bin = Path.home() / ".local" / "bin"
    hardlink = user_bin / "proxy-manager"

    user_bin.mkdir(parents=True, exist_ok=True)

    if hardlink.exists() or hardlink.is_symlink():
        hardlink.unlink()

    os.link(script_path, hardlink)
    print(f"Created hardlink: {hardlink} -> {script_path}")
    print("")
    print("See 'proxy-manager --help' for a quick start guide.")

    if str(user_bin) not in os.environ.get("PATH", ""):
        print("NOTE: Add ~/.local/bin to your PATH:")
        print(f'  export PATH="{user_bin}:$PATH"')
        print("  # Add to ~/.bashrc or ~/.zshrc to persist")


def main():
    parser = argparse.ArgumentParser(
        description="Manage Nginx proxy to route multiple ports to different docker app containers.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
----------------------------------------------------------------

Quick Start:
  # 1. Add containers
  proxy-manager add my-app-v1 "Foo" -p 8000
  proxy-manager add my-app-v2 "Bar" -p 8080

  # 2. Switch ports to containers (adds routes)
  proxy-manager switch my-app-v1 8000
  proxy-manager switch my-app-v2 8001

  # 3. Start the proxy (routes multiple ports)
  proxy-manager start

  # 4. View status
  proxy-manager status

Container Management:
  proxy-manager add <name> [label]            # Add container (auto-detects network)
  proxy-manager add <name> -p 8080            # Specify custom port
  proxy-manager add <name> -n custom-net      # Specify custom network
  proxy-manager list                          # Show all configured containers
  proxy-manager remove <name|label>           # Remove container from config (by name or label)

Route Management:
  proxy-manager switch <container> [port]      # Route host port to container (default: 8000)
  proxy-manager stop [port]                    # Stop routing for a port (removes route)

Proxy Operations:
  proxy-manager start                         # Start proxy with all configured routes
  proxy-manager stop [port]                   # Stop proxy (or stop routing for specific port)
  proxy-manager restart                       # Restart proxy
  proxy-manager reload                        # Apply config changes
  proxy-manager status                        # Show current status and all active routes

Logging:
  proxy-manager logs                          # Show proxy logs
  proxy-manager logs -f                       # Follow logs (tail -f mode)
  proxy-manager logs -n 50                    # Show last 50 lines

Discovery:
  proxy-manager detect                        # List all Docker containers
  proxy-manager detect [name]                 # Filter containers by name
  proxy-manager networks                      # List all Docker networks

Configuration:
  proxy-manager config                        # View config file and contents
  # Edit: ~/.local/share/proxy-manager/proxy-config.json

Installation:
  proxy-manager install                       # Create hardlink in ~/.local/bin
        """,
    )

    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    subparsers.add_parser("start", help="Start the proxy with all configured routes")
    stop_parser = subparsers.add_parser(
        "stop", help="Stop the proxy (or stop routing for specific port)"
    )
    stop_parser.add_argument(
        "port", nargs="?", type=int, help="Optional: Stop routing for specific port"
    )
    subparsers.add_parser("restart", help="Stop and start the proxy")
    subparsers.add_parser("reload", help="Apply config changes by rebuilding proxy")
    subparsers.add_parser("list", help="List all configured containers with settings")
    subparsers.add_parser(
        "networks", help="List all Docker networks with container counts"
    )
    subparsers.add_parser("status", help="Show proxy status and all active routes")
    subparsers.add_parser(
        "install", help="Create hardlink in ~/.local/bin for global access"
    )
    subparsers.add_parser("config", help="Show config file path and contents")

    logs_parser = subparsers.add_parser("logs", help="Show Nginx proxy container logs")
    logs_parser.add_argument(
        "-f", "--follow", action="store_true", help="Follow log output (like tail -f)"
    )
    logs_parser.add_argument(
        "-n",
        "--tail",
        type=int,
        default=100,
        help="Number of lines to show (default: 100)",
    )

    add_parser = subparsers.add_parser(
        "add", help="Add or update a container to config"
    )
    add_parser.add_argument("container", help="Docker container name")
    add_parser.add_argument("label", nargs="?", help="Optional display label")
    add_parser.add_argument(
        "-p",
        "--port",
        type=int,
        help="Port the container exposes (default: 8000)",
    )
    add_parser.add_argument(
        "-n",
        "--network",
        help="Network the container is on (default: auto-detects from container or uses config's network)",
    )

    remove_parser = subparsers.add_parser(
        "remove", help="Remove a container from the config"
    )
    remove_parser.add_argument("identifier", help="Container name or label to remove")

    switch_parser = subparsers.add_parser(
        "switch", help="Route a host port to a container"
    )
    switch_parser.add_argument("identifier", help="Container name or label to route to")
    switch_parser.add_argument(
        "port",
        nargs="?",
        type=int,
        help="Host port to route (default: 8000)",
    )

    detect_parser = subparsers.add_parser(
        "detect", help="List all Docker containers (optionally filtered)"
    )
    detect_parser.add_argument(
        "filter", nargs="?", help="Filter results by name pattern (case-insensitive)"
    )

    args = parser.parse_args()

    if not args.command:
        parser.print_help()
        return

    if args.command == "start":
        start_proxy()
    elif args.command == "stop":
        if args.port:
            stop_port(args.port)
        else:
            stop_proxy()
    elif args.command == "restart":
        stop_proxy()
        time.sleep(1)
        start_proxy()
    elif args.command == "reload":
        reload_proxy()
    elif args.command == "list":
        list_containers()
    elif args.command == "status":
        status()
    elif args.command == "add":
        add_container(args.container, args.label, args.port, args.network)
    elif args.command == "networks":
        list_networks()
    elif args.command == "remove":
        remove_container(args.identifier)
    elif args.command == "switch":
        switch_target(args.identifier, args.port)
    elif args.command == "detect":
        containers = detect_containers(args.filter)
        print("Running containers:")
        for c in containers:
            print(f"  {c}")
    elif args.command == "install":
        install_cli()
    elif args.command == "config":
        show_config()
    elif args.command == "logs":
        show_logs(follow=args.follow, tail=args.tail)


if __name__ == "__main__":
    main()
