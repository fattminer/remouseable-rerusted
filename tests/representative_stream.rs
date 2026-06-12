// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use remouseable::app::{Config, Orientation, debug_events, process_events};
use std::io::Cursor;

fn fixture_bytes() -> Vec<u8> {
    include_str!("../fixtures/representative-events.hex")
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(str::split_whitespace)
        .map(|value| u8::from_str_radix(value, 16).expect("fixture contains valid hex bytes"))
        .collect()
}

#[test]
fn representative_stream_produces_expected_actions() {
    let mut output = Vec::new();

    process_events(
        Cursor::new(fixture_bytes()),
        &mut output,
        Config {
            orientation: Orientation::Right,
            tablet_width: 100,
            tablet_height: 100,
            screen_width: 200,
            screen_height: 200,
            pressure_threshold: 1000,
            tablet_pressure_max: 4095,
            tablet_eraser_pressure_min: 184,
            tablet_eraser_pressure_max: 2506,
            tablet_tilt_max: 9000,
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
fn representative_stream_debug_output_names_events() {
    let mut output = Vec::new();

    debug_events(Cursor::new(fixture_bytes()), &mut output).unwrap();

    let output = String::from_utf8(output).unwrap();
    assert_eq!(output.lines().count(), 6);
    assert!(output.contains(r#""eventCodeName":"ABS_X""#));
    assert!(output.contains(r#""eventCodeName":"ABS_Y""#));
    assert!(output.contains(r#""eventCodeName":"ABS_PRESSURE""#));
}
