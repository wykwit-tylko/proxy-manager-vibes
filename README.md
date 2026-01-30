# proxy-manager (Rust)

Re-implementation of `proxy-manager.py` in Rust.

It builds an Nginx container image from your configured routes, runs it as `proxy-manager`, and connects it to the networks your target containers are on.

## Install

Build and install a hardlink into `~/.local/bin`:

```sh
cargo build --release
./target/release/proxy-manager install
```

## Quick Start (CLI)

```sh
# 1) Add containers (name, optional label, optional port/network)
proxy-manager add my-app-v1 Foo -p 8000
proxy-manager add my-app-v2 Bar -p 8080

# 2) Route host ports to containers
proxy-manager switch my-app-v1 8000
proxy-manager switch my-app-v2 8001

# 3) Start the proxy
proxy-manager start

# 4) Check status
proxy-manager status
```

## TUI

Run:

```sh
proxy-manager tui
```

Keys:

- `j`/`k` or arrow keys: move selection
- `enter`: switch `:8000` to selected container
- `s`: start proxy
- `t`: stop proxy
- `r`: reload proxy
- `u`: refresh
- `q` or `Ctrl-C`: quit

## Configuration

Config is stored at:

- `~/.local/share/proxy-manager/proxy-config.json`

Show current config:

```sh
proxy-manager config
```
