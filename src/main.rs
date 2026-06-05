// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use clap::{Parser, ValueEnum};
use remouseable::{
    DEFAULT_TABLET_HEIGHT, DEFAULT_TABLET_WIDTH, DriverKind, HostDriver, NativeDriver,
    app::{Config, Orientation, debug_events, process_events, process_events_with_driver},
    ssh::{SshOptions, open_event_stream},
};
use std::{
    error::Error,
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

const DEFAULT_EVENT_FILE: &str = "/dev/input/event1";

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OrientationArg {
    Right,
    Left,
    Vertical,
}

impl From<OrientationArg> for Orientation {
    fn from(orientation: OrientationArg) -> Self {
        match orientation {
            OrientationArg::Right => Self::Right,
            OrientationArg::Left => Self::Left,
            OrientationArg::Vertical => Self::Vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HostDriverArg {
    Auto,
    Enigo,
    Uinput,
    UinputTablet,
}

impl From<HostDriverArg> for DriverKind {
    fn from(driver: HostDriverArg) -> Self {
        match driver {
            HostDriverArg::Auto => Self::Auto,
            HostDriverArg::Enigo => Self::Enigo,
            HostDriverArg::Uinput => Self::Uinput,
            HostDriverArg::UinputTablet => Self::UinputTablet,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Local raw Evdev stream to process instead of connecting over SSH.
    #[arg(long)]
    input_file: Option<PathBuf>,

    /// Stream selected hardware events instead of emitting host actions.
    #[arg(long)]
    debug_events: bool,

    /// Disable custom drag events and emit ordinary movement while clicked.
    #[arg(long)]
    disable_drag_event: bool,

    /// Host mouse injection backend.
    #[arg(long, value_enum, default_value_t = HostDriverArg::Auto)]
    host_driver: HostDriverArg,

    /// Tablet orientation.
    #[arg(long, value_enum, default_value_t = OrientationArg::Right)]
    orientation: OrientationArg,

    /// Pen pressure value considered contact.
    #[arg(long, default_value_t = 1000)]
    pressure_threshold: i32,

    /// Host screen height.
    #[arg(long)]
    screen_height: Option<i32>,

    /// Host screen width.
    #[arg(long)]
    screen_width: Option<i32>,

    /// Tablet coordinate height.
    #[arg(long, default_value_t = DEFAULT_TABLET_HEIGHT)]
    tablet_height: i32,

    /// Tablet coordinate width.
    #[arg(long, default_value_t = DEFAULT_TABLET_WIDTH)]
    tablet_width: i32,

    /// Remote event path. Prompts when omitted for live SSH runs.
    #[arg(long)]
    event_file: Option<String>,

    /// Tablet SSH address.
    #[arg(long, default_value = "10.11.99.1:22")]
    ssh_ip: String,

    /// Tablet SSH user.
    #[arg(long, default_value = "root")]
    ssh_user: String,

    /// Tablet SSH password. Prompts when omitted or set to "-".
    #[arg(long)]
    ssh_password: Option<String>,

    /// SSH agent socket. Defaults to `SSH_AUTH_SOCK`.
    #[arg(long, env = "SSH_AUTH_SOCK", default_value = "")]
    ssh_socket: String,

    /// Verify the tablet host key against this OpenSSH `known_hosts` file.
    #[arg(long)]
    ssh_known_hosts: Option<PathBuf>,
}

fn run(args: &Args) -> Result<(), Box<dyn Error>> {
    let is_live = args.input_file.is_none();
    let input: Box<dyn Read> = if let Some(input_file) = &args.input_file {
        Box::new(BufReader::new(File::open(input_file)?))
    } else {
        let password = ssh_password_or_prompt(args.ssh_password.as_deref())?;
        let event_file = event_file_or_prompt(args.event_file.as_deref())?;
        if args.ssh_known_hosts.is_none() {
            eprintln!(
                "remouseable: warning: SSH host key verification disabled for compatibility; use --ssh-known-hosts <PATH>"
            );
        }
        Box::new(open_event_stream(&SshOptions {
            address: args.ssh_ip.clone(),
            user: args.ssh_user.clone(),
            password,
            agent_socket: args.ssh_socket.clone(),
            event_file,
            known_hosts: args.ssh_known_hosts.clone(),
        })?)
    };
    let mut output = BufWriter::new(io::stdout().lock());

    if args.debug_events {
        debug_events(input, &mut output)?;
        output.flush()?;
        return Ok(());
    }

    if is_live {
        let driver = NativeDriver::new(args.host_driver.into())?;
        let (detected_width, detected_height) = driver.screen_size()?;
        process_events_with_driver(input, driver, config(args, detected_width, detected_height))?;
    } else {
        process_events(input, &mut output, config(args, 1920, 1080))?;
    }
    output.flush()?;
    Ok(())
}

fn ssh_password_or_prompt(password: Option<&str>) -> io::Result<String> {
    match password {
        Some("-") | None => rpassword::prompt_password("SSH password: "),
        Some(password) => Ok(password.to_owned()),
    }
}

fn event_file_or_prompt(event_file: Option<&str>) -> io::Result<String> {
    match event_file {
        Some(event_file) => Ok(event_file.to_owned()),
        None => prompt_with_default(
            io::stdin().lock(),
            io::stderr().lock(),
            "Event file",
            DEFAULT_EVENT_FILE,
        ),
    }
}

fn prompt_with_default<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    label: &str,
    default: &str,
) -> io::Result<String> {
    write!(output, "{label} [{default}]: ")?;
    output.flush()?;

    let mut value = String::new();
    input.read_line(&mut value)?;
    let value = value.trim();
    if value.is_empty() {
        Ok(default.to_owned())
    } else {
        Ok(value.to_owned())
    }
}

fn config(args: &Args, default_width: i32, default_height: i32) -> Config {
    Config {
        orientation: args.orientation.into(),
        tablet_width: args.tablet_width,
        tablet_height: args.tablet_height,
        screen_width: args.screen_width.unwrap_or(default_width),
        screen_height: args.screen_height.unwrap_or(default_height),
        pressure_threshold: args.pressure_threshold,
        disable_drag_event: args.disable_drag_event,
    }
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("remouseable: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }

    #[test]
    fn parses_legacy_and_local_flags() {
        let args = Args::try_parse_from([
            "remouseable",
            "--input-file",
            "events.bin",
            "--event-file",
            "/dev/input/event1",
            "--orientation",
            "vertical",
            "--pressure-threshold",
            "1200",
            "--ssh-ip",
            "192.168.1.10:22",
            "--ssh-known-hosts",
            ".ssh/known_hosts",
        ])
        .unwrap();

        assert_eq!(args.input_file, Some(PathBuf::from("events.bin")));
        assert!(matches!(args.orientation, OrientationArg::Vertical));
        assert_eq!(args.event_file.as_deref(), Some("/dev/input/event1"));
        assert_eq!(args.pressure_threshold, 1200);
        assert_eq!(args.ssh_ip, "192.168.1.10:22");
        assert_eq!(
            args.ssh_known_hosts,
            Some(PathBuf::from(".ssh/known_hosts"))
        );
    }

    #[test]
    fn parses_every_original_launch_parameter() {
        let args = Args::try_parse_from([
            "remouseable",
            "--debug-events",
            "--disable-drag-event",
            "--event-file=/dev/input/event1",
            "--host-driver=uinput-tablet",
            "--orientation=left",
            "--pressure-threshold=1500",
            "--screen-height=1440",
            "--screen-width=2560",
            "--ssh-ip=remarkable.local:2222",
            "--ssh-password=secret",
            "--ssh-socket=/tmp/agent.sock",
            "--ssh-user=tablet",
            "--tablet-height=16000",
            "--tablet-width=21000",
        ])
        .unwrap();

        assert!(args.debug_events);
        assert!(args.disable_drag_event);
        assert_eq!(args.event_file.as_deref(), Some("/dev/input/event1"));
        assert!(matches!(args.host_driver, HostDriverArg::UinputTablet));
        assert!(matches!(args.orientation, OrientationArg::Left));
        assert_eq!(args.pressure_threshold, 1500);
        assert_eq!(args.screen_height, Some(1440));
        assert_eq!(args.screen_width, Some(2560));
        assert_eq!(args.ssh_ip, "remarkable.local:2222");
        assert_eq!(args.ssh_password.as_deref(), Some("secret"));
        assert_eq!(args.ssh_socket, "/tmp/agent.sock");
        assert_eq!(args.ssh_user, "tablet");
        assert_eq!(args.tablet_height, 16000);
        assert_eq!(args.tablet_width, 21000);
    }

    #[test]
    fn omits_prompted_live_values_from_cli_defaults() {
        let args = Args::try_parse_from(["remouseable"]).unwrap();

        assert_eq!(args.ssh_password, None);
        assert_eq!(args.event_file, None);
    }

    #[test]
    fn event_file_prompt_uses_default_for_blank_input() {
        let mut output = Vec::new();

        let event_file = prompt_with_default(
            "\n".as_bytes(),
            &mut output,
            "Event file",
            DEFAULT_EVENT_FILE,
        )
        .unwrap();

        assert_eq!(event_file, DEFAULT_EVENT_FILE);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Event file [/dev/input/event1]: "
        );
    }

    #[test]
    fn event_file_prompt_uses_entered_value() {
        let mut output = Vec::new();

        let event_file = prompt_with_default(
            "/dev/input/event2\n".as_bytes(),
            &mut output,
            "Event file",
            DEFAULT_EVENT_FILE,
        )
        .unwrap();

        assert_eq!(event_file, "/dev/input/event2");
    }
}
