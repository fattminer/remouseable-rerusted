// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::{HostDriver, MouseButton};
use display_info::DisplayInfo;
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use std::io;

pub struct NativeDriver {
    enigo: Enigo,
    screen_width: i32,
    screen_height: i32,
    left_pressed: bool,
    last_position: Option<(i32, i32)>,
}

impl NativeDriver {
    /// Creates a driver targeting the primary display.
    ///
    /// # Errors
    ///
    /// Returns an error when the primary display or input connection cannot be opened.
    pub fn new() -> io::Result<Self> {
        let displays = DisplayInfo::all().map_err(io::Error::other)?;
        let display = displays
            .iter()
            .find(|display| display.is_primary)
            .or_else(|| displays.first())
            .ok_or_else(|| io::Error::other("no host displays found"))?;
        let screen_width = i32::try_from(display.width)
            .map_err(|_| io::Error::other("primary display width exceeds i32"))?;
        let screen_height = i32::try_from(display.height)
            .map_err(|_| io::Error::other("primary display height exceeds i32"))?;
        let enigo = Enigo::new(&Settings::default()).map_err(io::Error::other)?;
        Ok(Self {
            enigo,
            screen_width,
            screen_height,
            left_pressed: false,
            last_position: None,
        })
    }
}

impl HostDriver for NativeDriver {
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

impl Drop for NativeDriver {
    fn drop(&mut self) {
        if self.left_pressed {
            let _ = self.enigo.button(Button::Left, Direction::Release);
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
