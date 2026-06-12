// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

pub mod app;
pub mod driver;
pub mod event;
pub mod pen;
pub mod runtime;
pub mod scale;
pub mod ssh;
pub mod state;
#[cfg(target_os = "windows")]
mod windows_pen;

pub use driver::{DriverKind, MonitorInfo, NativeDriver, available_monitors};
pub use event::{
    EvdevEvent, FilteringEventSource, ReaderEventSource, SelectingEventSource, event_code_name,
    event_type_name,
};
pub use pen::{
    DEFAULT_ERASER_PRESSURE_MAX, DEFAULT_ERASER_PRESSURE_MIN, DEFAULT_TABLET_PRESSURE_MAX,
    DEFAULT_TABLET_TILT_MAX, EvdevPenFrameSource, PenCalibration, PenDriver, PenInput,
    PenOrientation, PenPhase, PenRuntime, PenRuntimeError, PenSource, PenTool,
};
pub use runtime::{HostDriver, MouseButton, Runtime, RuntimeError};
pub use scale::{
    DEFAULT_TABLET_HEIGHT, DEFAULT_TABLET_WIDTH, LeftPositionScaler, PositionScaler,
    RightPositionScaler, VerticalPositionScaler,
};
pub use state::{ChangeSource, EvdevStateMachine, StateChange};
