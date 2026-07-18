# App Agent Findings

## Wave 1 Task 2 (2026-07-17)

- `radio/Cargo.toml` and `radio/src/*` are the real, reviewed crate (built
  by the `yaesu` agent). `Ft991a::new(session: S)` takes any
  `S: CatSession<Error = TransportError>` directly — same shape as
  `ts570d::Ts570d::new`. `SerialCatSession<T: Transport>` satisfies this
  bound (`type Error = TransportError`, confirmed in
  `radio-cat-rs/cat-transport-serial/src/session.rs`).
- The existing root `Cargo.toml` is a placeholder the `yaesu` agent added
  (its own header comment says so explicitly) purely so `radio` had a
  workspace root to build/test/lint against. It lists
  `members = ["radio"]` only and is missing `ui`, the `[package]`/`[[bin]]`
  section, and most of `ts570d`'s `[workspace.dependencies]` set
  (tracing/tracing-subscriber/libc/ratatui/crossterm/serde/bytes/futures/
  nix/local-sync). Superseding it, not extending it, per my task's explicit
  instruction.
- Judgment call: FT-991A CLI default stop-bits = 2 (not `ts570d`'s 1) —
  the architect's task_plan.md §1 cites `SerialConfig::default()`'s 8N2
  framing as the FT-991A's documented default serial framing (no explicit
  data/parity/stop-bit spec in the manual beyond "standard serial cable,
  not null-modem"); ts570d's CLI default of 1 stop bit is a TS-570D-specific
  choice recorded in ts570d's own manual, not something that transfers by
  default.
- `ui` crate: kept dependencies to `radio` + `monoio` + `thiserror` only —
  no `ratatui`/`crossterm` yet, since the stub doesn't render anything.
  Those go into `ui/Cargo.toml` when Wave 2 builds the real TUI.
