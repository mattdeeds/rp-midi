//! DIN MIDI over a UART.
//!
//! - [`DinOut`]: the send queue, which a UART's transmit interrupt drains a
//!   byte at a time. Hardware-free and tested on the host.
//! - [`rp235x`] (feature `rp235x`): the RP2350 UART driver around it, which
//!   also hands over every byte received the moment it lands, for
//!   timestamping.
//!
//! Tests run under `std`; the library itself never links it.

#![cfg_attr(not(test), no_std)]

mod out;
#[cfg(feature = "rp235x")]
pub mod rp235x;

pub use out::DinOut;

/// MIDI's baud rate.
pub const BAUD: u32 = 31_250;

/// Microseconds per bit at [`BAUD`].
pub const BIT_US: u32 = 1_000_000 / BAUD;

/// Microseconds per byte on the wire: a start bit, 8 data bits and a stop bit.
pub const BYTE_US: u32 = 10 * BIT_US;
