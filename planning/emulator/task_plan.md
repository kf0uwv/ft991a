# Emulator Agent Task Plan

## Goal
Deliver the `emulator/` crate: a PTY-hosted process running
`CatFramework<Ft991aRadio>` for out-of-process testing, per
`planning/architect/task_plan.md` §7 (Wave 2 design). This mirrors
`ts570d/emulator` closely — an emulator's job doesn't scale with
command-table size.

## Status: unblocked — Wave 1 (`radio` crate, `Ft991aRadio`/`Ft991aState`/
`FT991A_COMMAND_TABLE`) landed and committed (`e3698cf`). Architect has
completed the Wave 2 `emulator` design (task_plan.md §7) and dispatched
this task directly (see dispatch message). Proceeding without a further
approval gate per that dispatch.

## Source of truth
- `planning/architect/task_plan.md` §7 (lines ~667-748) — authoritative
  file-by-file plan.
- `ts570d/emulator/src/*.rs`, `ts570d/emulator/Cargo.toml` — read in full,
  reference implementation.
- `radio/src/ft991a_radio.rs` — `Ft991aRadio`/`Ft991aState`/`Ft991aEvent`/
  `FT991A_COMMAND_TABLE`.
- `radio/src/radio_trait.rs` — `Mode` enum (14 hex-nibble values, `.name()`).
- `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` pages 7-8 — menu items 029
  "232C RATE" / 031 "CAT RATE" baud option lists.

## Plan (single task, per-file actions)

1. **Copy verbatim** (confirmed radio-generic by reading the real ts570d
   source, not just the plan's paraphrase):
   - `lib.rs` — module list + `EmulatorError{Pty,Io}` + the `test_emulator_fa_round_trip`
     integration test (adapted: still generic, only exercises `FA;` which
     exists in both tables with the same 11-digit reply shape — for FT-991A
     `FA;` replies `FA<9 digits>;` not 11; **note**: ts570d's own test
     comment claims `FA\d{11}`) — see step-2 finding below, this needs a
     digit-count fix for FT-991A, so `lib.rs` is not 100% byte-identical,
     see Findings.
   - `pty.rs` — `PtyPair` wrapping `TTYPort::pair()`. No radio types
     referenced. Copy verbatim.
   - `io.rs` — `CommandFramer`/`EmulatorIo`. No radio types referenced.
     Copy verbatim.
   - `logger.rs` — `StateChange`/`LogEvent` NDJSON shape. Matches
     `Ft991aEvent{field,value}` shape exactly. Copy verbatim.
   - `main.rs` — CLI parsing, PTY/physical port selection, ctrlc handler.
     Imports only `Emulator`/`BackgroundLogger`/`port`. Copy verbatim.

2. **Copy with the one flagged fix**:
   - `port.rs` — structurally unchanged, EXCEPT `serialport::new(path, 4800)`
     → `serialport::new(path, 9600)`. Manual pages 7 (menu 029 "232C RATE")
     confirm the valid option set is `0:4800bps 1:9600bps 2:19200bps
     3:38400bps` (menu 031 "CAT RATE" identical). The manual's menu table
     does not visually mark a factory-default option on this page, so the
     default numeral itself is taken from this repo's own already-recorded
     convention: `src/main.rs`'s `--baud` default (9600, see comment there
     citing "FT-991A CAT baud rate default") and
     `planning/architect/task_plan.md` §1 / `planning/app/task_plan.md`
     line 65 ("Baud choices 4800|9600|19200|38400 ... default 9600"). Using
     9600 keeps the emulator's physical-port default consistent with the
     rest of this repo instead of silently inheriting ts570d's 4800.

3. **Retarget (2 type-name substitutions)**:
   - `emulator.rs` — `Ts570dRadio` → `Ft991aRadio`, `CatFramework<Ts570dRadio>`
     → `CatFramework<Ft991aRadio>`. Method bodies otherwise unchanged.

4. **Rewrite** `tui.rs` against `Ft991aState`'s actual field set (see §7.3
   of the architect plan):
   - Keep: three-column layout, `bargraph`, `tick_label_line`,
     `big_digit`/`render_big_freq`, `on_style`, `format_log_line`/
     `extract_command_code` pattern.
   - `lookup_description` → `FT991A_COMMAND_TABLE.find(code).map(|c| c.description)`.
   - `draw_rx_smeter`: rescale for `state.smeter` 0-255 (not ts570d's 0-30).
     S-unit boundaries follow the standard S-meter convention (6 dB/S-unit
     below S9, 10 dB/20dB-over-S9 steps above), linearly mapped onto the
     0-255 raw scale in 16-unit steps up to S9 (144), then wider steps to
     255 for +10/+20/+30/+40 over S9 (this is a reasonable synthetic
     mapping since the manual doesn't document `SM`'s raw-value-to-S-unit
     curve — flagged in Findings).
   - `draw_tx_meters`/meter column: when `state.cat_tx == 1`, show
     `PWR: <power_control>W` bar only, no SWR bar (no `RM` command in this
     slice).
   - LCD column: annunciators reduced to `power_on`/`cat_tx` only (no RIT/
     XIT/split/antenna/AGC/NB — fields don't exist). New `format_freq_ascii`
     for FT-991A's up-to-9-digit/470MHz range (`030.000.00` shape, 3-digit
     MHz). Mode row uses `radio::Mode::name()` (14 values), looked up via
     `Mode::try_from(state.mode)`.
   - No VFO A/B toggle badge — `Ft991aState` doesn't track an "active VFO"
     selector in this slice (both `vfo_a_hz`/`vfo_b_hz` exist but there's no
     `active_vfo`-equivalent field); display VFO A's frequency as the main
     readout (matches `FA`'s primacy — `ID`/`SM`/etc. don't reference VFO B).
     Flagged in Findings as a judgment call.

5. **`emulator/Cargo.toml`** per §7.2 — same dependency list as
   `ts570d/emulator/Cargo.toml`, `description` retargeted to FT-991A.

6. **Root `Cargo.toml`**: add `"emulator"` to `[workspace] members`. Touch
   nothing else in that file.

## Constraints
- Do not touch `radio/`, `ui/`, `src/main.rs`, or root `Cargo.toml`'s
  `[dependencies]`/`[[bin]]`/`[dev-dependencies]`.
- Do not touch `ts570d` or `radio-cat-rs` (read-only reference).
- Do not commit.
- Must pass: `cargo build -p emulator`, `cargo test -p emulator`,
  `cargo clippy -p emulator --all-targets -- -D warnings`, `cargo fmt --check`.

## Verification
Build, test, clippy, fmt — record results in `progress.md`.

## Findings / judgment calls (kept here — a separate findings.md write was
rejected by tooling as a report-file; folding in per repo convention's
intent)

1. **`lib.rs`'s integration test is not byte-identical copy-verbatim.**
   `ts570d/emulator/src/lib.rs`'s `test_emulator_fa_round_trip` asserts the
   `FA` response has exactly 11 frequency digits (`FA\d{11};}`). FT-991A's
   `FA` wire format (per `radio/src/ft991a_radio.rs` docs/tests) is
   `FA<9 digits>;`, not 11 — a radio-specific literal embedded in an
   otherwise-generic test the architect's table didn't flag. Fixing the
   digit count/expected literal in place (test purpose is unchanged and
   generic: round-trip `FA;` through the real PTY) rather than stopping,
   since it's a two-line mechanical literal fix, not a design change.
2. **No `active_vfo` field on `Ft991aState`.** Unlike `Ts570dState`, this
   slice's state has no "active VFO" selector. The rewritten `tui.rs`
   displays VFO A's frequency as the sole main readout and omits any VFO
   A/B toggle badge (no fabricated selector state).
3. **`SM`'s raw 0-255 value has no manual-documented S-unit curve.** The
   rewritten `draw_rx_smeter`'s S-unit label boundaries are a synthetic
   linear mapping (16 raw units/S-unit up to S9 at raw 144, then 4 wider
   bands to 255 for +10/+20/+30/+40-over-S9), documented as an assumption
   in `tui.rs`'s doc comment — affects only the TUI display label, not wire
   behavior.
4. **Baud default 9600, not ts570d's 4800.** Manual p.7, menu items 029
   "232C RATE"/031 "CAT RATE", confirms the valid option set
   (4800/9600/19200/38400) but doesn't visually flag a factory default on
   the scanned page. Used this repo's own already-recorded default (9600,
   `src/main.rs` + `planning/app/task_plan.md` line 65) for consistency.

## Wave 4, Task 10: `tui.rs` proportional growth (§11.5)

### Goal
Grow `emulator/src/tui.rs`'s existing regions (not a structural rewrite) to
track `Ft991aState`'s Wave 3 growth (11 -> 91 commands, first-slice -> full
state machine). Per `planning/architect/task_plan.md` §11.5: stay a flat,
wider annunciator list (now wrapped across 2-3 lines), not a grouped/
paginated display — the emulator has no keybindings, so `ui`'s
discoverability-driven redesign pressure doesn't apply here.

### Source of truth
- `planning/architect/task_plan.md` §11.5 — full design read before starting.
- `emulator/src/tui.rs` — Wave 2's implementation, read in full.
- `radio/src/ft991a_radio.rs` — `Ft991aState`'s real (now much larger) field
  list, `FT991A_COMMAND_TABLE` (confirmed 91 entries via the
  `Ft991aCommandId` enum variant count), `AgcMode`/`PreampMode`/`ScanState`
  domain enums (`radio_trait.rs`, all three publicly exported from `lib.rs`).
- `ts570d/emulator/src/tui.rs` — re-checked precedent for flat-annunciator
  density (ann_line1/ann_line2 pattern).

### Plan (single task, mechanical field-by-field extension)
1. **`draw_ann_line` -> `draw_ann_line1`/`draw_ann_line2`/`draw_ann_line3`**
   (ts570d's ann_line1/ann_line2 naming precedent, extended to 3 lines to
   fit the larger set): line 1 = core operating state (PWR/TX/RX plus
   MOX/LOCK/MENU/PLL-UNLK); line 2 = front-end/audio processing toggles
   (clarifier RX/TX-on, ATT, preamp AMP1/AMP2, NB, NR, NOTCH, NAR); line 3 =
   keying/scan/VOX/break-in plus an always-visible AGC mode label (AGC is
   never "off-screen" the way a toggle is — `OFF` is itself a meaningful
   state to show, same idiom the mode row already uses for the always-shown
   operating mode).
2. **`draw_freq_block`**: add a clarifier offset readout (`clarifier_readout`
   helper) appended to the big-digit block's middle row when either
   `rx_clarifier_on` or `tx_clarifier_on` is set — shows direction (RX/TX/
   R/T) and the shared signed `clarifier_offset_hz` value. No VFO A/B badge
   exists in this slice (Wave 2 finding, unchanged), so it has no badge to
   sit beside, unlike ts570d's RIT/XIT sub-display.
3. **`draw_tx_meter`**: extended with a 6-way meter bank (COMP/ALC/PO/SWR/
   ID/VDD, batch 9's `RM`/`MS` fields) below the existing PWR block, one row
   per meter via a new `draw_meter_row` helper (label + bargraph + raw
   0-255 value), with the front-panel `MS`-selected meter highlighted
   amber/bold and the other five dimmed. PWR stays a separate row (reads
   `power_control` watts, a distinct field/scale from `po_meter`'s raw `RM`
   reading) rather than being folded into the bank as a 7th row.
   `draw_rx_smeter` is unchanged — `RM`'s selector `1` (S-meter) is always
   `smeter` regardless of TX/RX, so RX-side stays S-meter-only, matching the
   real front panel's own RX/TX meter-source split.
4. **`lookup_description`**: no code change needed — it already reads
   `FT991A_COMMAND_TABLE.find(code)` unconditionally, and that static now
   has 91 entries (Wave 3 landed all batches) vs. Wave 2's 11-entry
   first-slice table, so the "retarget" is automatic. Added a regression
   test (`test_command_table_fully_wired_wave4`) asserting the table has
   grown past 11, plus extended `test_lookup_description_known_and_unknown`
   with a batch-6 command (`GT`) and the `EX` entry point, so a future
   accidental narrowing of the table or the lookup function would be caught.
5. **`EX`-state dump nice-to-have**: explicitly deferred, not scoped in —
   see Findings below.

### Constraints
- `emulator`-crate-only, specifically `tui.rs`; no other file needed.
- Do not touch `radio/`, `ui/`, `src/main.rs`, root `Cargo.toml`, `ts570d`,
  `radio-cat-rs`.
- Do not commit.
- Must pass: `cargo build -p emulator`, `cargo test -p emulator`,
  `cargo clippy -p emulator --all-targets -- -D warnings`,
  `cargo fmt --check -p emulator`; all 15 pre-existing tests must still pass.

### Verification
Build, test (22/22, 15 original + 7 new, no regressions), clippy, fmt all
clean at the time of implementation — see `progress.md` for the full
transcript and a note about a later, unrelated concurrent-session build
break in `radio/` observed during final re-verification.

### Findings / judgment calls

1. **`EX`-state dump nice-to-have: deferred, not scoped in.** §11.5
   explicitly frames this as optional ("your call to scope in or explicitly
   defer, not a requirement"). Scoping it in would mean adding a CLI flag
   or keypress, which touches `main.rs`/`emulator.rs` (args parsing, key
   handling) beyond this task's `tui.rs`-only actual need, and duplicating
   iteration logic over `EX_MENU_TABLE`'s 151 items — a meaningfully
   separate unit of work from the mechanical field-by-field screen growth
   this task is. Deferring keeps this task's diff scoped to exactly what
   §11.5 calls "one task... mechanical field-by-field extension," and
   avoids scope creep into files not strictly required. If wanted later,
   it should be its own small task.
2. **AGC mode shown always, not gated behind an "active" check.** The
   architect's list groups "AGC mode" alongside on/off toggles like "VOX
   on"/"attenuator on", but AGC always has *some* selected setting (`OFF`
   is itself meaningful, unlike e.g. "attenuator" which has a real
   "not present" state) — modeled as an always-visible label on ann line 3,
   same idiom `draw_mode_row` already uses for the operating mode, not
   filtered out like the rest of that line's items. Documented as a
   judgment call in `agc_mode_label`'s doc comment.
3. **Preamp/scan annunciators use FT-991A's actual domain enums, not raw
   booleans.** `PreampMode`/`ScanState` (from `radio_trait.rs`, already
   publicly exported) are 3-valued/3-valued, not plain bools — used
   `TryFrom<u8>` + a local label match, the same pattern the pre-existing
   mode row already uses for `Mode`. Preamp's `Ipo` (bypass) value shows no
   annunciator (matches how a real front panel doesn't light `AMP1`/`AMP2`
   when bypassed); scan's `Off` likewise shows nothing.
4. **`meter_select`/`selected_meter_reading` duplicated as public logic,
   not reused.** `Ft991aState::selected_meter_reading`/`meter_reading` in
   `radio/src/ft991a_radio.rs` are private (`fn`, not `pub fn`) methods on
   `impl Ft991aState` — not visible outside the `radio` crate. `tui.rs`
   cannot call them, so it re-implements the same `meter_select -> which
   *_meter field` mapping locally (`draw_meter_row`'s per-field calls in
   `draw_tx_meter`, `meter_select_label`) directly against the public
   `Ft991aState` fields (`meter_select`, `comp_meter`, ..., `vdd_meter`),
   not by inventing new state. This is view-layer duplication of a
   3-line match, not new radio-domain logic, and doesn't touch `radio/`.
5. **No discrepancies found between the architect's §11.5 annunciator list
   and `Ft991aState`'s real fields.** Every item on the architect's list
   (clarifier RX/TX-on + offset, keyer enabled, scan state, VOX on,
   attenuator on, preamp mode, noise blanker/reduction on, AGC mode,
   auto-notch on, narrow on, frequency lock, mox, break-in on,
   PLL-unlocked/menu-mode) maps 1:1 onto a real, public `Ft991aState`
   field — unlike Wave 2, where several architect-anticipated fields
   (RIT/XIT/split/antenna/AGC/NB annunciators) didn't exist yet. Wave 3
   landed all of them in the interim.
6. **A concurrent, unrelated `radio/` in-progress edit was observed during
   final re-verification**, not caused by this task — see `progress.md`.
