# App Agent Progress

## Wave 1 Task 2 — complete (2026-07-17)

Deliverables:
- Root `Cargo.toml` superseded (workspace `members = ["radio", "ui"]`,
  full `[workspace.dependencies]` set mirroring `ts570d`, `[package]`/
  `[[bin]]` for the `ft991a` binary, `[dev-dependencies]` with `radio` +
  `monoio` + `cat-transport-serial` — no `emulator` entry, deferred).
- `src/main.rs`: CLI `--port`/`--baud`/`--stop-bits` parsing (baud choices
  4800/9600/19200/38400, default 9600; stop-bits default 2 — an FT-991A-
  specific judgment call, see `findings.md`), `SerialPort::open`,
  `Ft991a::new(SerialCatSession::new(port))`, `ui::run(radio).await`.
- `ui/Cargo.toml` + `ui/src/lib.rs`: minimal placeholder crate, depends on
  `radio` + `monoio` + `thiserror` only. `run<R: radio::Radio +
  'static>(_radio: R) -> UiResult<()>` returns `Ok(())` immediately. Has
  its own unit tests (in-crate `MockRadio`, per `CLAUDE.md`'s testing
  rule) — not just an empty file.

Verification (all run from repo root, all green):
- `cargo build --workspace` — clean.
- `cargo test --workspace --all-targets` — 51 `radio` tests + 2 new `ui`
  tests + 0 `ft991a` bin tests, all pass. (Note: `--all-targets` excludes
  doctests by cargo's own design.)
- `cargo test --workspace --doc` — `radio`'s 1 doctest passes (ui has 0).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean after
  one fix (`ui/src/lib.rs` test: `std::io::Error::new(ErrorKind::Other,
  ..)` → `std::io::Error::other(..)` per `clippy::io_other_error`).
- `cargo fmt --all -- --check` — clean.
- Manual smoke check: `cargo run --bin ft991a --` (no args) prints the
  expected usage banner with the FT-991A-specific baud/stop-bits text and
  exits 1.

No obstacles hit; `radio`'s real API matched the architect's plan closely
enough that no scope conflicts arose. Did not touch `radio/`, `ts570d/`,
or `radio-cat-rs/`. Did not create or wire an `emulator` crate. Did not
build the real ratatui TUI. No commits made.
