#![allow(unsafe_code)]

// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::{PenDriver, PenInput, PenPhase};
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::{GetLastError, POINT},
    UI::{
        Controls::{
            CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
            POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
        },
        Input::Pointer::{
            InjectSyntheticPointerInput, POINTER_CHANGE_FIRSTBUTTON_DOWN,
            POINTER_CHANGE_FIRSTBUTTON_UP, POINTER_CHANGE_NONE, POINTER_FLAG_DOWN,
            POINTER_FLAG_FIRSTBUTTON, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE,
            POINTER_FLAG_NEW, POINTER_FLAG_PRIMARY, POINTER_FLAG_UP, POINTER_FLAG_UPDATE,
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
}

impl WindowsPenDriver {
    pub fn new() -> io::Result<Self> {
        let device = unsafe { CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_NONE) };
        if device.is_null() {
            return Err(last_error("creating Windows synthetic pen device"));
        }
        Ok(Self {
            device,
            started: false,
            contacting: false,
            last_input: None,
        })
    }

    fn inject(&mut self, input: PenInput) -> io::Result<()> {
        let packet = build_packet(input, !self.started);
        let success = unsafe { InjectSyntheticPointerInput(self.device, &raw const packet, 1) };
        if success == 0 {
            return Err(last_error("injecting Windows synthetic pen input"));
        }
        self.started = true;
        self.contacting = matches!(input.phase, PenPhase::Down | PenPhase::Contact);
        self.last_input = Some(input);
        Ok(())
    }
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
    let (phase_flags, button_change) = match input.phase {
        PenPhase::Hover => (POINTER_FLAG_UPDATE, POINTER_CHANGE_NONE),
        PenPhase::Down => (
            POINTER_FLAG_DOWN | POINTER_FLAG_INCONTACT | POINTER_FLAG_FIRSTBUTTON,
            POINTER_CHANGE_FIRSTBUTTON_DOWN,
        ),
        PenPhase::Contact => (
            POINTER_FLAG_UPDATE | POINTER_FLAG_INCONTACT | POINTER_FLAG_FIRSTBUTTON,
            POINTER_CHANGE_NONE,
        ),
        PenPhase::Up => (POINTER_FLAG_UP, POINTER_CHANGE_FIRSTBUTTON_UP),
    };
    let mut pointer_flags = POINTER_FLAG_INRANGE | POINTER_FLAG_PRIMARY | phase_flags;
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
        frameId: 0,
        pointerFlags: pointer_flags,
        sourceDevice: ptr::null_mut(),
        hwndTarget: ptr::null_mut(),
        ptPixelLocation: point,
        ptHimetricLocation: POINT::default(),
        ptPixelLocationRaw: point,
        ptHimetricLocationRaw: POINT::default(),
        dwTime: 0,
        historyCount: 1,
        InputData: 0,
        dwKeyStates: 0,
        PerformanceCount: 0,
        ButtonChangeType: button_change,
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
        pointerFlags: POINTER_FLAG_UPDATE | POINTER_FLAG_PRIMARY,
        ptPixelLocation: point,
        ptPixelLocationRaw: point,
        historyCount: 1,
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
        assert_eq!(
            pen.pointerInfo.ButtonChangeType,
            POINTER_CHANGE_FIRSTBUTTON_UP
        );
    }
}
