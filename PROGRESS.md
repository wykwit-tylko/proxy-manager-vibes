# Proxy Manager (Rust) Progress

Goal: Re-implement `proxy-manager.py` in Rust and add a TUI.

## Status

- Current iteration: 1
- Repo state: Rust crate initialized; implementation pending.

## Completed

- Initialize Rust crate skeleton (`cargo init`).
- Implement config model + JSON store + path helpers (unit-tested).
- Implement Nginx config generation (unit-tested).

## In Progress

- Implement CLI surface (clap) matching Python commands.

## Todo (Next)

- Implement Docker integration (build image, run container, connect networks, logs) via Docker API.
- Implement TUI (ratatui) for status + quick actions.
- Add tests (unit tests for pure logic; mock Docker for command logic).
- Add CI-quality checks in local flow: `cargo fmt`, `cargo test`, `cargo clippy`.
