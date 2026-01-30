# Proxy Manager

A Rust-based tool for managing Nginx proxy configurations to route multiple ports to different Docker app containers, featuring both CLI and TUI interfaces.

## Features

- **Multi-port routing**: Route incoming traffic to different Docker containers
- **CLI Interface**: Full command-line interface for all operations
- **TUI Interface**: Interactive terminal user interface with real-time updates
- **Docker Integration**: Direct integration with Docker daemon using bollard
- **Configuration Management**: JSON-based configuration with easy add/remove operations
- **Container Detection**: Automatically detect running Docker containers
- **Network Management**: Create and manage Docker networks for proxy routing

## Installation

```bash
cargo build --release
```

The binary will be available at `target/release/proxy-manager`.

For global access, install using:

```bash
proxy-manager install
```

## Usage

### CLI Commands

```bash
# Start the proxy
proxy-manager start

# Stop the proxy (optionally for specific port)
proxy-manager stop [--port PORT]

# Restart the proxy
proxy-manager restart

# Reload and rebuild proxy configuration
proxy-manager reload

# List configured containers
proxy-manager list

# List Docker networks
proxy-manager networks

# Show proxy status
proxy-manager status

# Show configuration
proxy-manager config

# Add a container to configuration
proxy-manager add <name> <container_id> <port>

# Remove a container from configuration
proxy-manager remove <name>

# Switch routing to a different container
proxy-manager switch <port>

# Detect running containers
proxy-manager detect

# Show container logs
proxy-manager logs [-f] [--tail N]

# Launch TUI interface
proxy-manager tui
```

### TUI Interface

Launch the interactive terminal interface:

```bash
proxy-manager tui
```

Features:
- Real-time status updates (5-second auto-refresh)
- Container listing with status
- Route management
- Keyboard navigation

## Configuration

Configuration is stored in `~/.config/proxy-manager/config.json`:

```json
{
  "containers": [
    {
      "name": "myapp",
      "container_id": "abc123...",
      "port": 8080,
      "enabled": true
    }
  ],
  "proxy_port": 80
}
```

## Architecture

- **cli.rs**: Command-line interface using clap
- **config.rs**: Configuration management with JSON serialization
- **docker.rs**: Docker client integration using bollard
- **proxy.rs**: Nginx configuration generation
- **tui.rs**: Terminal UI using ratatui

## Dependencies

- Rust 2021 Edition
- Docker daemon
- Nginx (for proxy functionality)

## Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Format code
cargo fmt

# Check for warnings
cargo clippy
```

## License

Proxy Manager Team
