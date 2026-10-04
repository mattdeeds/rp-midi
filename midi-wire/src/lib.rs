//! The MIDI wire format, whatever the transport.
//!
//! - [`parser`]: [`MidiParser`] turns bytes from a UART, USB or anything else
//!   into [`MidiMessage`]s.
//! - [`usb`]: unpacks USB MIDI packets into the bytes the parser takes.
//!
//! Tests run under `std`; the library itself never links it.

#![cfg_attr(not(test), no_std)]

pub mod parser;
pub mod usb;

pub use parser::{MidiMessage, MidiParser};
