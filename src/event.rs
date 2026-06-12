// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use std::io::{self, Read};

pub const RAW_EVENT_SIZE: usize = 16;

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const SYN_REPORT: u16 = 0x00;
pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_PRESSURE: u16 = 0x18;
pub const ABS_TILT_X: u16 = 0x1a;
pub const ABS_TILT_Y: u16 = 0x1b;
pub const BTN_TOOL_PEN: u16 = 0x140;
pub const BTN_TOOL_RUBBER: u16 = 0x141;

#[must_use]
pub const fn event_type_name(event_type: u16) -> &'static str {
    match event_type {
        EV_SYN => "EV_SYN",
        EV_KEY => "EV_KEY",
        EV_ABS => "EV_ABS",
        _ => "",
    }
}

#[must_use]
pub const fn event_code_name(event_type: u16, code: u16) -> &'static str {
    match (event_type, code) {
        (EV_SYN, SYN_REPORT) => "SYN_REPORT",
        (EV_KEY, BTN_TOOL_PEN) => "BTN_TOOL_PEN",
        (EV_KEY, BTN_TOOL_RUBBER) => "BTN_TOOL_RUBBER",
        (EV_ABS, ABS_X) => "ABS_X",
        (EV_ABS, ABS_Y) => "ABS_Y",
        (EV_ABS, ABS_PRESSURE) => "ABS_PRESSURE",
        (EV_ABS, ABS_TILT_X) => "ABS_TILT_X",
        (EV_ABS, ABS_TILT_Y) => "ABS_TILT_Y",
        _ => "",
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvdevEvent {
    pub seconds: u32,
    pub microseconds: u32,
    pub event_type: u16,
    pub code: u16,
    pub value: i32,
}

impl EvdevEvent {
    #[must_use]
    pub fn from_le_bytes(bytes: [u8; RAW_EVENT_SIZE]) -> Self {
        Self {
            seconds: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            microseconds: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            event_type: u16::from_le_bytes([bytes[8], bytes[9]]),
            code: u16::from_le_bytes([bytes[10], bytes[11]]),
            value: i32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        }
    }
}

pub trait EventSource {
    /// Returns the next decoded event, or `None` at a clean end of stream.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the source cannot provide a complete event.
    fn next_event(&mut self) -> io::Result<Option<EvdevEvent>>;
}

pub struct ReaderEventSource<R> {
    reader: R,
    finished: bool,
}

impl<R> ReaderEventSource<R> {
    pub const fn new(reader: R) -> Self {
        Self {
            reader,
            finished: false,
        }
    }

    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: Read> EventSource for ReaderEventSource<R> {
    fn next_event(&mut self) -> io::Result<Option<EvdevEvent>> {
        if self.finished {
            return Ok(None);
        }

        let mut bytes = [0_u8; RAW_EVENT_SIZE];
        match self.reader.read(&mut bytes) {
            Ok(0) => {
                self.finished = true;
                Ok(None)
            }
            Ok(length) => {
                self.reader.read_exact(&mut bytes[length..])?;
                Ok(Some(EvdevEvent::from_le_bytes(bytes)))
            }
            Err(error) => Err(error),
        }
    }
}

pub struct SelectingEventSource<S> {
    source: S,
    selected_types: Vec<u16>,
}

impl<S> SelectingEventSource<S> {
    pub fn new(source: S, selected_types: impl Into<Vec<u16>>) -> Self {
        Self {
            source,
            selected_types: selected_types.into(),
        }
    }

    pub fn into_inner(self) -> S {
        self.source
    }
}

impl<S: EventSource> EventSource for SelectingEventSource<S> {
    fn next_event(&mut self) -> io::Result<Option<EvdevEvent>> {
        while let Some(event) = self.source.next_event()? {
            if self.selected_types.contains(&event.event_type) {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
}

pub struct FilteringEventSource<S> {
    source: S,
    filtered_types: Vec<u16>,
}

impl<S> FilteringEventSource<S> {
    pub fn new(source: S, filtered_types: impl Into<Vec<u16>>) -> Self {
        Self {
            source,
            filtered_types: filtered_types.into(),
        }
    }

    pub fn into_inner(self) -> S {
        self.source
    }
}

impl<S: EventSource> EventSource for FilteringEventSource<S> {
    fn next_event(&mut self) -> io::Result<Option<EvdevEvent>> {
        while let Some(event) = self.source.next_event()? {
            if !self.filtered_types.contains(&event.event_type) {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, ErrorKind};

    struct OneByteAtATime<R>(R);

    impl<R: Read> Read for OneByteAtATime<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let length = buffer.len().min(1);
            self.0.read(&mut buffer[..length])
        }
    }

    struct CountingReader<R> {
        inner: R,
        reads: usize,
    }

    impl<R: Read> Read for CountingReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            self.inner.read(buffer)
        }
    }

    fn encode(event: EvdevEvent) -> [u8; RAW_EVENT_SIZE] {
        let mut bytes = [0_u8; RAW_EVENT_SIZE];
        bytes[0..4].copy_from_slice(&event.seconds.to_le_bytes());
        bytes[4..8].copy_from_slice(&event.microseconds.to_le_bytes());
        bytes[8..10].copy_from_slice(&event.event_type.to_le_bytes());
        bytes[10..12].copy_from_slice(&event.code.to_le_bytes());
        bytes[12..16].copy_from_slice(&event.value.to_le_bytes());
        bytes
    }

    #[test]
    fn decodes_tablet_event_layout() {
        let expected = EvdevEvent {
            seconds: 1,
            microseconds: 2,
            event_type: EV_ABS,
            code: ABS_PRESSURE,
            value: -42,
        };

        assert_eq!(EvdevEvent::from_le_bytes(encode(expected)), expected);
    }

    #[test]
    fn names_pen_frame_events() {
        assert_eq!(event_type_name(EV_SYN), "EV_SYN");
        assert_eq!(event_type_name(EV_KEY), "EV_KEY");
        assert_eq!(event_code_name(EV_SYN, SYN_REPORT), "SYN_REPORT");
        assert_eq!(event_code_name(EV_KEY, BTN_TOOL_PEN), "BTN_TOOL_PEN");
        assert_eq!(event_code_name(EV_KEY, BTN_TOOL_RUBBER), "BTN_TOOL_RUBBER");
        assert_eq!(event_code_name(EV_ABS, ABS_TILT_X), "ABS_TILT_X");
        assert_eq!(event_code_name(EV_ABS, ABS_TILT_Y), "ABS_TILT_Y");
    }

    #[test]
    fn reader_source_reads_multiple_events_and_clean_eof() {
        let first = EvdevEvent {
            event_type: EV_ABS,
            code: ABS_X,
            value: 12,
            ..EvdevEvent::default()
        };
        let second = EvdevEvent {
            event_type: EV_ABS,
            code: ABS_Y,
            value: 34,
            ..EvdevEvent::default()
        };
        let bytes = [encode(first), encode(second)].concat();
        let mut source = ReaderEventSource::new(Cursor::new(bytes));

        assert_eq!(source.next_event().unwrap(), Some(first));
        assert_eq!(source.next_event().unwrap(), Some(second));
        assert_eq!(source.next_event().unwrap(), None);
        assert_eq!(source.next_event().unwrap(), None);
    }

    #[test]
    fn reader_source_reads_complete_event_in_one_call() {
        let expected = EvdevEvent {
            event_type: EV_ABS,
            code: ABS_X,
            value: 12,
            ..EvdevEvent::default()
        };
        let reader = CountingReader {
            inner: Cursor::new(encode(expected)),
            reads: 0,
        };
        let mut source = ReaderEventSource::new(reader);

        assert_eq!(source.next_event().unwrap(), Some(expected));
        assert_eq!(source.into_inner().reads, 1);
    }

    #[test]
    fn reader_source_rejects_partial_event() {
        let mut source = ReaderEventSource::new(Cursor::new([0_u8; RAW_EVENT_SIZE - 1]));

        assert_eq!(
            source.next_event().unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn reader_source_accepts_fragmented_stream_reads() {
        let expected = EvdevEvent {
            event_type: EV_ABS,
            code: ABS_PRESSURE,
            value: 1234,
            ..EvdevEvent::default()
        };
        let reader = OneByteAtATime(Cursor::new(encode(expected)));
        let mut source = ReaderEventSource::new(reader);

        assert_eq!(source.next_event().unwrap(), Some(expected));
        assert_eq!(source.next_event().unwrap(), None);
    }

    #[test]
    fn selecting_source_keeps_selected_types() {
        let events = [
            EvdevEvent {
                event_type: 1,
                ..EvdevEvent::default()
            },
            EvdevEvent {
                event_type: EV_ABS,
                ..EvdevEvent::default()
            },
        ];
        let bytes = events.map(encode).concat();
        let reader = ReaderEventSource::new(Cursor::new(bytes));
        let mut source = SelectingEventSource::new(reader, vec![EV_ABS]);

        assert_eq!(source.next_event().unwrap(), Some(events[1]));
        assert_eq!(source.next_event().unwrap(), None);
    }

    #[test]
    fn filtering_source_removes_filtered_types() {
        let events = [
            EvdevEvent {
                event_type: EV_ABS,
                ..EvdevEvent::default()
            },
            EvdevEvent {
                event_type: 1,
                ..EvdevEvent::default()
            },
        ];
        let bytes = events.map(encode).concat();
        let reader = ReaderEventSource::new(Cursor::new(bytes));
        let mut source = FilteringEventSource::new(reader, vec![EV_ABS]);

        assert_eq!(source.next_event().unwrap(), Some(events[1]));
        assert_eq!(source.next_event().unwrap(), None);
    }
}
