# Proxy Manager (Rust) Progress

Goal: Re-implement `proxy-manager.py` in Rust and add a TUI.

## Status

- Current iteration: 1
- Repo state: Rust crate initialized; implementation pending.

## Completed

- Initialize Rust crate skeleton (`cargo init`).
- Implement config model + JSON store + path helpers (unit-tested).
- Implement Nginx config generation (unit-tested).
- Implement Rust CLI surface (clap) matching Python commands.
- Implement Docker integration (build image, run container, networks, logs) via Docker API.
- Implement initial TUI (ratatui): status view + start/stop/reload + route switch.

## In Progress

- Validate parity vs `proxy-manager.py` and finalize UX.

## Todo (Next)

- Compare remaining Python behaviors (edge cases, output text) and align.
- Add integration-ish tests for App flows (start/stop/reload) with a FakeDocker.
- Add docs/readme for TUI usage + keybindings.
