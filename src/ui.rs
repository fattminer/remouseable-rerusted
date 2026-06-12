// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::{Args, HostDriverArg, OrientationArg, config};
use remouseable::{
    HostDriver, NativeDriver,
    app::{process_events_with_driver, process_pen_events_with_driver},
    available_monitors,
    ssh::{SshOptions, open_event_stream_with_cancel},
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{
    error::Error,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

slint::include_modules!();

/// Opens the graphical frontend and wires Slint callbacks to Rust runtime work.
///
/// The UI thread remains responsive while the tablet stream runs on a worker
/// thread. User-entered values are copied into the worker before launch.
pub fn run_ui(args: &Args) -> Result<(), Box<dyn Error>> {
    let ui = RemouseableWindow::new()?;
    ui.set_ssh_ip(args.ssh_ip.clone().into());
    ui.set_ssh_user(args.ssh_user.clone().into());
    ui.set_event_file(
        args.event_file
            .as_deref()
            .unwrap_or("/dev/input/event1")
            .into(),
    );
    ui.set_orientation(orientation_name(args.orientation).into());
    ui.set_host_driver(host_driver_name(args.host_driver).into());
    ui.set_pressure_threshold(args.pressure_threshold.to_string().into());
    ui.set_disable_drag_event(args.disable_drag_event);
    let monitors = if cfg!(target_os = "windows") {
        available_monitors()?
    } else {
        Vec::new()
    };
    let monitor_labels = monitors
        .iter()
        .map(|monitor| SharedString::from(monitor.label.as_str()))
        .collect::<Vec<_>>();
    let selected_monitor = args
        .monitor_id
        .and_then(|id| monitors.iter().position(|monitor| monitor.id == id))
        .or_else(|| monitors.iter().position(|monitor| monitor.is_primary))
        .unwrap_or(0);
    ui.set_monitor_options(ModelRc::new(VecModel::from(monitor_labels)));
    ui.set_monitor_index(i32::try_from(selected_monitor).unwrap_or(0));
    ui.set_show_monitor_selector(cfg!(target_os = "windows"));

    // Keep CLI defaults as the base configuration. Start callback values
    // override only fields the UI exposes.
    let base_args = args.clone();
    let weak = ui.as_weak();

    // Shared cancellation slot. `None` means no stream is running. `Some(token)`
    // means Stop can ask the active SSH reader to end cleanly.
    let active_cancel = Arc::new(Mutex::new(None::<Arc<AtomicBool>>));
    let start_cancel = Arc::clone(&active_cancel);
    ui.on_start_requested(
        move |ssh_ip,
              ssh_user,
              ssh_password,
              event_file,
              orientation,
              host_driver,
              monitor_index,
              pressure_threshold,
              disable_drag_event| {
            // A fresh token belongs to one launched worker. Existing token means
            // user double-clicked Start or a previous stream is still shutting down.
            let cancel = Arc::new(AtomicBool::new(false));
            if let Ok(mut active) = start_cancel.lock() {
                if active.is_some() {
                    return;
                }
                *active = Some(Arc::clone(&cancel));
            }

            // Convert Slint shared strings to owned Rust strings before moving
            // work to the background thread.
            let launch = UiLaunchArgs {
                ssh_ip: ssh_ip.to_string(),
                ssh_user: ssh_user.to_string(),
                ssh_password: ssh_password.to_string(),
                event_file: event_file.to_string(),
                orientation: orientation.to_string(),
                host_driver: host_driver.to_string(),
                monitor_id: usize::try_from(monitor_index)
                    .ok()
                    .and_then(|index| monitors.get(index))
                    .map(|monitor| monitor.id),
                pressure_threshold: pressure_threshold.to_string(),
                disable_drag_event,
            };

            let weak = weak.clone();
            let base_args = base_args.clone();
            let finish_cancel = Arc::clone(&start_cancel);
            set_status(&weak, true, "Starting live stream...");

            // Native mouse injection and SSH reads are blocking, so keep them
            // off the Slint event loop.
            thread::spawn(move || {
                let result = run_live_from_ui(base_args, launch, cancel);
                if let Ok(mut active) = finish_cancel.lock() {
                    *active = None;
                }
                match result {
                    Ok(()) => set_status(&weak, false, "Stream ended."),
                    Err(error) => set_status(&weak, false, &format!("Error: {error}")),
                }
            });
        },
    );
    let stop_cancel = Arc::clone(&active_cancel);
    let stop_weak = ui.as_weak();
    ui.on_stop_requested(move || {
        // Stop is cooperative: signal the reader, then let runtime unwind and
        // release any held mouse button through driver shutdown behavior.
        if let Ok(active) = stop_cancel.lock()
            && let Some(cancel) = active.as_ref()
        {
            cancel.store(true, Ordering::Relaxed);
            set_status(&stop_weak, true, "Stopping...");
        }
    });

    ui.run()?;
    Ok(())
}

/// Values captured from the Slint callback at Start time.
struct UiLaunchArgs {
    ssh_ip: String,
    ssh_user: String,
    ssh_password: String,
    event_file: String,
    orientation: String,
    host_driver: String,
    monitor_id: Option<u32>,
    pressure_threshold: String,
    disable_drag_event: bool,
}

fn run_live_from_ui(
    mut args: Args,
    launch: UiLaunchArgs,
    cancel: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Rebuild Args so the existing CLI assembly path can be reused for UI mode.
    args.ssh_ip = launch.ssh_ip;
    args.ssh_user = launch.ssh_user;
    args.ssh_password = Some(launch.ssh_password);
    args.event_file = Some(launch.event_file);
    args.orientation = parse_orientation(&launch.orientation)?;
    args.host_driver = parse_host_driver(&launch.host_driver)?;
    args.monitor_id = launch.monitor_id;
    args.pressure_threshold = launch.pressure_threshold.trim().parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "pressure threshold must be an integer",
        )
    })?;
    args.disable_drag_event = launch.disable_drag_event;

    let event_file = super::event_file_or_prompt(args.event_file.as_deref())?;
    let driver = NativeDriver::new_for_monitor(
        args.host_driver.into(),
        args.monitor_id,
        args.windows_pen_interval_ms,
    )?;
    let (detected_width, detected_height) = driver.screen_size()?;

    // The SSH reader watches this token and reports EOF when Stop is requested.
    let input = open_event_stream_with_cancel(
        &SshOptions {
            address: args.ssh_ip.clone(),
            user: args.ssh_user.clone(),
            password: args.ssh_password.clone().unwrap_or_default(),
            agent_socket: args.ssh_socket.clone(),
            event_file,
            known_hosts: args.ssh_known_hosts.clone().or_else(default_known_hosts),
        },
        cancel,
    )?;

    let config = config(&args, detected_width, detected_height);
    if driver.supports_pen() {
        let screen_origin = driver.screen_origin();
        process_pen_events_with_driver(input, driver, config, screen_origin)?;
    } else {
        process_events_with_driver(input, driver, config)?;
    }
    Ok(())
}

/// Updates UI state from either Slint thread or worker thread.
fn set_status(weak: &slint::Weak<RemouseableWindow>, running: bool, message: &str) {
    let message = SharedString::from(message);
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_running(running);
        ui.set_status_text(message);
    });
}

/// Converts UI combobox text into the same enum clap uses for CLI arguments.
fn parse_orientation(value: &str) -> io::Result<OrientationArg> {
    match value.trim().to_ascii_lowercase().as_str() {
        "right" => Ok(OrientationArg::Right),
        "left" => Ok(OrientationArg::Left),
        "vertical" => Ok(OrientationArg::Vertical),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "orientation must be right, left, or vertical",
        )),
    }
}

/// Converts UI combobox text into the same enum clap uses for CLI arguments.
fn parse_host_driver(value: &str) -> io::Result<HostDriverArg> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(HostDriverArg::Auto),
        "enigo" => Ok(HostDriverArg::Enigo),
        "uinput" => Ok(HostDriverArg::Uinput),
        "uinput-tablet" => Ok(HostDriverArg::UinputTablet),
        "windows-pen" => Ok(HostDriverArg::WindowsPen),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "host driver must be auto, enigo, uinput, uinput-tablet, or windows-pen",
        )),
    }
}

const fn orientation_name(orientation: OrientationArg) -> &'static str {
    match orientation {
        OrientationArg::Right => "right",
        OrientationArg::Left => "left",
        OrientationArg::Vertical => "vertical",
    }
}

const fn host_driver_name(driver: HostDriverArg) -> &'static str {
    match driver {
        HostDriverArg::Auto => "auto",
        HostDriverArg::Enigo => "enigo",
        HostDriverArg::Uinput => "uinput",
        HostDriverArg::UinputTablet => "uinput-tablet",
        HostDriverArg::WindowsPen => "windows-pen",
    }
}

/// Returns the default OpenSSH known-hosts file when it exists.
fn default_known_hosts() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|home| home.join(".ssh").join("known_hosts"))
        .filter(|path| path.exists())
}
