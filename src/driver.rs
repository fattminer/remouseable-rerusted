// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

#[cfg(target_os = "windows")]
use crate::windows_pen::WindowsPenDriver;
use crate::{HostDriver, MouseButton, PenDriver, PenInput};
#[cfg(target_os = "linux")]
use crate::{PenPhase, PenTool};
use display_info::DisplayInfo;
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use std::{io, process::Command};

#[cfg(target_os = "linux")]
use std::{thread, time::Duration};

#[cfg(target_os = "linux")]
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, PropType,
    RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverKind {
    Auto,
    Enigo,
    Uinput,
    UinputTablet,
    WindowsPen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonitorInfo {
    pub id: u32,
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_primary: bool,
}

pub struct NativeDriver {
    inner: NativeDriverInner,
    screen_origin: (i32, i32),
    #[cfg(target_os = "windows")]
    screen_size: (i32, i32),
}

enum NativeDriverInner {
    Enigo(Box<EnigoDriver>),
    #[cfg(target_os = "windows")]
    WindowsPen(WindowsPenDriver),
    #[cfg(target_os = "linux")]
    Uinput(UinputDriver),
    #[cfg(target_os = "linux")]
    UinputTablet(UinputTabletDriver),
}

struct EnigoDriver {
    enigo: Enigo,
    screen_width: i32,
    screen_height: i32,
    left_pressed: bool,
    last_position: Option<(i32, i32)>,
}

impl NativeDriver {
    /// Creates a host input driver targeting the primary display.
    ///
    /// # Errors
    ///
    /// Returns an error when the display or selected input backend cannot be opened.
    pub fn new(kind: DriverKind) -> io::Result<Self> {
        Self::new_for_monitor(kind, None, 5)
    }

    /// Creates a host input driver targeting a selected display.
    ///
    /// # Errors
    ///
    /// Returns an error when display enumeration, monitor selection, or backend
    /// creation fails.
    pub fn new_for_monitor(
        kind: DriverKind,
        monitor_id: Option<u32>,
        windows_pen_interval_ms: u64,
    ) -> io::Result<Self> {
        #[cfg(not(target_os = "windows"))]
        let _ = windows_pen_interval_ms;
        let display = selected_display(monitor_id)?;
        let screen_width = display.width;
        let screen_height = display.height;
        let inner = match resolve_driver_kind(kind) {
            DriverKind::Auto | DriverKind::Enigo => EnigoDriver::new(screen_width, screen_height)
                .map(Box::new)
                .map(NativeDriverInner::Enigo),
            DriverKind::Uinput => {
                #[cfg(target_os = "linux")]
                {
                    UinputDriver::new(screen_width, screen_height).map(NativeDriverInner::Uinput)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "uinput driver is only available on Linux",
                    ))
                }
            }
            DriverKind::UinputTablet => {
                #[cfg(target_os = "linux")]
                {
                    match UinputTabletDriver::new(screen_width, screen_height) {
                        Ok(driver) => Ok(NativeDriverInner::UinputTablet(driver)),
                        Err(error) if kind == DriverKind::Auto => {
                            eprintln!(
                                "remouseable: warning: Wayland tablet injection unavailable ({error}); falling back to relative uinput mouse input"
                            );
                            UinputDriver::new(screen_width, screen_height)
                                .map(NativeDriverInner::Uinput)
                        }
                        Err(error) => Err(error),
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "uinput tablet driver is only available on Linux",
                    ))
                }
            }
            DriverKind::WindowsPen => {
                #[cfg(target_os = "windows")]
                {
                    match WindowsPenDriver::new(windows_pen_interval_ms) {
                        Ok(driver) => Ok(NativeDriverInner::WindowsPen(driver)),
                        Err(error) if kind == DriverKind::Auto => {
                            eprintln!(
                                "remouseable: warning: Windows pen injection unavailable ({error}); falling back to Enigo mouse input"
                            );
                            EnigoDriver::new(screen_width, screen_height)
                                .map(Box::new)
                                .map(NativeDriverInner::Enigo)
                        }
                        Err(error) => Err(error),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "windows-pen driver is only available on Windows 10 version 1809 or newer",
                    ))
                }
            }
        }?;
        #[cfg(target_os = "linux")]
        let screen_origin = if matches!(inner, NativeDriverInner::UinputTablet(_)) {
            // Wayland maps tablet coordinates into compositor space. Desktop
            // origins would exceed the virtual device's declared axis range.
            (0, 0)
        } else {
            (display.x, display.y)
        };
        #[cfg(not(target_os = "linux"))]
        let screen_origin = (display.x, display.y);

        Ok(Self {
            inner,
            screen_origin,
            #[cfg(target_os = "windows")]
            screen_size: (screen_width, screen_height),
        })
    }

    #[must_use]
    pub fn supports_pen(&self) -> bool {
        match self.inner {
            #[cfg(target_os = "windows")]
            NativeDriverInner::WindowsPen(_) => true,
            #[cfg(target_os = "linux")]
            NativeDriverInner::UinputTablet(_) => true,
            _ => false,
        }
    }

    #[must_use]
    pub const fn screen_origin(&self) -> (i32, i32) {
        self.screen_origin
    }
}

/// Returns all attached host displays in operating-system enumeration order.
///
/// # Errors
///
/// Returns an error when displays cannot be enumerated or dimensions exceed
/// the supported coordinate range.
pub fn available_monitors() -> io::Result<Vec<MonitorInfo>> {
    DisplayInfo::all()
        .map_err(io::Error::other)?
        .into_iter()
        .enumerate()
        .map(|(index, display)| monitor_info(index, &display))
        .collect()
}

#[derive(Clone, Copy)]
struct DisplayMetrics {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

fn selected_display(monitor_id: Option<u32>) -> io::Result<DisplayMetrics> {
    if monitor_id.is_none()
        && std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|desktop| desktop.eq_ignore_ascii_case("hyprland"))
        && let Some(size) = hyprland_focused_monitor_size()?
    {
        return Ok(DisplayMetrics {
            x: 0,
            y: 0,
            width: size.0,
            height: size.1,
        });
    }

    let monitors = available_monitors()?;
    let monitor = choose_monitor(&monitors, monitor_id)?;
    let (x, y) = injection_origin(&monitors, monitor);
    Ok(DisplayMetrics {
        x,
        y,
        width: monitor.width,
        height: monitor.height,
    })
}

fn injection_origin(monitors: &[MonitorInfo], monitor: &MonitorInfo) -> (i32, i32) {
    #[cfg(target_os = "windows")]
    {
        let virtual_x = monitors.iter().map(|item| item.x).min().unwrap_or(0);
        let virtual_y = monitors.iter().map(|item| item.y).min().unwrap_or(0);
        (
            monitor.x.saturating_sub(virtual_x),
            monitor.y.saturating_sub(virtual_y),
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = monitors;
        (monitor.x, monitor.y)
    }
}

fn choose_monitor(monitors: &[MonitorInfo], monitor_id: Option<u32>) -> io::Result<&MonitorInfo> {
    match monitor_id {
        Some(id) => monitors
            .iter()
            .find(|monitor| monitor.id == id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("monitor ID {id} is not attached"),
                )
            }),
        None => monitors
            .iter()
            .find(|monitor| monitor.is_primary)
            .or_else(|| monitors.first())
            .ok_or_else(|| io::Error::other("no host displays found")),
    }
}

fn monitor_info(index: usize, display: &DisplayInfo) -> io::Result<MonitorInfo> {
    let width =
        i32::try_from(display.width).map_err(|_| io::Error::other("display width exceeds i32"))?;
    let height = i32::try_from(display.height)
        .map_err(|_| io::Error::other("display height exceeds i32"))?;
    let name = if display.friendly_name.trim().is_empty() {
        display.name.trim()
    } else {
        display.friendly_name.trim()
    };
    let primary = if display.is_primary { " (Primary)" } else { "" };
    Ok(MonitorInfo {
        id: display.id,
        label: format!(
            "{}: {} - {}x{} at {},{}{}",
            index + 1,
            name,
            width,
            height,
            display.x,
            display.y,
            primary
        ),
        x: display.x,
        y: display.y,
        width,
        height,
        is_primary: display.is_primary,
    })
}

fn hyprland_focused_monitor_size() -> io::Result<Option<(i32, i32)>> {
    let output = match Command::new("hyprctl").args(["monitors", "-j"]).output() {
        Ok(output) if output.status.success() => output.stdout,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let output = String::from_utf8_lossy(&output);
    Ok(parse_hyprland_monitor_size(&output))
}

#[allow(clippy::cast_possible_truncation)]
fn parse_hyprland_monitor_size(monitors: &str) -> Option<(i32, i32)> {
    let focused_index = monitors
        .find(r#""focused": true"#)
        .unwrap_or(monitors.len());
    let width = json_number_before(monitors, "width", focused_index)
        .or_else(|| json_number(monitors, "width"))?;
    let height = json_number_before(monitors, "height", focused_index)
        .or_else(|| json_number(monitors, "height"))?;
    let scale = json_number_before(monitors, "scale", focused_index)
        .or_else(|| json_number(monitors, "scale"))
        .unwrap_or(1.0);
    if width <= 0.0 || height <= 0.0 || scale <= 0.0 {
        return None;
    }
    Some((
        (width / scale).round() as i32,
        (height / scale).round() as i32,
    ))
}

fn json_number_before(object: &str, key: &str, before: usize) -> Option<f64> {
    let before = before.min(object.len());
    let marker = format!(r#""{key}":"#);
    let start = object[..before].rfind(&marker)? + marker.len();
    parse_json_number(&object[start..])
}

fn json_number(object: &str, key: &str) -> Option<f64> {
    let marker = format!(r#""{key}":"#);
    let start = object.find(&marker)? + marker.len();
    parse_json_number(&object[start..])
}

fn parse_json_number(value: &str) -> Option<f64> {
    let value = value
        .trim_start()
        .chars()
        .take_while(|character| character.is_ascii_digit() || matches!(character, '.' | '-'))
        .collect::<String>();
    value.parse().ok()
}

fn resolve_driver_kind(kind: DriverKind) -> DriverKind {
    if kind != DriverKind::Auto {
        return kind;
    }
    resolve_auto_driver()
}

#[cfg(target_os = "linux")]
fn resolve_auto_driver() -> DriverKind {
    resolve_linux_auto_driver(std::env::var("XDG_SESSION_TYPE").ok().as_deref())
}

#[cfg(target_os = "windows")]
const fn resolve_auto_driver() -> DriverKind {
    DriverKind::WindowsPen
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
const fn resolve_auto_driver() -> DriverKind {
    DriverKind::Enigo
}

#[cfg(target_os = "linux")]
fn resolve_linux_auto_driver(session_type: Option<&str>) -> DriverKind {
    if session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland")) {
        DriverKind::UinputTablet
    } else {
        DriverKind::Enigo
    }
}

impl EnigoDriver {
    fn new(screen_width: i32, screen_height: i32) -> io::Result<Self> {
        Ok(Self {
            enigo: Enigo::new(&Settings::default()).map_err(io::Error::other)?,
            screen_width,
            screen_height,
            left_pressed: false,
            last_position: None,
        })
    }
}

impl HostDriver for EnigoDriver {
    type Error = io::Error;

    fn screen_size(&self) -> Result<(i32, i32), Self::Error> {
        Ok((self.screen_width, self.screen_height))
    }

    fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        if self.last_position == Some((x, y)) {
            return Ok(());
        }
        self.enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(io::Error::other)?;
        self.last_position = Some((x, y));
        Ok(())
    }

    fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        self.move_mouse(x, y)
    }

    fn press(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.enigo
            .button(enigo_button(button), Direction::Press)
            .map_err(io::Error::other)?;
        if button == MouseButton::Left {
            self.left_pressed = true;
        }
        Ok(())
    }

    fn release(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.enigo
            .button(enigo_button(button), Direction::Release)
            .map_err(io::Error::other)?;
        if button == MouseButton::Left {
            self.left_pressed = false;
        }
        Ok(())
    }
}

impl Drop for EnigoDriver {
    fn drop(&mut self) {
        if self.left_pressed {
            let _ = self.enigo.button(Button::Left, Direction::Release);
        }
    }
}

impl HostDriver for NativeDriver {
    type Error = io::Error;

    fn screen_size(&self) -> Result<(i32, i32), Self::Error> {
        match &self.inner {
            NativeDriverInner::Enigo(driver) => driver.screen_size(),
            #[cfg(target_os = "windows")]
            NativeDriverInner::WindowsPen(_) => Ok(self.screen_size),
            #[cfg(target_os = "linux")]
            NativeDriverInner::Uinput(driver) => driver.screen_size(),
            #[cfg(target_os = "linux")]
            NativeDriverInner::UinputTablet(driver) => driver.screen_size(),
        }
    }

    fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        match &mut self.inner {
            NativeDriverInner::Enigo(driver) => driver.move_mouse(x, y),
            #[cfg(target_os = "windows")]
            NativeDriverInner::WindowsPen(_) => {
                Err(io::Error::other("Windows pen driver requires pen frames"))
            }
            #[cfg(target_os = "linux")]
            NativeDriverInner::Uinput(driver) => driver.move_mouse(x, y),
            #[cfg(target_os = "linux")]
            NativeDriverInner::UinputTablet(driver) => driver.move_mouse(x, y),
        }
    }

    fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        match &mut self.inner {
            NativeDriverInner::Enigo(driver) => driver.drag_mouse(x, y),
            #[cfg(target_os = "windows")]
            NativeDriverInner::WindowsPen(_) => {
                Err(io::Error::other("Windows pen driver requires pen frames"))
            }
            #[cfg(target_os = "linux")]
            NativeDriverInner::Uinput(driver) => driver.drag_mouse(x, y),
            #[cfg(target_os = "linux")]
            NativeDriverInner::UinputTablet(driver) => driver.drag_mouse(x, y),
        }
    }

    fn press(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        match &mut self.inner {
            NativeDriverInner::Enigo(driver) => driver.press(button),
            #[cfg(target_os = "windows")]
            NativeDriverInner::WindowsPen(_) => {
                Err(io::Error::other("Windows pen driver requires pen frames"))
            }
            #[cfg(target_os = "linux")]
            NativeDriverInner::Uinput(driver) => driver.press(button),
            #[cfg(target_os = "linux")]
            NativeDriverInner::UinputTablet(driver) => driver.press(button),
        }
    }

    fn release(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        match &mut self.inner {
            NativeDriverInner::Enigo(driver) => driver.release(button),
            #[cfg(target_os = "windows")]
            NativeDriverInner::WindowsPen(_) => {
                Err(io::Error::other("Windows pen driver requires pen frames"))
            }
            #[cfg(target_os = "linux")]
            NativeDriverInner::Uinput(driver) => driver.release(button),
            #[cfg(target_os = "linux")]
            NativeDriverInner::UinputTablet(driver) => driver.release(button),
        }
    }
}

impl PenDriver for NativeDriver {
    type Error = io::Error;

    fn inject_pen(&mut self, input: PenInput) -> Result<(), Self::Error> {
        match &mut self.inner {
            #[cfg(target_os = "windows")]
            NativeDriverInner::WindowsPen(driver) => driver.inject_pen(input),
            #[cfg(target_os = "linux")]
            NativeDriverInner::UinputTablet(driver) => driver.inject_pen(input),
            _ => {
                let _ = input;
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "selected host driver does not support pen frames",
                ))
            }
        }
    }
}

const fn enigo_button(button: MouseButton) -> Button {
    match button {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
    }
}

#[cfg(target_os = "linux")]
pub struct UinputDriver {
    device: VirtualDevice,
    screen_width: i32,
    screen_height: i32,
    relative_scale_x: f64,
    relative_scale_y: f64,
    left_pressed: bool,
    last_position: Option<(i32, i32)>,
}

#[cfg(target_os = "linux")]
impl UinputDriver {
    fn new(screen_width: i32, screen_height: i32) -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::BTN_LEFT);
        keys.insert(KeyCode::BTN_RIGHT);
        keys.insert(KeyCode::BTN_MIDDLE);

        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        axes.insert(RelativeAxisCode::REL_X);
        axes.insert(RelativeAxisCode::REL_Y);

        let mut device = VirtualDevice::builder()?
            .name("reMouseable Virtual Mouse")
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .build()?;
        let (relative_scale_x, relative_scale_y) =
            calibrate_relative_scale(&mut device).unwrap_or((1.0, 1.0));
        eprintln!(
            "remouseable: uinput screen={screen_width}x{screen_height} relative-scale={relative_scale_x:.3}x{relative_scale_y:.3}"
        );

        Ok(Self {
            device,
            screen_width,
            screen_height,
            relative_scale_x,
            relative_scale_y,
            left_pressed: false,
            last_position: None,
        })
    }

    fn pointer_events(delta_x: i32, delta_y: i32) -> [InputEvent; 2] {
        [
            InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, delta_x),
            InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, delta_y),
        ]
    }

    fn emit_scaled_motion(&mut self, delta_x: i32, delta_y: i32) -> io::Result<()> {
        let delta_x = scale_relative_delta(delta_x, self.relative_scale_x);
        let delta_y = scale_relative_delta(delta_y, self.relative_scale_y);
        emit_relative_motion(&mut self.device, delta_x, delta_y)
    }
}

#[cfg(target_os = "linux")]
impl HostDriver for UinputDriver {
    type Error = io::Error;

    fn screen_size(&self) -> Result<(i32, i32), Self::Error> {
        Ok((self.screen_width, self.screen_height))
    }

    fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        if self.last_position.is_none() {
            let (home_x, home_y) = home_delta(self.screen_width, self.screen_height);
            self.emit_scaled_motion(home_x, home_y)?;
            self.emit_scaled_motion(x, y)?;
            self.last_position = Some((x, y));
            return Ok(());
        }

        let Some((delta_x, delta_y)) = relative_delta(&mut self.last_position, x, y) else {
            return Ok(());
        };
        if delta_x == 0 && delta_y == 0 {
            return Ok(());
        }
        self.emit_scaled_motion(delta_x, delta_y)?;
        Ok(())
    }

    fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        self.move_mouse(x, y)
    }

    fn press(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.device.emit(&[InputEvent::new(
            EventType::KEY.0,
            uinput_button(button).0,
            1,
        )])?;
        if button == MouseButton::Left {
            self.left_pressed = true;
        }
        Ok(())
    }

    fn release(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.device.emit(&[InputEvent::new(
            EventType::KEY.0,
            uinput_button(button).0,
            0,
        )])?;
        if button == MouseButton::Left {
            self.left_pressed = false;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn relative_delta(last_position: &mut Option<(i32, i32)>, x: i32, y: i32) -> Option<(i32, i32)> {
    let Some((last_x, last_y)) = *last_position else {
        *last_position = Some((x, y));
        return None;
    };
    *last_position = Some((x, y));
    Some((x - last_x, y - last_y))
}

#[cfg(target_os = "linux")]
const fn home_delta(screen_width: i32, screen_height: i32) -> (i32, i32) {
    (
        -screen_width.saturating_mul(2),
        -screen_height.saturating_mul(2),
    )
}

#[cfg(target_os = "linux")]
fn calibrate_relative_scale(device: &mut VirtualDevice) -> io::Result<(f64, f64)> {
    let Some(before) = hyprland_cursor_position()? else {
        return Ok((1.0, 1.0));
    };
    let test_delta = 400;
    emit_relative_motion(device, test_delta, test_delta)?;
    thread::sleep(Duration::from_millis(30));
    let Some(after) = hyprland_cursor_position()? else {
        return Ok((1.0, 1.0));
    };
    let scale_x = calibration_factor(test_delta, after.0 - before.0);
    let scale_y = calibration_factor(test_delta, after.1 - before.1);
    emit_relative_motion(device, -test_delta, -test_delta)?;
    Ok((scale_x, scale_y))
}

#[cfg(target_os = "linux")]
fn emit_relative_motion(device: &mut VirtualDevice, delta_x: i32, delta_y: i32) -> io::Result<()> {
    for (chunk_x, chunk_y) in motion_chunks(delta_x, delta_y) {
        device.emit(&UinputDriver::pointer_events(chunk_x, chunk_y))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn motion_chunks(delta_x: i32, delta_y: i32) -> Vec<(i32, i32)> {
    const MAX_CHUNK: i32 = 64;
    let largest_delta = delta_x.abs().max(delta_y.abs());
    let frames = ((largest_delta + MAX_CHUNK - 1) / MAX_CHUNK).max(1);
    let mut chunks = Vec::with_capacity(usize::try_from(frames).unwrap_or(1));
    let mut previous_x = 0;
    let mut previous_y = 0;
    for frame in 1..=frames {
        let current_x = scale_chunk(delta_x, frame, frames);
        let current_y = scale_chunk(delta_y, frame, frames);
        chunks.push((current_x - previous_x, current_y - previous_y));
        previous_x = current_x;
        previous_y = current_y;
    }
    chunks
}

#[cfg(target_os = "linux")]
fn scale_chunk(delta: i32, frame: i32, frames: i32) -> i32 {
    let numerator = i64::from(delta) * i64::from(frame);
    let denominator = i64::from(frames);
    i32::try_from(numerator / denominator).unwrap_or(delta)
}

#[cfg(target_os = "linux")]
fn hyprland_cursor_position() -> io::Result<Option<(i32, i32)>> {
    if !std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.eq_ignore_ascii_case("hyprland"))
    {
        return Ok(None);
    }
    let output = match Command::new("hyprctl").arg("cursorpos").output() {
        Ok(output) if output.status.success() => output.stdout,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let output = String::from_utf8_lossy(&output);
    Ok(parse_cursor_position(&output))
}

#[cfg(target_os = "linux")]
fn parse_cursor_position(position: &str) -> Option<(i32, i32)> {
    let (x, y) = position.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

#[cfg(target_os = "linux")]
fn calibration_factor(expected: i32, observed: i32) -> f64 {
    if observed == 0 {
        return 1.0;
    }
    (f64::from(expected) / f64::from(observed)).clamp(0.25, 8.0)
}

#[cfg(target_os = "linux")]
#[allow(clippy::cast_possible_truncation)]
fn scale_relative_delta(delta: i32, scale: f64) -> i32 {
    (f64::from(delta) * scale).round() as i32
}

#[cfg(target_os = "linux")]
impl Drop for UinputDriver {
    fn drop(&mut self) {
        if self.left_pressed {
            let _ = self
                .device
                .emit(&[InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.0, 0)]);
        }
    }
}

#[cfg(target_os = "linux")]
const fn uinput_button(button: MouseButton) -> KeyCode {
    match button {
        MouseButton::Left => KeyCode::BTN_LEFT,
        MouseButton::Right => KeyCode::BTN_RIGHT,
        MouseButton::Middle => KeyCode::BTN_MIDDLE,
    }
}

#[cfg(target_os = "linux")]
pub struct UinputTabletDriver {
    device: VirtualDevice,
    screen_width: i32,
    screen_height: i32,
    touching: bool,
    active_tool: Option<PenTool>,
    last_position: Option<(i32, i32)>,
}

#[cfg(target_os = "linux")]
impl UinputTabletDriver {
    fn new(screen_width: i32, screen_height: i32) -> io::Result<Self> {
        if screen_width <= 0 || screen_height <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tablet screen dimensions must be positive",
            ));
        }
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::BTN_TOOL_PEN);
        keys.insert(KeyCode::BTN_TOOL_RUBBER);
        keys.insert(KeyCode::BTN_TOUCH);
        keys.insert(KeyCode::BTN_STYLUS);

        let mut properties = AttributeSet::<PropType>::new();
        properties.insert(PropType::POINTER);

        let [
            x_axis,
            y_axis,
            pressure_axis,
            horizontal_tilt_axis,
            vertical_tilt_axis,
        ] = tablet_axes(screen_width, screen_height);

        let device = VirtualDevice::builder()?
            .name("reMouseable Virtual Tablet")
            .with_properties(&properties)?
            .with_keys(&keys)?
            .with_absolute_axis(&x_axis)?
            .with_absolute_axis(&y_axis)?
            .with_absolute_axis(&pressure_axis)?
            .with_absolute_axis(&horizontal_tilt_axis)?
            .with_absolute_axis(&vertical_tilt_axis)?
            .build()?;

        Ok(Self {
            device,
            screen_width,
            screen_height,
            touching: false,
            active_tool: None,
            last_position: None,
        })
    }

    fn position_events(x: i32, y: i32) -> [InputEvent; 3] {
        [
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 1),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, x),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, y),
        ]
    }
}

#[cfg(target_os = "linux")]
fn tablet_axes(screen_width: i32, screen_height: i32) -> [UinputAbsSetup; 5] {
    [
        UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_X,
            AbsInfo::new(0, 0, screen_width.saturating_sub(1), 0, 0, 0),
        ),
        UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_Y,
            AbsInfo::new(0, 0, screen_height.saturating_sub(1), 0, 0, 0),
        ),
        UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_PRESSURE,
            AbsInfo::new(0, 0, 1024, 0, 0, 0),
        ),
        UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_TILT_X,
            AbsInfo::new(0, -90, 90, 0, 0, 1),
        ),
        UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_TILT_Y,
            AbsInfo::new(0, -90, 90, 0, 0, 1),
        ),
    ]
}

#[cfg(target_os = "linux")]
const fn tablet_tool_key(tool: PenTool) -> KeyCode {
    match tool {
        PenTool::Tip => KeyCode::BTN_TOOL_PEN,
        PenTool::Eraser => KeyCode::BTN_TOOL_RUBBER,
    }
}

#[cfg(target_os = "linux")]
fn tablet_pen_events(
    input: PenInput,
    active_tool: Option<PenTool>,
    touching: bool,
) -> Vec<InputEvent> {
    let mut events = Vec::with_capacity(9);
    let pressure = if matches!(input.phase, PenPhase::Down | PenPhase::Contact) {
        i32::try_from(input.pressure.min(1_024)).unwrap_or(1_024)
    } else {
        0
    };

    if matches!(input.phase, PenPhase::OutOfRange) {
        if touching {
            events.push(InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOUCH.0, 0));
        }
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_PRESSURE.0,
            0,
        ));
        if let Some(tool) = active_tool {
            events.push(InputEvent::new(
                EventType::KEY.0,
                tablet_tool_key(tool).0,
                0,
            ));
        }
        return events;
    }

    if let Some(tool) = active_tool.filter(|tool| *tool != input.tool) {
        events.push(InputEvent::new(
            EventType::KEY.0,
            tablet_tool_key(tool).0,
            0,
        ));
    }
    events.push(InputEvent::new(
        EventType::KEY.0,
        tablet_tool_key(input.tool).0,
        1,
    ));
    events.push(InputEvent::new(
        EventType::ABSOLUTE.0,
        AbsoluteAxisCode::ABS_X.0,
        input.x,
    ));
    events.push(InputEvent::new(
        EventType::ABSOLUTE.0,
        AbsoluteAxisCode::ABS_Y.0,
        input.y,
    ));
    if let Some(tilt_x) = input.tilt_x {
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_TILT_X.0,
            tilt_x.clamp(-90, 90),
        ));
    }
    if let Some(tilt_y) = input.tilt_y {
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_TILT_Y.0,
            tilt_y.clamp(-90, 90),
        ));
    }
    events.push(InputEvent::new(
        EventType::ABSOLUTE.0,
        AbsoluteAxisCode::ABS_PRESSURE.0,
        pressure,
    ));
    events.push(InputEvent::new(
        EventType::KEY.0,
        KeyCode::BTN_TOUCH.0,
        i32::from(pressure > 0),
    ));
    events
}

#[cfg(target_os = "linux")]
impl PenDriver for UinputTabletDriver {
    type Error = io::Error;

    fn inject_pen(&mut self, mut input: PenInput) -> Result<(), Self::Error> {
        input.x = input.x.clamp(0, self.screen_width.saturating_sub(1));
        input.y = input.y.clamp(0, self.screen_height.saturating_sub(1));
        let events = tablet_pen_events(input, self.active_tool, self.touching);
        if !events.is_empty() {
            self.device.emit(&events)?;
        }
        if matches!(input.phase, PenPhase::OutOfRange) {
            self.active_tool = None;
            self.touching = false;
            self.last_position = None;
        } else {
            self.active_tool = Some(input.tool);
            self.touching = matches!(input.phase, PenPhase::Down | PenPhase::Contact);
            self.last_position = Some((input.x, input.y));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl HostDriver for UinputTabletDriver {
    type Error = io::Error;

    fn screen_size(&self) -> Result<(i32, i32), Self::Error> {
        Ok((self.screen_width, self.screen_height))
    }

    fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        if self.last_position == Some((x, y)) {
            return Ok(());
        }
        self.device.emit(&Self::position_events(x, y))?;
        self.last_position = Some((x, y));
        Ok(())
    }

    fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        self.move_mouse(x, y)
    }

    fn press(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if button != MouseButton::Left {
            return Ok(());
        }
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 1),
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOUCH.0, 1),
            InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_PRESSURE.0,
                1024,
            ),
        ])?;
        self.touching = true;
        Ok(())
    }

    fn release(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if button != MouseButton::Left {
            return Ok(());
        }
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOUCH.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_PRESSURE.0, 0),
        ])?;
        self.touching = false;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for UinputTabletDriver {
    fn drop(&mut self) {
        let tool = self.active_tool.unwrap_or(PenTool::Tip);
        let (x, y) = self.last_position.unwrap_or((0, 0));
        let events = tablet_pen_events(
            PenInput {
                x,
                y,
                pressure: 0,
                tilt_x: None,
                tilt_y: None,
                rotation: None,
                tool,
                phase: PenPhase::OutOfRange,
                position_changed: false,
            },
            self.active_tool,
            self.touching,
        );
        if !events.is_empty() {
            let _ = self.device.emit(&events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: u32, is_primary: bool) -> MonitorInfo {
        MonitorInfo {
            id,
            label: format!("Monitor {id}"),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary,
        }
    }

    #[cfg(target_os = "windows")]
    fn positioned_monitor(id: u32, x: i32, y: i32) -> MonitorInfo {
        MonitorInfo {
            x,
            y,
            ..monitor(id, false)
        }
    }

    #[test]
    fn monitor_selection_defaults_to_primary() {
        let monitors = [monitor(10, false), monitor(20, true)];

        assert_eq!(choose_monitor(&monitors, None).unwrap().id, 20);
    }

    #[test]
    fn monitor_selection_uses_requested_id() {
        let monitors = [monitor(10, true), monitor(20, false)];

        assert_eq!(choose_monitor(&monitors, Some(20)).unwrap().id, 20);
        assert!(choose_monitor(&monitors, Some(99)).is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn injection_origin_is_relative_to_virtual_screen_top_left() {
        let monitors = [
            positioned_monitor(10, -2560, 0),
            positioned_monitor(20, 0, 0),
        ];

        assert_eq!(injection_origin(&monitors, &monitors[0]), (0, 0));
        assert_eq!(injection_origin(&monitors, &monitors[1]), (2560, 0));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    fn pen_input(tool: PenTool, phase: PenPhase) -> PenInput {
        PenInput {
            x: 640,
            y: 400,
            pressure: 512,
            tilt_x: Some(-45),
            tilt_y: Some(30),
            rotation: None,
            tool,
            phase,
            position_changed: true,
        }
    }

    fn event_value(events: &[InputEvent], event_type: EventType, code: u16) -> Option<i32> {
        events
            .iter()
            .find(|event| event.event_type() == event_type && event.code() == code)
            .map(InputEvent::value)
    }

    #[test]
    fn tablet_contact_frame_contains_pressure_tilt_and_touch() {
        let events = tablet_pen_events(pen_input(PenTool::Tip, PenPhase::Down), None, false);

        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOOL_PEN.0),
            Some(1)
        );
        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOUCH.0),
            Some(1)
        );
        assert_eq!(
            event_value(
                &events,
                EventType::ABSOLUTE,
                AbsoluteAxisCode::ABS_PRESSURE.0
            ),
            Some(512)
        );
        assert_eq!(
            event_value(&events, EventType::ABSOLUTE, AbsoluteAxisCode::ABS_TILT_X.0),
            Some(-45)
        );
        assert_eq!(
            event_value(&events, EventType::ABSOLUTE, AbsoluteAxisCode::ABS_TILT_Y.0),
            Some(30)
        );
    }

    #[test]
    fn wayland_auto_selects_native_tablet() {
        assert_eq!(
            resolve_linux_auto_driver(Some("wayland")),
            DriverKind::UinputTablet
        );
        assert_eq!(
            resolve_linux_auto_driver(Some("Wayland")),
            DriverKind::UinputTablet
        );
        assert_eq!(resolve_linux_auto_driver(Some("x11")), DriverKind::Enigo);
    }

    #[test]
    fn tablet_axes_expose_screen_pressure_and_tilt_ranges() {
        let [x, y, pressure, tilt_x, tilt_y] = tablet_axes(1920, 1080);

        assert_eq!(x.code(), AbsoluteAxisCode::ABS_X.0);
        assert_eq!(x.absinfo().maximum(), 1919);
        assert_eq!(y.code(), AbsoluteAxisCode::ABS_Y.0);
        assert_eq!(y.absinfo().maximum(), 1079);
        assert_eq!(pressure.absinfo().maximum(), 1024);
        assert_eq!(tilt_x.absinfo().minimum(), -90);
        assert_eq!(tilt_x.absinfo().maximum(), 90);
        assert_eq!(tilt_y.absinfo().minimum(), -90);
        assert_eq!(tilt_y.absinfo().maximum(), 90);
    }

    #[test]
    fn tablet_eraser_frame_switches_tool_identity() {
        let events = tablet_pen_events(
            pen_input(PenTool::Eraser, PenPhase::Hover),
            Some(PenTool::Tip),
            false,
        );

        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOOL_PEN.0),
            Some(0)
        );
        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOOL_RUBBER.0),
            Some(1)
        );
        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOUCH.0),
            Some(0)
        );
        assert_eq!(
            event_value(
                &events,
                EventType::ABSOLUTE,
                AbsoluteAxisCode::ABS_PRESSURE.0
            ),
            Some(0)
        );
    }

    #[test]
    fn tablet_lift_ends_contact_but_keeps_tool_in_range() {
        let events = tablet_pen_events(
            pen_input(PenTool::Tip, PenPhase::Up),
            Some(PenTool::Tip),
            true,
        );

        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOOL_PEN.0),
            Some(1)
        );
        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOUCH.0),
            Some(0)
        );
        assert_eq!(
            event_value(
                &events,
                EventType::ABSOLUTE,
                AbsoluteAxisCode::ABS_PRESSURE.0
            ),
            Some(0)
        );
    }

    #[test]
    fn tablet_out_of_range_releases_touch_and_tool() {
        let events = tablet_pen_events(
            pen_input(PenTool::Eraser, PenPhase::OutOfRange),
            Some(PenTool::Eraser),
            true,
        );

        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOUCH.0),
            Some(0)
        );
        assert_eq!(
            event_value(&events, EventType::KEY, KeyCode::BTN_TOOL_RUBBER.0),
            Some(0)
        );
        assert_eq!(
            event_value(
                &events,
                EventType::ABSOLUTE,
                AbsoluteAxisCode::ABS_PRESSURE.0
            ),
            Some(0)
        );
    }

    #[test]
    fn uinput_relative_delta_uses_first_move_as_baseline() {
        let mut last_position = None;

        assert_eq!(relative_delta(&mut last_position, 100, 200), None);
        assert_eq!(
            relative_delta(&mut last_position, 110, 180),
            Some((10, -20))
        );
        assert_eq!(relative_delta(&mut last_position, 90, 210), Some((-20, 30)));
    }

    #[test]
    fn uinput_home_events_clamp_to_origin_then_move_to_target() {
        assert_eq!(home_delta(1920, 1080), (-3840, -2160));
    }

    #[test]
    fn uinput_motion_chunks_preserve_total_delta() {
        let chunks = motion_chunks(150, -90);

        assert!(chunks.iter().all(|(x, y)| x.abs() <= 64 && y.abs() <= 64));
        assert_eq!(chunks.iter().map(|(x, _)| x).sum::<i32>(), 150);
        assert_eq!(chunks.iter().map(|(_, y)| y).sum::<i32>(), -90);
    }

    #[test]
    fn uinput_motion_chunks_keep_zero_axis_zero() {
        let chunks = motion_chunks(0, 130);

        assert!(chunks.iter().all(|(x, _)| *x == 0));
        assert_eq!(chunks.iter().map(|(_, y)| y).sum::<i32>(), 130);
    }

    #[test]
    fn parses_hyprland_logical_monitor_size() {
        let monitors = r#"[{
            "width": 1920,
            "height": 1200,
            "scale": 1.50,
            "focused": true
        }]"#;

        assert_eq!(parse_hyprland_monitor_size(monitors), Some((1280, 800)));
    }

    #[test]
    fn parses_hyprland_monitor_with_nested_workspace_objects() {
        let monitors = r#"[{
            "width": 1920,
            "height": 1200,
            "activeWorkspace": { "id": 1, "name": "1" },
            "specialWorkspace": { "id": 0, "name": "" },
            "scale": 1.50,
            "focused": true
        }]"#;

        assert_eq!(parse_hyprland_monitor_size(monitors), Some((1280, 800)));
    }

    #[test]
    fn parses_hyprland_cursor_position() {
        assert_eq!(parse_cursor_position("531, 687\n"), Some((531, 687)));
    }

    #[test]
    fn calibration_factor_corrects_relative_pointer_units() {
        assert!((calibration_factor(400, 200) - 2.0).abs() < f64::EPSILON);
        assert!((calibration_factor(400, 800) - 0.5).abs() < f64::EPSILON);
        assert!((calibration_factor(400, 0) - 1.0).abs() < f64::EPSILON);
    }
}
