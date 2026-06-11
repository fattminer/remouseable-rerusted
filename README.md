# reMouseable

Use a reMarkable tablet stylus as a host-computer mouse.

reMouseable connects to the tablet over SSH, reads Linux Evdev events, maps
stylus coordinates to the active display, and emits native mouse movement,
click, and drag actions. The application is written in Rust and provides a
Slint graphical interface plus a terminal mode.

## Status

The Rust application supports:

- reMarkable event streams over SSH.
- Password and SSH-agent authentication.
- Optional OpenSSH `known_hosts` verification.
- Windows and macOS mouse injection through Enigo.
- Linux X11 mouse injection through Enigo.
- Linux Wayland mouse injection through a `uinput` virtual mouse.
- `right`, `left`, and `vertical` tablet orientations.
- Deterministic local event-stream processing for development and testing.

Linux Wayland behavior has been tested primarily with Hyprland. Broader
compositor and multi-monitor coverage remains limited.

## Requirements

- A reMarkable tablet reachable over USB at `10.11.99.1:22`, or over Wi-Fi at
  another stable address.
- Tablet root password, found under **Settings > Help > Copyrights and
  licenses** on current tablet software.
- Host accessibility/input permissions required by the operating system.
- Linux Wayland only: write access to `/dev/uinput`.

## Build

Install the current stable Rust toolchain, then run:

```shell
cargo build --release
```

The executable is written to `target/release/remouseable` on Linux and macOS,
or `target/release/remouseable.exe` on Windows.

Linux builds require X11 development libraries for the Enigo backend. On
Debian or Ubuntu:

```shell
sudo apt-get update
sudo apt-get install libx11-dev libxcb1-dev libxrandr-dev
```

Windows builds require a working MSVC Build Tools and Windows SDK environment.
macOS builds require Xcode command-line tools.

## Run

Launching without arguments opens the graphical interface:

```shell
remouseable
```

Enter the tablet password and select **Start**. The default tablet address is
`10.11.99.1:22`; the default event device is `/dev/input/event1`.

Use terminal mode for prompts, scripting, or detailed diagnostics:

```shell
remouseable --tui
```

Pass connection values directly when needed:

```shell
remouseable --tui \
  --ssh-password="TABLET_PASSWORD" \
  --event-file="/dev/input/event1"
```

For a tablet connected over Wi-Fi:

```shell
remouseable --tui \
  --ssh-ip="192.168.1.110:22" \
  --ssh-password="TABLET_PASSWORD"
```

The stylus moves the cursor while hovering. Pressure above the configured
threshold presses the left button; lifting the stylus releases it.

## SSH Authentication

Omit `--ssh-password`, or pass `--ssh-password=-`, to receive a hidden password
prompt in terminal mode.

To use an SSH agent, pass an explicitly empty password. `--ssh-socket` defaults
to `SSH_AUTH_SOCK`:

```shell
remouseable --tui --ssh-password=""
```

Host-key verification is disabled unless a known-hosts file is supplied:

```shell
remouseable --tui \
  --ssh-password="TABLET_PASSWORD" \
  --ssh-known-hosts="$HOME/.ssh/known_hosts"
```

Only absolute, shell-safe remote event paths are accepted.

## Host Drivers

`--host-driver=auto` selects:

- `uinput` on Linux Wayland.
- `enigo` on Windows, macOS, and Linux X11.

Available values are `auto`, `enigo`, `uinput`, and `uinput-tablet`.
`uinput-tablet` is experimental and is not the default because absolute-tablet
behavior was unreliable during Hyprland testing.

On Hyprland, reMouseable reads the focused monitor's logical dimensions from
`hyprctl monitors -j`. Override display detection when necessary:

```shell
remouseable --tui --screen-width=1280 --screen-height=800
```

## Local Event Streams

Process a captured 16-byte little-endian Evdev stream without moving the host
cursor:

```shell
remouseable --tui --input-file=path/to/events.bin
```

This writes scaled mouse actions as JSON Lines. To print selected raw events
instead:

```shell
remouseable --tui --input-file=path/to/events.bin --debug-events
```

Live `--debug-events` mode reads from the tablet but does not inject mouse
actions.

## Options

Run `remouseable --help` for the complete generated option list. Important
options include:

| Option | Purpose |
|---|---|
| `--tui` | Stay in terminal mode instead of opening the GUI |
| `--input-file <PATH>` | Process a local raw Evdev stream |
| `--debug-events` | Print selected hardware events |
| `--host-driver <DRIVER>` | Select `auto`, `enigo`, `uinput`, or `uinput-tablet` |
| `--orientation <VALUE>` | Select `right`, `left`, or `vertical` |
| `--pressure-threshold <VALUE>` | Set stylus contact threshold; default `1000` |
| `--screen-width <VALUE>` | Override detected host display width |
| `--screen-height <VALUE>` | Override detected host display height |
| `--event-file <PATH>` | Select remote event device |
| `--ssh-ip <HOST:PORT>` | Set tablet SSH address; default `10.11.99.1:22` |
| `--ssh-user <USER>` | Set tablet SSH user; default `root` |
| `--ssh-password <VALUE>` | Set password, or `-` to prompt |
| `--ssh-socket <PATH>` | Set SSH-agent socket |
| `--ssh-known-hosts <PATH>` | Enable tablet host-key verification |

## Platform Notes

### Windows

The application needs permission to control the mouse. GUI mode hides the
console window; use `--tui` when troubleshooting.

### macOS

Grant the launching terminal or application Accessibility permission under
**System Settings > Privacy & Security > Accessibility**.

### Linux

X11 uses Enigo. Wayland uses `/dev/uinput` by default. Configure an appropriate
udev rule or group membership so the current user can open `/dev/uinput`; avoid
running the entire application as root when a narrower permission is possible.

## Architecture

Core modules:

| Path | Responsibility |
|---|---|
| `src/main.rs` | CLI parsing, GUI/terminal selection, application assembly |
| `src/ui.rs` | Slint frontend and background live-stream control |
| `src/ssh.rs` | SSH authentication, host-key checks, remote event stream |
| `src/event.rs` | 16-byte Evdev decoding and event filtering |
| `src/state.rs` | Stylus position and pressure state machine |
| `src/scale.rs` | Orientation-aware coordinate scaling |
| `src/runtime.rs` | State-change dispatch to host drivers |
| `src/driver.rs` | Enigo and Linux `uinput` mouse backends |
| `src/app.rs` | Shared stream-processing pipeline |

Data flow:

```text
tablet event device -> SSH stream -> Evdev decoder -> state machine
                    -> coordinate scaler -> native host driver -> mouse
```

Tablet events use a fixed 16-byte little-endian layout with 32-bit timestamp
fields. Do not replace this parser with the host platform's native
`input_event` layout.

## Development

Run all checks before submitting changes:

```shell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --doc
```

`fixtures/representative-events.hex` and
`tests/representative_stream.rs` provide deterministic end-to-end coverage
without moving the real cursor. Native mouse injection still requires manual
smoke testing on each supported platform.

## License

GNU General Public License version 3. See [LICENSE](LICENSE).

## Origins

This Rust rewrite is based on the original
[reMouseable project](https://github.com/kevinconway/remouseable) created by
[Kevin Conway](https://github.com/kevinconway).

Thank you to Kevin for his hard work designing and building reMouseable. His
original implementation established the tablet event handling, coordinate
mapping, SSH workflow, and cross-platform mouse-control behavior that made this
rewrite possible.
