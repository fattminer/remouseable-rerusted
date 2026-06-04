// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::{
    EvdevStateMachine, HostDriver, LeftPositionScaler, MouseButton, PositionScaler,
    ReaderEventSource, RightPositionScaler, Runtime, RuntimeError, SelectingEventSource,
    VerticalPositionScaler,
    event::{EV_ABS, EventSource, event_code_name, event_type_name},
};
use std::{
    error::Error,
    fmt,
    io::{self, Read, Write},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Orientation {
    Right,
    Left,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub orientation: Orientation,
    pub tablet_width: i32,
    pub tablet_height: i32,
    pub screen_width: i32,
    pub screen_height: i32,
    pub pressure_threshold: i32,
    pub disable_drag_event: bool,
}

#[derive(Debug)]
pub enum AppError {
    Input(io::Error),
    Output(io::Error),
    Runtime(RuntimeError<io::Error>),
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => write!(formatter, "input stream failed: {error}"),
            Self::Output(error) => write!(formatter, "output stream failed: {error}"),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) | Self::Output(error) => Some(error),
            Self::Runtime(error) => Some(error),
        }
    }
}

enum AppScaler {
    Right(RightPositionScaler),
    Left(LeftPositionScaler),
    Vertical(VerticalPositionScaler),
}

impl AppScaler {
    const fn from_config(config: Config) -> Self {
        match config.orientation {
            Orientation::Right => Self::Right(RightPositionScaler {
                tablet_width: config.tablet_width,
                tablet_height: config.tablet_height,
                screen_width: config.screen_width,
                screen_height: config.screen_height,
            }),
            Orientation::Left => Self::Left(LeftPositionScaler {
                tablet_width: config.tablet_width,
                tablet_height: config.tablet_height,
                screen_width: config.screen_width,
                screen_height: config.screen_height,
            }),
            Orientation::Vertical => Self::Vertical(VerticalPositionScaler {
                tablet_width: config.tablet_width,
                tablet_height: config.tablet_height,
                screen_width: config.screen_width,
                screen_height: config.screen_height,
            }),
        }
    }
}

impl PositionScaler for AppScaler {
    fn scale(&self, x: i32, y: i32) -> (i32, i32) {
        match self {
            Self::Right(scaler) => scaler.scale(x, y),
            Self::Left(scaler) => scaler.scale(x, y),
            Self::Vertical(scaler) => scaler.scale(x, y),
        }
    }
}

struct JsonDriver<W> {
    output: W,
    screen_width: i32,
    screen_height: i32,
}

impl<W: Write> JsonDriver<W> {
    fn write_action(&mut self, action: &str) -> io::Result<()> {
        writeln!(self.output, "{action}")
    }
}

impl<W: Write> HostDriver for JsonDriver<W> {
    type Error = io::Error;

    fn screen_size(&self) -> Result<(i32, i32), Self::Error> {
        Ok((self.screen_width, self.screen_height))
    }

    fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        self.write_action(&format!(r#"{{"action":"move","x":{x},"y":{y}}}"#))
    }

    fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
        self.write_action(&format!(r#"{{"action":"drag","x":{x},"y":{y}}}"#))
    }

    fn press(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.write_action(&format!(
            r#"{{"action":"press","button":"{}"}}"#,
            button_name(button)
        ))
    }

    fn release(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.write_action(&format!(
            r#"{{"action":"release","button":"{}"}}"#,
            button_name(button)
        ))
    }
}

const fn button_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
    }
}

/// Writes selected raw events as JSON Lines.
///
/// # Errors
///
/// Returns an error when reading input or writing output fails.
pub fn debug_events<R: Read, W: Write>(input: R, mut output: W) -> Result<(), AppError> {
    let reader = ReaderEventSource::new(input);
    let mut events = SelectingEventSource::new(reader, vec![EV_ABS]);

    while let Some(event) = events.next_event().map_err(AppError::Input)? {
        writeln!(
            output,
            r#"{{"eventType":{},"eventTypeName":"{}","eventCode":{},"eventCodeName":"{}","eventValue":{}}}"#,
            event.event_type,
            event_type_name(event.event_type),
            event.code,
            event_code_name(event.event_type, event.code),
            event.value
        )
        .map_err(AppError::Output)?;
    }
    Ok(())
}

/// Converts a raw event stream into scaled host actions written as JSON Lines.
///
/// # Errors
///
/// Returns an error when reading input, processing events, or writing output fails.
pub fn process_events<R: Read, W: Write>(
    input: R,
    output: W,
    config: Config,
) -> Result<(), AppError> {
    let reader = ReaderEventSource::new(input);
    let events = SelectingEventSource::new(reader, vec![EV_ABS]);
    let changes = EvdevStateMachine::with_dragging(
        events,
        config.pressure_threshold,
        !config.disable_drag_event,
    );
    let scaler = AppScaler::from_config(config);
    let driver = JsonDriver {
        output,
        screen_width: config.screen_width,
        screen_height: config.screen_height,
    };
    let mut runtime = Runtime::new(changes, scaler, driver);

    while runtime.step().map_err(AppError::Runtime)? {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ABS_PRESSURE, ABS_X, ABS_Y, EvdevEvent, RAW_EVENT_SIZE};
    use std::io::Cursor;

    fn encode(events: &[EvdevEvent]) -> Vec<u8> {
        events
            .iter()
            .flat_map(|event| {
                let mut bytes = [0_u8; RAW_EVENT_SIZE];
                bytes[0..4].copy_from_slice(&event.seconds.to_le_bytes());
                bytes[4..8].copy_from_slice(&event.microseconds.to_le_bytes());
                bytes[8..10].copy_from_slice(&event.event_type.to_le_bytes());
                bytes[10..12].copy_from_slice(&event.code.to_le_bytes());
                bytes[12..16].copy_from_slice(&event.value.to_le_bytes());
                bytes
            })
            .collect()
    }

    fn event(code: u16, value: i32) -> EvdevEvent {
        EvdevEvent {
            event_type: EV_ABS,
            code,
            value,
            ..EvdevEvent::default()
        }
    }

    #[test]
    fn processes_local_stream_end_to_end() {
        let input = encode(&[
            event(ABS_X, 25),
            event(ABS_Y, 50),
            event(ABS_PRESSURE, 2000),
            event(ABS_X, 50),
            event(ABS_Y, 100),
            event(ABS_PRESSURE, 0),
        ]);
        let mut output = Vec::new();

        process_events(
            Cursor::new(input),
            &mut output,
            Config {
                orientation: Orientation::Right,
                tablet_width: 100,
                tablet_height: 100,
                screen_width: 200,
                screen_height: 200,
                pressure_threshold: 1000,
                disable_drag_event: false,
            },
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "{\"action\":\"move\",\"x\":50,\"y\":100}\n",
                "{\"action\":\"press\",\"button\":\"left\"}\n",
                "{\"action\":\"drag\",\"x\":100,\"y\":200}\n",
                "{\"action\":\"release\",\"button\":\"left\"}\n",
            )
        );
    }

    #[test]
    fn writes_debug_events() {
        let input = encode(&[event(ABS_PRESSURE, 2000)]);
        let mut output = Vec::new();

        debug_events(Cursor::new(input), &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "{\"eventType\":3,\"eventTypeName\":\"EV_ABS\",\"eventCode\":24,\"eventCodeName\":\"ABS_PRESSURE\",\"eventValue\":2000}\n"
        );
    }
}
