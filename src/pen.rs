// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::{
    PositionScaler,
    event::{
        ABS_PRESSURE, ABS_TILT_X, ABS_TILT_Y, ABS_X, ABS_Y, EV_ABS, EV_SYN, EventSource, SYN_REPORT,
    },
};
use std::{cmp::Ordering, error::Error, fmt, io};

pub const DEFAULT_TABLET_PRESSURE_MAX: i32 = 4_095;
pub const DEFAULT_TABLET_TILT_MAX: i32 = 9_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PenPhase {
    Hover,
    Down,
    Contact,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PenInput {
    pub x: i32,
    pub y: i32,
    pub pressure: u32,
    pub tilt_x: Option<i32>,
    pub tilt_y: Option<i32>,
    pub rotation: Option<u32>,
    pub phase: PenPhase,
    pub position_changed: bool,
}

pub trait PenSource {
    /// Returns the next complete pen frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying event stream cannot be read.
    fn next_pen(&mut self) -> io::Result<Option<PenInput>>;
}

pub trait PenDriver {
    type Error: Error;

    /// Injects one normalized host pen frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the host rejects the frame.
    fn inject_pen(&mut self, input: PenInput) -> Result<(), Self::Error>;
}

const FRAME_CHANGED: u8 = 1;
const POSITION_CHANGED: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PenOrientation {
    Right,
    Left,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PenCalibration {
    pub pressure_max: i32,
    pub tilt_max: i32,
    pub rotation_max: Option<i32>,
}

impl PenCalibration {
    /// Validates calibration maxima.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error when an enabled maximum is not positive.
    pub fn validate(self) -> io::Result<Self> {
        if self.pressure_max <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tablet pressure maximum must be positive",
            ));
        }
        if self.tilt_max <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tablet tilt maximum must be positive",
            ));
        }
        if self.rotation_max.is_some_and(|maximum| maximum <= 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tablet rotation maximum must be positive",
            ));
        }
        Ok(self)
    }
}

pub struct EvdevPenFrameSource<S> {
    source: S,
    pressure_threshold: i32,
    x: Option<i32>,
    y: Option<i32>,
    pressure: i32,
    tilt_x: Option<i32>,
    tilt_y: Option<i32>,
    rotation: Option<i32>,
    change_flags: u8,
    contacting: bool,
    finished: bool,
}

impl<S> EvdevPenFrameSource<S> {
    pub const fn new(source: S, pressure_threshold: i32) -> Self {
        Self {
            source,
            pressure_threshold,
            x: None,
            y: None,
            pressure: 0,
            tilt_x: None,
            tilt_y: None,
            rotation: None,
            change_flags: 0,
            contacting: false,
            finished: false,
        }
    }

    fn apply(&mut self, event_type: u16, code: u16, value: i32) {
        if event_type != EV_ABS {
            return;
        }
        match code {
            ABS_X => {
                let changed = self.x != Some(value);
                if changed {
                    self.change_flags |= FRAME_CHANGED | POSITION_CHANGED;
                }
                self.x = Some(value);
            }
            ABS_Y => {
                let changed = self.y != Some(value);
                if changed {
                    self.change_flags |= FRAME_CHANGED | POSITION_CHANGED;
                }
                self.y = Some(value);
            }
            ABS_PRESSURE => {
                if self.pressure != value {
                    self.change_flags |= FRAME_CHANGED;
                }
                self.pressure = value;
            }
            ABS_TILT_X => {
                if self.tilt_x != Some(value) {
                    self.change_flags |= FRAME_CHANGED;
                }
                self.tilt_x = Some(value);
            }
            ABS_TILT_Y => {
                if self.tilt_y != Some(value) {
                    self.change_flags |= FRAME_CHANGED;
                }
                self.tilt_y = Some(value);
            }
            _ => {}
        }
    }

    fn finish_frame(&mut self) -> Option<PenInput> {
        if self.change_flags & FRAME_CHANGED == 0 {
            return None;
        }
        let (Some(x), Some(y)) = (self.x, self.y) else {
            return None;
        };
        let next_contacting = match self.pressure.cmp(&self.pressure_threshold) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => self.contacting,
        };
        let phase = match (self.contacting, next_contacting) {
            (false, false) => PenPhase::Hover,
            (false, true) => PenPhase::Down,
            (true, true) => PenPhase::Contact,
            (true, false) => PenPhase::Up,
        };
        self.contacting = next_contacting;
        let position_changed = self.change_flags & POSITION_CHANGED != 0;
        self.change_flags = 0;
        Some(PenInput {
            x,
            y,
            pressure: self.pressure.max(0).unsigned_abs(),
            tilt_x: self.tilt_x,
            tilt_y: self.tilt_y,
            rotation: self.rotation.map(i32::unsigned_abs),
            phase,
            position_changed,
        })
    }
}

impl<S: EventSource> PenSource for EvdevPenFrameSource<S> {
    fn next_pen(&mut self) -> io::Result<Option<PenInput>> {
        if self.finished {
            return Ok(None);
        }
        while let Some(event) = self.source.next_event()? {
            if event.event_type == EV_SYN && event.code == SYN_REPORT {
                if let Some(frame) = self.finish_frame() {
                    return Ok(Some(frame));
                }
            } else {
                self.apply(event.event_type, event.code, event.value);
            }
        }
        self.finished = true;
        Ok(self.finish_frame())
    }
}

#[derive(Debug)]
pub enum PenRuntimeError<E> {
    Source(io::Error),
    Driver(E),
}

impl<E: fmt::Display> fmt::Display for PenRuntimeError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(formatter, "pen source failed: {error}"),
            Self::Driver(error) => write!(formatter, "pen driver failed: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for PenRuntimeError<E> {}

pub struct PenRuntime<S, P, D> {
    source: S,
    scaler: P,
    driver: D,
    orientation: PenOrientation,
    calibration: PenCalibration,
    screen_origin: (i32, i32),
}

impl<S, P, D> PenRuntime<S, P, D> {
    /// Creates a normalized pen runtime.
    ///
    /// # Errors
    ///
    /// Returns an error when a calibration maximum is not positive.
    pub fn new(
        source: S,
        scaler: P,
        driver: D,
        orientation: PenOrientation,
        calibration: PenCalibration,
        screen_origin: (i32, i32),
    ) -> io::Result<Self> {
        Ok(Self {
            source,
            scaler,
            driver,
            orientation,
            calibration: calibration.validate()?,
            screen_origin,
        })
    }

    pub fn into_parts(self) -> (S, P, D) {
        (self.source, self.scaler, self.driver)
    }
}

impl<S: PenSource, P: PositionScaler, D: PenDriver> PenRuntime<S, P, D> {
    /// Processes one complete pen frame.
    ///
    /// # Errors
    ///
    /// Returns an error when reading or injecting a frame fails.
    pub fn step(&mut self) -> Result<bool, PenRuntimeError<D::Error>> {
        let Some(mut input) = self.source.next_pen().map_err(PenRuntimeError::Source)? else {
            return Ok(false);
        };
        (input.x, input.y) = self.scaler.scale(input.x, input.y);
        input.x += self.screen_origin.0;
        input.y += self.screen_origin.1;
        input.pressure = normalize_u32(input.pressure, self.calibration.pressure_max, 1_024);
        (input.tilt_x, input.tilt_y) = transform_tilt(
            input.tilt_x,
            input.tilt_y,
            self.orientation,
            self.calibration.tilt_max,
        );
        input.rotation = input.rotation.and_then(|rotation| {
            self.calibration.rotation_max.map(|maximum| {
                transform_rotation(normalize_u32(rotation, maximum, 359), self.orientation)
            })
        });
        self.driver
            .inject_pen(input)
            .map_err(PenRuntimeError::Driver)?;
        Ok(true)
    }
}

fn normalize_u32(value: u32, source_max: i32, target_max: u32) -> u32 {
    let source_max = u32::try_from(source_max).unwrap_or(1);
    value.min(source_max).saturating_mul(target_max) / source_max
}

fn normalize_tilt(value: i32, maximum: i32) -> i32 {
    value.clamp(-maximum, maximum).saturating_mul(90) / maximum
}

fn transform_tilt(
    tilt_x: Option<i32>,
    tilt_y: Option<i32>,
    orientation: PenOrientation,
    maximum: i32,
) -> (Option<i32>, Option<i32>) {
    let x = tilt_x.map(|value| normalize_tilt(value, maximum));
    let y = tilt_y.map(|value| normalize_tilt(value, maximum));
    match orientation {
        PenOrientation::Right => (x, y),
        PenOrientation::Left => (x.map(i32::wrapping_neg), y.map(i32::wrapping_neg)),
        PenOrientation::Vertical => (y, x.map(i32::wrapping_neg)),
    }
}

const fn transform_rotation(rotation: u32, orientation: PenOrientation) -> u32 {
    let offset = match orientation {
        PenOrientation::Right => 0,
        PenOrientation::Left => 180,
        PenOrientation::Vertical => 90,
    };
    (rotation + offset) % 360
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EvdevEvent;
    use std::collections::VecDeque;

    struct Events(VecDeque<EvdevEvent>);

    struct IdentityScaler;

    impl PositionScaler for IdentityScaler {
        fn scale(&self, x: i32, y: i32) -> (i32, i32) {
            (x, y)
        }
    }

    #[derive(Default)]
    struct Driver(Vec<PenInput>);

    impl PenDriver for Driver {
        type Error = io::Error;

        fn inject_pen(&mut self, input: PenInput) -> Result<(), Self::Error> {
            self.0.push(input);
            Ok(())
        }
    }

    impl EventSource for Events {
        fn next_event(&mut self) -> io::Result<Option<EvdevEvent>> {
            Ok(self.0.pop_front())
        }
    }

    fn event(event_type: u16, code: u16, value: i32) -> EvdevEvent {
        EvdevEvent {
            event_type,
            code,
            value,
            ..EvdevEvent::default()
        }
    }

    fn report() -> EvdevEvent {
        event(EV_SYN, SYN_REPORT, 0)
    }

    #[test]
    fn assembles_frames_and_contact_phases() {
        let source = Events(
            [
                event(EV_ABS, ABS_X, 10),
                event(EV_ABS, ABS_Y, 20),
                event(EV_ABS, ABS_TILT_X, 100),
                report(),
                event(EV_ABS, ABS_PRESSURE, 1_001),
                report(),
                event(EV_ABS, ABS_PRESSURE, 1_000),
                report(),
                event(EV_ABS, ABS_PRESSURE, 999),
                report(),
            ]
            .into(),
        );
        let mut frames = EvdevPenFrameSource::new(source, 1_000);

        assert_eq!(frames.next_pen().unwrap().unwrap().phase, PenPhase::Hover);
        assert_eq!(frames.next_pen().unwrap().unwrap().phase, PenPhase::Down);
        assert_eq!(frames.next_pen().unwrap().unwrap().phase, PenPhase::Contact);
        assert_eq!(frames.next_pen().unwrap().unwrap().phase, PenPhase::Up);
        assert_eq!(frames.next_pen().unwrap(), None);
    }

    #[test]
    fn flushes_changed_frame_at_eof() {
        let source = Events([event(EV_ABS, ABS_X, 10), event(EV_ABS, ABS_Y, 20)].into());
        let mut frames = EvdevPenFrameSource::new(source, 1_000);

        assert!(frames.next_pen().unwrap().is_some());
        assert_eq!(frames.next_pen().unwrap(), None);
    }

    #[test]
    fn emits_pressure_and_tilt_only_frames_but_skips_unchanged_coordinates() {
        let source = Events(
            [
                event(EV_ABS, ABS_X, 10),
                event(EV_ABS, ABS_Y, 20),
                report(),
                event(EV_ABS, ABS_PRESSURE, 500),
                report(),
                event(EV_ABS, ABS_TILT_Y, -900),
                report(),
                event(EV_ABS, ABS_X, 10),
                event(EV_ABS, ABS_Y, 20),
                report(),
            ]
            .into(),
        );
        let mut frames = EvdevPenFrameSource::new(source, 1_000);

        assert!(frames.next_pen().unwrap().unwrap().position_changed);
        assert!(!frames.next_pen().unwrap().unwrap().position_changed);
        assert_eq!(frames.next_pen().unwrap().unwrap().tilt_y, Some(-900));
        assert_eq!(frames.next_pen().unwrap(), None);
    }

    #[test]
    fn normalizes_and_rotates_tilt() {
        assert_eq!(normalize_u32(2_048, 4_095, 1_024), 512);
        assert_eq!(normalize_u32(9_999, 4_095, 1_024), 1_024);
        assert_eq!(
            transform_tilt(Some(9_000), Some(-4_500), PenOrientation::Vertical, 9_000),
            (Some(-45), Some(-90))
        );
        assert_eq!(transform_rotation(300, PenOrientation::Vertical), 30);
    }

    #[test]
    fn rejects_invalid_calibration() {
        assert!(
            PenCalibration {
                pressure_max: 0,
                tilt_max: 9_000,
                rotation_max: None,
            }
            .validate()
            .is_err()
        );
        assert!(
            PenCalibration {
                pressure_max: 4_095,
                tilt_max: -1,
                rotation_max: None,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn runtime_normalizes_and_applies_screen_origin() {
        let source = Events(
            [
                event(EV_ABS, ABS_X, 10),
                event(EV_ABS, ABS_Y, 20),
                event(EV_ABS, ABS_PRESSURE, 2_048),
                event(EV_ABS, ABS_TILT_X, 9_000),
                event(EV_ABS, ABS_TILT_Y, -4_500),
                report(),
            ]
            .into(),
        );
        let frames = EvdevPenFrameSource::new(source, 1_000);
        let mut runtime = PenRuntime::new(
            frames,
            IdentityScaler,
            Driver::default(),
            PenOrientation::Right,
            PenCalibration {
                pressure_max: 4_095,
                tilt_max: 9_000,
                rotation_max: None,
            },
            (-100, 50),
        )
        .unwrap();

        assert!(runtime.step().unwrap());
        let (_, _, driver) = runtime.into_parts();
        assert_eq!(driver.0[0].x, -90);
        assert_eq!(driver.0[0].y, 70);
        assert_eq!(driver.0[0].pressure, 512);
        assert_eq!(driver.0[0].tilt_x, Some(90));
        assert_eq!(driver.0[0].tilt_y, Some(-45));
    }
}
