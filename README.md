# rp-midi

`no_std` MIDI crates for RP2350 projects.

| Crate | Role |
|---|---|
| `midi-wire` | Bytes to messages, with running status, real-time bytes mid-message and SysEx handled; messages back to bytes, compact for a UART or whole for USB; USB MIDI packets to bytes. `no_std`, no allocator, tested on the host. |
| `din-midi` | DIN MIDI over a UART. `DinOut` is the send queue the transmit interrupt drains: whole messages only, running status within a burst, and room kept for note-offs when it fills. With the `rp235x` feature, `rp235x::Din` is the RP2350 UART driver around it, with the FIFOs off so every received byte can be timestamped as it lands. |

Planned, once a project needs it: a USB MIDI device on `rp235x-hal`.

## Using it

Depend on a commit:

```toml
midi-wire = { git = "https://github.com/mattdeeds/rp-midi", rev = "<commit>" }
```

To work on this repo and a project together, point the project at a local
checkout without committing it, in the project's `.cargo/config.toml`:

```toml
[patch."https://github.com/mattdeeds/rp-midi"]
midi-wire = { path = "../rp-midi/midi-wire" }
```

## Testing

```
cargo test
cargo clippy --all-targets
cargo build --target thumbv8m.main-none-eabihf
cargo clippy -p din-midi --features rp235x --target thumbv8m.main-none-eabihf
```

`din-midi`'s `rp235x` driver can only be checked on a chip: wire a UART's TX
pin to its RX pin, and every byte sent comes back to be timed.
