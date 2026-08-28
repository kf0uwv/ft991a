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
- **Headless network server mode** (`ft991a server ...`, Linux and Windows)
  — one process owns the physical serial port, exposed to WSJT-X (a Hamlib
  rigctld-compatible TCP listener) and/or other `radio-cat-rs`-aware
  clients (raw TCP/UDP) at the same time.
- **Remote TCP client mode** (`ft991a --server <host:port>`, Linux and
  Windows) — connects the normal control TUI to a remote `ft991a server`'s
  raw TCP listener instead of a local serial port.
- **Shared diagnostics screen** (`[D]` in the main menu) — exercises every
  command in the FT-991A command table and reports pass/fail/timeout/
  skipped with live per-command progress and per-row latency/detail.

## Platform support at a glance

| Feature | Linux | Windows |
|---|---|---|
| `--port` (local serial) | yes | yes |
| `--server <host:port>` (TCP client) | yes | yes |
| `server ...` (headless network server) | yes | yes |
| Diagnostics screen (`[D]`) | yes | yes |
| Built-in emulator | yes | no (PTY-only, Unix-specific) |

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
group, `[E]` for the `EX` settings menu, `[D]` to run the diagnostics
screen, `[L]` for settings profiles, `[Q]` to quit. Each screen shows its
own keybindings.

### Headless network server mode

```sh
ft991a server --port /dev/ttyUSB0 --rigctl-port 4532 --raw-tcp-port 7300  # Linux
ft991a.exe server --port COM4 --rigctl-port 4532 --raw-tcp-port 7300      # Windows
```

One process owns the physical serial port; `--rigctl-port` exposes a
Hamlib rigctld-compatible TCP listener (for WSJT-X's "Hamlib NET rigctl"
rig type), `--raw-tcp-port`/`--raw-udp-port` expose `radio-cat-rs`'s raw
protocols for other clients. At least one of the three is required. For a
full worked example — Windows, COM4, WSJT-X and this project's own TUI
both connected over the network at once — see
[`docs/windows-server-wsjtx-tui.md`](docs/windows-server-wsjtx-tui.md).

### Remote TCP client mode

```sh
ft991a --server 192.168.1.50:7300     # connects to a remote `ft991a server --raw-tcp-port`
```

Runs the normal control TUI against a remote server's raw TCP listener
instead of a local serial port — mutually exclusive with `--port`.
Available on both Linux and Windows.

### Packaging

```sh
./packaging/build-deb.sh          # Linux: ft991a-radio-control_<version>_amd64.deb
pwsh ./packaging/build-windows-package.ps1   # Windows: a zip in the repo root
```

Both scripts run `cargo build --release` first unless passed
`--skip-build` (Debian script only; used by the shared CI release
workflow, see `docs/adr/0005-debian-and-windows-packaging.md`).

## Architecture

```
radio/       Ft991aCommandId, FT991A_COMMAND_TABLE (91 commands),
             EX_MENU_TABLE (151 settings items), Ft991aRadio (protocol
             state machine), Radio/Ft991aExtras/CwKeying traits, and
             Ft991a<S: CatSession>, the typed controller client. Also
             wraps radio-cat-rs's shared cat-diagnostics engine behind
             Ft991aExtras::run_diagnostics_with (see docs/adr/0004).
ui/          Ratatui/crossterm terminal interface. Depends on radio only —
             including for the diagnostics screen, which never depends on
             cat-diagnostics itself (see docs/adr/0004).
emulator/    PTY-hosted FT-991A simulator with its own TUI, for testing
             without hardware. Linux/Unix-only.
server/      Headless network server mode (ft991a server ...). Linux and
             Windows — wraps radio-cat-rs's cat-rigctl, which gained a
             Windows backend in that repo's docs/adr/0006 amendment (see
             this repo's docs/adr/0003 amendment).
src/         Application wiring — the only place a concrete transport type
             is named, and the platform-specific entry point.
packaging/   Debian (.deb) and Windows (.zip) packaging scripts, consumed
             by both local use and radio-cat-rs's shared release workflow
             (see docs/adr/0005).
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
cargo install cargo-xwin --locked
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc -p ft991a
```

There is no Windows build of the emulator — validate a Windows build
against real hardware.

## License

Apache License, Version 2.0.
