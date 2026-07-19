# Yaesu FT-991A Radio Control

Terminal-based CAT control for the Yaesu FT-991A HF/VHF/UHF transceiver. The
**second** radio built on top of [`radio-cat-rs`](https://github.com/kf0uwv/radio-cat-rs),
a shared, radio-independent CAT engine — the first being
[`ts570d`](https://github.com/kf0uwv/ts570d) (Kenwood TS-570D/S).

## Status: full command coverage, Windows-buildable

- **`radio`** — all 91 top-level FT-991A CAT commands and 151 of 153 `EX`
  menu settings items implemented (the remaining two, "TIME ZONE" and "RADIO
  ID", have no resolvable wire encoding in the official manual). Every
  command was independently re-verified against the manual page-by-page, not
  assumed from Kenwood/TS-570D parity — several real manual inconsistencies
  were found and resolved along the way (documented in `planning/yaesu/`).
  Includes RTS/DTR-based real-time CW keying (`EX` menu 060 "PC KEYING") via
  `radio-cat-rs`'s `ModemControlLines` capability.
- **`ui`** — a ratatui/crossterm terminal interface, currently a flat
  single-screen design (built when the command surface was much smaller); a
  grouped-menu redesign proportional to the full 91-command/151-setting
  surface is in progress.
- **`emulator`** — a PTY-hosted FT-991A protocol simulator for testing
  without real hardware, mirroring `ts570d`'s emulator.
- **Windows**: `cat-transport-serial` (in `radio-cat-rs`) has a native Win32
  COM-port backend alongside the Linux io_uring path, and this application's
  entry point is platform-gated (`#[monoio::main]` on Linux, a hand-rolled
  thread-parking executor on Windows, since `monoio`/`tokio` don't exist
  there). Verified via `cargo check --target x86_64-pc-windows-gnu` —
  real cross-compilation type-checking. Runtime behavior against a physical
  Windows machine has not been validated in this environment.

See [`docs/adr/0001-second-radio-on-shared-cat-framework.md`](docs/adr/0001-second-radio-on-shared-cat-framework.md)
and [`docs/adr/0002-rts-dtr-ptt-cw-keying.md`](docs/adr/0002-rts-dtr-ptt-cw-keying.md)
for the design record, and [`docs/adr/README.md`](docs/adr/README.md) for
current repository status.

## Why this repo exists

`ts570d` was refactored so a generic, radio-independent CAT engine could be
extracted into `radio-cat-rs` and reused "unchanged" by a second radio,
providing only its own `CommandId` enum, command table, state machine,
domain types, and a `CatRadio` implementation. This repository is that
second radio, targeting the Yaesu FT-991A.

## Relationship to sibling repositories

```text
radio-cat-rs   shared library (cat-framework, cat-client, cat-transport-*, cat-server)
                    ▲                              ▲
                    │ depends on                   │ depends on
                    │                               │
                ts570d                          ft991a  (this repo)
        (Kenwood TS-570D/S, first radio)   (Yaesu FT-991A, second radio)
```

`ts570d` and `ft991a` are independent siblings — neither depends on the
other. Both depend on `radio-cat-rs` for the generic engine, transport
traits, and (where applicable) serial/TCP/UDP transport implementations.

## The FT-991A is not the TS-570D

The FT-991A is a Yaesu radio; its CAT command set, framing conventions,
parameter encodings, and response layouts are Yaesu's own and differ from
Kenwood's TS-570D command table throughout — different command codes,
different parameter widths, different response formats. Nothing in this
repo's command table was transferred from `ts570d` by assumption; every
command was derived from the official Yaesu FT-991A CAT operation reference
manual directly (see `.claude/agents/yaesu.md`).

## Building

```sh
cargo build --workspace           # Linux (native)
cargo check --target x86_64-pc-windows-gnu -p ft991a   # Windows (cross-compile check)
```

Running against real hardware:

```sh
cargo run --bin ft991a -- --port /dev/ttyUSB0 --baud 9600
```

Running against the emulator (no hardware needed):

```sh
cargo run -p emulator -- --background   # prints PTY_SLAVE=<path>
cargo run --bin ft991a -- --port <path> --baud 9600
```

## Layout

- `radio/` — `Ft991aCommandId`, `FT991A_COMMAND_TABLE`, `Ft991aRadio` (a
  `cat_framework::CatRadio` implementation), `Ft991aState`/`Ft991aEvent`,
  the `Radio`/`Ft991aExtras`/`CwKeying` traits plus FT-991A domain types, and
  `Ft991a<S: CatSession>`, the typed controller client.
- `ui/` — ratatui/crossterm terminal interface, depends on `radio` only.
- `emulator/` — PTY-hosted FT-991A simulator + optional TUI for observing
  simulated CAT traffic.
- `src/main.rs` — application wiring: the only place a concrete transport
  type is named.

No local `serial` crate — serial transport is consumed directly from
`radio-cat-rs`'s `cat-transport-serial`.

## License

Apache License, Version 2.0, matching the sibling `ts570d` project.
