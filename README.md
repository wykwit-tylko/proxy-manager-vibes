# Proxy Manager

A Rust re-implementation of the proxy-manager CLI tool with an interactive TUI (Terminal User Interface).

## Features

- **CLI Interface**: Manage proxy routes via command line
- **TUI Interface**: Interactive terminal UI for visual route/container management
- **Docker Integration**: Automatically discovers containers and their networks
- **Nginx Configuration**: Generates and manages Nginx reverse proxy configurations
- **Configuration Management**: YAML-based configuration with hot-reloading

## Installation

```bash
cargo build --release
```

## Usage

### CLI Mode

```bash
# Add a new route
proxy-manager add <domain> <container> <port>

# Remove a route
proxy-manager remove <domain>

# List all routes
proxy-manager list

# Validate configuration
proxy-manager validate
```

### TUI Mode

```bash
proxy-manager tui
```

Navigate with arrow keys:
- `Tab` / `Shift+Tab` - Switch between tabs
- `↑` / `↓` - Navigate items
- `r` - Refresh configuration
- `q` / `Esc` - Quit
- `h` / `?` - Toggle help

## Configuration

Configuration is stored in `~/.config/proxy-manager/config.yaml`:

```yaml
nginx_config_path: /etc/nginx/conf.d
routes:
  - domain: example.com
    container: myapp
    port: 8080
```

## Development

```bash
# Run tests
cargo test

# Check formatting
cargo fmt --check

# Run clippy
cargo clippy
```

## License

MIT
