# Research notes (ts570d parity diagnostics)

## ts570d/ui/src/terminal.rs pattern (fully read, 2556 lines)
- `RadioSnapshot` struct: every settable+gettable field as `Option<T>`, filled via
  `snapshot_state()` (each getter `.ok()`-mapped, failures -> None, snapshot always succeeds).
- `restore_state()`: unconditional at end, best-effort (`let _ =` on every setter).
  Order: receive() first (clear PTT) -> most setters in snapshot order -> memory_ch0
  special-cased (vacant -> clear_memory_channel(0), else write_memory_channel(0, entry))
  -> antenna/keyer/cw/etc -> power_on LAST.
- `run_diagnostics_task`: snapshot first; LABELS: &[&str] const array (107 for ts570d);
  `'outer: for round in 1..=DIAG_ROUNDS` (DIAG_ROUNDS=3) { for each step_idx, check_esc()
  break both loops, `match step_idx { N => { ... } }` }. After all rounds: restore_state()
  unconditionally, then DiagDone.
- Three macros for common shapes: `diag_get!` (call getter, Ok=pass), `diag_action!` (call
  action/setter-only, Ok=pass), `diag_set_get!` (set X then get, compare == target).
  Bespoke inline blocks used when the step needs custom verification (e.g. set_vfo_a verifies
  range, then separately compares to target; if_shift needs tuple compare; transmit calls
  receive() immediately after to self-undo inline (56); IF cross-checks re-verify via
  get_information() to catch encoding bugs).
- `DIAG_STEP_COUNT: usize = 107` const, used by UI for progress "N/total".
- Channel pattern: `Chan<T> = Rc<RefCell<VecDeque<T>>>` (single-threaded, cooperative).
  `RadioUpdate::DiagProgress { label: &'static str, round, passed, detail }` sent per step
  (both to a `results` Vec local to the task AND via channel for live UI); `RadioUpdate::DiagDone`
  sent at the very end after restore_state completes.
- UI side: `ControlState::Diagnostic(DiagState)`; `DiagState::{Idle, Running{current_label,
  current_round,results}, Done{results,scroll}}` (diag.rs). `DiagResult{label:&'static str,
  round,passed,detail:String}`. KeyResult::StartDiag on 'D' from Menu transitions to Running
  and sends RadioCmd::StartDiagnostics. Esc while Running is polled via `check_esc()` INSIDE
  the diag task's loop (non-blocking event::poll) -- note this means the diag task itself
  polls stdin directly (single-threaded coop model), not the UI task filtering it out.
  Done state: Esc -> back to Menu; Up/Down/PgUp/PgDn -> scroll.
- Rendering (layout.rs draw_diag_panel): Running shows "Running... (N/total commands x R
  rounds)" + "Now testing: <label> [round C/R]" + scrolled tail of per-round summary lines +
  "[Esc] abort" hint. Done shows "Complete: X/Y passed, Z failed" + per-label summary (OK
  green / FAILED red with indented per-round failing detail) + scroll hint.
  build_summary_lines groups multi-round results per unique label; a label is OK only if
  ALL rounds passed.

## ft991a domain findings (radio_trait.rs / ft991a_radio.rs / ft991a.rs)

- 91 commands confirmed in `FT991A_COMMAND_TABLE` (ft991a_radio.rs lines 1707-1933).
- `MemoryChannelEntry` has NO `vacant` flag (unlike ts570d) and there is NO
  `clear_memory_channel` method at all in this trait. `read_memory_channel(ch)` always
  returns a full entry (channels 1-117, no channel-0 VFO sentinel in MR/MW's own numbering).
  => restore is simpler than ts570d: always snapshot Option<MemoryChannelEntry>, always
  write it back verbatim on restore (no vacant/clear branching needed).
- **Clarifier (RD/RU) are ABSOLUTE sets, NOT relative increments** — confirmed via
  ft991a_radio.rs emulator impl comments at RD/RU ("Absolute set, not an incremental
  step") and radio_trait.rs's own doc comments on `clarifier_down`/`clarifier_up`
  ("Set the shared clarifier offset to offset_hz below/above the tuned frequency... An
  absolute set, not an incremental step"). This CONTRADICTS the task brief's assumption
  that exact restore is impossible. `Ft991aExtras::get_information()` (ChannelStatusFields)
  exposes the exact signed `clarifier_offset_hz: i16` (P3) plus rx/tx clarifier on/off —
  so full, exact restore IS achievable: snapshot clarifier_offset_hz + rx/tx on flags via
  get_information(); to restore, call clarifier_clear() if 0, else clarifier_down(|offset|)
  if negative or clarifier_up(offset) if positive, then restore rx/tx on flags.
- `KY` (CW Keying) is NOT "send arbitrary CW text" (unlike ts570d's `send_cw`) — it is
  `Ft991aExtras::play_keyer_memory(channel: 1-5, mode: KeyerPlaybackMode)`, which triggers
  playback of a **pre-stored `KM` keyer-memory message** (`write_keyer_memory`/
  `read_keyer_memory`, channel 1-5, message 1-50 printable ASCII chars, no `;`). There is
  no zero-length/no-op payload — `write_keyer_memory` client-side-validates length 1..=50
  BEFORE any wire I/O (rejects "" locally, never reaching the radio). Default/factory KM
  channel state (`Ft991aState::keyer_memories` initializer) is `String::new()` (empty) for
  all 5 channels — so the "channel was vacant, can't restore to vacant" edge case is a real,
  reachable scenario against the emulator, not hypothetical.
  => Plan: snapshot `read_keyer_memory(1)` (Option<String>). Test: write the operator-
  supplied "TEST <CALLSIGN>" message to channel 1, call `play_keyer_memory(1, KeyerMemory)`,
  verify Ok. Restore: if snapshot was non-empty, write it back verbatim; if snapshot was
  empty (vacant), we CANNOT restore to empty via `write_keyer_memory` (client-side rejects
  ""), so this is a genuine, documented residual limitation (only in that one edge case).
- Quick Split (`QS`) toggles `Ft991aState::split: bool` via a pure `!self.state.split` XOR,
  confirmed in emulator (`Qs => self.state.split = !self.state.split`). No getter exists
  anywhere (not even via get_information()) for split state, and findings.md confirms no
  dedicated on/off pair exists in the master table ("QS... no dedicated split on/off
  command exists... implemented as a plain boolean toggle" — planning/yaesu/findings.md).
  => Safe to call quick_split() twice (verify Ok both times) — two calls always net-identity
  since it's specified/implemented as a pure toggle action with no alternate on/off form.
- `[V/M]` Key Function (`VM` -> `toggle_vfo_memory_mode`) toggles `Ft991aState::channel_select`
  but via `if channel_select == 0 {1} else {0}` — this COLLAPSES any non-zero starting value
  (e.g. QMB modes 3/4 surfaced via `get_information().select`) to 0 on the first call, so a
  double-toggle only restores exactly when the starting `select` was 0 or 1. `select` (P7,
  ChannelStatusFields) is readable via `get_information()`.
  => Plan: read `get_information().select` first. If 0 or 1: toggle, verify flip, toggle
  again, verify restored — full coverage. If 2-6 (QMB/other non-VFO/Memory mode): SKIP with
  documented reason ("current select mode cannot be exactly restored after VM, which
  collapses any non-VFO/Memory state to VFO on toggle") rather than corrupt an
  unrestorable state.
- Channel Up/Down (`CH` -> `memory_channel_up`/`memory_channel_down`): confirmed manual
  gives no boundary behavior; emulator wraps at 1..=117 (judgment call, findings.md).
  `get_memory_channel()`/`set_memory_channel()` exist -> snapshot index, call up, verify
  wrap-aware expected value, restore via `set_memory_channel(orig)` (absolute set, per task
  guidance, not stepping back down).
- QMB Store/Recall (`QI`/`QR` -> `qmb_store`/`qmb_recall`): dedicated single-slot storage
  (`Ft991aState::qmb: MemoryChannelRecord`), NOT one of the 117 numbered channels
  (findings.md, confirmed via IF's own P7 legend: 3=QMB,4=QMB-MT vs 1=Memory). No direct
  "read QMB slot" method exists — only via `qmb_recall()` which overwrites VFO-A. Both
  qmb_store/qmb_recall take NO channel/other args (always operate on the one QMB slot vs.
  VFO-A). Plan: `qmb_recall()` first to pull the QMB's *current* contents into VFO-A (capture
  as our own local "orig QMB" snapshot of freq+mode, since VFO-A itself is separately
  snapshotted/restored anyway); if recall fails (Err), treat as "QMB unreadable/empty",
  proceed with store+recall round-trip test only, document no-restore-possible. If recall
  succeeds: set VFO-A to test freq/mode, qmb_store() (overwrites QMB with test data), verify
  via qmb_recall()+get_vfo_a() == test values, then restore original QMB contents by setting
  VFO-A back to the captured orig values and calling qmb_store() again. Final restore_state
  fixes VFO-A itself back to its true pre-run value regardless.
- Band Select/Up/Down (`BS`/`BU`/`BD` -> `set_band`/`band_up`/`band_down`): **no `get_band`
  getter exists at all** in the trait (checked radio_trait.rs Radio trait method list in
  full — confirmed absent). Only observable indirectly via `get_vfo_a()` frequency (BU/BD
  step `selected_band` in the emulator but that's not itself exposed by any getter either —
  confirmed no `Radio::get_band`/no band field on `ChannelStatusFields`). Verification must
  be frequency-based: Band Select(FourteenMHz) then check `get_vfo_a().hz()` falls in the
  14.000-14.350 MHz ham-band range is NOT reliable — selecting a band on a real/emulated
  radio does not necessarily *retune* VFO-A into that band's edges (it only changes which
  band's own remembered frequency/mode becomes active — band-stacking register concept).
  Given no `get_band` getter exists anywhere, honest choice: verify Ok only for `set_band`/
  `band_up`/`band_down` (can't independently verify the band actually changed without a
  getter) — document as a known trait-surface gap, not guessed at. VFO-A's frequency is
  restored by the final snapshot/restore regardless of what BS/BU/BD did to it.
- Encoder Down/Up (`ED`/`EU`) and Ent Key (`EK`) and Zero In (`ZI`): confirmed via emulator
  source that **none of these mutate any persisted `Ft991aState` field** (module docs:
  "structurally and semantically validated... but mutate no persisted Ft991aState field —
  see module docs' ED/EU section for why (no Hz-per-step mapping is knowable from this
  manual page alone)"; ZI/EK are zero-width Action triggers with no state effect either).
  => verify Ok only for all four (no getter can ever observe an effect, by the emulator's
  own documented design — real hardware behavior is genuinely undocumented/context-
  dependent per the manual, consistent with the task's "if genuinely unpredictable, fine to
  leave skipped/Ok-only" guidance). NOT left `Skipped` — `diag_action!`-style Ok-only checks
  are still real coverage (mirrors ts570d's own treatment of `mic_up`/`mic_down`/`zero_in`-
  shaped actions).
- Down (MIC DWN) / Up (`DN`/`UP` -> `Radio::mic_down`/`mic_up`): DO mutate `vfo_a_hz` by a
  fixed `MIC_STEP_HZ`, saturating at the `FA` range edges (30_000..=470_000_000 Hz) — these
  ARE already Radio-trait methods (not new), just previously skipped by the read-only
  engine since DN/UP have no query form. Treat exactly like ts570d's own mic_up/mic_down
  steps (100/101): call each once, verify Ok only, rely on final VFO-A restore (safe in
  normal test conditions, nowhere near the saturating edges).

## Key architectural decision to carry over
- Diagnostics logic (snapshot/steps/restore) lives in `ui` crate (terminal.rs), generic
  over `R: Radio` (ts570d) — for ft991a it must be generic over `R: Radio + Ft991aExtras +
  CwKeying` per task instructions, matching ui::run's existing bound.
- ts570d does NOT have any restriction analogous to ft991a's current `cat-diagnostics`
  dependency — it hand-writes everything, confirming full parity is architecturally sound
  to place in ft991a's `ui` crate.

