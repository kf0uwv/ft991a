# ADR 0007 — The console protocol, and a GUI that is not ours

Date: 2026-09-02

## Status

Accepted.

## Context

This radio's server spoke raw CAT and rigctl. `ts570d` had grown a typed
console protocol, a device picker, spectrum and audio over the wire, and a
GPU console — and none of it was reachable here.

The obvious move was to copy `ts570d/gui` across. Measuring it first
killed that idea: 1442 lines with **one** radio-specific mention in them,
and that one a demo status string. Everything the console draws it derives
from the capability document.

## Decision

**The console moved to `cat-ui-egui`; this repo supplies a window.**

`ft991a/gui` is `main.rs` (title, address, `eframe::run_native`), a
`Window` newtype whose whole body forwards to `Console::draw`, and a demo
fixture for offscreen stills. `eframe` stays out of `cat-ui-egui` — a
window and its event loop belong to a binary (radio-cat-rs ADR 0011's
seam), and keeping them apart is also what lets the still renderer draw a
console with no window at all.

**The server gained `--console-port`**, via `cat_rigctl::run_with_native`
and a `NativeRadio` impl in `server/src/console.rs`. `ServerConfig` already
carried `native_port`; nothing in `cat-rigctl` needed changing.

**The mode mapping lives in `radio::capabilities`**, beside the
declaration, because two callers need it and a radio that disagreed with
itself about `DataUsb` depending on which end of the socket asked would be
a bad afternoon.

## Consequences

### This radio's features arrive without anyone writing them

The console reads the capability document, so an FT-991A console shows
what an FT-991A is: fourteen modes against the TS-570D's eight, 151 menu
items against 52, five meters against four, a FILTER control the Kenwood
does not get, a NOTCH control, memory channels numbered **from 1**, and
**no SPECTRUM workspace at all** — because this radio declares
`SignalSupport::None`, having a scope display but no CAT command that
returns scope data.

That last one is the case worth naming. The absence is derived from the
radio's own declaration, not from an FT-991A special case in a console,
and it is why the declaration says so explicitly rather than staying
silent.

### Both listeners, at once

`--rigctl-port` and `--console-port` bind together and were verified
answering together: rigctl returning `14000000`/`USB` to a raw socket
while the native protocol identified the radio on the other port. WSJT-X
and a console are not alternatives.

### Path dependencies, temporarily

This repo pinned `radio-cat-rs` at tag `v0.3.0`, which predates every
crate this needed. A `[patch]` cannot substitute across a version
mismatch, so all thirteen shared crates are path dependencies onto the
sibling checkout. `ts570d` reaches the same end through a tagged
dependency plus a patch. Both revert when radio-cat-rs cuts a release
containing `cat-native` and `cat-ui-egui`.

### The TUI draws the same console

Same argument as the GUI, one crate over. `ts570d/ui/src/console.rs` was
1805 lines whose only radio-specific parts were an S-meter table and a
lookup parsing a mode label back into a typed mode — both questions the
capability document already answers. It moved to
`cat-ui-ratatui::console`, and this repo reaches it through a ~40-line
conversion from `Ft991aDisplay` into the shared `RadioDisplay`.

The label lookup did not move; it was deleted. Parsing "CW" back into a
mode works for exactly one radio's spelling, and this radio writes "CW-U"
and has a "DATA-U" with no Kenwood counterpart. `RadioDisplay` carries
`mode_id` beside the label now.

This repo's own command groups overlay the tab body, exactly as the
TS-570D's feature menus do, so nothing that worked before stopped working.

### Still to do: a native client mode for the TUI

`ft991a --server` speaks raw CAT, as `ts570d --server` did until recently.
Pointing it at `--console-port` instead means an adapter over
`cat_native::Client`, which is a separate piece of work.

### A pre-existing fault this made visible

`main.rs` initialises `tracing_subscriber::fmt()` writing to stdout, and
those lines land on top of the alternate screen while the console is
drawing. It predates this change — reproduced against the old layout by
stashing — and affects `ts570d` identically. Worth fixing in both;
deliberately not fixed here, because changing where two applications send
their logs is not a side effect a console port should have.
