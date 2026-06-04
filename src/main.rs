// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use clap::{Parser, ValueEnum};
use remouseable::{
    DEFAULT_TABLET_HEIGHT, DEFAULT_TABLET_WIDTH,
    app::{Config, Orientation, debug_events, process_events},
    ssh::{SshOptions, open_event_stream},
};
use std::{
    error::Error,
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

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

    /// Tablet orientation.
    #[arg(long, value_enum, default_value_t = OrientationArg::Right)]
    orientation: OrientationArg,

    /// Pen pressure value considered contact.
    #[arg(long, default_value_t = 1000)]
    pressure_threshold: i32,

    /// Host screen height.
    #[arg(long, default_value_t = 1080)]
    screen_height: i32,

    /// Host screen width.
    #[arg(long, default_value_t = 1920)]
    screen_width: i32,

    /// Tablet coordinate height.
    #[arg(long, default_value_t = DEFAULT_TABLET_HEIGHT)]
    tablet_height: i32,

    /// Tablet coordinate width.
    #[arg(long, default_value_t = DEFAULT_TABLET_WIDTH)]
    tablet_width: i32,

    /// Remote event path.
    #[arg(long, default_value = "/dev/input/event0")]
    event_file: String,

    /// Tablet SSH address.
    #[arg(long, default_value = "10.11.99.1:22")]
    ssh_ip: String,

    /// Tablet SSH user.
    #[arg(long, default_value = "root")]
    ssh_user: String,

    /// Tablet SSH password. Use "-" to prompt securely.
    #[arg(long, default_value = "")]
    ssh_password: String,

    /// SSH agent socket. Defaults to `SSH_AUTH_SOCK`.
    #[arg(long, env = "SSH_AUTH_SOCK", default_value = "")]
    ssh_socket: String,

    /// Verify the tablet host key against this OpenSSH `known_hosts` file.
    #[arg(long)]
    ssh_known_hosts: Option<PathBuf>,
}

fn run(mut args: Args) -> Result<(), Box<dyn Error>> {
    let input: Box<dyn Read> = if let Some(input_file) = args.input_file {
        Box::new(BufReader::new(File::open(input_file)?))
    } else {
        if args.ssh_password == "-" {
            args.ssh_password = rpassword::prompt_password("Enter Password: ")?;
        }
        if args.ssh_known_hosts.is_none() {
            eprintln!(
                "remouseable: warning: SSH host key verification disabled for compatibility; use --ssh-known-hosts <PATH>"
            );
        }
        Box::new(open_event_stream(&SshOptions {
            address: args.ssh_ip,
            user: args.ssh_user,
            password: args.ssh_password,
            agent_socket: args.ssh_socket,
            event_file: args.event_file,
            known_hosts: args.ssh_known_hosts,
        })?)
    };
    let mut output = BufWriter::new(io::stdout().lock());

    if args.debug_events {
        debug_events(input, &mut output)?;
        output.flush()?;
        return Ok(());
    }

    process_events(
        input,
        &mut output,
        Config {
            orientation: args.orientation.into(),
            tablet_width: args.tablet_width,
            tablet_height: args.tablet_height,
            screen_width: args.screen_width,
            screen_height: args.screen_height,
            pressure_threshold: args.pressure_threshold,
            disable_drag_event: args.disable_drag_event,
        },
    )?;
    output.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
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
        assert_eq!(args.event_file, "/dev/input/event1");
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
        assert_eq!(args.event_file, "/dev/input/event1");
        assert!(matches!(args.orientation, OrientationArg::Left));
        assert_eq!(args.pressure_threshold, 1500);
        assert_eq!(args.screen_height, 1440);
        assert_eq!(args.screen_width, 2560);
        assert_eq!(args.ssh_ip, "remarkable.local:2222");
        assert_eq!(args.ssh_password, "secret");
        assert_eq!(args.ssh_socket, "/tmp/agent.sock");
        assert_eq!(args.ssh_user, "tablet");
        assert_eq!(args.tablet_height, 16000);
        assert_eq!(args.tablet_width, 21000);
    }
}
