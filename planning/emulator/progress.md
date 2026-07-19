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

## Task: Wave 4 Task 10 — `tui.rs` proportional growth (§11.5) — COMPLETE

Extended `emulator/src/tui.rs` in place per `task_plan.md`'s Wave 4 section
above. Summary of the diff (332 insertions / 14 deletions, single file):

- `draw_ann_line` split into `draw_ann_line1`/`draw_ann_line2`/
  `draw_ann_line3` (grew from 3 annunciators to ~20, real fields: `mox_on`,
  `lock_on`, `menu_mode`, `pll_unlocked`, `rx_clarifier_on`,
  `tx_clarifier_on`, `attenuator_on`, `preamp_mode`, `noise_blanker_on`,
  `noise_reduction_on`, `auto_notch_on`, `narrow_on`, `keyer_on`,
  `scan_state`, `vox_on`, `break_in_on`, `agc_mode`).
- `draw_freq_block` gained a clarifier offset readout (new
  `clarifier_readout` helper, reads `rx_clarifier_on`/`tx_clarifier_on`/
  `clarifier_offset_hz`).
- `draw_tx_meter` extended with a 6-way meter bank (`comp_meter`/
  `alc_meter`/`po_meter`/`swr_meter`/`id_meter`/`vdd_meter`, `meter_select`
  for highlighting), new `draw_meter_row`/`meter_select_label` helpers.
  `draw_rx_smeter` unchanged.
- `lookup_description` unchanged (already reads the full, now-91-entry
  `FT991A_COMMAND_TABLE` unconditionally) — added regression tests instead
  of a code change.
- `EX`-state dump nice-to-have: deferred, not scoped in (see `task_plan.md`
  Findings #1).
- Low-level visual helpers (`bargraph`, `tick_label_line`, `big_digit`/
  `render_big_freq`, `on_style`) and the three-column layout convention:
  unchanged, as required.
- 7 new unit tests added, matching the existing pure-function test style
  (no rendering-output/snapshot tests existed to extend — Wave 2's own
  tests are all pure-function-level, so new tests followed that same
  pattern): `test_command_table_fully_wired_wave4`, `test_meter_select_label`,
  `test_agc_mode_label_full_7valued_domain`, `test_preamp_ann_label_ipo_hidden`,
  `test_scan_ann_label_off_hidden`, `test_clarifier_readout_hidden_when_both_off`,
  `test_clarifier_readout_shows_direction_and_signed_offset`. Also extended
  `test_lookup_description_known_and_unknown` with a batch-6 (`GT`) and the
  `EX` command code.

### Verification (all clean at implementation time)
- `cargo build -p emulator` — clean.
- `cargo test -p emulator` — 22/22 passed (15 pre-existing + 7 new), no
  regressions.
- `cargo clippy -p emulator --all-targets -- -D warnings` — clean (one
  `field_reassign_with_default` lint fixed by switching a test to struct-
  update syntax).
- `cargo fmt --check -p emulator` — clean (after `cargo fmt -p emulator`).
- `cargo build --workspace` — clean at the time of this verification pass.
- Manual smoke test (not committed, scratch file deleted afterward): wrote
  a throwaway example rendering `tui::draw` via `ratatui::backend::
  TestBackend` against a "maximally busy" `Ft991aState` (every new
  annunciator lit, extreme values: clarifier -9999 Hz, all 6 meters at 255,
  `agc_mode: 6`) across terminal sizes from 200x60 down to 10x5 and even
  0x0, for both `cat_tx` 0 and 1 — confirmed no panics from layout
  overflow/underflow (e.g. `saturating_sub` guards in `draw_meter_row`'s
  bar-width computation, the `if rows[n].height > 0` guards throughout).

### Note: concurrent `radio/` build break observed during final re-check
After the above verification passed cleanly, a later `cargo build
--workspace` re-run (done purely as a final sanity check, no code changes
made after the clean run above) failed with ~79 errors in
`radio/src/ft991a_radio.rs` (`ExMenuValueKind::Enumerated`'s wire-string
array literals no longer matching a changed `&'static [(&'static str,
&'static str)]` tuple-pair signature). `git status` showed `radio/src/
ft991a_radio.rs` as modified with no corresponding edit by this task —
this is Wave 4 Task 1's (`yaesu` agent, `Ft991aExtras`/`ExMenuValueKind`
label-table work per `planning/architect/task_plan.md` §11.6) in-progress,
uncommitted, mid-edit state from a concurrent session sharing this
non-worktree-isolated working directory, not anything this task touched or
broke. `emulator/src/tui.rs`'s own changes are complete, correct, and were
fully verified clean in isolation before this was observed. Not fixed here
per the standing "do NOT touch `radio/`" constraint — flagging for the
architect/coordinating session's awareness.

**Update**: re-ran the full verification suite again a short time later —
the concurrent session's `radio/` edit had by then reached a consistent
state on its own. `cargo test -p emulator` (22/22), `cargo clippy -p
emulator --all-targets -- -D warnings`, `cargo fmt --check -p emulator`,
and `cargo build --workspace` are all clean as of this final check. No
action was needed from this task.
