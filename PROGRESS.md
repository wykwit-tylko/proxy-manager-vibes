# Proxy Manager (Rust) Progress

Goal: Re-implement `proxy-manager.py` in Rust and add a TUI.

## Status

- Current iteration: 3
- Repo state: Rust CLI + Docker backend + TUI implemented; fmt/tests/clippy clean.

## Completed

- Initialize Rust crate skeleton (`cargo init`).
- Implement config model + JSON store + path helpers (unit-tested).
- Implement Nginx config generation (unit-tested).
- Implement Rust CLI surface (clap) matching Python commands.
- Implement Docker integration (build image, run container, networks, logs) via Docker API.
- Implement initial TUI (ratatui): status view + start/stop/reload + route switch.
- Add unit tests for App flow behavior via FakeDocker.
- Add README covering CLI + TUI usage.

## In Progress

- (none)

## Todo (Next)

- (none)
