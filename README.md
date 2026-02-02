# Proxy Manager

A Rust re-implementation of the proxy-manager CLI tool with an additional TUI (Terminal User Interface) for managing Nginx proxies to route multiple ports to different Docker containers.

## Features

- **CLI Interface**: Full-featured command-line interface matching the original Python implementation
- **TUI Mode**: Interactive terminal UI for visual management
- **Docker Integration**: Direct Docker API integration for container and network management
- **Nginx Configuration**: Automatic generation of Nginx proxy configurations
- **Multi-Network Support**: Connect proxy to multiple Docker networks
- **Auto-Detection**: Automatically detect container networks

## Installation

### From Source

```bash
# Clone the repository
git clone <repository-url>
cd proxy-manager

# Build release version
cargo build --release

# Install to ~/.local/bin
./target/release/proxy-manager install
```

## Usage

### CLI Mode

```bash
# Add containers
proxy-manager add my-app-v1 "Foo" -p 8000
proxy-manager add my-app-v2 "Bar" -p 8080

# Switch ports to containers (adds routes)
proxy-manager switch my-app-v1 8000
proxy-manager switch my-app-v2 8001

# Start the proxy
proxy-manager start

# View status
proxy-manager status

# Show logs
proxy-manager logs -f

# Stop the proxy
proxy-manager stop
```

### TUI Mode

```bash
# Launch interactive terminal UI
proxy-manager tui
```

The TUI provides:
- **Containers Tab**: View and manage configured containers (add/remove)
- **Routes Tab**: View and manage port routes
- **Status Tab**: View proxy status and active routes
- **Logs Tab**: View proxy logs with scrolling

### Available Commands

```
proxy-manager [COMMAND]

Commands:
  start       Start the proxy with all configured routes
  stop        Stop the proxy (or stop routing for specific port)
  restart     Stop and start the proxy
  reload      Apply config changes by rebuilding proxy
  list        List all configured containers with settings
  networks    List all Docker networks with container counts
  status      Show proxy status and all active routes
  install     Create hardlink in ~/.local/bin for global access
  config      Show config file path and contents
  logs        Show Nginx proxy container logs
  add         Add or update a container to config
  remove      Remove a container from the config
  switch      Route a host port to a container
  detect      List all Docker containers (optionally filtered)
  tui         Launch interactive TUI mode

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## Configuration

Configuration is stored in `~/.local/share/proxy-manager/proxy-config.json`:

```json
{
  "containers": [
    {
      "name": "my-app",
      "label": "My Application",
      "port": 8000,
      "network": "my-network"
    }
  ],
  "routes": [
    {
      "host_port": 8000,
      "target": "my-app"
    }
  ],
  "proxy_name": "proxy-manager",
  "network": "proxy-net"
}
```

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release
```

### Testing

```bash
# Run all tests
cargo test

# Run tests requiring Docker (integration tests)
cargo test -- --ignored
```

### Code Quality

```bash
# Format code
cargo fmt

# Run clippy
cargo clippy

# Run clippy with auto-fix
cargo clippy --fix
```

## Architecture

The project is organized into modules:

- **`src/cli/`**: Command-line interface implementation using `clap`
- **`src/config/`**: Configuration management (JSON serialization)
- **`src/docker/`**: Docker API client using `bollard`
- **`src/nginx/`**: Nginx configuration file generation
- **`src/tui/`**: Terminal UI using `ratatui`

## Dependencies

- **bollard**: Docker API client
- **clap**: Command-line argument parsing
- **ratatui**: Terminal UI framework
- **serde**: JSON serialization
- **tokio**: Async runtime
- **tar**: TAR archive creation for Docker builds

## License

MIT
