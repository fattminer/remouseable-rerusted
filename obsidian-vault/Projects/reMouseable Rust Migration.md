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
repository: C:/Users/mfiner/GIT/remouseable
updated: 2026-06-12
priority: high
---

# reMouseable Rust Migration

> [!success] Current State
> Core rewrite and live Rust application are implemented. Remaining migration
> work centers on Rust-only packaging, cross-platform acceptance, secure SSH
> defaults, and advanced native input behavior.

## Why Convert

- Remove the original vendored RobotGo C/CGO surface.
- Replace string event types and runtime assertions with Rust enums and traits.
- Improve error handling, testability, security, and packaging.
- Support Linux Wayland through native `uinput` integration.
- Create a maintainable base for future pressure, tilt, buttons, and monitors.

Runtime performance was not the primary reason for conversion. Main value is
maintainability, safety, packaging, and future feature work.

## Historical Scope

The original application was a small Go core surrounded by generated event
codes and a large vendored RobotGo/native layer. Historical Go references in
this note explain compatibility and migration decisions; current implementation
is Rust-only.

## Implemented Rust Architecture

```text
Cargo.toml
src/
  app.rs       shared event-processing pipeline
  driver.rs    host driver selection, Enigo, and Linux uinput
  event.rs     explicit 16-byte Evdev decoder
  lib.rs       public module surface and constants
  main.rs      CLI and GUI/terminal selection
  pen.rs       framed pressure/tilt domain and runtime
  runtime.rs   scaled state-change dispatch
  scale.rs     orientation mapping
  ssh.rs       live russh event source
  state.rs     pressure and position state machine
  ui.rs        Slint frontend controller
  windows_pen.rs Windows synthetic pen driver
ui/
  remouseable.slint
tests/
  representative_stream.rs
fixtures/
  representative-events.hex
```

Core domain boundaries remain trait-based for deterministic tests:

- `EventSource`
- `ChangeSource`
- `PositionScaler`
- `HostDriver`

## Migration Phases

### Phase 0: Baseline and Fixtures

- [x] Record expected state changes and scaled coordinates.
- [x] Add deterministic synthetic wire-format fixture.
- [x] Add Rust end-to-end fixture tests.
- [ ] Capture and commit a sanitized representative real-tablet stream.
- [ ] Compare sanitized capture against documented expected actions.

`fixtures/representative-events.hex` is synthetic. It validates wire decoding
and pipeline behavior but is not a substitute for a real-device capture.

### Phase 1: Pure Rust Core

- [x] Initialize Cargo project.
- [x] Implement explicit 16-byte decoder tolerant of fragmented reads.
- [x] Implement event filtering and naming.
- [x] Implement pressure/position state machine.
- [x] Implement three orientation scalers.
- [x] Implement runtime dispatch through fake drivers.
- [x] Port relevant compatibility tests.

### Phase 2: CLI, GUI, and Local Sources

- [x] Implement `clap` CLI.
- [x] Add local/static event-file source.
- [x] Add structured errors and process exit behavior.
- [x] Add debug event output.
- [x] Add Slint GUI as default launch mode.
- [x] Keep terminal behavior behind `--tui`.
- [x] Run live work off the Slint UI thread.
- [x] Add cooperative Stop/cancellation behavior.
- [x] Connect from the GUI by pressing Enter in the password field.

### Phase 3: SSH

- [x] Implement password and prompted authentication.
- [x] Implement SSH-agent socket authentication.
- [x] Validate password authentication against real tablet.
- [ ] Validate agent/RSA authentication against real tablet.
- [x] Validate remote event paths against shell injection.
- [x] Stream command output without buffering the whole stream.
- [x] Add optional `known_hosts` verification.
- [ ] Make host verification secure by default with usable first-use flow.

The Ring-backed `russh` implementation connected to a real tablet and streamed
`/dev/input/event1` on June 4, 2026. Keep `Cargo.lock` committed because SSH and
cryptographic dependency resolution is sensitive.

### Phase 4: Host Drivers

- [x] Implement Enigo driver.
- [x] Detect primary display size using `display-info`.
- [x] Validate Windows hover/press/drag/release pipeline with real tablet.
- [x] Add Linux Wayland relative `uinput` backend.
- [x] Add Hyprland focused-monitor logical-size detection.
- [x] Chunk relative movement to avoid compositor/libinput delta limits.
- [x] Add experimental absolute `uinput-tablet` backend.
- [ ] Test macOS, especially drag semantics.
- [ ] Test Linux X11 with real tablet.
- [ ] Test additional Wayland compositors.
- [x] Add explicit Windows pen monitor selection and virtual-screen offsets.
- [x] Add Windows synthetic pen pressure/tilt backend.
- [x] Add attached-monitor selection for Windows pen mapping.
- [x] Map `BTN_TOOL_RUBBER` to Windows inverted eraser input.

Windows validation on June 4, 2026 processed live `/dev/input/event1` stylus
events through SSH, parsing, scaling, and Enigo without runtime errors.

Hyprland validation on June 5, 2026 detected a 1920x1200 monitor at scale 1.50
as 1280x800 logical pixels. Relative movement chunking materially improved
full-screen scaling. Experimental absolute tablet injection remained unreliable.

### Phase 5: Packaging and Cutover

- [x] Make Rust implementation the repository source of truth.
- [x] Replace user README with Rust-only usage/build documentation.
- [x] Preserve original creator attribution and GPL license.
- [ ] Remove obsolete Go jobs from GitHub Actions.
- [ ] Replace release workflows with Rust builds for all target platforms.
- [ ] Convert devcontainer from Go tooling to Rust tooling.
- [x] Produce and live smoke-test Windows release binary locally.
- [ ] Produce and smoke-test macOS Intel and ARM release binaries.
- [ ] Produce and smoke-test Linux release binary.
- [ ] Document `/dev/uinput` permissions and supported compositor behavior.

## Performance Work Completed

- Read complete 16-byte events in one call when available.
- Bridge `russh` chunks using `Bytes` without redundant copying.
- Use one Tokio SSH worker instead of a CPU-count pool.
- Remove redundant production event selection.
- Track coordinate changes with compact bit flags.
- Suppress duplicate native absolute positions.
- Enable TCP `NODELAY`.
- Coalesce Windows hover/contact frames with a 5 ms target while preserving
  every contact and proximity transition.
- The attempted 1-20 ms GUI control was removed after live telemetry remained
  near 32 Hz at both extremes; Windows error-87 retry timing dominated it.

Movement coalescing remains possible, but must preserve pressure transition
ordering so click and drag boundaries are never lost.

## Key Risks

### macOS Drag Semantics

Confirm Enigo produces behavior accepted by drawing applications. Implement a
thin Core Graphics driver if explicit dragged events are required.

### SSH Compatibility and Security

Password authentication works against tested Dropbear 2022.83 firmware. Agent
authentication still needs hardware validation. Host verification remains
opt-in for compatibility and should become secure by default.

### Wayland

Wayland support uses Linux `uinput`. Hyprland has partial validation; broader
compositor coverage, permission documentation, and multi-monitor behavior remain
open.

### Windows Pen Semantics

Windows `auto` now attempts a synthetic `PT_PEN` device, preserving continuous
pressure and X/Y tilt from complete `SYN_REPORT` frames. It warns and falls back
to Enigo if device creation fails; explicit `windows-pen` fails instead. Native
mode requires Windows 10 version 1809 or newer.

Real `/dev/input/event1` capture on June 11, 2026 verified pressure `0..4095`,
tilt X/Y `-9000..9000`, X `0..20966`, and Y `0..15725`. The hardware exposed no
rotation axis, so rotation is intentionally omitted rather than synthesized.
Windows Ink application validation remains pending.

On June 12, 2026, a live Windows run remained active for 60 seconds without
error 87 after pacing synthetic frames, retrying transient injection failures,
and mapping `BTN_TOOL_PEN` proximity to out-of-range/new-pointer transitions.
Windows monitor coordinates must be converted from desktop coordinates to
coordinates relative to the virtual screen's top-left before injection. For a
monitor left of the primary display, this prevents otherwise-correct scaling
from targeting the wrong screen.

### Dependency and Toolchain Risk

Input injection and SSH crates are sensitive dependencies. Pin lockfile, review
updates, run `cargo audit`, and validate Windows with a complete MSVC/SDK setup.

## Decision Log

| Date | Decision | Reason |
|---|---|---|
| 2026-06-04 | Preserve explicit remote event decoder | Tablet ABI uses 32-bit timestamps and may differ from host ABI |
| 2026-06-04 | Keep event core trait-based | Deterministic tests without SSH or native cursor movement |
| 2026-06-04 | Start with Enigo | Fastest path to cross-platform behavior parity |
| 2026-06-04 | Emit JSON actions for local streams | Executable end-to-end tests without native injection |
| 2026-06-04 | Use Ring-backed `russh` | Compatible with tested tablet without OpenSSL runtime dependency |
| 2026-06-04 | Keep `Cargo.lock` committed | Avoid incompatible cryptographic prerelease resolution |
| 2026-06-04 | Preserve insecure host-key default temporarily | Compatibility while offering warning and known-hosts upgrade path |
| 2026-06-04 | Optimize hot path without coalescing | Reduce latency without losing pressure transitions |
| 2026-06-05 | Add Linux Wayland `uinput` backend | Enigo/X11 injection is unreliable on Wayland |
| 2026-06-05 | Auto-select `uinput` only on Linux Wayland | Preserve Windows, macOS, and X11 behavior |
| 2026-06-05 | Chunk relative `uinput` movement | Avoid compositor/libinput large-delta limitations |
| 2026-06-11 | Treat Rust as source of truth | Go implementation is absent; migration history remains documentation only |
| 2026-06-11 | Keep handoff and migration notes | They preserve hardware findings, risks, decisions, and acceptance context |
| 2026-06-11 | Omit barrel rotation | Captured hardware exposes pressure and tilt but no genuine rotation axis |
| 2026-06-11 | Prefer Windows synthetic pen in auto mode | Preserve pressure and tilt while retaining Enigo compatibility fallback |
| 2026-06-12 | Use virtual-screen-relative Windows monitor origins | Synthetic pointer coordinates are relative to the virtual desktop top-left |
| 2026-06-12 | Pace and retry Windows pen frames | Buffered SSH frames can arrive faster than synthetic pointer injection accepts |
| 2026-06-12 | Map `BTN_TOOL_PEN` proximity | Windows requires explicit out-of-range and fresh pointer lifecycle transitions |
| 2026-06-12 | Map `BTN_TOOL_RUBBER` eraser side | Preserve Marker Plus tool identity and emit Windows inverted eraser flags |
| 2026-06-12 | Calibrate tip/eraser pressure separately | Live capture measured tip `1..4095`, eraser `184..2506`; use shared contact threshold `200` |

## Validation

```shell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --doc
cargo build --release
```

Native mouse tests require manual per-platform smoke testing. Warn operators
before tests that move or click the real cursor.

## Agent Handoff Prompt

> Read `obsidian-vault/Projects/reMouseable AI Handoff.md` and
> `obsidian-vault/Projects/reMouseable Rust Migration.md`. Inspect current
> repository state before editing. Preserve the fixed 16-byte tablet event ABI,
> deterministic local fixture path, original-project attribution, and unrelated
> user changes. Treat current Rust code as source of truth. Use historical Go
> behavior only as compatibility context. Validate platform-specific changes on
> each affected platform and warn before native input smoke tests.

## Related Notes

- [[reMouseable AI Handoff]]
- [[000 - Project Index|Project Index]]
