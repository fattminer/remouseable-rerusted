#![allow(unsafe_code)]

// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::{PenDriver, PenInput, PenPhase};
use std::{
    io, ptr, thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, ERROR_NOT_READY, GetLastError, POINT},
    UI::{
        Controls::{
            CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
            POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
        },
        Input::Pointer::{
            InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT,
            POINTER_FLAG_INRANGE, POINTER_FLAG_NEW, POINTER_FLAG_UP, POINTER_FLAG_UPDATE,
            POINTER_INFO, POINTER_PEN_INFO,
        },
        WindowsAndMessaging::{
            PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN,
        },
    },
};

pub struct WindowsPenDriver {
    device: HSYNTHETICPOINTERDEVICE,
    started: bool,
    contacting: bool,
    last_input: Option<PenInput>,
    last_injection: Option<Instant>,
    last_update_injection: Option<Instant>,
    update_interval: Duration,
}

impl WindowsPenDriver {
    pub fn new(update_interval_ms: u64) -> io::Result<Self> {
        if !(1..=20).contains(&update_interval_ms) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows pen update interval must be between 1 and 20 milliseconds",
            ));
        }
        let device = unsafe { CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_NONE) };
        if device.is_null() {
            return Err(last_error("creating Windows synthetic pen device"));
        }
        Ok(Self {
            device,
            started: false,
            contacting: false,
            last_input: None,
            last_injection: None,
            last_update_injection: None,
            update_interval: Duration::from_millis(update_interval_ms),
        })
    }

    fn inject(&mut self, input: PenInput) -> io::Result<()> {
        let mut input = input;
        if matches!(input.phase, PenPhase::OutOfRange) {
            if !self.started {
                return Ok(());
            }
            if self.contacting {
                let mut up = input;
                up.phase = PenPhase::Up;
                self.inject_one(up, false)?;
            }
            self.inject_one(input, false)?;
            self.started = false;
            self.contacting = false;
            return Ok(());
        }
        if !self.started {
            let mut hover = input;
            hover.phase = PenPhase::Hover;
            hover.pressure = 0;
            self.inject_one(hover, true)?;
            if matches!(input.phase, PenPhase::Hover) {
                return Ok(());
            }
        }

        if self.contacting && matches!(input.phase, PenPhase::Hover) {
            let mut up = input;
            up.phase = PenPhase::Up;
            up.pressure = 0;
            self.inject_one(up, false)?;
        }

        if !self.contacting && matches!(input.phase, PenPhase::Contact) {
            input.phase = PenPhase::Down;
        }
        if should_coalesce_update(
            input.phase,
            self.last_update_injection.map(|last| last.elapsed()),
            self.update_interval,
        ) {
            return Ok(());
        }
        self.inject_one(input, false)?;

        // Windows requires the next DOWN to transition from HOVER, but a tablet
        // can report no changed frame between lift and the next contact.
        if matches!(input.phase, PenPhase::Up) {
            let mut hover = input;
            hover.phase = PenPhase::Hover;
            hover.pressure = 0;
            self.inject_one(hover, false)?;
        }
        Ok(())
    }

    fn inject_one(&mut self, input: PenInput, first: bool) -> io::Result<()> {
        if let Some(last) = self.last_injection {
            let elapsed = last.elapsed();
            let minimum = Duration::from_millis(1);
            if let Some(wait) = minimum.checked_sub(elapsed) {
                thread::sleep(wait);
            }
        }
        let packet = build_packet(input, first);
        inject_packet(self.device, &packet).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "{error}; phase={:?} position=({}, {}) pressure={} tilt=({:?}, {:?}) previous={:?}",
                    input.phase,
                    input.x,
                    input.y,
                    input.pressure,
                    input.tilt_x,
                    input.tilt_y,
                    self.last_input.map(|previous| previous.phase)
                ),
            )
        })?;
        self.last_injection = Some(Instant::now());
        if matches!(input.phase, PenPhase::Hover | PenPhase::Contact) {
            self.last_update_injection = self.last_injection;
        }
        self.started = true;
        self.contacting = matches!(input.phase, PenPhase::Down | PenPhase::Contact);
        self.last_input = Some(input);
        Ok(())
    }
}

fn should_coalesce_update(
    phase: PenPhase,
    elapsed: Option<Duration>,
    update_interval: Duration,
) -> bool {
    matches!(phase, PenPhase::Hover | PenPhase::Contact)
        && elapsed.is_some_and(|elapsed| elapsed < update_interval)
}

fn inject_packet(device: HSYNTHETICPOINTERDEVICE, packet: &POINTER_TYPE_INFO) -> io::Result<usize> {
    const MAX_NOT_READY_RETRIES: usize = 20;
    for retry in 0..=MAX_NOT_READY_RETRIES {
        let success = unsafe { InjectSyntheticPointerInput(device, ptr::from_ref(packet), 1) };
        if success != 0 {
            return Ok(retry);
        }
        let error = unsafe { GetLastError() };
        let transient = error == ERROR_NOT_READY || error == ERROR_INVALID_PARAMETER;
        if !transient || retry == MAX_NOT_READY_RETRIES {
            return Err(windows_error(
                "injecting Windows synthetic pen input",
                error,
            ));
        }
        thread::sleep(Duration::from_millis(2));
    }
    unreachable!()
}

impl PenDriver for WindowsPenDriver {
    type Error = io::Error;

    fn inject_pen(&mut self, input: PenInput) -> Result<(), Self::Error> {
        self.inject(input)
    }
}

impl Drop for WindowsPenDriver {
    fn drop(&mut self) {
        if let Some(mut input) = self.last_input {
            if self.contacting {
                input.phase = PenPhase::Up;
                input.pressure = 0;
                let _ = self.inject(input);
            }
            let packet = build_out_of_range_packet(input);
            unsafe {
                let _ = InjectSyntheticPointerInput(self.device, &raw const packet, 1);
                DestroySyntheticPointerDevice(self.device);
            }
        } else {
            unsafe { DestroySyntheticPointerDevice(self.device) };
        }
    }
}

fn build_packet(input: PenInput, first: bool) -> POINTER_TYPE_INFO {
    let phase_flags = match input.phase {
        PenPhase::Hover | PenPhase::OutOfRange => POINTER_FLAG_UPDATE,
        PenPhase::Down => POINTER_FLAG_DOWN | POINTER_FLAG_INCONTACT,
        PenPhase::Contact => POINTER_FLAG_UPDATE | POINTER_FLAG_INCONTACT,
        PenPhase::Up => POINTER_FLAG_UP,
    };
    let mut pointer_flags = phase_flags;
    if !matches!(input.phase, PenPhase::OutOfRange) {
        pointer_flags |= POINTER_FLAG_INRANGE;
    }
    if first {
        pointer_flags |= POINTER_FLAG_NEW;
    }

    let point = POINT {
        x: input.x,
        y: input.y,
    };
    let mut pen_mask = PEN_MASK_PRESSURE;
    if input.tilt_x.is_some() {
        pen_mask |= PEN_MASK_TILT_X;
    }
    if input.tilt_y.is_some() {
        pen_mask |= PEN_MASK_TILT_Y;
    }
    if input.rotation.is_some() {
        pen_mask |= PEN_MASK_ROTATION;
    }
    let pointer_info = POINTER_INFO {
        pointerType: PT_PEN,
        pointerId: 1,
        pointerFlags: pointer_flags,
        ptPixelLocation: point,
        ..POINTER_INFO::default()
    };
    let pen_info = POINTER_PEN_INFO {
        pointerInfo: pointer_info,
        penFlags: 0,
        penMask: pen_mask,
        pressure: input.pressure,
        rotation: input.rotation.unwrap_or(0),
        tiltX: input.tilt_x.unwrap_or(0),
        tiltY: input.tilt_y.unwrap_or(0),
    };
    POINTER_TYPE_INFO {
        r#type: PT_PEN,
        Anonymous: POINTER_TYPE_INFO_0 { penInfo: pen_info },
    }
}

fn build_out_of_range_packet(input: PenInput) -> POINTER_TYPE_INFO {
    let point = POINT {
        x: input.x,
        y: input.y,
    };
    let pointer_info = POINTER_INFO {
        pointerType: PT_PEN,
        pointerId: 1,
        pointerFlags: POINTER_FLAG_UPDATE,
        ptPixelLocation: point,
        ..POINTER_INFO::default()
    };
    POINTER_TYPE_INFO {
        r#type: PT_PEN,
        Anonymous: POINTER_TYPE_INFO_0 {
            penInfo: POINTER_PEN_INFO {
                pointerInfo: pointer_info,
                ..POINTER_PEN_INFO::default()
            },
        },
    }
}

fn last_error(context: &str) -> io::Error {
    let error = unsafe { GetLastError() };
    windows_error(context, error)
}

fn windows_error(context: &str, error: u32) -> io::Error {
    io::Error::new(
        io::Error::from_raw_os_error(i32::try_from(error).unwrap_or(i32::MAX)).kind(),
        format!("{context} failed with Windows error {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_contains_pressure_tilt_and_contact_flags() {
        let packet = build_packet(
            PenInput {
                x: -100,
                y: 200,
                pressure: 512,
                tilt_x: Some(-45),
                tilt_y: Some(30),
                rotation: None,
                phase: PenPhase::Down,
                position_changed: true,
            },
            true,
        );
        let pen = unsafe { packet.Anonymous.penInfo };
        assert_eq!(pen.pointerInfo.ptPixelLocation.x, -100);
        assert_eq!(pen.pressure, 512);
        assert_eq!(pen.tiltX, -45);
        assert_eq!(pen.tiltY, 30);
        assert_eq!(pen.penMask & PEN_MASK_ROTATION, 0);
        assert_ne!(pen.penMask & PEN_MASK_PRESSURE, 0);
        assert_ne!(pen.pointerInfo.pointerFlags & POINTER_FLAG_DOWN, 0);
        assert_ne!(pen.pointerInfo.pointerFlags & POINTER_FLAG_NEW, 0);
    }

    #[test]
    fn packet_masks_optional_axes_independently() {
        let packet = build_packet(
            PenInput {
                x: 10,
                y: 20,
                pressure: 0,
                tilt_x: None,
                tilt_y: Some(15),
                rotation: None,
                phase: PenPhase::Hover,
                position_changed: false,
            },
            false,
        );
        let pen = unsafe { packet.Anonymous.penInfo };

        assert_eq!(pen.penMask & PEN_MASK_TILT_X, 0);
        assert_ne!(pen.penMask & PEN_MASK_TILT_Y, 0);
        assert_eq!(pen.penMask & PEN_MASK_ROTATION, 0);
        assert_ne!(pen.pointerInfo.pointerFlags & POINTER_FLAG_UPDATE, 0);
        assert_eq!(pen.pointerInfo.pointerFlags & POINTER_FLAG_INCONTACT, 0);
    }

    #[test]
    fn lift_packet_ends_contact() {
        let packet = build_packet(
            PenInput {
                x: 10,
                y: 20,
                pressure: 0,
                tilt_x: None,
                tilt_y: None,
                rotation: None,
                phase: PenPhase::Up,
                position_changed: false,
            },
            false,
        );
        let pen = unsafe { packet.Anonymous.penInfo };

        assert_ne!(pen.pointerInfo.pointerFlags & POINTER_FLAG_UP, 0);
        assert_eq!(pen.pointerInfo.pointerFlags & POINTER_FLAG_INCONTACT, 0);
    }

    #[test]
    fn out_of_range_packet_ends_pointer_lifetime() {
        let packet = build_packet(
            PenInput {
                x: 10,
                y: 20,
                pressure: 0,
                tilt_x: None,
                tilt_y: None,
                rotation: None,
                phase: PenPhase::OutOfRange,
                position_changed: false,
            },
            false,
        );
        let pen = unsafe { packet.Anonymous.penInfo };

        assert_ne!(pen.pointerInfo.pointerFlags & POINTER_FLAG_UPDATE, 0);
        assert_eq!(pen.pointerInfo.pointerFlags & POINTER_FLAG_INRANGE, 0);
    }

    #[test]
    fn coalesces_only_high_frequency_update_frames() {
        assert!(should_coalesce_update(
            PenPhase::Hover,
            Some(Duration::from_millis(2)),
            Duration::from_millis(5)
        ));
        assert!(should_coalesce_update(
            PenPhase::Contact,
            Some(Duration::from_millis(4)),
            Duration::from_millis(5)
        ));
        assert!(!should_coalesce_update(
            PenPhase::Contact,
            Some(Duration::from_millis(5)),
            Duration::from_millis(5)
        ));
        assert!(!should_coalesce_update(
            PenPhase::Down,
            Some(Duration::ZERO),
            Duration::from_millis(5)
        ));
        assert!(!should_coalesce_update(
            PenPhase::Up,
            Some(Duration::ZERO),
            Duration::from_millis(5)
        ));
    }
}
