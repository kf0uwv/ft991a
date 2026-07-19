# 2. RTS/DTR PTT and CW keying: RS-232C-only, DTR deferred

Date: 2026-07-19

## Status

Accepted

## Context

The FT-991A supports PTT and CW keying via a mechanism entirely separate
from CAT commands: `EX` menu item 060 "PC KEYING" (`OFF`/`DAKY`/`RTS`/
`DTR`) configures the radio to watch the serial connection's RTS or DTR
hardware control line directly. When set to `RTS` (or `DTR`), asserting
that line keys CW in real time — the radio reacts to the line's electrical
state, not to any byte sequence.

Two manual-inconsistent assumptions needed resolving before this could be
implemented correctly, and both are recorded here rather than left implicit
in code comments, since getting them wrong would misrepresent what this
software actually supports:

1. **The brief that kicked off this work assumed a single CAT menu item
   "048 PTT SELECT."** Reading the manual directly found this wrong: item
   048 is "AM PORT SELECT." The actual PTT-select items are four
   mode-specific entries (047 AM, 071 DATA, 076 FM-PKT, 108 SSB), each tied
   to a *different physical connector* (the rear DATA jack or USB audio,
   for soundcard-driven PTT) that this CAT-only application never opens.
   Item 060 "PC KEYING" is the only one of the PTT/keying-family menu items
   that is directly usable over the existing CAT connection.
2. **The RS-232C 9-pin CAT connector's pinout (manual p.1) has RTS (pin 7)
   and CTS (pin 8), but no DTR pin.** DTR-based keying is real (community
   ham-radio documentation, not this manual) but only reachable over a USB
   connection — the FT-991A's USB interface exposes it as a Silicon Labs
   Dual CP210x bridge presenting two virtual COM ports, one ("Enhanced")
   carrying CAT, the other ("Standard") carrying RTS/DTR-based PTT/keying.
   This detail is not manual-cited; it's treated as well-established
   community convention, not an official Yaesu specification available to
   this project.

## Decision

**Implement RTS-based PTT/CW keying over the single existing RS-232C CAT
connection. Defer DTR and USB dual-port support entirely — no code path
for either exists yet.**

Concretely:

- `radio-cat-rs` gained a new `ModemControlLines` trait (see that
  repository's [ADR 0003](https://github.com/kf0uwv/radio-cat-rs/blob/main/docs/adr/0003-modem-control-lines.md))
  exposing `set_rts`/`set_dtr`/`read_cts`/`read_dsr`/`read_dcd` as a
  transport-layer capability, additive to `Transport`/`CatSession` — this
  repo does not implement or duplicate that capability locally, consuming
  it as an external dependency like every other CAT primitive.
- `Ft991a<S>` gained a new, purely additive `impl<S> Ft991a<S> where S:
  CatSession<Error = TransportError> + ModemControlLines` block
  (`assert_rts`, `assert_dtr`, `read_cts`, `read_dsr`, `read_dcd`) — it does
  not touch or narrow the existing `impl<S: CatSession<...>> Ft991a<S>`
  block, so nothing that doesn't need modem-line control is affected.
  `SharedSession<S>` gained a matching blanket `ModemControlLines`
  delegation, mirroring its existing `CatSession` delegation shape exactly.
- **This required zero changes to `src/main.rs`.** The existing wiring
  (`Ft991a::new(SerialCatSession::new(port))`, where `port: SerialPort`)
  already satisfies the new bound, because `SerialPort: ModemControlLines`
  and `SerialCatSession<T: Transport + ModemControlLines>:
  ModemControlLines` compose automatically. `assert_rts`/`read_cts`/etc.
  became callable immediately, with no CLI flag and no second constructor —
  confirmed by building against the real dependency, not assumed.
- `assert_dtr`, `read_dsr`, and `read_dcd` exist on `Ft991a<S>` for
  completeness and forward compatibility (any future USB-based session type
  would need them), but are documented as **not reachable over this
  application's current RS-232C-only wiring** — calling them against a
  `SerialCatSession<SerialPort>` connected via RS-232C will fail at the
  `ioctl` level (no DTR line exists on that connector to control), not
  silently do nothing.
- **USB dual-port support is explicitly out of scope, not designed
  speculatively.** Supporting it would mean `Ft991a` can no longer assume
  "the CAT session's own transport is also the modem-control handle" — it
  would need a second, independent handle (a generic `M: ModemControlLines`
  type parameter, or a separately-supplied handle at construction) supplied
  apart from `S: CatSession`. That is real added complexity with no
  concrete driver: this application has no USB dual-port target to test
  against, and the RS-232C path already satisfies the feature as
  documented. Revisit if/when USB dual-port support is actually requested.

## Consequences

- A user who wires this application to the FT-991A over USB and sets Menu
  060 to `DTR` gets a CW-keying mode this software cannot drive — `EX` menu
  060 itself (once implemented; not yet landed as of this ADR) can still
  *read/report* that the radio is configured for `DTR`, but no code path in
  this repository will ever assert it. This is a known, documented
  limitation, not an oversight.
- `EX` menu items 047/048/071/072/076/077/108/109 (the mode-specific
  PTT/port-select family) are recorded in `radio`'s `EX_MENU_TABLE` as
  readable/writable CAT settings, but none of them changes what this
  application can actually *do* — it never opens the DATA jack or USB audio
  device those settings refer to. Only item 060 has real behavioral
  consequence here.
- No `ui`/`app` keybinding calls `assert_rts` yet — this ADR covers the
  `radio`-crate capability only. Exposing it as an operator-facing control
  (e.g. a CW-send key that asserts RTS for the keying duration) is separate,
  smaller follow-on work, tracked in `planning/architect/task_plan.md`.
- If USB dual-port support is ever undertaken, this ADR's "zero `main.rs`
  changes" property will not hold — that work will need a real design pass
  of its own, not a mechanical extension of what's here.
