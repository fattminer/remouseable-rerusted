---
title: reMouseable Rust Migration
aliases:
  - reMouseable Rust Rewrite
tags:
  - project/remouseable
  - rust
  - migration
  - roadmap
status: proposed
language: Rust
repository: C:\Users\mfiner\GIT\remouseable
updated: 2026-06-04
priority: high
---

# reMouseable Rust Migration

> [!success] Feasibility
> Rust conversion is feasible. Core application is small, modular, and well tested. Main risk is cross-platform mouse injection behavior, not event-processing logic.

## Why Convert

- Remove large vendored RobotGo C/CGO surface.
- Improve type safety by replacing string event types and runtime type assertions with enums.
- Simplify cross-platform build tooling.
- Gain mature Rust options for Evdev/uinput and future Linux virtual tablet support.
- Improve error handling and security while preserving behavior.

Rust rewrite is not expected to materially improve runtime performance. Primary value is maintainability, safety, packaging, and future feature work.

## Scope Assessment

Inspected project contains approximately:

| Scope | Size |
|---|---:|
| Production Go excluding generated codes and RobotGo | About 650 lines |
| Vendored C/header files | 52 files, about 260 KB |
| Go files including tests/generated files | 22 files |

Likely reliable parity effort: **2–4 weeks**, including real-device and cross-platform validation.

## Proposed Rust Dependencies

| Need | Candidate |
|---|---|
| CLI | `clap` |
| Async runtime | `tokio` |
| SSH | `russh` |
| Password prompt | `rpassword` |
| Mouse injection | `enigo` initially |
| Display enumeration | `display-info` |
| Errors | `thiserror`, optionally `anyhow` at CLI boundary |
| Logging | `tracing`, `tracing-subscriber` |
| Tests | Built-in Rust tests with small fake trait implementations |

Do not use host-native `evdev::InputEvent` to parse remote byte stream. Keep explicit 16-byte little-endian decoder.

## Proposed Module Layout

```text
Cargo.toml
src/
  main.rs
  cli.rs
  error.rs
  event/
    mod.rs
    raw.rs
    source.rs
  state.rs
  scale.rs
  runtime.rs
  driver/
    mod.rs
    enigo.rs
  ssh.rs
tests/
  captured_stream.rs
fixtures/
  remarkable-events.bin
```

## Proposed Domain Model

```rust
enum StateChange {
    Move { x: i32, y: i32 },
    Drag { x: i32, y: i32 },
    Press(MouseButton),
    Release(MouseButton),
}

trait PositionScaler {
    fn scale(&self, x: i32, y: i32) -> (i32, i32);
}

trait HostDriver {
    fn screen_size(&self) -> Result<(i32, i32), DriverError>;
    fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), DriverError>;
    fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), DriverError>;
    fn press(&mut self, button: MouseButton) -> Result<(), DriverError>;
    fn release(&mut self, button: MouseButton) -> Result<(), DriverError>;
}
```

Prefer enums over string discriminators. Keep driver and source boundaries abstract for tests.

## Phased Plan

### Phase 0: Baseline and Fixtures

- [ ] Install Go locally or rely on CI.
- [ ] Confirm existing Go tests pass.
- [ ] Capture representative tablet event streams.
- [ ] Add Go regression test using captured stream.
- [ ] Record expected state changes and scaled coordinates.

Exit condition: deterministic compatibility fixture exists.

### Phase 1: Pure Rust Core

- [ ] Initialize Cargo project beside Go implementation.
- [ ] Implement explicit 16-byte event decoder using `read_exact`.
- [ ] Implement event filtering.
- [ ] Implement state machine.
- [ ] Implement three position scalers.
- [ ] Implement runtime dispatch using fake driver.
- [ ] Port relevant Go unit tests.

Exit condition: Rust core produces same output as Go fixture tests.

### Phase 2: CLI and Event Sources

- [ ] Implement `clap` CLI with compatible flags.
- [ ] Add local/static-file event source for debugging and tests.
- [ ] Implement structured errors and exit codes.
- [ ] Implement debug event output.

Exit condition: application runs end-to-end from recorded event stream.

### Phase 3: SSH

- [ ] Implement password authentication.
- [ ] Implement SSH agent authentication.
- [ ] Validate legacy RSA algorithm compatibility against real tablet.
- [ ] Secure host-key handling.
- [ ] Validate or safely encode event path.
- [ ] Stream command output without buffering full stream.

Exit condition: Rust app receives live tablet events.

### Phase 4: Host Driver

- [ ] Implement initial `enigo` driver.
- [ ] Test move, press, release, and drag on Windows.
- [ ] Test on macOS, paying special attention to explicit drag events.
- [ ] Test on Linux X11.
- [ ] Replace `enigo` with platform-specific thin drivers only where behavior requires it.
- [ ] Add monitor selection using `display-info`.

Exit condition: real-device parity on all supported platforms.

### Phase 5: Packaging and Cutover

- [ ] Add Rust GitHub Actions build/test jobs.
- [ ] Produce release binaries for all existing targets.
- [ ] Document permissions and migration behavior.
- [ ] Run captured-stream and real-device acceptance tests.
- [ ] Keep Go release available during transition.
- [ ] Remove Go/CGO implementation only after stable Rust release.

## Key Risks

### macOS Drag Semantics

Current native code emits `kCGEventLeftMouseDragged`, not only movement while button is down. Confirm `enigo` behavior. If incompatible, implement macOS driver using `core-graphics`.

### Legacy SSH Compatibility

Tablet firmware may require older RSA behavior. `russh` supports `ssh-rsa`, but configuration must be tested on actual devices.

### Agent Authentication

Unix agent behavior should be viable. Windows agent behavior requires explicit testing and may retain current limitation.

### Wayland

Do not make Wayland part of initial parity promise. Enigo Wayland/libei support is experimental. Track as later feature.

### Dependency Risk

Input-injection and SSH crates are security-sensitive dependencies. Pin versions, review release changes, and use `cargo audit`.

## Improvements to Include

- Use `read_exact` for events.
- Secure SSH host verification by default.
- Eliminate command injection through `--event-file`.
- Return useful errors instead of panics.
- Support monitor selection and offsets.
- Allow more mouse buttons through generic press/release domain events.
- Add local event-file source.
- Add captured-stream integration tests.
- Keep debug output stable or explicitly version it.

## Decision Log

| Date | Decision | Reason |
|---|---|---|
| 2026-06-04 | Rust conversion assessed as feasible | Core is small and modular; Rust ecosystem covers required capabilities |
| 2026-06-04 | Preserve explicit remote event decoder | Tablet event ABI uses 32-bit timestamps and differs from possible host ABI |
| 2026-06-04 | Start with Enigo, allow native fallback | Fastest route to parity; macOS drag behavior may require native API |
| 2026-06-04 | Run Go and Rust side by side during migration | Enables fixture comparison and safe rollback |

## Agent Handoff Prompt

Use this when starting implementation:

> Read `obsidian-vault/Projects/reMouseable AI Handoff.md` and `obsidian-vault/Projects/reMouseable Rust Migration.md`. Inspect current repository state before editing. Implement next incomplete migration phase while preserving existing Go behavior. Use captured 16-byte little-endian tablet event fixtures for parity. Do not remove Go implementation or change release defaults until cross-platform real-device acceptance criteria pass.

## Related Notes

- [[reMouseable AI Handoff]]
- [[000 - Project Index|Project Index]]
