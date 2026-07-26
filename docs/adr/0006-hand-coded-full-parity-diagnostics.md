# 6. Hand-coded, full-parity diagnostics (replaces `cat-diagnostics`)

Date: 2026-07-26

## Status

Accepted

## Context

The `[D]` diagnostics screen introduced by [ADR 0004](0004-shared-diagnostics-screen.md)
wraps `radio-cat-rs`'s generic `cat_diagnostics::run_diagnostics_with`. That
engine is deliberately read-only: for each of the 91 top-level
`FT991A_COMMAND_TABLE` commands it only ever issues the command's
documented safe query/selector-read form, and marks a command `Skipped` if
no such safe read form exists at all — 63 commands tested, 28 skipped.

The user was asked directly whether to fix this limitation via (a) full
parity with `ts570d`'s own diagnostics screen — which actually exercises
write/action commands, including keying the transmitter and CW, with a
snapshot/restore safety net — or (b) a safer, read-only-plus-restore-wrapper
option. **The user explicitly chose full parity**, understanding this means
the diagnostic briefly keys a real transmitter if ever run against real
hardware. Two follow-up requirements were added once implementation was
under way (both gating steps before/during the run, addressed below):
1. The CW keying test must identify the station (`"TEST <CALLSIGN>"`, not
   bare `"TEST"`) — an unidentified test transmission is a real amateur
   radio regulatory problem, not just cosmetic — collected via an
   interactive, in-screen prompt, not a CLI flag.
2. A confirmation screen, requiring an explicit keypress to proceed (not a
   passive warning line), must be shown before any diagnostic command is
   sent, stating plainly that the run keys the transmitter and that the
   radio must be on a proper antenna or dummy load.

### What was read before deciding

- `ts570d/ui/src/terminal.rs` in full (2556 lines): `RadioSnapshot`,
  `snapshot_state`/`restore_state`, the complete `run_diagnostics_task` (all
  107 steps), the `diag_get!`/`diag_action!`/`diag_set_get!` macros, and how
  `RadioUpdate::DiagProgress`/`ControlState::Diagnostic` drive live
  rendering.
- **`ts570d`'s own newly-landed `docs/adr/0007-diagnostics-tx-safety-gate.md`**
  and the `ControlState::DiagWarning`/`InputAction::DiagCallsign`/
  `KeyResult::StartDiag(Option<String>)` machinery it added to
  `ts570d/ui/src/control.rs` — a sibling implementation of exactly the two
  follow-up safety requirements above, landed independently while this task
  was in progress. Mirrored closely for cross-repo consistency (see
  Decision §2 below), adapted for this repo's different `KY` semantics and
  single-sequential-loop architecture (no separate radio/UI tasks).
- `radio/src/radio_trait.rs`'s full `Radio`/`Ft991aExtras`/`CwKeying` trait
  definitions (~3100 lines) and `radio/src/ft991a_radio.rs`'s emulator
  implementation for every one of the 28 previously-skipped commands, to
  determine real, safe test/restore behavior rather than guessing.
- `planning/yaesu/findings.md` for FT-991A protocol quirks already recorded
  from the manual (QMB's dedicated slot, `CH`/`QS` judgment calls, `VM`'s
  meaning uncertainty, `KY`/`KM` keyer-memory relationship).
- `radio/src/diagnostics.rs` (the file this ADR deletes) and its own doc
  comment explaining why `radio` had to wrap `cat_diagnostics` — that
  constraint (`ui` cannot obtain a `CatClient` from a generic `R: Radio +
  Ft991aExtras + CwKeying` value) is about calling `cat_diagnostics`
  specifically; it does not apply here, since this engine never touches
  `CatClient`/`cat_framework` at all — it only calls typed `Radio`/
  `Ft991aExtras` methods, exactly like `ts570d`'s own diagnostics screen
  always has.

### Findings that changed the plan from what was assumed going in

- **`RD`/`RU` (clarifier down/up) are absolute sets, not relative steps.**
  The task brief assumed otherwise (a common real-world CAT convention) and
  suggested documenting an unavoidable "cannot restore the exact offset"
  limitation. Reading `ft991a_radio.rs`'s own emulator implementation
  (`Rd`/`Ru` match arms, explicit comment: *"Absolute set, not an
  incremental step"*) and `radio_trait.rs`'s doc comments on
  `clarifier_down`/`clarifier_up` (*"Set the shared clarifier offset to
  `offset_hz` below/above the tuned frequency... An absolute set, not an
  incremental step"*) shows the opposite: combined with
  `Ft991aExtras::get_information()` exposing the exact signed
  `clarifier_offset_hz: i16` (`IF`'s P3 field), **exact restoration is
  achievable** via one computed `clarifier_clear`/`clarifier_down`/
  `clarifier_up` call. Verified end-to-end against the emulator (see
  Verified below) — no documented limitation needed for this one.
- **`KY` is not "send arbitrary CW text."** Unlike `ts570d`'s `send_cw`,
  FT-991A's `KY` (`Ft991aExtras::play_keyer_memory(channel, mode)`)
  triggers playback of a **pre-stored `KM` keyer-memory message** — there is
  no zero-length/no-op payload, and `write_keyer_memory` client-side-rejects
  empty messages (1-50 printable ASCII chars required) before any wire I/O.
  This changes the safety design: the test writes `"TEST <CALLSIGN>"` into
  a keyer memory channel, then plays it back, then restores the channel's
  original message — except when the channel started genuinely vacant
  (factory default: empty string), in which case exact restoration to
  vacant is architecturally impossible via this API (the same client-side
  validation that lets us test also blocks restoring "empty"). This is a
  real, disclosed protocol-surface limitation, not an oversight — it also
  applies to the separate `KM` diagnostic step (channel 2), which
  deliberately uses a different channel than `KY`'s test (channel 1) so the
  two don't interfere.
- **`VM` (`toggle_vfo_memory_mode`) collapses any non-VFO/Memory select mode
  to VFO(0) on its first call**, confirmed via `ft991a_radio.rs`'s emulator
  source (`if channel_select == 0 {1} else {0}`) — not a pure binary XOR as
  the task brief's "call it twice to restore" framing assumed. A
  double-toggle only restores exactly when the starting
  `get_information().select` was 0 or 1; for QMB/other select values
  (2-6), this step is honestly `Skipped` with the exact reason, rather than
  guessed at.
- **No `Radio::get_band` getter exists anywhere on the trait** (checked in
  full) — `BS`/`BU`/`BD` (Band Select/Up/Down) can only be verified
  `Ok`-only. Selecting a band is a band-stacking-register concept, not
  necessarily a retune of VFO-A into that band's edges, so a guessed
  frequency-range check would have been dishonest, not just imprecise.
- **`ED`/`EU`/`EK`/`ZI` (Encoder Down/Up, Ent Key, Zero In) mutate no
  persisted state at all** in this radio's own implementation (confirmed
  via `ft991a_radio.rs`'s emulator source and its own module-doc
  commentary: *"structurally and semantically validated... but mutate no
  persisted `Ft991aState` field... no Hz-per-step mapping is knowable from
  this manual page alone"*). `Ok`-only verification is therefore the
  honest ceiling for these four, not a shortcut — real hardware behavior
  here is genuinely undocumented/context-dependent per the manual itself.
- **`MemoryChannelEntry` has no `vacant` flag and there is no
  `clear_memory_channel` method** in this trait (unlike `ts570d`) —
  `read_memory_channel` always returns a full entry. This actually
  *simplifies* memory-channel restore versus `ts570d`'s own vacant/clear
  branching: snapshot the entry, always write it back verbatim.
- QMB (`QI`/`QR`) is a **dedicated single slot**, confirmed via `IF`'s own
  P7 legend (`3`=QMB, `4`=QMB-MT vs. `1`=Memory) in
  `planning/yaesu/findings.md` — not one of the 117 numbered memory
  channels. There is no direct "read QMB" method; `qmb_recall()` (which
  copies QMB into VFO-A) is the only way to observe its contents, so that's
  used to capture the original contents before overwriting them with test
  data.

## Decision

### 1. Full parity, hand-coded, living in `ui` (not `radio`)

`radio/src/diagnostics.rs` is deleted; `Ft991aExtras::run_diagnostics_with`
and its `Ft991a<S>` implementation are removed; `cat-diagnostics` is
dropped as a dependency everywhere in this repo (`radio/Cargo.toml`, the
root workspace `[workspace.dependencies]` table, and its `[patch]` entry).
The shared `cat-diagnostics` crate in `radio-cat-rs` itself is untouched —
this repo simply stops consuming it for this feature, exactly as `ts570d`
never did.

The constraint that forced the old design into `radio` (`ui` cannot obtain
a `CatClient` from a generic `R: Radio + Ft991aExtras + CwKeying` value) is
specific to calling `cat_diagnostics::run_diagnostics_with`, which needs a
concrete `CatClient` directly. This new engine never touches
`cat_framework`/`cat_client`/`CatClient` at all — every step calls a real
typed `Radio`/`Ft991aExtras` method through the existing generic bound
`ui::run` already carries, exactly the shape `ts570d`'s own diagnostics
screen has always used. So it belongs in `ui`, mirroring `ts570d`'s file
layout: `ui/src/diagnostics.rs` holds the `ui`-owned data model
(`DiagOutcome`/`DiagResult`/`DiagSummary`, replacing
`radio::Diagnostic{Outcome,Result,Summary}`); the engine itself
(`RadioSnapshot`, `snapshot_state`/`restore_state`,
`run_diagnostics_task`) lives in `ui/src/terminal.rs`, since it needs
`Terminal`/live-progress redraw access `diagnostics.rs` deliberately does
not depend on.

`DiagResult` drops `Timeout` (every step calls a typed method and gets a
plain `RadioResult<T>` back — there's no separate "no response" case
distinct from any other error) and drops `request` (no raw wire strings;
steps call typed methods). It adds nothing that changes the "every step
verifiable + best-effort restore" spirit `ts570d` established.

**Single pass, not `ts570d`'s 3 rounds.** A deliberate reduction in
RF-safety exposure: a run already keys PTT and, if a callsign was supplied,
sends real CW once; repeating that 3× per run buys `ts570d`-style
robustness at a transmit-time cost this repo's own diagnostics screen
doesn't spend by default. `DIAG_STEP_COUNT = 114` (verified equal to the
engine's actual output by
`test_diag_step_count_matches_actual_output`).

### 2. Transmit-safety gate, mirroring `ts570d`'s own `ADR 0007` closely

`ts570d` landed an almost identical requirement (operator acknowledgment +
callsign-gated CW test) independently, via its own `docs/adr/0007-
diagnostics-tx-safety-gate.md`, while this task was in progress. Its design
is mirrored here for cross-repo consistency, adapted to this repo's
single-sequential-loop architecture (no separate radio/UI tasks to hand a
callsign across, unlike `ts570d`'s channel-based `RadioCmd::
StartDiagnostics { callsign }`):

- **`ControlState::DiagWarning`** — pressing `[D]` from `Menu` now
  transitions here (`KeyResult::Continue`, no radio access needed) instead
  of directly returning `KeyResult::RunDiagnostics`.
  `layout::draw_diag_warning_panel` replaces the whole Controls panel with
  a red-bordered, centered warning (near-identical wording to `ts570d`'s
  own) stating the run keys the transmitter and the radio must be on a
  proper antenna or dummy load. `Enter`/`y`/`Y` proceeds; `Esc` cancels to
  `Menu` with nothing sent to the radio.
- **`InputAction::DiagCwCallsign` + `ControlState::TextInput`** —
  acknowledging the warning transitions to the *existing* `TextInput`
  widget with the prompt `"Callsign for CW test (blank to skip):"`,
  reusing the exact rendering and typing/backspace/Esc behavior every other
  free-text prompt in this app already uses (per the coordinator's
  explicit instruction to reuse the pattern, not invent a new widget).
  `Enter` is special-cased in `handle_key`'s `TextInput` arm (before
  `validate_text_input`, since confirming this starts a diagnostic run, not
  a single radio command): the buffer is trimmed (blank -> `None`, meaning
  "skip"), and `KeyResult::RunDiagnostics(Option<String>)` carries it
  through to `terminal.rs`'s `run_loop`, which calls
  `run_diagnostics_screen(radio, terminal, &display, cw_callsign)`.
- **The `KY` step branches on the callsign.** `Some(callsign)` ->
  `"TEST <CALLSIGN>"` is written to keyer-memory channel 1 and played back
  via `play_keyer_memory`; `None` -> the step is `Skipped` (not attempted
  bare, not a hard error) with an explicit reason, and every other step
  still runs normally — the run's `passed`/`failed`/`skipped` breakdown
  reflects this honestly (`DiagResult::Skipped` is a first-class third
  state, same as `ts570d`'s own `ADR 0007` treats it).

`KeyResult::RunDiagnostics` changed from a unit variant to
`RunDiagnostics(Option<String>)` — a small, contained signature change; no
other crate references it.

### 3. Per-command safety reasoning for the 28 previously-skipped commands

All 28 are now under real test-and-restore coverage (two remain
*conditionally* `Skipped` at runtime, both honestly, not blanket-skipped):

| Command(s) | Treatment |
|---|---|
| S-Meter Reading, Read Meter, Radio Information, Memory Channel Read | Pure reads needing a typed parameter (`Meter`, `RadioIndicator`, channel number) the generic engine couldn't guess — now called directly (`get_smeter`, `get_meter(Meter::Po)`, `get_radio_indicator(RadioIndicator::TxLed)`, `read_memory_channel(1)`). No restore needed. |
| Memory Channel Write | Self-contained: snapshot channel 3's contents, write a test entry, verify by reading it back, write the original back verbatim (no `vacant` concept in this trait — see Findings). |
| VFO-A to VFO-B, VFO-B to VFO-A | `copy_vfo_a_to_b`/`copy_vfo_b_to_a` called and verified `Ok`; VFO-A/B are already in the global snapshot, so the final restore covers the copy's effect — no bespoke per-step restore needed. |
| VFO-A to Memory Channel | Self-contained: select a test channel (2), snapshot its contents and the prior selection, set VFO-A/mode to test values, `store_vfo_to_memory`, verify by reading the channel back, restore the channel's contents and the prior selection. |
| [V/M] Key Function | Conditionally `Skipped`: only double-toggled (exact restore) when `get_information().select` starts at 0 or 1 (VFO/Memory); any other value (QMB modes) is honestly skipped, since `VM` collapses those to VFO(0) irreversibly — see Findings. |
| Memory Channel to VFO-A | `recall_memory_to_vfo` called and verified `Ok`; VFO-A/mode are covered by the final global restore. |
| Channel Up/Down | Two independent self-contained steps: snapshot the current index, step, verify it changed, restore via an *absolute* `set_memory_channel` (not stepping the opposite direction, which might not land on the same index across the documented-judgment-call wrap boundary). |
| QMB Store, QMB Recall | One combined round-trip step: `qmb_recall()` first to capture the QMB slot's *current* contents (via VFO-A, the only way to observe them), overwrite with test data via `qmb_store()`, verify via `qmb_recall()`+`get_vfo_a()`, then restore the original contents by setting VFO-A back and calling `qmb_store()` again. If the initial recall fails (QMB unreadable/empty), the round trip still runs but restoration is honestly noted as not possible. |
| Quick Split | No dedicated on/off pair exists anywhere in the 91-command table (findings.md) and no getter exists either — called twice, which always nets identity for a pure toggle action with no alternate form. |
| Swap VFO | Reversible by construction: swap, verify VFO-A/B actually exchanged, swap back — inline self-undo, same pattern as `transmit`/`receive`. |
| Clarifier Clear, Clarifier Down, Clarifier Up | Full exact restoration achievable (see Findings' `RD`/`RU`-are-absolute-sets discovery) via `get_information().clarifier_offset_hz` snapshot + one computed `clarifier_clear`/`clarifier_down`/`clarifier_up` call in the final restore pass. |
| CW Keying (`KY`) | Gated behind the `DiagWarning`/`DiagCwCallsign` prompt (see Decision §2); sends `"TEST <CALLSIGN>"` via a keyer-memory write + playback, restoring the original message afterward (except the genuinely-vacant edge case — see Findings). `Skipped`, not attempted bare, when no callsign is supplied. |
| Zero In | Mutates no persisted state in this radio's implementation (see Findings) — `Ok`-only verification, the honest ceiling. |
| Band Select, Band Up, Band Down | `Ok`-only verification — no `get_band` getter exists anywhere on the trait (see Findings); VFO-A is restored by the final global restore regardless of what these do to it. |
| Encoder Down, Encoder Up | `Ok`-only verification — mutate no persisted state in this radio's implementation (see Findings), matching the manual's own silence on Hz-per-step mapping. |
| Ent Key | `Ok`-only verification — zero-width Action trigger with no persisted-state effect (confirmed via the emulator source), same category as `ZI`/`RC`. |
| Down (MIC DWN), Up | Already-existing `Radio::mic_down`/`mic_up` methods, previously unreachable by the read-only engine (zero-width Action, no query form) — called once each, `Ok`-only, relying on the final VFO-A restore (these do mutate `vfo_a_hz` by a fixed step, but nowhere near the saturating band edges under normal test conditions). |

Deliberately **out of scope** (left at their pre-existing get-only
treatment, not newly expanded): `AC` (antenna tuner state — `state=2`
plausibly starts a real low-power tuning cycle), `MX` (MOX — setting it ON
directly keys the transmitter), and DVS (`LM`/`PB` — start/stop
recording/playback overwrites a voice memory slot). None of these three
were among the 28 commands this round's full-parity work targeted, and
none has a getter/setter shape as safe as the 28 above — conservative,
disclosed scope, not an oversight.

### 4. Rendering: same shapes as before, retargeted types

`layout::draw_diagnostics_panel`/`draw_diagnostics_live` keep their exact
shape (scrolling list, live-progress auto-follow, completed-report
cursor-scroll, per-row detail line) — just retargeted from
`radio::Diagnostic*` to `crate::diagnostics::{DiagOutcome, DiagResult}`,
with `count_passed`/`count_failed`/`count_skipped` as shared free functions
(used identically for a growing in-progress slice and a finished
`DiagSummary`).

## Consequences

- `radio/Cargo.toml` and the root workspace no longer depend on
  `cat-diagnostics` at all (removed from `[workspace.dependencies]` and the
  temporary `[patch]` table too) — `ui/Cargo.toml` still depends on `radio`
  only, unchanged.
- `radio/src/ft991a.rs`'s test-only `AutoAckSession` fixture (existed
  solely to exercise the old `run_diagnostics_with` wiring) is removed
  along with the three tests that used it.
- New unit tests in `ui/src/terminal.rs` (`MockRadio`-based, per this
  repo's CLAUDE.md testing rule) cover structural/safety properties: the
  step count matches the engine's real output, the run completes without
  panicking even when nearly every method fails, the `KY` skip/attempt
  contract, and a real snapshot/restore round trip for the one field
  `MockRadio` backs with genuine state (`vfo_a`). Per-step
  pass/fail-correctness for FT-991A-specific state machine behavior (the
  other ~113 steps) is verified against the live `emulator` instead — see
  below — matching this repo's own precedent that deep protocol-behavior
  correctness belongs in emulator-level verification, not mocks.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
  warnings`, and `cargo test --workspace` are all clean (1000+ tests across
  the workspace). `cargo check --target x86_64-pc-windows-gnu -p ft991a`
  stays green (no OS-specific code in this feature).
- **Verified manually against the live `emulator`** (Linux, via `tmux`,
  `cargo run -p emulator -- --background --log-file`):
  - `[D]` from `Menu` -> red-bordered warning panel rendered correctly;
    `Esc` cancelled cleanly back to `Menu` with the emulator's command log
    unchanged (confirmed via the log file — nothing sent).
  - `[D]` -> proceed -> callsign `W1AW` -> run completed
    **114 passed / 0 failed / 0 skipped / 114 total**, including every one
    of the 28 previously-skipped commands now showing `OK` (`VM`, `CH`
    up/down, `QI/QR`, `QS`, `SV`, `RC`/`RD`/`RU`, `KY`, etc.). The `KY` row's
    detail line read `ok (sent "TEST W1AW")`; the emulator's own JSON
    command log confirmed exactly one `KY1;` command sent for the whole
    session.
  - `[D]` -> proceed -> blank callsign -> run completed
    **113 passed / 0 failed / 1 skipped / 114 total** — only the `KY` step
    skipped (reason: *"no callsign supplied — CW keying test requires
    station ID"*), every other step still ran and passed. The emulator's
    command log confirmed **zero** additional `KY` commands sent for that
    run (only the one `KY1;` from the earlier run with a callsign).
  - **Before/after state comparison**: the top status bar (`VFO A 14.000.000
    MHz USB`, `VFO B 14.100.000 MHz`, `AF`/`RF` bargraphs, `SQL: 0`,
    `PWR:100W`, `PS:ON`) was read before the first run, and was
    **byte-for-byte identical** after both runs completed and the screen
    returned to `Menu` — despite the runs having mutated VFO-A/B (multiple
    times, across `FA`/`FB`/`AB`/`BA`/`AM`/`MA`/`SV`/`QI`/`QR`/`CH`/band/
    encoder/mic-up-down steps), mode (`MD` steps cycling USB/LSB/CW),
    AF/RF gain, squelch, and TX power — demonstrating `restore_state`
    genuinely restores real emulator state, not just returning `Ok`. The
    emulator's `TX0;`/`TX1;` command counts (2× `TX1;`, one per run; 6×
    `TX0;`, matching the inline receive-after-transmit + explicit receive
    step + restore's own PTT-clear per run) independently corroborate the
    exact number of transmit-adjacent commands sent.
