---
type: index
project: remouseable
status: active
language: Rust
updated: 2026-06-11
---

# reMouseable Project Index

## Start Here

- [Repository README](../README.md)
- [Rust package manifest](../Cargo.toml)
- [[Projects/reMouseable AI Handoff|AI Handoff]]

## Implementation

- `src/main.rs`: CLI and application assembly
- `src/ui.rs`: Slint graphical frontend
- `src/ssh.rs`: tablet SSH event source
- `src/event.rs`: Evdev decoding
- `src/state.rs`: stylus state machine
- `src/scale.rs`: coordinate mapping
- `src/runtime.rs`: event dispatch
- `src/driver.rs`: native mouse backends
- `src/app.rs`: shared processing pipeline

## Verification

- `tests/representative_stream.rs`: captured-stream integration tests
- `fixtures/representative-events.hex`: deterministic Evdev fixture
- `.github/workflows/pr-workflow.yaml`: pull-request checks

## Current Focus

- Cross-platform Rust release builds.
- Linux Wayland compositor and multi-monitor validation.
- Secure SSH host-key defaults and clear first-use setup.
