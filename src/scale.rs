// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

pub const DEFAULT_TABLET_HEIGHT: i32 = 15_725;
pub const DEFAULT_TABLET_WIDTH: i32 = 20_967;

pub trait PositionScaler {
    fn scale(&self, x: i32, y: i32) -> (i32, i32);
}

#[allow(clippy::cast_possible_truncation)]
fn scale_axis(value: i32, target_size: i32, source_size: i32) -> i32 {
    // Preserve established float-to-int truncation behavior.
    (f64::from(target_size) / f64::from(source_size) * f64::from(value)) as i32
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RightPositionScaler {
    pub tablet_width: i32,
    pub tablet_height: i32,
    pub screen_width: i32,
    pub screen_height: i32,
}

impl PositionScaler for RightPositionScaler {
    fn scale(&self, x: i32, y: i32) -> (i32, i32) {
        (
            scale_axis(x, self.screen_width, self.tablet_width),
            scale_axis(y, self.screen_height, self.tablet_height),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeftPositionScaler {
    pub tablet_width: i32,
    pub tablet_height: i32,
    pub screen_width: i32,
    pub screen_height: i32,
}

impl PositionScaler for LeftPositionScaler {
    fn scale(&self, x: i32, y: i32) -> (i32, i32) {
        let x = self.tablet_width - x;
        let y = self.tablet_height - y;
        (
            scale_axis(x, self.screen_width, self.tablet_width),
            scale_axis(y, self.screen_height, self.tablet_height),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerticalPositionScaler {
    pub tablet_width: i32,
    pub tablet_height: i32,
    pub screen_width: i32,
    pub screen_height: i32,
}

impl PositionScaler for VerticalPositionScaler {
    fn scale(&self, x: i32, y: i32) -> (i32, i32) {
        let rotated_x = y;
        let rotated_y = self.tablet_width - x;
        (
            scale_axis(rotated_x, self.screen_width, self.tablet_height),
            scale_axis(rotated_y, self.screen_height, self.tablet_width),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_scaler_maps_tablet_coordinates() {
        let scaler = RightPositionScaler {
            tablet_width: 100,
            tablet_height: 200,
            screen_width: 400,
            screen_height: 200,
        };

        assert_eq!(scaler.scale(50, 100), (200, 100));
    }

    #[test]
    fn left_scaler_maps_tablet_coordinates() {
        let scaler = LeftPositionScaler {
            tablet_width: 100,
            tablet_height: 200,
            screen_width: 400,
            screen_height: 200,
        };

        assert_eq!(scaler.scale(50, 100), (200, 100));
    }

    #[test]
    fn vertical_scaler_maps_tablet_coordinates() {
        let scaler = VerticalPositionScaler {
            tablet_width: 100,
            tablet_height: 200,
            screen_width: 400,
            screen_height: 200,
        };

        assert_eq!(scaler.scale(50, 100), (200, 100));
    }

    #[test]
    fn scalers_preserve_integer_truncation() {
        let scaler = RightPositionScaler {
            tablet_width: 3,
            tablet_height: 3,
            screen_width: 10,
            screen_height: 10,
        };

        assert_eq!(scaler.scale(1, 2), (3, 6));
    }
}
