# Proxy Manager

A Rust re-implementation of the Python proxy-manager CLI tool with an additional TUI interface for managing Nginx proxy containers that route multiple ports to different Docker app containers.

## Features

- **Full CLI support**: All commands from the original Python tool
- **Interactive TUI**: Navigate and manage your proxy setup with a terminal UI
- **Docker integration**: Seamlessly manages Docker containers and networks
- **Nginx proxy management**: Build, start, stop, and reload Nginx proxies
- **Route management**: Switch host ports to different containers dynamically
- **Container management**: Add, remove, list, and detect Docker containers
- **Log viewing**: View proxy logs with optional follow mode

## Installation

### From Source

```bash
cargo install --path .
```

This installs two binaries:
- `proxy-manager` - CLI tool
- `proxy-manager-tui` - Interactive TUI

## Usage

### CLI

```bash
# View all commands
proxy-manager --help

# Add a container
proxy-manager add my-app-v1 "Production" -p 8000

# Add another container
proxy-manager add my-app-v2 "Staging" -p 8080

# Route ports to containers
proxy-manager switch my-app-v1 8000
proxy-manager switch my-app-v2 8001

# Start the proxy
proxy-manager start

# View status
proxy-manager status

# View logs
proxy-manager logs -f

# Stop the proxy
proxy-manager stop
```

### TUI

```bash
# Launch the interactive interface
proxy-manager-tui
```

Use arrow keys (or j/k) to navigate, Enter to select, q/Esc to quit.

## Commands

### Container Management
- `add <name> [label] [-p PORT] [-n NETWORK]` - Add or update a container
- `remove <identifier>` - Remove a container from config
- `list` - List all configured containers
- `detect [filter]` - List all Docker containers (optionally filtered)
- `networks` - List all Docker networks with container counts

### Route Management
- `switch <container> [port]` - Route host port to container (default: 8000)
- `stop [port]` - Stop routing for a port (removes route)

### Proxy Operations
- `start` - Start proxy with all configured routes
- `stop [port]` - Stop proxy (or stop routing for specific port)
- `restart` - Stop and start the proxy
- `reload` - Apply config changes by rebuilding proxy
- `status` - Show proxy status and all active routes
- `logs [-f] [-n N]` - Show Nginx proxy container logs

### Other
- `install` - Create hardlink in ~/.local/bin for global access
- `config` - Show config file path and contents
- `tui` - Launch TUI interface

## Architecture

```
src/
├── lib.rs           # Library with shared modules
├── main.rs          # CLI entry point
├── tui.rs           # TUI entry point
├── config.rs        # Config file management (JSON)
├── docker.rs        # Docker client wrapper (bollard)
├── proxy.rs         # Proxy operations (start/stop/reload)
├── routes.rs        # Route management
├── containers.rs    # Container management
└── nginx.rs         # Nginx config generation
```

## Configuration

Configuration is stored in `~/.local/share/proxy-manager/proxy-config.json`:

```json
{
  "containers": [
    {
      "name": "my-app-v1",
      "label": "Production",
      "port": 8000,
      "network": "proxy-net"
    }
  ],
  "routes": [
    {
      "host_port": 8000,
      "target": "my-app-v1"
    }
  ],
  "proxy_name": "proxy-manager",
  "network": "proxy-net"
}
```

## Requirements

- Rust 1.70 or later
- Docker daemon running
- Docker network connectivity

## Testing

```bash
cargo test
```

## Development

```bash
# Format code
cargo fmt

# Check for warnings
cargo clippy

# Run tests
cargo test

# Build CLI
cargo build --release --bin proxy-manager

# Build TUI
cargo build --release --bin proxy-manager-tui
```

## License

MIT

## Original Python Implementation

This is a Rust re-implementation of the Python proxy-manager tool.
