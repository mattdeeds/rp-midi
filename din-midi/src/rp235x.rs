//! The RP2350's UART, feeding a [`DinOut`] to the wire and handing over each
//! byte it receives.
//!
//! The UART's FIFOs are off. With them on, the receive interrupt waits for 4
//! bytes or 32 bit periods of quiet, about 1 ms at 31,250 baud, and a byte's
//! arrival time is lost; off, every byte interrupts as it lands. The one
//! switch covers both directions, so the transmitter takes a byte per
//! interrupt too: at most 3,125 a second.
//!
//! [`Din::send`] and [`Din::on_interrupt`] both touch the queue, so a `Din`
//! shared between interrupt handlers goes behind a critical-section `Mutex`.

use midi_wire::MidiMessage;
use nb::Error::{Other, WouldBlock};
use rp235x_hal::fugit::HertzU32;
use rp235x_hal::uart::{
    DataBits, Disabled, Enabled, Error, StopBits, UartConfig, UartDevice, UartPeripheral, ValidUartPinout,
};

use crate::{BAUD, DinOut};

/// A UART running DIN MIDI, with `N` bytes of send queue.
pub struct Din<D: UartDevice, P: ValidUartPinout<D>, const N: usize> {
    uart: UartPeripheral<Enabled, D, P>,
    out: DinOut<N>,
    receive_errors: u32,
}

impl<D: UartDevice, P: ValidUartPinout<D>, const N: usize> Din<D, P, N> {
    /// Sets the UART up for MIDI and enables its receive interrupt. Unmasking
    /// the UART's interrupt and calling [`on_interrupt`](Self::on_interrupt)
    /// from it is up to the caller.
    pub fn new(uart: UartPeripheral<Disabled, D, P>, peripheral_clock: HertzU32) -> Result<Self, Error> {
        let config = UartConfig::new(HertzU32::Hz(BAUD), DataBits::Eight, None, StopBits::One);
        let mut uart = uart.enable(config, peripheral_clock)?;
        uart.set_fifos(false);
        uart.enable_rx_interrupt();
        Ok(Self { uart, out: DinOut::new(), receive_errors: 0 })
    }

    /// Queues a message, and starts sending if the line is idle. False if the
    /// queue had no room.
    pub fn send(&mut self, message: MidiMessage) -> bool {
        let queued = self.out.send(message);
        self.feed();
        queued
    }

    /// Call from the UART's interrupt. Hands each byte received to `received`,
    /// and keeps the transmitter fed.
    pub fn on_interrupt(&mut self, mut received: impl FnMut(u8)) {
        let mut buffer = [0];
        loop {
            match self.uart.read_raw(&mut buffer) {
                Ok(_) => received(buffer[0]),
                Err(WouldBlock) => break,
                // A framing error or a break is noise or a cable coming
                // unplugged; an overrun means this interrupt ran too late.
                // Either way the byte is gone.
                Err(Other(_)) => self.receive_errors = self.receive_errors.saturating_add(1),
            }
        }
        self.feed();
    }

    /// Moves queued bytes into the transmitter while it has room.
    fn feed(&mut self) {
        while self.uart.uart_is_writable() {
            let Some(byte) = self.out.next_byte() else {
                // The transmit interrupt says there's room, and there will be
                // until the next send, so it stays masked until then.
                self.uart.disable_tx_interrupt();
                return;
            };
            // Can't block: the transmitter has room.
            let _ = self.uart.write_raw(&[byte]);
        }
        // The write cleared the transmit interrupt. It fires again as the
        // byte moves out of the holding register, for the next one.
        self.uart.enable_tx_interrupt();
    }

    /// Bytes waiting to be sent.
    pub fn pending(&self) -> usize {
        self.out.pending()
    }

    /// Messages refused for want of queue room.
    pub fn dropped(&self) -> u32 {
        self.out.dropped()
    }

    /// Bytes lost to framing errors, breaks or overruns.
    pub fn receive_errors(&self) -> u32 {
        self.receive_errors
    }
}
