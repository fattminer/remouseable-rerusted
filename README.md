# reMouseable - ReRusted

Use a reMarkable tablet as a host mouse.

This repository is now a Rust rewrite of Kevin Conway's original
[`remouseable`](https://github.com/kevinconway/remouseable) project. The legacy
Go implementation and documentation are no longer present.
The active implementation is the Rust crate in `src/`.

## Current Status

The Rust conversion is functional and has replaced the original Go runtime for
active development.

Validated so far:

- Live SSH event streaming from a reMarkable 2 using password authentication.
- Default reMarkable 2 event path `/dev/input/event1`.
- Windows host mouse control through Enigo.
- Linux Wayland host mouse control through a `/dev/uinput` virtual mouse.
- Hyprland focused-monitor detection with logical screen scaling.
- Local raw evdev stream processing for deterministic debugging and tests.

Implemented but not fully validated:

- Linux X11 through the Enigo backend.
- macOS through the Enigo backend.
- SSH agent authentication with `SSH_AUTH_SOCK` / `--ssh-socket`.
- OpenSSH `known_hosts` verification through `--ssh-known-hosts`.
- Experimental Linux absolute tablet injection through `--host-driver=uinput-tablet`.

Known caveats:

- Release packaging is not finished. Build from source for this Rust version.
- SSH host-key verification is disabled by default for compatibility with the
  original tool. Use `--ssh-known-hosts <PATH>` when you want verification.
- Linux Wayland support requires permission to open `/dev/uinput`.
- `uinput-tablet` is experimental and was not reliable in Hyprland testing.
- Multi-monitor behavior is limited. Hyprland uses the focused monitor; other
  systems use the primary display or first detected display unless overridden.

## How It Works

`remouseable` connects to the tablet over SSH and reads raw Linux evdev input
events from the tablet's input device. It decodes stylus coordinates and
pressure, maps tablet coordinates to host screen coordinates, and injects host
mouse movement/clicks through a selectable host driver.

The Rust implementation currently uses:

- `russh` with the `ring` crypto backend for SSH.
- `enigo` for cross-platform host mouse injection.
- Linux `evdev` + `/dev/uinput` for Wayland-friendly virtual mouse injection.
- `display-info` for host display detection.
- `clap` for command-line parsing.

## Quick Start

Connect the tablet over USB, then run the release binary:

```sh
remouseable
```

The program prompts for the tablet SSH password and the remote event file. Press
Enter at the event-file prompt to use the default reMarkable 2 path:

```text
Event file [/dev/input/event1]:
```

You can also pass both values directly:

```sh
remouseable --ssh-password="TABLET_PASSWORD" --event-file="/dev/input/event1"
```

For a wireless tablet, pass the tablet address:

```sh
remouseable --ssh-ip="192.168.1.110:22" --ssh-password="TABLET_PASSWORD" --event-file="/dev/input/event1"
```

The default USB SSH address is `10.11.99.1:22` and the default user is `root`.

## Build From Source

Install a stable Rust toolchain first:

```sh
rustup toolchain install stable
rustup default stable
```

Then build the optimized binary:

```sh
cargo build --release
```

The output binary is:

- Windows: `target\release\remouseable.exe`
- Linux/macOS: `target/release/remouseable`

### Windows Build Notes

Use the MSVC Rust toolchain and install Visual Studio Build Tools with the C++
toolchain and Windows SDK. Dependencies such as `ring` compile native C code and
need a complete MSVC environment.

If you see an error like `Cannot open include file: 'vcruntime.h'`, the MSVC
environment is incomplete or the shell was not launched from a configured
Developer PowerShell / Developer Command Prompt.

Recommended setup:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup default stable-x86_64-pc-windows-msvc
cargo build --release
```

### Linux Build Notes

Install Rust plus native development packages. On Debian/Ubuntu, start with:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libx11-dev libxcb1-dev libxrandr-dev libxi-dev libxtst-dev
cargo build --release
```

For Wayland sessions, `--host-driver=auto` selects the Linux `uinput` backend.
The user running `remouseable` must be able to open `/dev/uinput`. Configure
your distro's uinput permissions or run with appropriate privileges.

Example udev rule pattern:

```text
KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"
```

Adding a user to broad input-related groups can expose sensitive input devices.
Use the narrowest permission model your distro supports.

### macOS Build Notes

Install Rust and the Xcode command-line tools:

```sh
xcode-select --install
cargo build --release
```

macOS also requires Accessibility permission for the terminal or application
that launches `remouseable`, because the program controls the mouse.

## Usage

### Host Drivers

`--host-driver=auto` is the default.

| Driver | Platforms | Notes |
| --- | --- | --- |
| `auto` | All | Uses `uinput` on Linux Wayland and Enigo otherwise. |
| `enigo` | Windows, macOS, Linux | Cross-platform backend. Best fit for Windows, macOS, and Linux X11. |
| `uinput` | Linux only | Relative virtual mouse for Wayland. Requires `/dev/uinput`. |
| `uinput-tablet` | Linux only | Experimental absolute virtual tablet. Not the default. |

Force a driver when needed:

```sh
remouseable --host-driver=enigo
remouseable --host-driver=uinput
remouseable --host-driver=uinput-tablet
```

### Debug A Tablet Stream

Print decoded hardware events instead of moving the host mouse:

```sh
remouseable --debug-events --ssh-password="TABLET_PASSWORD" --event-file="/dev/input/event1"
```

Process a local captured raw evdev stream:

```sh
remouseable --input-file path/to/events.bin
```

Print decoded events from a local stream:

```sh
remouseable --input-file path/to/events.bin --debug-events
```

### Screen And Tablet Mapping

The host screen size is detected automatically for live mouse control. Override
it when detection is wrong:

```sh
remouseable --screen-width=1920 --screen-height=1080
```

Tablet coordinate defaults are tuned for the reMarkable 2:

```text
tablet width:  20967
tablet height: 15725
```

You normally should not need to change those values.

### Orientation

The default orientation is `right`, matching the original project behavior.
Available values are:

- `right`
- `left`
- `vertical`

Example:

```sh
remouseable --orientation=vertical
```

### SSH Authentication

Password authentication:

```sh
remouseable --ssh-password="TABLET_PASSWORD"
```

Prompt for password:

```sh
remouseable --ssh-password="-"
```

Try SSH agent authentication by passing an empty password and making sure
`SSH_AUTH_SOCK` is set:

```sh
remouseable --ssh-password=""
```

Use a specific agent socket:

```sh
remouseable --ssh-password="" --ssh-socket="/path/to/agent.sock"
```

Verify the tablet host key with an OpenSSH known-hosts file:

```sh
remouseable --ssh-known-hosts="$HOME/.ssh/known_hosts"
```

## Command-Line Options

```text
--input-file <INPUT_FILE>
    Local raw Evdev stream to process instead of connecting over SSH.

--debug-events
    Stream selected hardware events instead of emitting host actions.

--disable-drag-event
    Disable custom drag events and emit ordinary movement while clicked.

--host-driver <auto|enigo|uinput|uinput-tablet>
    Host mouse injection backend. Default: auto.

--orientation <right|left|vertical>
    Tablet orientation. Default: right.

--pressure-threshold <PRESSURE_THRESHOLD>
    Pen pressure value considered contact. Default: 1000.

--screen-height <SCREEN_HEIGHT>
--screen-width <SCREEN_WIDTH>
    Override detected host screen size.

--tablet-height <TABLET_HEIGHT>
--tablet-width <TABLET_WIDTH>
    Tablet coordinate bounds. Defaults: 15725 x 20967.

--event-file <EVENT_FILE>
    Remote event path. Prompts when omitted for live SSH runs.

--ssh-ip <SSH_IP>
    Tablet SSH address. Default: 10.11.99.1:22.

--ssh-user <SSH_USER>
    Tablet SSH user. Default: root.

--ssh-password <SSH_PASSWORD>
    Tablet SSH password. Prompts when omitted or set to "-".

--ssh-socket <SSH_SOCKET>
    SSH agent socket. Defaults to SSH_AUTH_SOCK.

--ssh-known-hosts <SSH_KNOWN_HOSTS>
    Verify the tablet host key against this OpenSSH known-hosts file.
```

## Development

Run the Rust validation suite:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

The repository still contains Go workflows and technical documentation from the
original implementation. Those are useful for historical context, but the Rust
crate is the implementation being moved forward.

## Original Project

This project started as Kevin Conway's `remouseable`, which provided portable
binaries for using a reMarkable tablet as a mouse. The Rust rewrite keeps the
same core launch parameters where practical while adding modern SSH handling,
local event replay, and Linux Wayland support.

The original Go documentation remains in `technical-documentation/`.

## License

`remouseable` is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License version 3 as published by the Free
Software Foundation.

`remouseable` is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE. See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with
`remouseable`. If not, see <https://www.gnu.org/licenses/>.

## Thanks

Thanks to [Kevin Conway](https://github.com/kevinconway/) for creating the
original project.

The original implementation referenced
[`golang-evdev`](https://github.com/gvalkov/golang-evdev) for evdev parsing and
embedded parts of [`robotgo`](https://github.com/go-vgo/robotgo) for host mouse
control. The Rust rewrite now uses native Rust dependencies for those layers.
