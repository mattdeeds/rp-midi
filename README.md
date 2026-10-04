# rp-midi

`no_std` MIDI crates for RP2350 projects.

| Crate | Role |
|---|---|
| `midi-wire` | Bytes to messages, with running status, real-time bytes mid-message and SysEx handled; USB MIDI packets to bytes. `no_std`, no allocator, tested on the host. |

Planned, once a project needs them: DIN MIDI in and out over a UART, and a USB
MIDI device, both on `rp235x-hal`.

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
```
