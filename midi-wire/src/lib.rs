//! The MIDI wire format, whatever the transport.
//!
//! - [`parser`]: [`MidiParser`] turns bytes from a UART, USB or anything else
//!   into [`MidiMessage`]s.
//! - [`encode`]: [`Encoder`] turns messages back into bytes, compactly for a
//!   UART or whole for USB and computer MIDI APIs.
//! - [`usb`]: unpacks USB MIDI packets into the bytes the parser takes.
//!
//! Tests run under `std`; the library itself never links it.

#![cfg_attr(not(test), no_std)]

pub mod encode;
pub mod parser;
pub mod usb;

pub use encode::{Encoded, Encoder};
pub use parser::{MidiMessage, MidiParser};
