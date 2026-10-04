//! The send queue.

use midi_wire::{Encoder, MidiMessage};

/// Messages waiting for the UART, as bytes.
///
/// DIN MIDI moves 3,125 bytes a second, so a chord takes milliseconds to send,
/// and a busy moment's messages wait here while the UART's transmit interrupt
/// takes a byte at a time. Messages go in whole or not at all, so the wire
/// never carries half of one.
///
/// Bytes are encoded for a UART: running status within a burst, and every
/// burst starting with a full status byte, so a receiver plugged in mid-stream
/// catches up within one burst. A refused message never touches the running
/// status, which only ever follows what was queued.
///
/// When the queue is nearly full, note-offs still get in: everything else is
/// refused once fewer than [`RESERVE`](Self::RESERVE) bytes would be left, so
/// the offs for notes already sounding fit and nothing hangs.
///
/// Real-time bytes such as clock wait their turn like everything else.
pub struct DinOut<const N: usize> {
    encoder: Encoder,
    bytes: [u8; N],
    head: usize,
    len: usize,
    dropped: u32,
}

impl<const N: usize> DinOut<N> {
    /// Bytes kept free for note-offs.
    pub const RESERVE: usize = N / 4;

    pub const fn new() -> Self {
        const { assert!(N >= 4, "room for at least one message and a note-off") };
        Self { encoder: Encoder::serial(), bytes: [0; N], head: 0, len: 0, dropped: 0 }
    }

    /// Queues a message. False if it was refused for want of room.
    pub fn send(&mut self, message: MidiMessage) -> bool {
        let mut encoder = self.encoder;
        if self.len == 0 {
            encoder.reset();
        }
        let encoded = encoder.encode(message);
        let reserve = if is_note_off(message) { 0 } else { Self::RESERVE };
        if N - self.len < encoded.len() + reserve {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        for &byte in encoded.iter() {
            self.bytes[(self.head + self.len) % N] = byte;
            self.len += 1;
        }
        self.encoder = encoder;
        true
    }

    /// The next byte for the UART, oldest first.
    pub fn next_byte(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let byte = self.bytes[self.head];
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(byte)
    }

    /// Bytes waiting.
    pub fn pending(&self) -> usize {
        self.len
    }

    /// Messages refused for want of room. Raise `N` if this climbs.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }
}

impl<const N: usize> Default for DinOut<N> {
    fn default() -> Self {
        Self::new()
    }
}

fn is_note_off(message: MidiMessage) -> bool {
    matches!(message, MidiMessage::NoteOff { .. } | MidiMessage::NoteOn { velocity: 0, .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use midi_wire::MidiParser;
    use MidiMessage::*;

    fn on(channel: u8, note: u8) -> MidiMessage {
        NoteOn { channel, note, velocity: 100 }
    }

    fn off(channel: u8, note: u8) -> MidiMessage {
        NoteOff { channel, note, velocity: 0 }
    }

    fn drain<const N: usize>(out: &mut DinOut<N>) -> Vec<u8> {
        core::iter::from_fn(|| out.next_byte()).collect()
    }

    fn parse(bytes: &[u8]) -> Vec<MidiMessage> {
        let mut parser = MidiParser::new();
        bytes.iter().filter_map(|&b| parser.push(b)).collect()
    }

    #[test]
    fn a_burst_shares_its_status_and_the_next_burst_resends_it() {
        let mut out = DinOut::<64>::new();
        assert!(out.send(on(0, 60)));
        assert!(out.send(on(0, 64)));
        assert_eq!(drain(&mut out), [0x90, 60, 100, 64, 100]);
        assert!(out.send(on(0, 67)));
        assert_eq!(drain(&mut out), [0x90, 67, 100]);
    }

    #[test]
    fn note_offs_get_in_when_nothing_else_does() {
        // 16 bytes, 4 kept for note-offs.
        let mut out = DinOut::<16>::new();
        for note in [60, 62, 64, 65, 67] {
            assert!(out.send(on(0, note)), "note {note}");
        }
        assert_eq!(out.pending(), 11);
        assert!(!out.send(on(0, 69)), "2 bytes would leave fewer than 4");
        assert!(!out.send(ControlChange { channel: 0, controller: 7, value: 100 }));
        assert!(out.send(off(0, 60)));
        assert!(out.send(off(0, 62)));
        assert_eq!(out.pending(), 15);
        assert!(!out.send(off(0, 64)), "and the reserve is used up");
        assert_eq!(out.dropped(), 3);
    }

    #[test]
    fn a_refused_message_leaves_running_status_alone() {
        let mut out = DinOut::<16>::new();
        assert!(out.send(on(1, 40)));
        for note in [60, 62, 64, 65] {
            assert!(out.send(on(0, note)));
        }
        // Refused, so the receiver never sees its status byte, and still has
        // the one for channel 1...
        assert!(!out.send(on(1, 41)));
        // ...so this note-off on channel 2 has to send its own.
        assert!(out.send(off(1, 40)));
        assert_eq!(
            parse(&drain(&mut out)),
            [on(1, 40), on(0, 60), on(0, 62), on(0, 64), on(0, 65), off(1, 40)]
        );
    }

    #[test]
    fn whatever_is_accepted_arrives_intact() {
        // Random messages and random draining, through a queue small enough
        // to refuse some: the bytes that come out parse back to exactly the
        // messages that were accepted, in order.
        let mut state = 0x2545_f491_u32;
        let mut random = move |below: u32| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state % below
        };
        let mut out = DinOut::<24>::new();
        let (mut accepted, mut sent) = (Vec::new(), Vec::new());
        let mut refused = 0;
        for _ in 0..20_000 {
            let channel = random(3) as u8;
            let note = 60 + random(5) as u8;
            let message = match random(6) {
                0 | 1 => on(channel, note),
                2 | 3 => off(channel, note),
                4 => ControlChange { channel, controller: 7, value: random(128) as u8 },
                _ => Clock,
            };
            if out.send(message) {
                accepted.push(message);
            } else {
                refused += 1;
            }
            for _ in 0..random(4) {
                sent.extend(out.next_byte());
            }
        }
        sent.extend(drain(&mut out));
        assert!(refused > 100, "only {refused} refused: the queue never filled");
        assert_eq!(out.dropped(), refused);
        assert_eq!(parse(&sent), accepted);
    }
}
