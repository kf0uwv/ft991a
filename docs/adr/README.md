# Architecture Decision Records

Decisions are recorded as [ADRs](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
(Michael Nygard format). Each file is one decision; numbers are stable and never reused.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-second-radio-on-shared-cat-framework.md) | Second radio on the shared CAT framework | Accepted |
| [0002](0002-rts-dtr-ptt-cw-keying.md) | RTS/DTR PTT and CW keying: RS-232C-only, DTR deferred | Accepted |
| [0003](0003-consume-radio-cat-rs-windows-network-transport.md) | Consuming radio-cat-rs's unpushed ADR 0006 work: temporary `[patch]`, Windows-enabling `--server`, and `NoModemControlLines` | Accepted |
| [0004](0004-shared-diagnostics-screen.md) | Shared diagnostics screen (`cat-diagnostics`) | Accepted |
| [0005](0005-debian-and-windows-packaging.md) | Debian/Windows packaging and CI/release automation | Accepted |

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

See [ADR 0001](0001-second-radio-on-shared-cat-framework.md),
[ADR 0002](0002-rts-dtr-ptt-cw-keying.md),
[ADR 0003](0003-consume-radio-cat-rs-windows-network-transport.md),
[ADR 0004](0004-shared-diagnostics-screen.md), and
[ADR 0005](0005-debian-and-windows-packaging.md) for the design record.
