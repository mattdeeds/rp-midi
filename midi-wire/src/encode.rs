//! Messages to bytes.
//!
//! Data bytes are kept to 7 bits and channels to 0–15, so a bad value can never
//! turn into a status byte.

use core::ops::Deref;

use crate::MidiMessage;

/// The release velocity a note-off carries when the sender has none to give.
const NO_RELEASE_VELOCITY: u8 = 0x40;

/// Turns [`MidiMessage`]s into bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Encoder {
    serial: bool,
    /// The channel status the receiver last saw in full, for running status.
    running: Option<u8>,
}

impl Encoder {
    /// For a UART: running status, and note-offs without a release velocity
    /// sent as note-ons with velocity 0, so a chord's ons and offs on one
    /// channel all share one status byte. A triad goes out in 7 bytes rather
    /// than 9.
    pub const fn serial() -> Self {
        Self { serial: true, running: None }
    }

    /// For USB and computer MIDI APIs, which want every message whole: no
    /// running status, and real note-offs.
    pub const fn packets() -> Self {
        Self { serial: false, running: None }
    }

    /// Send the next status byte in full, for when the receiver may have lost
    /// track: a cable just plugged in, say.
    pub fn reset(&mut self) {
        self.running = None;
    }

    pub fn encode(&mut self, message: MidiMessage) -> Encoded {
        use MidiMessage::*;
        let (status, data, len) = match message {
            NoteOff { channel, note, velocity }
                if self.serial && (velocity == 0 || velocity == NO_RELEASE_VELOCITY) =>
            {
                (0x90 | channel & 0x0f, [note, 0], 2)
            }
            NoteOff { channel, note, velocity } => (0x80 | channel & 0x0f, [note, velocity], 2),
            NoteOn { channel, note, velocity } => (0x90 | channel & 0x0f, [note, velocity], 2),
            PolyPressure { channel, note, pressure } => (0xa0 | channel & 0x0f, [note, pressure], 2),
            ControlChange { channel, controller, value } => (0xb0 | channel & 0x0f, [controller, value], 2),
            ProgramChange { channel, program } => (0xc0 | channel & 0x0f, [program, 0], 1),
            ChannelPressure { channel, pressure } => (0xd0 | channel & 0x0f, [pressure, 0], 1),
            PitchBend { channel, value } => {
                let value = (i32::from(value) + 8192).clamp(0, 0x3fff) as u16;
                (0xe0 | channel & 0x0f, [value as u8, (value >> 7) as u8], 2)
            }
            SongPosition(sixteenths) => (0xf2, [sixteenths as u8, (sixteenths >> 7) as u8], 2),
            // Real-time bytes stand alone and leave running status as it was.
            Clock => return Encoded::single(0xf8),
            Start => return Encoded::single(0xfa),
            Continue => return Encoded::single(0xfb),
            Stop => return Encoded::single(0xfc),
            ActiveSensing => return Encoded::single(0xfe),
            SystemReset => return Encoded::single(0xff),
        };
        let data = data.map(|byte| byte & 0x7f);
        if status >= 0xf0 {
            // System common cancels running status.
            self.running = None;
        } else if self.serial && self.running == Some(status) {
            return Encoded { bytes: [data[0], data[1], 0], len };
        } else if self.serial {
            self.running = Some(status);
        }
        Encoded { bytes: [status, data[0], data[1]], len: len + 1 }
    }
}

/// One message's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Encoded {
    bytes: [u8; 3],
    len: u8,
}

impl Encoded {
    const fn single(byte: u8) -> Self {
        Self { bytes: [byte, 0, 0], len: 1 }
    }
}

impl Deref for Encoded {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MidiParser;
    use MidiMessage::*;

    fn on(channel: u8, note: u8) -> MidiMessage {
        NoteOn { channel, note, velocity: 100 }
    }

    fn off(channel: u8, note: u8) -> MidiMessage {
        NoteOff { channel, note, velocity: NO_RELEASE_VELOCITY }
    }

    fn bytes(mut encoder: Encoder, messages: &[MidiMessage]) -> Vec<u8> {
        messages.iter().flat_map(|&message| encoder.encode(message).to_vec()).collect()
    }

    fn parse(bytes: &[u8]) -> Vec<MidiMessage> {
        let mut parser = MidiParser::new();
        bytes.iter().filter_map(|&b| parser.push(b)).collect()
    }

    #[test]
    fn serial_shares_one_status_across_a_chord_and_its_release() {
        let chord = [on(0, 60), on(0, 64), on(0, 67), off(0, 60), off(0, 64), off(0, 67)];
        assert_eq!(bytes(Encoder::serial(), &chord), [0x90, 60, 100, 64, 100, 67, 100, 60, 0, 64, 0, 67, 0]);
    }

    #[test]
    fn serial_resends_the_status_when_the_channel_changes() {
        let messages = [on(0, 60), on(1, 48), on(0, 64)];
        assert_eq!(bytes(Encoder::serial(), &messages), [0x90, 60, 100, 0x91, 48, 100, 0x90, 64, 100]);
    }

    #[test]
    fn serial_keeps_a_real_release_velocity() {
        let messages = [on(0, 60), NoteOff { channel: 0, note: 60, velocity: 20 }, off(0, 64)];
        assert_eq!(bytes(Encoder::serial(), &messages), [0x90, 60, 100, 0x80, 60, 20, 0x90, 64, 0]);
    }

    #[test]
    fn reset_sends_the_next_status_in_full() {
        let mut encoder = Encoder::serial();
        assert_eq!(*encoder.encode(on(0, 60)), [0x90, 60, 100]);
        assert_eq!(*encoder.encode(on(0, 64)), [64, 100]);
        encoder.reset();
        assert_eq!(*encoder.encode(on(0, 67)), [0x90, 67, 100]);
    }

    #[test]
    fn real_time_bytes_leave_running_status_alone() {
        let messages = [on(0, 60), Clock, on(0, 64)];
        assert_eq!(bytes(Encoder::serial(), &messages), [0x90, 60, 100, 0xf8, 64, 100]);
    }

    #[test]
    fn song_position_cancels_running_status() {
        let messages = [on(0, 60), SongPosition(16), on(0, 64)];
        assert_eq!(bytes(Encoder::serial(), &messages), [0x90, 60, 100, 0xf2, 16, 0, 0x90, 64, 100]);
    }

    #[test]
    fn packets_are_whole_with_real_note_offs() {
        let messages = [on(0, 60), on(0, 64), off(0, 60)];
        assert_eq!(bytes(Encoder::packets(), &messages), [0x90, 60, 100, 0x90, 64, 100, 0x80, 60, 0x40]);
    }

    #[test]
    fn values_are_kept_to_midi_ranges() {
        let mut encoder = Encoder::packets();
        assert_eq!(*encoder.encode(NoteOn { channel: 17, note: 200, velocity: 255 }), [0x91, 200 & 0x7f, 0x7f]);
        assert_eq!(*encoder.encode(SongPosition(0xffff)), [0xf2, 0x7f, 0x7f]);
    }

    #[test]
    fn every_message_parses_back() {
        let messages = [
            on(0, 60),
            on(0, 64),
            NoteOff { channel: 0, note: 60, velocity: 20 },
            off(0, 64),
            PolyPressure { channel: 2, note: 60, pressure: 30 },
            ControlChange { channel: 3, controller: 7, value: 99 },
            Clock,
            ControlChange { channel: 3, controller: 10, value: 64 },
            ProgramChange { channel: 4, program: 5 },
            ChannelPressure { channel: 5, pressure: 6 },
            PitchBend { channel: 6, value: -8192 },
            PitchBend { channel: 6, value: 8191 },
            PitchBend { channel: 6, value: 1 },
            SongPosition(1000),
            Start,
            Continue,
            Stop,
            ActiveSensing,
            SystemReset,
        ];
        assert_eq!(parse(&bytes(Encoder::packets(), &messages)), messages);
        // Serial sends a note-off with no release velocity as a velocity-0
        // note-on, which parses back as a note-off with velocity 0.
        let serial: Vec<_> = messages
            .iter()
            .map(|&message| match message {
                NoteOff { channel, note, velocity: NO_RELEASE_VELOCITY } => NoteOff { channel, note, velocity: 0 },
                other => other,
            })
            .collect();
        assert_eq!(parse(&bytes(Encoder::serial(), &messages)), serial);
    }
}
