# Progress

## Completed
- Bootstrapped Rust project (`cargo`) and moved implementation to modular architecture.
- Re-implemented config model and persistence from Python version:
  - default values
  - JSON load/save in `~/.local/share/proxy-manager/proxy-config.json`
  - container/route lookup helpers
- Implemented Docker integration layer using Docker CLI wrapper:
  - detect containers
  - inspect container network
  - list/create networks
  - build image, run/stop/remove proxy container
  - inspect status and stream logs
- Implemented core proxy manager behavior:
  - add/remove containers
  - switch routes and stop route by port
  - build/start/stop/reload/restart
  - status/list/config output
  - install hardlink in `~/.local/bin/proxy-manager`
- Re-implemented nginx + Dockerfile generation.
- Added full Rust CLI command surface matching original commands plus `tui` command.
- Added TUI implementation (Ratatui + Crossterm) with refresh/start/stop/reload controls.
- Added unit tests for config, Docker parsing helpers, manager logic, nginx/dockerfile generation, and TUI state navigation.

## Verification
- `cargo test` passing.
- `cargo clippy -- -D warnings` passing.
- `cargo fmt` completed.

## Remaining
- None.
