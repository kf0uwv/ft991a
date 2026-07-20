# Yaesu FT-991A Radio Control

A terminal-based CAT (Computer Aided Transceiver) control application for
the Yaesu FT-991A HF/VHF/UHF transceiver, written in Rust.

## Features

- **Full command coverage** — all 91 top-level FT-991A CAT commands and 151
  of 153 `EX` menu settings items (the remaining two, "TIME ZONE" and "RADIO
  ID", have no resolvable wire encoding in the official manual). Every
  command was independently verified against the official Yaesu manual
  page-by-page; several real manual inconsistencies were found and resolved
  along the way (see `planning/yaesu/findings.md`).
- **Grouped terminal UI** — a ratatui/crossterm interface with a 12-group
  menu (frequency & levels, memory channels, clarifier/tone, keyer/CW,
  scan/VOX, attenuator/noise/AGC, speech/mic, band/step, meters, system/
  tuner/DVS) sized to the full command surface, plus two ways to reach any
  of the 151 `EX` settings items: a number-entry escape hatch and themed
  browsing.
- **Real-time CW keying** — RTS/DTR hardware-line keying (`EX` menu 060 "PC
  KEYING"), independent of CAT commands, with optimistic UI feedback and
  rollback if the underlying line assertion fails.
- **Built-in emulator** — a PTY-hosted FT-991A protocol simulator with its
  own live TUI, for developing and testing without real hardware.
- **Linux and Windows** — native io_uring serial I/O on Linux, native Win32
  COM-port I/O on Windows (see [Platform support](#platform-support)).

## Installing

Prebuilt binaries: see the [releases page](https://github.com/kf0uwv/ft991a/releases).

From source:

```sh
git clone https://github.com/kf0uwv/ft991a
cd ft991a
cargo build --release --workspace
```

## Usage

Against real hardware:

```sh
ft991a --port /dev/ttyUSB0 --baud 9600     # Linux
ft991a.exe --port COM3 --baud 9600         # Windows
```

Against the built-in emulator, no hardware needed (Linux only):

```sh
cargo run -p emulator -- --background      # prints PTY_SLAVE=<path>
cargo run --bin ft991a -- --port <path> --baud 9600
```

`--baud` accepts 4800/9600/19200/38400 (default 9600, matching the radio's
own default). `--stop-bits` accepts 1 or 2 (default 2).

Once running, press a bracketed key from the main menu to enter a command
group, `[E]` for the `EX` settings menu, `[Q]` to quit. Each screen shows its
own keybindings.

## Architecture

```
radio/       Ft991aCommandId, FT991A_COMMAND_TABLE (91 commands),
             EX_MENU_TABLE (151 settings items), Ft991aRadio (protocol
             state machine), Radio/Ft991aExtras/CwKeying traits, and
             Ft991a<S: CatSession>, the typed controller client.
ui/          Ratatui/crossterm terminal interface. Depends on radio only.
emulator/    PTY-hosted FT-991A simulator with its own TUI, for testing
             without hardware.
src/         Application wiring — the only place a concrete transport type
             is named, and the platform-specific entry point.
```

`radio` implements what a command *means* (FT-991A protocol semantics, state,
domain types). The generic mechanics of parsing and dispatching a CAT
command — independent of which radio is being controlled — along with the
serial transport itself, are consumed as external crates from
[`radio-cat-rs`](https://github.com/kf0uwv/radio-cat-rs), a shared CAT
library also used by a sibling application for a different radio. There is
no local `serial` crate in this repo.

Design decisions are recorded as ADRs in [`docs/adr/`](docs/adr/); start
with [`docs/adr/README.md`](docs/adr/README.md) for the index and current
status.

## Platform support

**Linux** — native io_uring serial I/O (`monoio`), the primary development
and testing platform. The emulator is Linux/Unix-only (PTY-based).

**Windows** — native Win32 COM-port I/O, no `monoio`/`tokio` dependency
(the application's entry point and event loop use a small hand-rolled
executor on Windows, since `monoio` requires io_uring and doesn't build
there at all). Cross-compile with:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu -p ft991a
```

There is no Windows build of the emulator — validate a Windows build
against real hardware.

## License

Apache License, Version 2.0.
