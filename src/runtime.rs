// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use crate::{scale::PositionScaler, state::ChangeSource, state::StateChange};
use std::{error::Error, fmt, io};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

pub trait HostDriver {
    type Error: Error;

    /// Returns target screen dimensions.
    ///
    /// # Errors
    ///
    /// Returns an error when screen dimensions cannot be queried.
    fn screen_size(&self) -> Result<(i32, i32), Self::Error>;

    /// Moves the pointer without an explicit drag event.
    ///
    /// # Errors
    ///
    /// Returns an error when the host rejects the pointer event.
    fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error>;

    /// Moves the pointer using host-specific drag semantics.
    ///
    /// # Errors
    ///
    /// Returns an error when the host rejects the drag event.
    fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error>;

    /// Presses and holds a mouse button.
    ///
    /// # Errors
    ///
    /// Returns an error when the host rejects the button event.
    fn press(&mut self, button: MouseButton) -> Result<(), Self::Error>;

    /// Releases a mouse button.
    ///
    /// # Errors
    ///
    /// Returns an error when the host rejects the button event.
    fn release(&mut self, button: MouseButton) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum RuntimeError<E> {
    Source(io::Error),
    Driver(E),
}

impl<E: fmt::Display> fmt::Display for RuntimeError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(formatter, "event source failed: {error}"),
            Self::Driver(error) => write!(formatter, "host driver failed: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for RuntimeError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Driver(error) => Some(error),
        }
    }
}

pub struct Runtime<C, P, D> {
    changes: C,
    scaler: P,
    driver: D,
}

impl<C, P, D> Runtime<C, P, D> {
    pub const fn new(changes: C, scaler: P, driver: D) -> Self {
        Self {
            changes,
            scaler,
            driver,
        }
    }

    pub fn into_parts(self) -> (C, P, D) {
        (self.changes, self.scaler, self.driver)
    }
}

impl<C, P, D> Runtime<C, P, D>
where
    C: ChangeSource,
    P: PositionScaler,
    D: HostDriver,
{
    /// Processes one state change.
    ///
    /// Returns `false` when the change source is exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error when the change source or host driver fails.
    pub fn step(&mut self) -> Result<bool, RuntimeError<D::Error>> {
        let Some(change) = self.changes.next_change().map_err(RuntimeError::Source)? else {
            return Ok(false);
        };

        match change {
            StateChange::Move { x, y } => {
                let (x, y) = self.scaler.scale(x, y);
                self.driver.move_mouse(x, y).map_err(RuntimeError::Driver)?;
            }
            StateChange::Drag { x, y } => {
                let (x, y) = self.scaler.scale(x, y);
                self.driver.drag_mouse(x, y).map_err(RuntimeError::Driver)?;
            }
            StateChange::Click => self
                .driver
                .press(MouseButton::Left)
                .map_err(RuntimeError::Driver)?,
            StateChange::Unclick => self
                .driver
                .release(MouseButton::Left)
                .map_err(RuntimeError::Driver)?,
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug, Eq, PartialEq)]
    enum Action {
        Move(i32, i32),
        Drag(i32, i32),
        Press(MouseButton),
        Release(MouseButton),
    }

    #[derive(Debug)]
    struct DriverError;

    impl fmt::Display for DriverError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("driver failure")
        }
    }

    impl Error for DriverError {}

    struct Changes(VecDeque<StateChange>);

    impl ChangeSource for Changes {
        fn next_change(&mut self) -> io::Result<Option<StateChange>> {
            Ok(self.0.pop_front())
        }
    }

    struct FailingChanges;

    impl ChangeSource for FailingChanges {
        fn next_change(&mut self) -> io::Result<Option<StateChange>> {
            Err(io::Error::other("source failure"))
        }
    }

    struct DoubleScaler;

    impl PositionScaler for DoubleScaler {
        fn scale(&self, x: i32, y: i32) -> (i32, i32) {
            (x * 2, y * 2)
        }
    }

    #[derive(Default)]
    struct Driver {
        actions: Vec<Action>,
    }

    impl HostDriver for Driver {
        type Error = DriverError;

        fn screen_size(&self) -> Result<(i32, i32), Self::Error> {
            Ok((1920, 1080))
        }

        fn move_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
            self.actions.push(Action::Move(x, y));
            Ok(())
        }

        fn drag_mouse(&mut self, x: i32, y: i32) -> Result<(), Self::Error> {
            self.actions.push(Action::Drag(x, y));
            Ok(())
        }

        fn press(&mut self, button: MouseButton) -> Result<(), Self::Error> {
            self.actions.push(Action::Press(button));
            Ok(())
        }

        fn release(&mut self, button: MouseButton) -> Result<(), Self::Error> {
            self.actions.push(Action::Release(button));
            Ok(())
        }
    }

    #[test]
    fn dispatches_all_state_changes() {
        let changes = Changes(
            [
                StateChange::Move { x: 1, y: 2 },
                StateChange::Drag { x: 3, y: 4 },
                StateChange::Click,
                StateChange::Unclick,
            ]
            .into(),
        );
        let mut runtime = Runtime::new(changes, DoubleScaler, Driver::default());

        assert!(runtime.step().unwrap());
        assert!(runtime.step().unwrap());
        assert!(runtime.step().unwrap());
        assert!(runtime.step().unwrap());
        assert!(!runtime.step().unwrap());

        let (_, _, driver) = runtime.into_parts();
        assert_eq!(
            driver.actions,
            [
                Action::Move(2, 4),
                Action::Drag(6, 8),
                Action::Press(MouseButton::Left),
                Action::Release(MouseButton::Left),
            ]
        );
    }

    #[test]
    fn propagates_source_errors() {
        let mut runtime = Runtime::new(FailingChanges, DoubleScaler, Driver::default());

        assert!(matches!(runtime.step(), Err(RuntimeError::Source(_))));
    }
}
