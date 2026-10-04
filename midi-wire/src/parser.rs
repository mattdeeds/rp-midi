//! A byte-level MIDI parser.
//!
//! Transport-agnostic on purpose: USB and UART deliver the same bytes, so both
//! feed this. No allocation, no buffering beyond the two data bytes of the
//! message being assembled.
//!
//! Three details of the wire format are easy to get wrong, and all three are
//! tested:
//!
//! 1. **Running status.** A data byte where a status byte was expected reuses
//!    the previous *channel* status. Sequencers lean on this heavily.
//! 2. **Real-time bytes interleave.** `0xF8`..=`0xFF` may appear anywhere,
//!    including between the two data bytes of another message, and must not
//!    disturb it or clear running status.
//! 3. **System common clears running status.** `0xF0`..=`0xF7` do, real-time
//!    bytes do not.

/// A parsed MIDI message.
///
/// System common messages other than Song Position are consumed correctly but
/// not reported: nothing here acts on MTC or song select, and a variant that
/// is never matched is a variant that never gets tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiMessage {
    NoteOff { channel: u8, note: u8, velocity: u8 },
    NoteOn { channel: u8, note: u8, velocity: u8 },
    PolyPressure { channel: u8, note: u8, pressure: u8 },
    ControlChange { channel: u8, controller: u8, value: u8 },
    ProgramChange { channel: u8, program: u8 },
    ChannelPressure { channel: u8, pressure: u8 },
    /// Assembled from the 14-bit pair, centred on zero: -8192..=8191.
    PitchBend { channel: u8, value: i16 },
    /// 16th notes from the top of the song, assembled LSB first.
    SongPosition(u16),
    Clock,
    Start,
    Continue,
    Stop,
    ActiveSensing,
    SystemReset,
}

/// How many data bytes follow a status byte. Returns 0 for anything that takes
/// none, or that this parser handles separately.
fn expected_data_bytes(status: u8) -> u8 {
    match status & 0xf0 {
        0x80 | 0x90 | 0xa0 | 0xb0 | 0xe0 => 2,
        0xc0 | 0xd0 => 1,
        0xf0 => match status {
            0xf2 => 2,        // song position
            0xf1 | 0xf3 => 1, // MTC quarter frame, song select
            _ => 0,
        },
        _ => 0,
    }
}

#[derive(Clone, Debug, Default)]
pub struct MidiParser {
    /// The status byte governing incoming data bytes, or zero when there is
    /// none. **This is also what implements running status**: a completed
    /// channel message leaves it in place, so the next data byte starts another
    /// message of the same kind. A separate `running_status` field would be
    /// dead weight - every path that clears this one clears that too.
    status: u8,
    data: [u8; 2],
    data_count: u8,
    in_sysex: bool,
}

impl MidiParser {
    pub const fn new() -> Self {
        Self { status: 0, data: [0; 2], data_count: 0, in_sysex: false }
    }

    /// Feeds one byte. Returns a message on the byte that completes one.
    pub fn push(&mut self, byte: u8) -> Option<MidiMessage> {
        // Real-time bytes are single, may appear mid-message, and leave every
        // other piece of state alone. Handled before anything else for exactly
        // that reason.
        if byte >= 0xf8 {
            return match byte {
                0xf8 => Some(MidiMessage::Clock),
                0xfa => Some(MidiMessage::Start),
                0xfb => Some(MidiMessage::Continue),
                0xfc => Some(MidiMessage::Stop),
                0xfe => Some(MidiMessage::ActiveSensing),
                0xff => Some(MidiMessage::SystemReset),
                _ => None, // 0xf9 and 0xfd are undefined
            };
        }

        if byte >= 0x80 {
            self.data_count = 0;
            self.in_sysex = byte == 0xf0;
            // Every system common byte - 0xF0 through 0xF7, the terminator
            // included - cancels running status. Only a channel status, or a
            // system common that still has data bytes owing, is worth keeping.
            self.status = if byte < 0xf0 || expected_data_bytes(byte) != 0 { byte } else { 0 };
            return None;
        }

        // A data byte.
        if self.in_sysex || self.status == 0 {
            return None;
        }

        self.data[self.data_count as usize] = byte;
        self.data_count += 1;
        if self.data_count < expected_data_bytes(self.status) {
            return None;
        }

        let status = self.status;
        self.data_count = 0;
        let (a, b) = (self.data[0], self.data[1]);
        if status >= 0xf0 {
            // System common: complete, and unlike a channel status it does
            // not persist for the next data byte.
            self.status = 0;
            return (status == 0xf2).then(|| MidiMessage::SongPosition((u16::from(b) << 7) | u16::from(a)));
        }

        let channel = status & 0x0f;
        Some(match status & 0xf0 {
            0x80 => MidiMessage::NoteOff { channel, note: a, velocity: b },
            // Velocity zero is the near-universal note-off idiom, and is
            // reported as one so callers cannot forget to handle it.
            0x90 if b == 0 => MidiMessage::NoteOff { channel, note: a, velocity: 0 },
            0x90 => MidiMessage::NoteOn { channel, note: a, velocity: b },
            0xa0 => MidiMessage::PolyPressure { channel, note: a, pressure: b },
            0xb0 => MidiMessage::ControlChange { channel, controller: a, value: b },
            0xc0 => MidiMessage::ProgramChange { channel, program: a },
            0xd0 => MidiMessage::ChannelPressure { channel, pressure: a },
            _ => MidiMessage::PitchBend {
                channel,
                value: (((b as i16) << 7) | a as i16) - 8192,
            },
        })
    }

    /// Drops any partially assembled message and forgets running status.
    ///
    /// For a transport that has just reconnected, where the next byte may land
    /// mid-message.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds a byte slice and collects everything that comes out.
    fn parse(bytes: &[u8]) -> Vec<MidiMessage> {
        let mut parser = MidiParser::new();
        bytes.iter().filter_map(|&b| parser.push(b)).collect()
    }

    #[test]
    fn parses_the_channel_messages() {
        assert_eq!(parse(&[0x91, 0x3c, 0x40]), [MidiMessage::NoteOn { channel: 1, note: 0x3c, velocity: 0x40 }]);
        assert_eq!(parse(&[0x82, 0x3c, 0x20]), [MidiMessage::NoteOff { channel: 2, note: 0x3c, velocity: 0x20 }]);
        assert_eq!(parse(&[0xa3, 0x3c, 0x50]), [MidiMessage::PolyPressure { channel: 3, note: 0x3c, pressure: 0x50 }]);
        assert_eq!(
            parse(&[0xb4, 0x4a, 0x7f]),
            [MidiMessage::ControlChange { channel: 4, controller: 0x4a, value: 0x7f }]
        );
        assert_eq!(parse(&[0xc5, 0x03]), [MidiMessage::ProgramChange { channel: 5, program: 3 }]);
        assert_eq!(parse(&[0xd6, 0x55]), [MidiMessage::ChannelPressure { channel: 6, pressure: 0x55 }]);
    }

    #[test]
    fn note_on_with_zero_velocity_is_a_note_off() {
        assert_eq!(parse(&[0x90, 0x3c, 0x00]), [MidiMessage::NoteOff { channel: 0, note: 0x3c, velocity: 0 }]);
    }

    #[test]
    fn pitch_bend_is_assembled_lsb_first_and_centred() {
        let cases = [
            ([0xe0, 0x00, 0x40], 0i16), // centre
            ([0xe0, 0x00, 0x00], -8192), // full down
            ([0xe0, 0x7f, 0x7f], 8191), // full up
            ([0xe0, 0x01, 0x40], 1),    // one step up: LSB is the first byte
        ];
        for (bytes, want) in cases {
            assert_eq!(parse(&bytes), [MidiMessage::PitchBend { channel: 0, value: want }], "bytes {bytes:02x?}");
        }
    }

    #[test]
    fn running_status_reuses_the_last_channel_status() {
        // Three note-ons from one status byte, which is how a sequencer packs
        // a chord.
        assert_eq!(
            parse(&[0x90, 0x3c, 0x40, 0x40, 0x41, 0x43, 0x42]),
            [
                MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x40 },
                MidiMessage::NoteOn { channel: 0, note: 0x40, velocity: 0x41 },
                MidiMessage::NoteOn { channel: 0, note: 0x43, velocity: 0x42 },
            ]
        );
    }

    #[test]
    fn real_time_bytes_interleave_without_disturbing_anything() {
        // A clock byte landing between the two data bytes of a note-on must
        // not corrupt it, and must not clear running status either.
        assert_eq!(
            parse(&[0x90, 0x3c, 0xf8, 0x40, 0xfe, 0x40, 0x41]),
            [
                MidiMessage::Clock,
                MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x40 },
                MidiMessage::ActiveSensing,
                MidiMessage::NoteOn { channel: 0, note: 0x40, velocity: 0x41 },
            ]
        );
    }

    #[test]
    fn system_common_clears_running_status() {
        // Unlike real-time. After the song select the trailing pair has no
        // status to belong to and is dropped rather than becoming a phantom
        // note.
        assert_eq!(
            parse(&[0x90, 0x3c, 0x40, 0xf3, 0x05, 0x40, 0x41]),
            [MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x40 }]
        );
    }

    #[test]
    fn sysex_is_skipped_whole() {
        assert_eq!(
            parse(&[0xf0, 0x7d, 0x01, 0x02, 0x7f, 0xf7, 0x90, 0x3c, 0x40]),
            [MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x40 }]
        );
    }

    #[test]
    fn a_stray_end_of_sysex_still_cancels_running_status() {
        assert_eq!(
            parse(&[0x90, 0x3c, 0x40, 0xf7, 0x3e, 0x40]),
            [MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x40 }]
        );
    }

    #[test]
    fn running_status_survives_a_real_time_byte_between_messages() {
        assert_eq!(
            parse(&[0x90, 0x3c, 0x40, 0xf8, 0xf8, 0x3e, 0x41]),
            [
                MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x40 },
                MidiMessage::Clock,
                MidiMessage::Clock,
                MidiMessage::NoteOn { channel: 0, note: 0x3e, velocity: 0x41 },
            ]
        );
    }

    #[test]
    fn real_time_bytes_pass_through_sysex() {
        assert_eq!(parse(&[0xf0, 0x7d, 0xf8, 0x01, 0xf7]), [MidiMessage::Clock]);
    }

    #[test]
    fn orphan_data_bytes_are_dropped() {
        // Bytes arriving mid-message after a hot plug, before any status byte.
        assert_eq!(parse(&[0x3c, 0x40, 0x41]), []);
        // And a reset mid-message drops the partial one.
        let mut parser = MidiParser::new();
        assert_eq!(parser.push(0x90), None);
        assert_eq!(parser.push(0x3c), None);
        parser.reset();
        assert_eq!(parser.push(0x40), None, "the stale data byte must not complete a note");
    }

    #[test]
    fn song_position_is_assembled_lsb_first() {
        assert_eq!(parse(&[0xf2, 0x00, 0x00]), [MidiMessage::SongPosition(0)]);
        assert_eq!(parse(&[0xf2, 0x10, 0x00]), [MidiMessage::SongPosition(16)]);
        assert_eq!(parse(&[0xf2, 0x00, 0x01]), [MidiMessage::SongPosition(128)]);
        assert_eq!(parse(&[0xf2, 0x7f, 0x7f]), [MidiMessage::SongPosition(16383)]);
        // A clock between its data bytes doesn't disturb it.
        assert_eq!(parse(&[0xf2, 0x10, 0xf8, 0x00]), [MidiMessage::Clock, MidiMessage::SongPosition(16)]);
    }

    #[test]
    fn song_position_clears_running_status() {
        assert_eq!(
            parse(&[0x90, 0x3c, 0x40, 0xf2, 0x10, 0x00, 0x3e, 0x40]),
            [MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x40 }, MidiMessage::SongPosition(16)]
        );
    }
}
