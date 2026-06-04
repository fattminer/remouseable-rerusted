---
title: reMouseable Rust Migration
aliases:
  - reMouseable Rust Rewrite
tags:
  - project/remouseable
  - rust
  - migration
  - roadmap
status: in-progress
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
| SSH | `russh` with Ring crypto backend |
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

- [x] Initialize Cargo project beside Go implementation.
- [x] Implement explicit 16-byte event decoder using `read_exact`.
- [x] Implement event filtering.
- [x] Implement state machine.
- [x] Implement three position scalers.
- [x] Implement runtime dispatch using fake driver.
- [x] Port relevant Go unit tests.

Exit condition: Rust core produces same output as Go fixture tests.

Implementation started on June 4, 2026. Pure Rust core lives in `src/` and has no external dependencies. Fifteen Rust unit tests pass. Captured real-tablet fixture comparison remains incomplete, so Phase 1 exit condition is not fully satisfied.

Validation commands:

```shell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

On the inspected Windows workstation, native MSVC tests require this library path because the Visual Studio developer environment is incomplete:

```powershell
$env:LIB='C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.51.36231\lib\onecore\x64'
cargo test --all-targets
```

Rust CI now runs format, tests, and Clippy on Ubuntu.

### Phase 2: CLI and Event Sources

- [x] Implement `clap` CLI with compatible flags.
- [x] Add local/static-file event source for debugging and tests.
- [x] Implement structured errors and exit codes.
- [x] Implement debug event output.

Exit condition: application runs end-to-end from recorded event stream.

Phase 2 completed on June 4, 2026. `cargo run -- --input-file <PATH>` processes a raw 16-byte Evdev stream and emits scaled action JSON Lines. Add `--debug-events` to emit named raw events instead.

`fixtures/representative-events.hex` provides a deterministic synthetic wire-format fixture used by integration tests. It is not a real tablet capture; obtaining and comparing a real-device capture remains open Phase 0 work.

### Phase 3: SSH

- [x] Implement password authentication.
- [x] Implement SSH agent authentication.
- [x] Validate password authentication against real tablet.
- [ ] Validate agent/RSA authentication against real tablet.
- [ ] Secure host-key handling.
- [x] Validate or safely encode event path.
- [x] Stream command output without buffering full stream.

Exit condition: Rust app receives live tablet events.

Phase 3 implementation completed on June 4, 2026. The Ring-backed `russh`
backend successfully connected and authenticated with a real reMarkable tablet,
then streamed `/dev/input/event1` for a timed smoke test. The tablet was idle
during the test, so no stylus events were captured. Password authentication is
validated; agent/RSA authentication still requires a real-device test.

The app preserves the Go application's insecure host-key default for launch
compatibility and warns; `--ssh-known-hosts <PATH>` enables strict verification.
`--event-file` accepts only safe absolute paths, blocking the Go implementation's
command-injection issue.

All original launch parameters are parsed by an explicit compatibility test.
The source reports nonzero remote/OpenSSH command exits. Local validation:
26 tests pass, format passes, and strict Clippy passes.

### Phase 4: Host Driver

- [x] Implement initial `enigo` driver.
- [x] Run move, press, release, and drag pipeline on Windows with real tablet input.
- [ ] Test on macOS, paying special attention to explicit drag events.
- [ ] Test on Linux X11.
- [ ] Replace `enigo` with platform-specific thin drivers only where behavior requires it.
- [x] Detect primary display size using `display-info`.
- [ ] Add explicit monitor selection and offsets.

Exit condition: real-device parity on all supported platforms.

Windows Phase 4 implementation landed on June 4, 2026. A real tablet produced
21,657 stylus events from `/dev/input/event1` during a 20-second scan. The live
Rust pipeline then ran SSH, event parsing, scaling, and native Enigo mouse
injection together for a timed validation window without errors. `/dev/input/event2`
was identified as touch input. The native driver releases a held left button
when it shuts down.

Latency optimization pass completed June 4, 2026:

- Read complete 16-byte events in one call when the source has them available.
- Transfer `russh` channel chunks through the blocking bridge as zero-copy `Bytes`.
- Use one Tokio SSH worker instead of the default CPU-count worker pool.
- Remove the redundant production `EV_ABS` selection layer.
- Track coordinate changes with a compact bitmask.
- Suppress duplicate absolute native mouse positions.

TCP `NODELAY` was already enabled. The event stream is push-based, so there is
no polling interval to increase. Movement coalescing remains a possible future
optimization, but it must preserve pressure transition ordering to avoid missed
click/drag boundaries. Use a release build for live operation.

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

Password authentication works against the tested Dropbear 2022.83 tablet.
Agent authentication, especially RSA agent keys, still requires real-device testing.

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
| 2026-06-04 | Keep initial Rust core dependency-free | Core event parsing, state, scaling, and dispatch need only standard library |
| 2026-06-04 | Improve partial-read behavior during port | Rust event source accepts fragmented reads and rejects truncated 16-byte events |
| 2026-06-04 | Emit JSON actions before host driver integration | Makes local stream processing executable and testable before native input injection |
| 2026-06-04 | Use Ring-backed `russh` for live SSH | Works against the real tablet's modern Dropbear algorithms without OpenSSL/libssh2 runtime dependencies |
| 2026-06-04 | Pin RustCrypto prerelease dependencies in `Cargo.lock` | `russh 0.61.1` otherwise resolves incompatible `primefield` prerelease versions |
| 2026-06-04 | Use Enigo for initial host driver | Safe cross-platform API integrates with existing driver trait; Windows real-tablet pipeline runs without errors |
| 2026-06-04 | Optimize live hot path without movement coalescing | Removes copies, duplicate injections, and redundant work while preserving every distinct position and pressure transition |
| 2026-06-04 | Preserve insecure host-key default temporarily | Keeps original launch behavior usable; warning and `--ssh-known-hosts` provide an upgrade path |

## Agent Handoff Prompt

Use this when starting implementation:

> Read `obsidian-vault/Projects/reMouseable AI Handoff.md` and `obsidian-vault/Projects/reMouseable Rust Migration.md`. Inspect current repository state before editing. Implement next incomplete migration phase while preserving existing Go behavior. Use captured 16-byte little-endian tablet event fixtures for parity. Do not remove Go implementation or change release defaults until cross-platform real-device acceptance criteria pass.

## Related Notes

- [[reMouseable AI Handoff]]
- [[000 - Project Index|Project Index]]
