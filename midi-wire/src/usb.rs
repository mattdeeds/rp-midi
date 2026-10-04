//! USB MIDI 1.0 event packets, as a device's bulk OUT endpoint receives them.
//!
//! A transfer is a run of 4-byte packets. The first byte holds the cable number
//! and the code index number (CIN); the CIN says how many of the other three are
//! MIDI bytes. Those bytes go to a [`MidiParser`](super::MidiParser) like any
//! other transport's. Cable numbers are ignored: the device has one port.

/// The MIDI bytes one packet carries. The reserved CINs 0 and 1 carry none.
pub fn packet_bytes(packet: &[u8; 4]) -> &[u8] {
    let len = match packet[0] & 0x0f {
        0x0 | 0x1 => 0,
        // One-byte system common or SysEx end, and single bytes.
        0x5 | 0xf => 1,
        // Two-byte system common or SysEx end, program change, channel pressure.
        0x2 | 0x6 | 0xc | 0xd => 2,
        _ => 3,
    };
    &packet[1..1 + len]
}

/// The MIDI bytes of a whole transfer, in order. A trailing partial packet is
/// dropped.
pub fn transfer_bytes(transfer: &[u8]) -> impl Iterator<Item = u8> + '_ {
    transfer.as_chunks::<4>().0.iter().flat_map(|packet| packet_bytes(packet).iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MidiMessage, MidiParser};

    #[test]
    fn a_packet_carries_the_bytes_its_code_index_names() {
        let cases: &[([u8; 4], &[u8])] = &[
            ([0x00, 0x90, 0x3c, 0x64], &[]),                 // reserved
            ([0x01, 0x90, 0x3c, 0x64], &[]),                 // reserved
            ([0x02, 0xf3, 0x01, 0x7f], &[0xf3, 0x01]),       // song select
            ([0x03, 0xf2, 0x01, 0x02], &[0xf2, 0x01, 0x02]), // song position
            ([0x04, 0xf0, 0x7d, 0x01], &[0xf0, 0x7d, 0x01]), // SysEx starts
            ([0x05, 0xf7, 0x7f, 0x7f], &[0xf7]),             // SysEx ends in one
            ([0x06, 0x02, 0xf7, 0x7f], &[0x02, 0xf7]),       // SysEx ends in two
            ([0x07, 0x02, 0x03, 0xf7], &[0x02, 0x03, 0xf7]), // SysEx ends in three
            ([0x08, 0x80, 0x3c, 0x00], &[0x80, 0x3c, 0x00]), // note-off
            ([0x09, 0x90, 0x3c, 0x64], &[0x90, 0x3c, 0x64]), // note-on
            ([0x19, 0x90, 0x3c, 0x64], &[0x90, 0x3c, 0x64]), // note-on, cable 1
            ([0x0a, 0xa0, 0x3c, 0x10], &[0xa0, 0x3c, 0x10]), // poly pressure
            ([0x0b, 0xb0, 0x07, 0x40], &[0xb0, 0x07, 0x40]), // control change
            ([0x0c, 0xc0, 0x05, 0x7f], &[0xc0, 0x05]),       // program change
            ([0x0d, 0xd0, 0x40, 0x7f], &[0xd0, 0x40]),       // channel pressure
            ([0x0e, 0xe0, 0x00, 0x40], &[0xe0, 0x00, 0x40]), // pitch bend
            ([0x0f, 0xf8, 0x7f, 0x7f], &[0xf8]),             // single byte
        ];
        for (packet, bytes) in cases {
            assert_eq!(packet_bytes(packet), *bytes, "packet {packet:02x?}");
        }
    }

    #[test]
    fn a_transfer_parses_as_the_bytes_it_carries() {
        let transfer = [
            0x09, 0x90, 0x3c, 0x64, // note-on
            0x04, 0xf0, 0x7d, 0x3c, // SysEx, whose data bytes are not a note
            0x07, 0x64, 0x01, 0xf7, //
            0x0f, 0xf8, 0x00, 0x00, // clock
            0x03, 0xf2, 0x10, 0x00, // song position
            0x0c, 0xc2, 0x03, 0x40, // program change; 0x40 is padding, not a second one
            0x09, 0x90, // a partial packet
        ];
        let mut parser = MidiParser::new();
        let messages: Vec<_> = transfer_bytes(&transfer).filter_map(|b| parser.push(b)).collect();
        assert_eq!(
            messages,
            [
                MidiMessage::NoteOn { channel: 0, note: 0x3c, velocity: 0x64 },
                MidiMessage::Clock,
                MidiMessage::SongPosition(16),
                MidiMessage::ProgramChange { channel: 2, program: 3 },
            ]
        );
    }
}
