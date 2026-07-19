# Emulator Agent Progress

## Task: Wave 2 `emulator` crate — COMPLETE

Delivered `emulator/` per `task_plan.md` / architect `task_plan.md` §7.

### Files
- `Cargo.toml` — per §7.2, `description` retargeted to FT-991A.
- `src/lib.rs` — copy verbatim except one mechanical fix: the `FA`
  round-trip integration test's expected digit count (11→9, FT-991A's
  wire format).
- `src/pty.rs`, `src/io.rs`, `src/logger.rs`, `src/main.rs` — copy verbatim.
- `src/port.rs` — copy with the one flagged fix: physical-mode baud
  4800→9600.
- `src/emulator.rs` — retargeted: `Ts570dRadio`→`Ft991aRadio`,
  `CatFramework<Ts570dRadio>`→`CatFramework<Ft991aRadio>`.
- `src/tui.rs` — rewritten against `Ft991aState`'s field set (see
  `task_plan.md` Findings section for judgment calls: S-meter rescale,
  no active-VFO badge, FT-991A `format_freq_ascii`, `Mode::name()` mode
  row, PWR-only TX meter).
- Root `Cargo.toml`: added `"emulator"` to `[workspace] members`. No other
  section touched.

### Verification (all clean)
- `cargo build -p emulator` — clean.
- `cargo test -p emulator` — 15/15 passed (framer, PTY, port-arg parsing,
  tui unit tests, and the real-PTY `FA` round-trip integration test).
- `cargo clippy -p emulator --all-targets -- -D warnings` — clean.
- `cargo fmt --check -p emulator` — clean (after `cargo fmt -p emulator`
  fixed 2 files' formatting).
- `cargo build --workspace` / `cargo test --workspace` — clean; `radio`
  and `ui` unaffected (59 `ui` tests, `radio` doctest, all still pass).
- Manual smoke test: ran the compiled `emulator --background` binary
  against its real PTY slave, sent `FA;`/`MD01;`/`MD0;`/`TX1;`/`PC050;`/`PC;`
  from a separate process, confirmed correct wire responses
  (`FA014000000;`, `MD01;`, `PC050;`) and correct NDJSON `state_change`
  events for `mode`/`cat_tx`/`power_control`. No stray processes left
  running afterward.

### Not committed (per instructions).
