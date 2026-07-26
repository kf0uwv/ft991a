# Architecture Decision Records

Decisions are recorded as [ADRs](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
(Michael Nygard format). Each file is one decision; numbers are stable and never reused.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-second-radio-on-shared-cat-framework.md) | Second radio on the shared CAT framework | Accepted |
| [0002](0002-rts-dtr-ptt-cw-keying.md) | RTS/DTR PTT and CW keying: RS-232C-only, DTR deferred | Accepted |
| [0003](0003-consume-radio-cat-rs-windows-network-transport.md) | Consuming radio-cat-rs's unpushed ADR 0006 work: temporary `[patch]`, Windows-enabling `--server`, and `NoModemControlLines` | Accepted |
| [0004](0004-shared-diagnostics-screen.md) | Shared diagnostics screen (`cat-diagnostics`) | Superseded by [0006](0006-hand-coded-full-parity-diagnostics.md) |
| [0005](0005-debian-and-windows-packaging.md) | Debian/Windows packaging and CI/release automation | Accepted |
| [0006](0006-hand-coded-full-parity-diagnostics.md) | Hand-coded, full-parity diagnostics (replaces `cat-diagnostics`) | Accepted |

## Repository status

**Full command coverage, grouped-menu UI, Windows-buildable.**

- **Wave 1** (`e3698cf`): an 11-command first slice, `Ft991a<S: CatSession>`
  controller client, workspace scaffold.
- **Wave 2** (`5197a8c`): a right-sized flat-screen `ui` crate and a
  PTY-hosted `emulator`, verified interoperating end-to-end over a real PTY.
- **Wave 3** (`5197a8c`): full CAT command coverage — all 91 top-level
  commands and 151 of 153 `EX` menu items (the remaining two, "TIME ZONE"
  and "RADIO ID", have no resolvable wire encoding in the manual). The
  RTS/DTR PTT/CW-keying feature landed (see [ADR 0002](0002-rts-dtr-ptt-cw-keying.md)),
  and this application became Windows-buildable
  (`cargo check --target x86_64-pc-windows-gnu`), riding on `radio-cat-rs`'s
  new Win32 COM serial backend.
- **Wave 4**: the `ui` crate's originally-flat, 9-keybinding design (built
  for 11 commands) was redesigned into a 12-group menu proportional to the
  full command surface, plus two `EX` menu access paths — a number-entry
  escape hatch and themed browsing with real scrolling for the largest
  (45-item) theme — that converge on the same value-entry flow. The RTS
  CW-keying feature is now reachable from the UI (optimistic toggle with
  rollback on error). `emulator`'s display grew to match. See
  `planning/architect/task_plan.md` §11 for the full design record,
  including the Rust coherence constraint that determined where the RTS
  keybinding's supporting traits had to live.

964 tests passing across the workspace (`radio` + `ui` + `emulator`), zero
regressions across the whole build-up, clippy/fmt clean.

- **Wave 5+** (2026-07-26): consumed `radio-cat-rs`'s ADR 0006/0007/0008
  work (initially only available as unpushed local commits there, via a
  temporary `[patch]` — [ADR 0003](0003-consume-radio-cat-rs-windows-network-transport.md)):
  the `--server <host:port>` TCP client mode became Windows-buildable too
  (`ft991a server`'s headless mode stays Linux-only — its `cat-rigctl`
  dependency has no Windows backend upstream yet, unlike the transports
  ADR 0006 actually fixed), `TcpClientSession` now composes the new
  `cat_transport_core::NoModemControlLines` adapter instead of hand-rolling
  it, and a shared diagnostics screen (`[D]`) was added, wrapping the new
  `cat-diagnostics` crate behind `radio::Ft991aExtras::run_diagnostics_with`
  ([ADR 0004](0004-shared-diagnostics-screen.md)). Debian packaging,
  Windows packaging, and GitHub Actions CI/release workflows were added for
  the first time ([ADR 0005](0005-debian-and-windows-packaging.md)). 1115+
  tests passing, clippy/fmt clean, `cargo check --target
  x86_64-pc-windows-gnu -p ft991a` green.

- **2026-07-26 (later the same day)**: the `[D]` diagnostics screen was
  rebuilt from scratch as a hand-coded, `ts570d`-parity engine living in
  `ui` — full test-and-restore coverage (set, verify, restore) for all 91
  commands, including the 28 previously read-only-skipped ones, gated
  behind an explicit transmit-safety warning screen and a callsign prompt
  for the CW-keying test ([ADR 0006](0006-hand-coded-full-parity-diagnostics.md)).
  `radio` no longer depends on `cat-diagnostics` at all — superseding
  [ADR 0004](0004-shared-diagnostics-screen.md)'s design (kept below for
  history). Verified end-to-end against the live `emulator`: 114/114
  passed with a supplied callsign, 113 passed/1 skipped without one (zero
  `KY` commands sent on the wire), and byte-for-byte identical radio state
  before and after each run.

See [ADR 0001](0001-second-radio-on-shared-cat-framework.md),
[ADR 0002](0002-rts-dtr-ptt-cw-keying.md),
[ADR 0003](0003-consume-radio-cat-rs-windows-network-transport.md),
[ADR 0004](0004-shared-diagnostics-screen.md) (superseded by ADR 0006 for
the diagnostics engine itself),
[ADR 0005](0005-debian-and-windows-packaging.md), and
[ADR 0006](0006-hand-coded-full-parity-diagnostics.md) for the design record.
