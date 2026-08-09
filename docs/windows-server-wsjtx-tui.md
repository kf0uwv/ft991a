# Windows: run `ft991a server` on COM4, connect WSJT-X and the TUI over the network

This walks through the concrete topology of one Windows PC owning the
FT-991A's serial port and sharing it, over the network, with both WSJT-X
and this project's own TUI (`ft991a.exe` in its normal, non-server mode).
It's one specific, fully-worked case of the two general modes documented
in the root `README.md` ("Headless network server mode" / "Remote TCP
client mode") and `CLAUDE.md`.

Everything here runs on a single Windows machine unless noted — the server,
WSJT-X, and the TUI can all run on that one PC, or on separate machines on
the same LAN; the only requirement is that they can all reach the server
machine's IP address on the ports you choose.

## Why a server at all

A serial port can only be opened by one process at a time. If you want
WSJT-X *and* this project's own TUI both talking to the radio at once — the
common case, since WSJT-X doesn't show you the radio's full control surface
(EX menu, meters, etc.) — one process has to own the actual COM port and
hand out access to everyone else over the network. That's `ft991a server`.

```
FT-991A --(USB/serial)-- COM4 --[ft991a server]--+--(rigctld protocol)--> WSJT-X
                                                  |
                                                  +--(raw TCP)-----------> ft991a.exe (TUI, --server)
```

## 1. Find the radio's COM port

Plug the FT-991A into the PC via USB (or a USB-serial adapter), then open
**Device Manager → Ports (COM & LPT)**. Look for something like `Silicon
Labs CP210x USB to UART Bridge (COM4)` — the number is whatever Windows
assigned; it does not have to be `COM4`, substitute whatever you see.

Confirm the radio's own CAT baud rate (menu item **029 "232C RATE"** if
using the USB-to-serial "232C" port, or **031 "CAT RATE"** on the
front-panel menu) matches what you'll pass as `--baud` below. The FT-991A's
own default is 9600 baud, 8 data bits, 2 stop bits — this app's own
defaults (`--baud 9600 --stop-bits 2`) already match that, so you only need
`--baud`/`--stop-bits` flags if you changed the radio's own CAT rate menu
item away from the default.

## 2. Start `ft991a server`

Open a terminal (`cmd.exe` or PowerShell) and run:

```
ft991a.exe server --port COM4 --rigctl-port 4532 --raw-tcp-port 7300
```

- `--port COM4` — the port from step 1.
- `--rigctl-port 4532` — a Hamlib rigctld-compatible TCP listener, for
  WSJT-X's "Hamlib NET rigctl" rig type. `4532` is Hamlib's own
  conventional default port; any free port works.
- `--raw-tcp-port 7300` — `radio-cat-rs`'s own raw protocol, for this
  project's TUI (or any other `radio-cat-rs`-aware client). Not something
  WSJT-X or any other Hamlib-based program understands — it's a separate
  listener from `--rigctl-port`, on a separate port, for a separate kind of
  client.

Leave this window open — it prints log lines (radio connect, listener
bind, per-command activity at `RUST_LOG=debug`) and must keep running for
as long as you want either client connected. Closing it (or Ctrl+C)
releases COM4 and disconnects every client immediately.

**Windows Firewall**: the first time you run this, Windows will likely pop
up "Windows Defender Firewall has blocked some features of this app" for
`ft991a.exe`. Check **Private networks** (check **Public** too only if
your other machine reaches this PC over a network Windows classifies as
public) and click **Allow access** — otherwise no other machine on the LAN
can reach either listener, and if you're doing everything on one PC, even
`localhost` connections would still need this if a Windows Store-app-style
firewall profile is in play. If you missed the prompt, add it manually:
**Windows Defender Firewall → Advanced settings → Inbound Rules → New
Rule… → Program →** point at `ft991a.exe`.

**Find this PC's LAN IP** (only needed if WSJT-X/the TUI run on a
*different* machine — skip this if everything is on one PC and you'll use
`localhost`/`127.0.0.1`): run `ipconfig` in another terminal and note the
`IPv4 Address` under your active adapter (e.g. `192.168.1.50`).

## 3. Connect WSJT-X

In WSJT-X: **Settings → Radio tab**:

- **Rig**: `Hamlib NET rigctl`
- **Network Server**: the server PC's address and rigctl port from step 2
  — `localhost:4532` if WSJT-X runs on the same PC as the server, or
  `192.168.1.50:4532` (your actual LAN IP) if it's on a different machine.
- **PTT Method**: **CAT** — not `RTS`/`DTR`/`DTR+RTS`. This matters:
  the physical serial port's hardware control lines are owned exclusively
  by the `ft991a server` process, not exposed to WSJT-X as a network
  client, so any RTS/DTR-based PTT method in WSJT-X will silently fail to
  key the radio. CAT-based PTT (WSJT-X sending the rigctld `T` command
  over the network connection, which the server translates into the
  FT-991A's own `TX` CAT command) is the only PTT path this topology
  supports.
- **Poll Interval**: WSJT-X's default is fine; the server can be polled by
  multiple clients concurrently.

Click **Test CAT** (or just close Settings and watch the frequency display
update) to confirm the connection. Test PTT briefly with **Tune** — with a
dummy load connected, as usual — to confirm the transceiver keys.

## 4. Connect the TUI

On the same PC or a different one, run the normal `ft991a.exe`, but with
`--server` instead of `--port`, pointing at the **raw TCP port** from step
2 (`7300`, *not* the rigctl port `4532` — those two listeners speak
different protocols and are not interchangeable):

```
ft991a.exe --server localhost:7300
```

or, from a different machine on the LAN:

```
ft991a.exe --server 192.168.1.50:7300
```

This runs the full TUI — all 12 command groups, the `EX` settings menu,
the diagnostics screen — against the radio through the server, exactly as
if it had opened the COM port directly, with one disclosed exception: see
below.

**No RTS/DTR keying in this mode.** The TUI's `[K]` CW keying feature and
the RTS-line-based PTT toggle documented in `docs/adr/0002` both require
direct ownership of the serial port's hardware control lines. Since the
server process — not this TUI instance — owns COM4, those lines aren't
reachable from a `--server` client on any platform. CAT-based transmit
control (the `[F]` screen's "Toggle TX" action, which sends the FT-991A's
`TX` CAT command) still works normally, the same path WSJT-X's CAT PTT
uses in step 3.

## 5. Running both at once

Steps 3 and 4 are independent — start them in either order, run one
without the other, or run both concurrently. The server process brokers
requests from as many connected clients as you like; each client (WSJT-X
via rigctl, the TUI via raw TCP, or any other `radio-cat-rs`-aware client)
sees the current radio state, including changes made by the *other*
client — e.g. a frequency change from WSJT-X's own click-to-tune shows up
in the TUI's VFO display on its next poll cycle, and vice versa.

## Troubleshooting

- **`ft991a.exe server` exits immediately with a serial open error** —
  something else already has COM4 open (Yaesu's own SCU-17 utility, another
  `ft991a` instance, a leftover terminal program). Only one process may
  hold the port; close the other one first.
- **WSJT-X shows "Rig control error" / never connects** — check the
  firewall step above first; then confirm the rigctl port in WSJT-X's
  Network Server field matches `--rigctl-port` exactly (not the raw TCP
  port), and that the `ft991a server` console window is still running and
  hasn't logged a listener error.
- **PTT does nothing from WSJT-X** — confirm **PTT Method** is `CAT`, not
  `RTS`/`DTR`/`DTR+RTS` (see step 3).
- **The TUI's `--server` connection is refused** — same firewall check,
  and confirm you're pointing at `--raw-tcp-port`'s port, not
  `--rigctl-port`'s.
- **AF/RF gain or squelch showing `?;`/error in the TUI, or S-meter/AF/RF/
  squelch fields never updating** — this was a real protocol bug (the app
  sent an invalid wire form for those three reads against actual hardware)
  fixed in this project; make sure you're running a build that includes
  the fix (see `planning/yaesu/findings.md`'s 2026-08-08 correction entry
  and the project changelog/release notes for the version it shipped in).
- **Keyboard input in the TUI seems to trigger every action twice** — this
  was a real Windows-only bug (crossterm reports both key-down and key-up
  on Windows; a build without the fix acted on both) fixed in this
  project; update to a build that includes it.
