# 3. Consuming radio-cat-rs's unpushed ADR 0006 work: temporary `[patch]`, Windows-enabling `--server`, and `NoModemControlLines`

Date: 2026-07-26

## Status

Accepted

## Context

`radio-cat-rs` (this repo's shared CAT library dependency) landed a set of
local commits — not yet pushed to `https://github.com/kf0uwv/radio-cat-rs`
— giving `cat-transport-tcp`, `cat-transport-udp`, and `cat-server` real
Windows backends (its own ADR 0006), plus a reusable
`cat_transport_core::NoModemControlLines<S>` adapter and a shared
`cat-transport-serial` `pin-test` binary. This repo's `Cargo.toml` pulls
every `cat-*` crate as a `branch = "main"` git dependency, which would
silently resolve against the old, unmodified *remote* `main` and miss all
of this work.

### What was read before deciding

- `radio-cat-rs/docs/adr/0006-windows-network-transport.md` in full (see
  its own text for the exact design — not reproduced here).
- `radio-cat-rs/cat-rigctl/Cargo.toml` and `src/{lib,rigctl}.rs` — the
  Hamlib rigctld bridge this repo's own `server` crate wraps. Its listener
  orchestration (`monoio::net::{TcpListener, UdpSocket}`, `monoio::spawn`)
  is used **unconditionally in source**, with no `#[cfg(target_os =
  "linux")]` gate and no Windows-side module at all — confirmed by running
  `cargo check --target x86_64-pc-windows-gnu -p cat-rigctl` against the
  patched local checkout, which fails with `E0433: unresolved crate
  monoio` at every such call site.
- This repo's own `Cargo.toml`, `src/main.rs` (`TcpClientSession`,
  `run_over_tcp`, `run_server_mode`), `server/Cargo.toml`,
  `server/src/lib.rs` — the exact gates and their stated reasons, per
  `grep -rn 'target_os = "linux"' Cargo.toml src/main.rs server/ emulator/
  radio/ ui/`.

## Decision

### 1. A temporary path-based `[patch]`, explicitly documented as removable

Added `[patch."https://github.com/kf0uwv/radio-cat-rs"]` to the root
`Cargo.toml`, pointing every `cat-*` crate (including the new
`cat-diagnostics`, added as a plain git dependency for
[ADR 0004](0004-shared-diagnostics-screen.md)) at
`../radio-cat-rs/<crate>` — a sibling checkout on the same machine. This is
explicitly a stopgap: the comment above the `[patch]` table says, in so
many words, to delete the whole table once a human pushes `radio-cat-rs`'s
`main` branch and the `branch = "main"` git deps above resolve against the
real thing again. Verified with `cargo tree -p cat-transport-tcp` (resolves
to the local path) and `cargo build --workspace` (succeeds).

### 2. `--server <host:port>` (TCP client mode) becomes Windows-buildable; `ft991a server` (headless mode) does **not**

The task brief that kicked off this round assumed both features would
become Windows-buildable together, on the theory that "the transport
limitation is now fixed." Investigation found this only half true:

- **`--server <host:port>`** depends directly on `cat-transport-tcp`/
  `cat-transport-core`, both of which genuinely got a Windows backend in
  `radio-cat-rs` ADR 0006 §3 (same public `TcpCatSession`/`CatSession` API
  on both platforms). Ungating this was correct and is now done: the
  `[target.'cfg(target_os = "linux")'.dependencies]` block in this repo's
  root `Cargo.toml` no longer names `cat-transport-core`/`cat-transport-tcp`/
  `async-trait`; they moved to the unconditional `[dependencies]` block.
  `src/main.rs`'s `TcpClientSession`, its `CatSession` impl, and
  `run_over_tcp` lost their `#[cfg(target_os = "linux")]` gates; the
  `#[cfg(not(target_os = "linux"))]` stub version of `run_over_tcp` (which
  used to print "Linux-only" and exit) was deleted outright — there is
  nothing left for it to guard.
- **`ft991a server ...`** (the `server` workspace crate, wrapping
  `cat-rigctl::run`) stays Linux-only. `radio-cat-rs` ADR 0006 explicitly
  scoped its Windows work to `cat-transport-tcp`/`cat-transport-udp`/
  `cat-server` — not the `cat-rigctl` layer built on top of `cat-server`,
  which this repo's `server` crate actually depends on directly. `cat-rigctl`
  still imports `monoio` unconditionally with no Windows path, confirmed by
  a real failing `cargo check --target x86_64-pc-windows-gnu -p cat-rigctl`
  run (not just read from a doc comment). `server = { path = "server" }`
  remains in `Cargo.toml`'s `[target.'cfg(target_os = "linux")'.dependencies]`
  block, `main.rs`'s `ServerArgs`/`run_server_mode`/etc. remain
  `#[cfg(target_os = "linux")]`-gated, and every stale comment claiming "no
  Windows backend upstream for cat-server" was corrected to name the real,
  current bottleneck (`cat-rigctl`) instead of leaving a now-false
  statement in place. Lifting this for real is a `radio-cat-rs`/`cat-rigctl`
  follow-on, out of this repo's scope to do unilaterally (and out of this
  task's explicit constraint against modifying that sibling repo).

Verified: `cargo check --target x86_64-pc-windows-gnu -p ft991a` — clean.
`cargo check --target x86_64-pc-windows-gnu -p ft991a -p server` — fails,
exactly as expected, with the `cat-rigctl`/`monoio` errors described above
(this is the correct, honest outcome, not a regression to fix).
`cargo tree --target x86_64-pc-windows-gnu -p ft991a` confirms
`cat-transport-tcp`/`cat-transport-core` are now genuinely part of the
Windows dependency graph and `server` is not.

### 3. `TcpClientSession` no longer hand-writes `ModemControlLines`; it composes `NoModemControlLines` instead

Deleted `impl cat_transport_core::ModemControlLines for TcpClientSession`
and its `tcp_modem_lines_unsupported()` helper (five near-identical
`Err(...)` bodies) from `src/main.rs`. `run_over_tcp` now builds
`Ft991a::new(cat_transport_core::NoModemControlLines::new(
TcpClientSession::new(session)))` instead of
`Ft991a::new(TcpClientSession::new(session))`. `TcpClientSession` itself is
unchanged in shape — it still exists solely to map `TcpSessionError` to
`TransportError` (an orphan-rule-bound concern `NoModemControlLines` itself
deliberately does not solve, per its own doc comment) — it just no longer
also carries the unrelated modem-control-lines concern. This is the exact
composition `radio-cat-rs` ADR 0006 §7 designed `NoModemControlLines` for,
and it is in fact the origin of that adapter (`radio-cat-rs`'s own ADR
credits this repo's original hand-rolled version as the shape it
generalized).

Verified: `cargo test --workspace` — 1103 tests passed, 0 failed (no
regressions). Manual end-to-end check on Linux: `cargo run -p emulator --
--background`, then `ft991a server --port <pty> --raw-tcp-port 17991`,
then `ft991a --server 127.0.0.1:17991` in a `tmux` session — the TUI
connected, rendered live radio state (`ID:0670`, `PS:ON`, VFO
frequencies), and produced no errors in either process's log across
startup and clean `[Q]`-key exit.

## Consequences

- This repo is now sensitive to the sibling `radio-cat-rs` checkout's exact
  local path (`../radio-cat-rs`) for as long as the `[patch]` table is in
  place — a machine without that sibling checked out at that relative path
  cannot build this repo until the patch is removed and the real `branch =
  "main"` git dependency is pushed to. This is a deliberate, temporary,
  clearly-labeled trade, not an oversight.
- `ft991a server ...` remaining Linux-only is a real, user-visible
  limitation this ADR does not resolve — WSJT-X/Hamlib users on Windows
  still cannot run the headless network server mode. Only `--server
  <host:port>` (the TCP *client* side) gained Windows support this round.
- No behavior change on Linux for either feature — same public APIs, same
  test suite, same manual verification steps as before this round.

## Amendment (2026-07-26): `ft991a server` is Windows-buildable too

The limitation recorded above is resolved: `radio-cat-rs`
docs/adr/0006-windows-network-transport.md's same-day amendment gave
`cat-rigctl` a real Windows backend (extracting its radio-independent wire
protocol into a new `cat-rigctl::protocol` module, shared by a Linux
`monoio`-based accept loop and a new `std`/genuine-OS-thread one), closing
exactly the gap this ADR's §2 described. `server`'s `Cargo.toml` dependency
and `main.rs`'s `server` subcommand handling are no longer platform-gated;
`server::run`/`run_server_mode` are now `#[cfg]`-selected per platform
(`async fn` on Linux unchanged, a plain blocking `fn` on Windows, matching
`cat_rigctl::run`'s own split — `#[monoio::main]` cannot exist on Windows).

`ft991a server ...` — including `--rigctl-port`/WSJT-X support — now
builds for both platforms with the same CLI surface. Verified: `cargo check
--target x86_64-pc-windows-gnu -p ft991a -p server` clean; full workspace
suite (1115 tests) passing on Linux with no regressions; manual end-to-end
verification against the emulator (raw TCP + rigctld listeners) unchanged
from this repo's existing Linux verification, not re-run specifically for
this amendment since the Linux code path is byte-for-byte unchanged (only
wrapped in an explicit `#[cfg(target_os = "linux")]` it previously had
implicitly via the dependency graph). Windows runtime behavior is, as
always, unverified in this sandbox — real hardware validation is the user's
own follow-up, per this project's existing convention for every Windows
feature so far.
