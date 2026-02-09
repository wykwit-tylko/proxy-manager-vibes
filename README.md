# Proxy Manager (Rust)

A complete Rust re-implementation of the Python proxy-manager.py CLI tool with an added interactive TUI (Text User Interface).

## Features

- **Docker Integration**: Manage Docker containers and networks
- **Nginx Proxy**: Dynamic proxy configuration and management
- **CLI Interface**: Full-featured command-line interface
- **Interactive TUI**: Terminal user interface for easy management
- **Port Routing**: Route multiple host ports to different containers
- **Network Management**: Auto-detect and manage Docker networks
- **Real-time Logs**: Stream and follow container logs
- **Configuration**: JSON-based persistent configuration

## Installation

### Build from source

```bash
cargo build --release
```

The binary will be available at `target/release/proxy-manager`.

### Install locally

```bash
cargo install --path .
```

## Quick Start

### Using CLI

```bash
# 1. Add containers
proxy-manager add my-app-v1 "Version 1" -p 8000
proxy-manager add my-app-v2 "Version 2" -p 8080

# 2. Configure routes (map host ports to containers)
proxy-manager switch my-app-v1 8000
proxy-manager switch my-app-v2 8001

# 3. Start the proxy
proxy-manager start

# 4. Check status
proxy-manager status

# 5. View logs
proxy-manager logs -f
```

### Using TUI

```bash
# Launch interactive interface
proxy-manager tui
```

The TUI provides:
- Tab navigation (Tab/Shift+Tab)
- Container list view
- Route management view
- Status overview
- Keyboard shortcuts for common operations

## Commands

### Container Management

```bash
proxy-manager add <name> [label] [-p PORT] [-n NETWORK]
proxy-manager remove <name|label>
proxy-manager list
```

### Route Management

```bash
proxy-manager switch <container> [PORT]
proxy-manager stop [PORT]
```

### Proxy Operations

```bash
proxy-manager start     # Start proxy with all routes
proxy-manager stop      # Stop proxy completely
proxy-manager restart   # Restart proxy
proxy-manager reload    # Apply config changes
proxy-manager status    # Show status and routes
```

### Discovery

```bash
proxy-manager detect [FILTER]    # List Docker containers
proxy-manager networks            # List Docker networks
```

### Logging

```bash
proxy-manager logs              # Show last 100 lines
proxy-manager logs -f           # Follow logs
proxy-manager logs -n 50        # Show last 50 lines
```

### Configuration

```bash
proxy-manager config    # View configuration file
```

## Architecture

### Modules

- **config**: Configuration management with JSON serialization
- **docker**: Docker API client wrapper (using bollard)
- **nginx**: Nginx configuration and Dockerfile generation
- **commands**: CLI command implementations
- **tui**: Interactive terminal user interface (using ratatui)
- **main**: CLI parser and command dispatcher

### Dependencies

- `clap`: Command-line argument parsing
- `serde` + `serde_json`: Serialization/deserialization
- `bollard`: Docker Engine API client
- `tokio`: Async runtime
- `anyhow`: Error handling
- `ratatui` + `crossterm`: Terminal UI framework
- `directories`: Cross-platform directory paths
- `tar`: Archive creation for Docker builds
- `futures-util`: Async stream utilities

## Configuration

Configuration is stored in JSON format at:
- Linux: `~/.local/share/proxy-manager/proxy-config.json`
- macOS: `~/Library/Application Support/proxy-manager/proxy-config.json`
- Windows: `%APPDATA%\proxy-manager\proxy-config.json`

### Configuration Structure

```json
{
  "containers": [
    {
      "name": "my-app",
      "label": "My Application",
      "port": 8080,
      "network": "proxy-net"
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

## How It Works

1. **Add Containers**: Register Docker containers with their exposed ports
2. **Configure Routes**: Map host ports to container targets
3. **Generate Config**: Creates Nginx configuration for routing
4. **Build Image**: Builds a custom Nginx Docker image
5. **Start Proxy**: Launches the proxy container with port mappings
6. **Route Traffic**: Nginx routes requests to target containers

The proxy handles:
- Dynamic DNS resolution for container names
- Graceful error handling when containers are down
- Multiple network support
- Automatic failover pages

## Improvements over Python Version

1. **Type Safety**: Rust's type system prevents runtime errors
2. **Performance**: Compiled binary is significantly faster
3. **Memory Safety**: No memory leaks or undefined behavior
4. **Better Errors**: Detailed error messages with context
5. **Async Operations**: Non-blocking Docker operations
6. **Interactive TUI**: New feature for easier management
7. **Cross-platform**: Single binary works everywhere
8. **Testing**: Comprehensive unit tests

## Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Check code quality
cargo clippy

# Format code
cargo fmt
```

All tests pass with no warnings.

## Development

### Project Structure

```
proxy-manager/
├── Cargo.toml
├── src/
│   ├── main.rs       # CLI entry point
│   ├── config.rs     # Configuration management
│   ├── docker.rs     # Docker client
│   ├── nginx.rs      # Nginx config generation
│   ├── commands.rs   # Command implementations
│   └── tui.rs        # Terminal UI
├── PROGRESS.md       # Implementation progress
└── README.md         # This file
```

### Adding New Features

1. Add function to appropriate module
2. Write unit tests
3. Update commands.rs if needed
4. Update CLI in main.rs
5. Run `cargo test && cargo clippy`
6. Format with `cargo fmt`

## License

This is a re-implementation for educational and practical purposes.

## Comparison with Python Version

| Feature | Python | Rust |
|---------|--------|------|
| Container management | ✓ | ✓ |
| Route management | ✓ | ✓ |
| Proxy operations | ✓ | ✓ |
| Network detection | ✓ | ✓ |
| Log streaming | ✓ | ✓ |
| Interactive TUI | ✗ | ✓ |
| Type safety | ✗ | ✓ |
| Compile-time checks | ✗ | ✓ |
| Memory safety | Runtime | Compile-time |
| Performance | Good | Excellent |
| Binary size | N/A | ~5MB |
| Startup time | ~100ms | ~10ms |

## Contributing

Contributions are welcome! Please ensure:
- All tests pass (`cargo test`)
- No clippy warnings (`cargo clippy`)
- Code is formatted (`cargo fmt`)
- New features have tests

## Troubleshooting

### Docker connection failed
Ensure Docker daemon is running:
```bash
docker ps
```

### Permission denied
Add your user to the docker group:
```bash
sudo usermod -aG docker $USER
```

### Network not found
The proxy will auto-create networks, but ensure Docker networking is enabled.

### Container not found
Check container names with:
```bash
proxy-manager detect
```
