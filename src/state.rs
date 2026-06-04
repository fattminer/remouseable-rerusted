// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::event::{ABS_PRESSURE, ABS_X, ABS_Y, EV_ABS, EvdevEvent, EventSource};
use std::io;

const X_CHANGED: u8 = 0b01;
const Y_CHANGED: u8 = 0b10;
const POSITION_CHANGED: u8 = X_CHANGED | Y_CHANGED;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateChange {
    Move { x: i32, y: i32 },
    Drag { x: i32, y: i32 },
    Click,
    Unclick,
}

pub trait ChangeSource {
    /// Returns the next significant state change, or `None` when finished.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the underlying event source fails.
    fn next_change(&mut self) -> io::Result<Option<StateChange>>;
}

pub struct EvdevStateMachine<S> {
    source: S,
    pressure_threshold: i32,
    dragging: bool,
    x: i32,
    y: i32,
    position_changes: u8,
    clicked: bool,
}

impl<S> EvdevStateMachine<S> {
    pub const fn new(source: S, pressure_threshold: i32) -> Self {
        Self::with_dragging(source, pressure_threshold, false)
    }

    pub const fn with_dragging(source: S, pressure_threshold: i32, dragging: bool) -> Self {
        Self {
            source,
            pressure_threshold,
            dragging,
            x: 0,
            y: 0,
            position_changes: 0,
            clicked: false,
        }
    }

    pub fn into_inner(self) -> S {
        self.source
    }

    fn process(&mut self, event: EvdevEvent) -> Option<StateChange> {
        if event.event_type != EV_ABS {
            return None;
        }

        match event.code {
            ABS_X => {
                self.x = event.value;
                self.position_changes |= X_CHANGED;
            }
            ABS_Y => {
                self.y = event.value;
                self.position_changes |= Y_CHANGED;
            }
            ABS_PRESSURE if event.value > self.pressure_threshold && !self.clicked => {
                self.clicked = true;
                return Some(StateChange::Click);
            }
            ABS_PRESSURE if event.value < self.pressure_threshold && self.clicked => {
                self.clicked = false;
                return Some(StateChange::Unclick);
            }
            _ => {}
        }

        if self.position_changes == POSITION_CHANGED {
            self.position_changes = 0;
            if self.dragging && self.clicked {
                return Some(StateChange::Drag {
                    x: self.x,
                    y: self.y,
                });
            }
            return Some(StateChange::Move {
                x: self.x,
                y: self.y,
            });
        }

        None
    }
}

impl<S: EventSource> ChangeSource for EvdevStateMachine<S> {
    fn next_change(&mut self) -> io::Result<Option<StateChange>> {
        while let Some(event) = self.source.next_event()? {
            if let Some(change) = self.process(event) {
                return Ok(Some(change));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Events(VecDeque<EvdevEvent>);

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

    #[test]
    fn skips_irrelevant_events_and_waits_for_both_coordinates() {
        let source = Events(
            [
                event(0x11, 0, 0),
                event(EV_ABS, ABS_X, 12),
                event(EV_ABS, ABS_Y, 34),
            ]
            .into(),
        );
        let mut machine = EvdevStateMachine::new(source, 1000);

        assert_eq!(
            machine.next_change().unwrap(),
            Some(StateChange::Move { x: 12, y: 34 })
        );
        assert_eq!(machine.next_change().unwrap(), None);
    }

    #[test]
    fn emits_click_and_unclick_only_when_crossing_threshold() {
        let source = Events(
            [
                event(EV_ABS, ABS_PRESSURE, 1000),
                event(EV_ABS, ABS_PRESSURE, 2000),
                event(EV_ABS, ABS_PRESSURE, 3000),
                event(EV_ABS, ABS_PRESSURE, 1000),
                event(EV_ABS, ABS_PRESSURE, 500),
                event(EV_ABS, ABS_PRESSURE, 0),
            ]
            .into(),
        );
        let mut machine = EvdevStateMachine::new(source, 1000);

        assert_eq!(machine.next_change().unwrap(), Some(StateChange::Click));
        assert_eq!(machine.next_change().unwrap(), Some(StateChange::Unclick));
        assert_eq!(machine.next_change().unwrap(), None);
    }

    #[test]
    fn converts_movement_to_drag_while_clicked() {
        let source = Events(
            [
                event(EV_ABS, ABS_PRESSURE, 2000),
                event(EV_ABS, ABS_X, 12),
                event(EV_ABS, ABS_Y, 34),
                event(EV_ABS, ABS_PRESSURE, 500),
            ]
            .into(),
        );
        let mut machine = EvdevStateMachine::with_dragging(source, 1000, true);

        assert_eq!(machine.next_change().unwrap(), Some(StateChange::Click));
        assert_eq!(
            machine.next_change().unwrap(),
            Some(StateChange::Drag { x: 12, y: 34 })
        );
        assert_eq!(machine.next_change().unwrap(), Some(StateChange::Unclick));
    }
}
