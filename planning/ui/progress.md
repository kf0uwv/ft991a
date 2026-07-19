# UI Agent Progress

## Session: Wave 2 real TUI implementation

Note: this repo's `CLAUDE.md`/agent persona convention calls for a separate
`findings.md` alongside `task_plan.md`/`progress.md`. The harness's Write
tool declined to create a file named `findings.md` in this session
("subagents should return findings as text, not write report files").
Source-reading findings that would have gone there are folded into
`task_plan.md`'s "Design decisions made while reading source" section
instead, and restated in the final report back to the architect/dispatcher.

## Status: DONE

Steps:
1. [x] Read architect plan §6, `radio` crate source, Wave 1 `ui` placeholder,
   `ts570d/ui` reference files.
2. [x] Write task_plan.md documenting design decisions/judgment calls.
3. [x] Implement `ui/src/control.rs` (state machine, keybindings, validation).
4. [x] Implement `ui/src/layout.rs` (render functions).
5. [x] Implement `ui/src/terminal.rs` (run loop, terminal setup/teardown).
6. [x] Implement `ui/src/lib.rs` (Ft991aDisplay, module wiring, re-export).
7. [x] Update `ui/Cargo.toml` (ratatui/crossterm deps).
8. [x] `cargo build -p ui` clean. `cargo test -p ui`: 59 passed, 0 failed.
   `cargo clippy -p ui --all-targets -- -D warnings` clean. `cargo fmt -p ui
   -- --check` clean (after one `cargo fmt -p ui` pass). `cargo build
   --workspace` also clean (confirms no breakage of the `emulator`/`app`
   crates another agent landed concurrently in the same workspace).
9. [x] Report results back — done, see final message to dispatcher.

## Note on a concurrent change observed mid-session

Root `Cargo.toml`'s `[workspace] members` gained `"emulator"` partway
through this session (another agent's parallel work, per the task brief).
Not touched by this agent; `cargo build --workspace` confirms it's
unaffected by the `ui` changes.

## Session: Wave 4 Task 2 — grouped-menu skeleton

### Status: DONE

Steps:
1. [x] Read `planning/architect/task_plan.md` §11.2/§11.3/§11.6 item 2 in
   full, `radio/src/radio_trait.rs`'s real `Ft991aExtras`/`CwKeying`
   definitions, the current Wave 2 `ui/src/{control,layout,terminal,lib}.rs`
   in full, and `ts570d/ui/src/control.rs` (sibling checkout) for the
   `CommandGroup`/`GroupMenu`/`CommandKind` structural reference.
2. [x] Wrote `planning/ui/task_plan.md`'s new "Wave 4 Task 2" section and
   `findings.md`'s new section documenting design decisions/discrepancies
   before writing code.
3. [x] Rewrote `ui/src/control.rs`: `CommandGroup` (12 variants + key/label
   lookups), `ControlState::{Menu,GroupMenu,TextInput,ListSelect,Feedback}`,
   `GroupCommand`/`CommandKind` (renamed from the old flat `Command`), 12
   `{group}_commands()` functions (group 1 fully ported from the old flat
   table unchanged; groups 2-12 real, empty, doc-commented stubs pointing at
   the Wave 4 dispatch-queue item that will populate them), `handle_key`
   restructured for `Menu` -> `GroupMenu` -> `{TextInput,ListSelect}` ->
   `Feedback` (which always returns to `Menu`, matching `ts570d`).
4. [x] Updated `ui/src/layout.rs`'s `draw_control_panel` with `Menu`/
   `GroupMenu` render arms (group list; per-group command list or "no
   commands yet" placeholder + `[Esc] Back`); `TextInput`/`ListSelect`/
   `Feedback` 3-line layout unchanged.
5. [x] Widened `ui::run`'s bound in `ui/src/terminal.rs` to `Radio +
   Ft991aExtras + CwKeying + 'static`; added
   `impl Ft991aExtras for MockRadio {}` / `impl CwKeying for MockRadio {}`
   to both `terminal.rs`'s and `lib.rs`'s in-crate `MockRadio` test doubles.
6. [x] Updated `ui/src/lib.rs` module docs to disclose the FT-991A-only
   scope narrowing (§11.3 point 5); updated the compile-time bound-check
   helper and added a runtime test exercising the state machine against a
   value of the widened-bound `MockRadio` type.
7. [x] Verified `src/main.rs` needs **no edit** — confirmed by
   `cargo build --workspace` succeeding unchanged (`ui::run(radio).await`
   call site is textually identical; `Ft991a<SerialCatSession<SerialPort>>`
   satisfies the widened bound unconditionally per the `ModemControlLines`
   grep in `findings.md`).
8. [x] `cargo build -p ui` clean. `cargo test -p ui`: **68 passed, 0
   failed** (up from 59 — net +9 new tests; 13 of the old flat-`Normal`-
   state tests were renamed/adapted in place to `GroupMenu`-state
   equivalents since `ControlState::Normal` no longer exists, documented
   test-by-test via `git diff`, no test silently dropped — see
   `findings.md`/final report for the full before/after list).
   `cargo clippy -p ui --all-targets -- -D warnings` clean. `cargo fmt -p ui
   -- --check` clean (after one `cargo fmt -p ui` pass — 4 formatting fixes:
   one long `match` arm, one long function call, one long `assert_eq!`, one
   long `use` reordering + one multi-pattern match arm reflow). `cargo build
   --workspace` clean.
9. [x] Report results back — done, see final message to dispatcher.

## Session: Wave 4 Task 3 — populate groups 3/4/6 (Memory Channels,
## Clarifier/Tone/IF-Shift, Scan/VOX/Busy)

### Status: DONE

Steps:
1. [x] Read `planning/architect/task_plan.md` §11.2/§11.6 item 3 in full,
   confirmed groups 3/4/6's method sets directly against
   `radio/src/radio_trait.rs`'s `Radio` trait body and `radio/src/ft991a.rs`
   client implementation's doc comments (validation ranges), the current
   `ui/src/{control,terminal}.rs` skeleton in full, and
   `ts570d/ui/src/{control,terminal}.rs` (sibling checkout) for structural
   reference on the `on_off()`/`ReadMemoryChannel`/
   `WriteMemoryChannelFromVfoA`/`execute_action`-return-shape conventions.
2. [x] Wrote `planning/ui/task_plan.md`'s and `findings.md`'s new "Wave 4
   Task 3" sections documenting design decisions/discrepancies before
   writing code (confirmed §11.2's "100% `Radio`" claim holds exactly for
   all three groups — no discrepancy to flag there).
3. [x] Implemented `ui/src/control.rs`: `InputAction`/`SelectAction`/
   `ExecuteAction` variants for groups 3/4/6; real
   `memory_channels_commands()` (6 keys), `clarifier_tone_if_shift_commands()`
   (9 keys), `scan_vox_busy_commands()` (4 keys, `BY` deliberately
   keyless); `validate_text_input`/`select_action_to_execute`/
   `initial_list_cursor` grown accordingly; new helpers
   (`on_off_options`, `ctcss_tone_options`, `dcs_code_options`,
   `tone_squelch_mode_options`, `scan_state_options`,
   `get_memory_channel_immediate`, `clarifier_clear_immediate`,
   `parse_memory_channel`); ~52 new unit tests (reachability, per-key
   transitions, validation-range boundaries, cursor<->table-value lockstep).
4. [x] Implemented `ui/src/terminal.rs`: `execute_action`'s return type
   widened to `(&'static str, RadioResult<String>)` (needed to surface
   fetched values for the new read-type actions — see `findings.md`
   decision 7); new match arms for every new `ExecuteAction` variant, each
   calling the real `Radio` trait method; `run_loop`'s feedback-message
   construction updated for the new `Ok(String)` shape (adopted
   `ts570d::ui::terminal`'s own convention verbatim).
5. [x] `ui/src/layout.rs`/`ui/src/lib.rs` — confirmed no changes needed
   (groups 3/4/6 reuse `Menu`/`GroupMenu`/`TextInput`/`ListSelect`/
   `Feedback` rendering verbatim; no new `Ft991aDisplay` fields, a
   documented judgment call — see `findings.md`).
6. [x] Hit one `cargo clippy` failure (`enum_variant_names` on
   `SelectAction` once it grew past a single `Set*` variant) — fixed by
   renaming the 3 boolean-toggle variants to `Toggle*`, matching
   `ts570d::ui::control::SelectAction`'s own precedent (confirmed by
   reading it, not guessed).
7. [x] `cargo build -p ui` clean. `cargo test -p ui`: **120 passed, 0
   failed** (up from 68 — net +52 new tests, zero regressions, all 68
   pre-existing tests still pass unchanged). `cargo clippy -p ui
   --all-targets -- -D warnings` clean (after the `enum_variant_names`
   fix above). `cargo fmt -p ui -- --check` clean (after one `cargo fmt -p
   ui` pass). `cargo build --workspace` clean.
8. [x] Report results back — done, see final message to dispatcher.

## Session: Wave 4 Task 4 — populate group 5 (Keyer/CW/Break-In) including
## the RTS real-time CW-keying keybinding

### Status: DONE

Steps:
1. [x] Read `planning/architect/task_plan.md` §11.3 point 6 and §11.6 item
   4 in full, `ui/src/control.rs`'s landed `KeyerCwBreakIn` stub and
   groups 3/4/6's just-populated implementations as structural template,
   `ui/src/lib.rs`'s `Ft991aDisplay` field list, `radio/src/radio_trait.rs`
   (`Radio`'s batch-4 section, `Ft991aExtras`'s `KM`/`KY` section,
   `CwKeying`'s full definition) and `radio/src/ft991a.rs`'s client
   implementations for every validation range.
2. [x] Wrote `planning/ui/task_plan.md`'s new "Wave 4 Task 4" section and
   `findings.md`'s new section documenting design decisions/discrepancies
   before writing code (confirmed §11.2's trait-mix claim for group 5
   exactly, confirmed `assert_rts`'s signature exactly, found `SD`/`KS`
   have no step constraint unlike `VD`'s precedent).
3. [x] Implemented `ui/src/control.rs`: `InputAction`/`SelectAction`/
   `ExecuteAction` variants for group 5 (incl. `ToggleRts(bool)`); real
   `keyer_cw_break_in_commands()` (12 keys: `K` RTS toggle, `B`/`S`/`E`
   boolean-toggle `List`s, `D`/`W`/`P` numeric `Text`s, `Z` immediate
   zero-in trigger, `R`/`M`/`Y`/`J` keyer-memory read/write/play `Text`s);
   new helpers (`toggle_rts`, `zero_in_immediate`, `parse_keyer_channel`);
   `validate_text_input`/`select_action_to_execute`/`initial_list_cursor`
   grown accordingly; ~70 new tests.
4. [x] Implemented `ui/src/terminal.rs`: `execute_action`'s bound widened
   to `Radio + Ft991aExtras + CwKeying`, gained a `display: &mut
   Ft991aDisplay` parameter for the RTS optimistic-set/rollback logic
   (the only arm that mutates `display`); new match arms for all 12
   group-5 `ExecuteAction` variants; `run_loop`'s bound widened to match;
   `MockRadio`'s `CwKeying` impl filled in with a real `assert_rts`
   override branching on its existing `fail_all` flag; 3 new RTS tests
   (optimistic-success, rollback-on-error from `false`, rollback-on-error
   from `true`).
5. [x] Implemented `ui/src/lib.rs`: `Ft991aDisplay` gains `rts_asserted:
   bool` (default `false`, documented locally-tracked/never-polled); 1
   new test.
6. [x] Implemented `ui/src/layout.rs`: new `rts_label(bool) -> (&'static
   str, Color)` helper (mirrors `tx_state_label`'s shape); `draw_status`
   row 1 gains an `RTS: ON/OFF` indicator next to the existing `TX`/`RX`
   one; module doc updated; 2 new tests.
7. [x] Fixed one pre-existing Wave 4 Task 2 test whose premise broke once
   group 5 stopped being an empty stub (`test_stub_group_char_key_is_a_no_op`
   pressed `'z'`, which is now group 5's real zero-in key) — retargeted at
   a group still genuinely stubbed, added a group-5-specific replacement;
   updated `test_stub_groups_have_no_commands_yet`'s `populated` array.
8. [x] Hit two `cargo clippy`/`cargo fmt` findings during verification:
   `field_reassign_with_default` on a test that built `Ft991aDisplay`
   then mutated `rts_asserted` (fixed with struct-update syntax); 3
   `cargo fmt` reflow spots (fixed with `cargo fmt -p ui`).
9. [x] `cargo build -p ui` clean. `cargo test -p ui`: **175 passed, 0
   failed** (up from 120 — net +55 new tests, zero regressions, all 120
   pre-existing tests still pass, 2 of them updated in place per the
   documented fix above, not silently changed). `cargo clippy -p ui
   --all-targets -- -D warnings` clean (after the `field_reassign_with_
   default` fix). `cargo fmt -p ui -- --check` clean (after one `cargo
   fmt -p ui` pass). `cargo build --workspace` clean.
10. [x] Report results back — done, see final message to dispatcher.

## Session: Wave 4 Task 5 — populate group 2 (VFO/Memory Quick-Ops) and
## group 9 (Band/Step/Encoder)

### Status: DONE

Steps:
1. [x] Read `planning/architect/task_plan.md` §11.2/§11.6 item 5 in full,
   `ui/src/control.rs`'s landed `vfo_memory_quick_ops_commands`/
   `band_step_encoder_commands` stubs and group 5's populated
   implementation as structural template, `radio/src/radio_trait.rs`'s
   `Radio` batch-1/batch-8 sections and `Ft991aExtras`'s VFO-quick-ops and
   encoder/ENT-key sections, `radio/src/ft991a.rs`'s client implementations
   for every validation range/doc comment.
2. [x] Wrote `planning/ui/task_plan.md`'s new "Wave 4 Task 5" section and
   `findings.md`'s new section documenting design decisions/discrepancies
   before writing code (confirmed §11.2's trait-mix claims exactly for both
   groups, worked through the `ED`/`EU` dead-end question concretely rather
   than deferring it).
3. [x] Implemented `ui/src/control.rs`: `InputAction` gains
   `EncoderDown`/`EncoderUp`; `SelectAction` gains `SetBand`/
   `ToggleFineStep`; `ExecuteAction` gains 11 group-2 + 9 group-9 variants;
   real `vfo_memory_quick_ops_commands()` (11 keys, all `Immediate`) and
   `band_step_encoder_commands()` (9 keys: 1 `List` band select, 1 `List`
   fine-step toggle, 5 `Immediate` triggers, 2 `Text` encoder-nudge
   inputs); new helpers (`BAND_ORDER`/`band_options`, 16 small
   `_immediate` functions across both groups, `parse_encoder_selector`,
   `parse_encoder_steps`); `validate_text_input`/`select_action_to_execute`/
   `initial_list_cursor` grown accordingly; module doc comment updated;
   ~43 new unit tests (reachability, key-count+uniqueness, per-key
   transitions, cursor<->value lockstep, validation-range boundaries,
   no-op regression guards).
4. [x] Implemented `ui/src/terminal.rs`: `execute_action` gained 20 new
   match arms (11 group-2 + 9 group-9), all plain `ok_unit(radio.<method>
   (...).await)` passthroughs — no signature change needed (unlike group
   5's RTS arm), no new tests needed (matches Task 3's established
   division of test coverage between `control.rs` and `terminal.rs`).
5. [x] Confirmed `ui/src/layout.rs`/`ui/src/lib.rs` needed **no changes**
   from this task (neither group needs new `Ft991aDisplay` fields or
   render logic) — and confirmed via `git diff --stat` that this session's
   edits are limited to `ui/src/control.rs`/`ui/src/terminal.rs`, since
   both `layout.rs`/`lib.rs` already carried unrelated uncommitted changes
   from Wave 4 Task 4 at session start.
6. [x] Fixed one pre-existing test whose doc comment/premise went stale
   once group 2 stopped being an empty stub
   (`test_stub_group_char_key_is_a_no_op`, previously targeting
   `CommandGroup::VfoMemoryQuickOps`) — retargeted to
   `CommandGroup::AttenuatorNoiseAgcNotchFilter` (still genuinely
   stubbed), added dedicated no-op regression tests for groups 2/9.
7. [x] `cargo build -p ui` clean. `cargo test -p ui`: **218 passed, 0
   failed** (up from 175 — net +43 new tests, zero regressions, all 175
   pre-existing tests still pass, 1 of them retargeted in place per step 6
   above, documented, not silently changed). `cargo clippy -p ui
   --all-targets -- -D warnings` clean on the first pass (no new lint
   findings this task). `cargo fmt -p ui -- --check` found 4 reflow spots
   — fixed with `cargo fmt -p ui`, clean after. `cargo build --workspace`
   clean (confirms `src/main.rs` and the concurrently-uncommitted
   `radio`/`emulator` changes from other agents' sessions are unaffected).
8. [x] Report results back — done, see final message to dispatcher.

## Session: Wave 4 Task 6 — populate group 7 (Attenuator/Noise/AGC/Notch/
## Filter) and group 8 (Speech/Mic/Monitor)

### Status: DONE

Steps:
1. [x] Read `planning/architect/task_plan.md` §11.2/§11.6 item 6 in full,
   `ui/src/control.rs`'s landed `attenuator_noise_agc_notch_filter_commands`/
   `speech_mic_monitor_commands` stubs and groups 5/2/9's populated
   implementations as structural templates, `radio/src/radio_trait.rs`'s
   `Radio` batch-6/batch-7 sections (lines ~1444-1652) and `Ft991aExtras`'s
   contour/APF/manual-notch and parametric-mic-EQ sections (lines
   ~1932-2000), `radio/src/ft991a.rs`'s client implementations (lines
   ~1578-2064) for every validation range/doc comment, and
   `radio/src/ft991a_radio.rs`'s `SH_BANDWIDTH_TABLE`/`filter_bandwidth_hz`/
   `mode_family_for` (confirmed all four re-exported from `radio`'s crate
   root).
2. [x] Wrote `planning/ui/task_plan.md`'s new "Wave 4 Task 6" section and
   `findings.md`'s new section documenting design decisions before writing
   code — in particular, confirmed the `CO`/`BP` shared-P3-field parametric
   shape is already fully resolved into separate `get_*`/`set_*` method
   pairs at the `radio`-crate client boundary (Wave 3's own work), so no new
   `ControlState` variant was needed; and worked out `SH`'s filter-width
   `ListSelect` labeling from the static `SH_BANDWIDTH_TABLE` constant
   rather than live radio state, since `CommandKind::List`'s `options: fn()
   -> Vec<String>` has no `&Ft991aDisplay` parameter to read live mode from
   even if `NA`'s state were polled (it isn't).
3. [x] Implemented `ui/src/control.rs`: `InputAction` gains 5 group-7 +
   3 group-8 `Text`-backed variants; `SelectAction` gains 11 group-7 +
   3 group-8 `List`-backed variants; `ExecuteAction` gains 16 group-7 +
   6 group-8 variants; real `attenuator_noise_agc_notch_filter_commands()`
   (16 keys: `A P B L N R G U W F C H X Y M Z`) and
   `speech_mic_monitor_commands()` (6 keys: `G L S M V E`); new helpers
   (`PREAMP_ORDER`/`preamp_options`, `AGC_ORDER`/`agc_options`,
   `filter_width_options`); `validate_text_input`/`select_action_to_execute`/
   `initial_list_cursor` grown accordingly; module doc comment updated to
   list groups 1-9 populated, 10-12 remaining stubs; 69 new unit tests
   (reachability, key-count+uniqueness, per-key transitions, cursor<->value
   lockstep for `SetPreampMode`/`SetAgcMode`/`SetFilterWidthIndex`,
   validation-range boundaries for every new numeric field, no-op regression
   guards).
4. [x] Implemented `ui/src/terminal.rs`: `execute_action` gained 22 new
   match arms (16 group-7 + 6 group-8), all plain `ok_unit(radio.<method>
   (...).await)` passthroughs — no signature change, no new tests needed
   (matches Tasks 3/5's established division of test coverage between
   `control.rs` and `terminal.rs`: only actions with executor-side
   branching logic, like group 5's RTS optimistic-set/rollback, get
   `terminal.rs`-level tests).
5. [x] Confirmed `ui/src/layout.rs`/`ui/src/lib.rs` needed **no changes**
   from this task (neither group needs new `Ft991aDisplay` fields or render
   logic — this task does not extend polling, matching groups 2/9's own
   established "not polled yet" precedent) — confirmed via `git diff
   --stat` that this session's edits are limited to
   `ui/src/control.rs`/`ui/src/terminal.rs`.
6. [x] Fixed two pre-existing tests whose premise broke once groups 7/8
   stopped being empty stubs: `test_stub_groups_have_no_commands_yet`'s
   `populated` list (added both groups) and its stale header comment; and
   `test_stub_group_char_key_is_a_no_op` (previously targeted
   `CommandGroup::AttenuatorNoiseAgcNotchFilter` with key `'z'`, which
   became a real key — manual notch frequency — in this task), retargeted
   to `CommandGroup::MetersStatus` (still genuinely stubbed). Added
   dedicated `'Q'`-key no-op regression guards for groups 7/8.
7. [x] `cargo build -p ui` clean. `cargo test -p ui`: **287 passed, 0
   failed** (up from 218 — net +69 new tests, zero regressions, all 218
   pre-existing tests still pass, 2 of them updated in place per step 6
   above, documented, not silently changed). `cargo clippy -p ui
   --all-targets -- -D warnings`: found 1 `doc_lazy_continuation` lint on a
   doc comment (a `+`-prefixed line inside a bullet paragraph read as an
   unindented markdown list continuation) — fixed by rewording, clean after.
   `cargo fmt -p ui -- --check` found reflow spots in the new
   `terminal.rs` match arms — fixed with `cargo fmt -p ui`, clean after.
   `cargo build --workspace` clean (confirms `src/main.rs` and the
   concurrently-uncommitted `radio`/`emulator` changes from other agents'
   sessions are unaffected).
8. [x] Report results back — done, see final message to dispatcher.

## Wave 4 Task 7 — group 10 (Meters/Status) + group 11 (System/Tuner/DVS)

1. [x] Read `planning/architect/task_plan.md` §11.2/§11.6 item 7,
   `radio/src/radio_trait.rs`'s meters (`Radio` lines ~1036-1053,
   `Ft991aExtras` lines ~1841-1879) and batch-10 (`Radio` lines ~1730-1801,
   `Ft991aExtras` lines ~2021-2125) sections, and `radio/src/ft991a.rs`'s
   corresponding client implementations, in full, before writing code.
   Confirmed one discrepancy in §11.2's table prose (the `RS`/`UL` trait-
   backing description is swapped vs. the real source) — documented in
   `control.rs`, not silently corrected without a trace.
2. [x] Confirmed via `ui/src/terminal.rs`'s `poll_radio_state` (read
   directly) that none of group 10's 8 fields are part of the passive
   200ms polling loop or `Ft991aDisplay` — so this group needed real keys
   for all 8 fields, not a mostly-display screen with a couple of keys, per
   the task brief's own conditional instruction.
3. [x] Designed and confirmed the date/time (`DT`) multi-field entry
   flow: 3 separate keybindings (set + read pairs = 6 keys total), each
   with its own `TextInput`/format-specific parser
   (`parse_date`/`parse_time`/`parse_time_zone_offset`) — no new
   `ControlState` variant needed, since `radio/src/ft991a.rs` already
   splits the wire-level 3-shape `DT` command into 3 independent method
   pairs. The task brief's STOP-and-report condition ("if the underlying
   method genuinely can't be split into 3 independent calls") did not
   apply — checked before writing any code, not assumed.
4. [x] Implemented `ui/src/control.rs`: `InputAction` gained 6 group-11
   `Text`-backed variants; `SelectAction` gained 3 group-10 + 7 group-11
   `List`-backed variants; `ExecuteAction` gained 8 group-10 + 22 group-11
   variants; real `meters_status_commands()` (8 keys) and
   `system_tuner_dvs_commands()` (22 keys), replacing the `Vec::new()`
   stubs; `validate_text_input`/`select_action_to_execute`/
   `initial_list_cursor` grown with every new arm; new helper functions
   (meter/radio-indicator/repeater-shift/TX-VFO/antenna-tuner option
   lists, 14 `_immediate` functions, 5 parser functions); 87 new tests
   (reachability, key-count+uniqueness, per-key transitions, cursor<->value
   lockstep for every new `List` action, validation-range boundaries for
   dimmer/date/time/time-zone-offset/DVS-channel incl. valid+invalid cases
   for every new format, no-op regression guards).
5. [x] Implemented `ui/src/terminal.rs`: `execute_action` gained 30 new
   match arms (8 group-10 + 22 group-11), all plain `Radio`/`Ft991aExtras`
   passthroughs (some formatting a fetched value into feedback text, same
   shape Task 3 established for `GetMemoryChannel`) — no signature change,
   no new tests needed (matches Tasks 3/5/6's established division of test
   coverage: only actions with executor-side branching logic, like group
   5's RTS optimistic-set/rollback, get `terminal.rs`-level tests).
6. [x] Confirmed `ui/src/layout.rs`/`ui/src/lib.rs` needed **no changes**
   from this task (neither group needs new `Ft991aDisplay` fields or render
   logic — this task does not extend polling, matching groups 2/5/6/7/8/9's
   own established "not polled yet" precedent) — confirmed via `git diff
   --stat` that this session's edits are limited to
   `ui/src/control.rs`/`ui/src/terminal.rs`.
7. [x] Fixed two pre-existing tests whose premise broke once groups 10/11
   stopped being empty stubs: `test_stub_groups_have_no_commands_yet`'s
   `populated` list (added both groups) and its stale header comment; and
   `test_stub_group_char_key_is_a_no_op` (previously targeted
   `CommandGroup::MetersStatus` with key `'z'`, which is now populated),
   retargeted to `CommandGroup::ExMenu` (the one group still genuinely
   stubbed after this task). Added dedicated `'Q'`-key no-op regression
   guards for groups 10/11.
8. [x] `cargo build -p ui` clean. `cargo test -p ui`: **374 passed, 0
   failed** (up from 287 — net +87 new tests, zero regressions, all 287
   pre-existing tests still pass, 2 of them updated in place per step 7
   above, documented, not silently changed). `cargo clippy -p ui
   --all-targets -- -D warnings` clean on the first pass. `cargo fmt -p ui
   -- --check` found reflow spots (import list, a `const` array literal,
   several chained `.parse()` calls, a couple of long match arms/`vec!`/
   `assert_eq!` lines) — fixed with `cargo fmt -p ui`, clean after. `cargo
   build --workspace` clean (confirms `src/main.rs` and the concurrently-
   uncommitted `radio`/`emulator` changes from other agents' sessions are
   unaffected).
9. [x] Report results back — done, see final message to dispatcher.

## Session: Wave 4 Task 8 — `EX` menu number-entry escape hatch (path (b))

### Status: DONE

Steps:
1. [x] Read `planning/architect/task_plan.md` §11.4 in full (authoritative
   spec — the `ExNumberEntry`/`ExValueEntry` state machine, the `[X]`
   top-level key, the `Enumerated`/`Range` fork, the recommended
   read-first-then-edit UX).
2. [x] Confirmed the Wave 4 Task 1 `yaesu` prerequisite had already landed:
   `radio::Ft991aExtras::get_ex_menu_item`/`set_ex_menu_item` exist with the
   exact signatures §11.4 specified, and `ExMenuValueKind::Enumerated`
   already carries `(wire, label)` pairs (the label extension §11.4 flagged
   as a recommended addition), not the pre-Task-1 bare wire-string shape.
3. [x] Confirmed `radio::ex_menu_item(p1) -> Option<&'static ExMenuItem>`
   (a crate-level lookup helper wrapping `EX_MENU_TABLE.iter().find(...)`)
   already exists and is re-exported at the crate root — used it instead of
   re-deriving the same `EX_MENU_TABLE.iter().find(|i| i.p1 == p1)`
   expression §11.4's pseudocode writes inline, for DRY; functionally
   identical.
4. [x] **Discrepancy found and resolved, flagged in the final report**:
   §11.4's own `'[X]'` key notation collides with `CommandGroup::
   ScanVoxBusy`'s already-assigned top-level `group_key`, `'X'` (a Wave 4
   Task 2 judgment call the architect's plan text — written in the same
   session as the design but before this crate's actual single-letter key
   assignments existed — couldn't have anticipated). Chose `'N'`
   ("Number entry") as the replacement, verified unique against all 12
   group keys + `Q`.
5. [x] Implemented `ControlState::ExNumberEntry { buffer, error }` as a
   genuinely new state (not folded into `TextInput`), per the task's
   explicit shape requirement. `[N]`/`[n]` from `Menu` enters it; digits
   (max 3) grow `buffer`; `Backspace` shrinks it; `Enter` on an empty
   buffer or an unknown `p1` sets `error` and stays; `Enter` on a valid
   `p1` forks directly into `ListSelect` (`Enumerated`) or `TextInput`
   (`Range`) via a new `enter_ex_value_entry` helper — matching §11.4's own
   wording that `ExValueEntry` "reuses `ControlState::ListSelect`/
   `TextInput` verbatim" rather than being a third distinct `ControlState`
   variant.
6. [x] Added `InputAction::SetExMenuItem(u16)` /
   `SelectAction::SetExMenuItem(u16)` / `ExecuteAction::SetExMenuItem(u16,
   i32)` — each carries `p1` so the confirming code can re-look-up the item
   at validate/execute time without widening `TextInput`/`ListSelect`'s
   shared shape (used by ~30/~25 other actions) with a `&'static
   ExMenuItem` field.
7. [x] `validate_text_input`'s new `SetExMenuItem` arm generalizes off
   `item.kind`'s own `min`/`max`/`step` (via `ex_menu_item(p1)`), not a
   hardcoded per-field range — the exact generalization §11.4 asked for.
   `select_action_to_execute`'s new arm re-looks-up the item and indexes
   its `Enumerated` values by cursor, parsing the plain-decimal wire string
   directly (`ExMenuValueKind::parse`/`format` are `pub(crate)` to `radio`,
   not visible from `ui` — confirmed unsigned decimal digits are the only
   wire shape any landed `Enumerated` item uses, so a plain `str::parse`
   suffices).
8. [x] Wired `Esc` from the forked `TextInput`/`ListSelect` states back to
   `ExNumberEntry` (buffer restored to the already-entered `p1`) **only**
   when the action is `SetExMenuItem` — every other `TextInput`/
   `ListSelect` action's `Esc` still goes to `Menu` unchanged (2 new tests
   confirm both the EX-specific redirect and the non-EX regression guard).
9. [x] **Read-first-then-edit evaluated and skipped**, per §11.4's own
   "recommended, not required" framing — documented in
   `enter_ex_value_entry`'s doc comment: `handle_key` is synchronous with
   only `&Ft991aDisplay` access (no `&mut R: Radio`), and `Ft991aDisplay`
   has no per-`p1` `EX` value cache (unlike `SetMode`'s
   `initial_list_cursor`, which reads an already-polled `display.mode`
   field). Forcing it would mean either polling all 151 `EX` items every
   200ms cycle (wasteful for rarely-changed settings) or making
   `handle_key` async (a much larger, out-of-scope architecture change).
   Cursor/buffer default the same way every other not-yet-polled
   `ListSelect` field in this module already does.
10. [x] `ui/src/layout.rs` needed a **new match arm**, not confirmed-
    unchanged like Task 7: `draw_control_panel`'s two `match state`
    blocks are exhaustive over `ControlState`, so adding `ExNumberEntry`
    forced a compile error until a render arm was added. Folded it into
    the existing `TextInput`/`ListSelect`/`Feedback` "3-line layout" group
    (per §11.4's own "reuses the `TextInput` rendering shell" wording) with
    a fixed prompt line ("EX menu item number (001-153):") and the same
    buffer/error rendering `TextInput` already uses. Not in this task's
    "read first" list, but a mechanical, unavoidable knock-on change, not
    scope creep.
11. [x] Added 31 new tests across `ui/src/control.rs` (27) and
    `ui/src/terminal.rs` (4, including 2 full round-trip tests — one per
    `ExMenuValueKind` fork — through `handle_key` into `execute_action`
    into a `MockRadio` that records `set_ex_menu_item` call arguments).
    `MockRadio` gained an `ex_menu_calls: Vec<(u16, i32)>` field and an
    overridden (not default-`NotImplemented`) `set_ex_menu_item`, mirroring
    the `CwKeying::assert_rts` override precedent (Task 4) for testing a
    genuinely new, not-just-re-exposed `Ft991aExtras` capability.
12. [x] `cargo build -p ui` clean. `cargo test -p ui`: **405 passed, 0
    failed** (up from 374 — net +31 new tests, zero regressions, all 374
    pre-existing tests still pass unmodified). `cargo clippy -p ui
    --all-targets -- -D warnings` clean on the first pass. `cargo fmt -p ui
    -- --check` found reflow spots (a couple of `match` arm formattings, a
    let-else binding, an `assert_eq!`/`execute_action` call each spanning
    the 100-col limit) — fixed with `cargo fmt -p ui`, clean after. `cargo
    build --workspace` clean.
13. [x] Report results back — done, see final message to dispatcher.

## Session: Wave 4 Task 9 — `EX` menu themed browsing (path (a)) — Wave 4's
final task

### Status: DONE

Steps:
1. [x] Read `planning/architect/task_plan.md` §11.4's "(a) Themed browsing
   sub-groups" paragraph in full, plus §10.6 (the original sub-batching
   rationale it points back to).
2. [x] **Off-by-one resolved, flagged in the final report**: §11.4's prose
   calls this "five sub-groups" but then lists six labeled `p1` ranges
   (001-046/047-079/080-091/092-110/111-136/137-153). Counted the ranges
   actually given rather than trusting the stated number — implemented
   **six** `ExTheme` variants: `GeneralAgcCw`, `TxAudioChain`, `Mixed`,
   `RttySsbTxChain`, `MeterScope`, `BandLimitVox`.
3. [x] Verified the flagged 79/80 and 91/92 boundaries against
   `radio::EX_MENU_TABLE`'s actual item names (not the manual PDF directly
   — the table is already a faithful transcription per `radio`'s own
   findings): `079` "FM PKT MODE" -> `080` "RPT SHIFT 28MHz" and `091`
   "STANDBY BEEP" -> `092` "RTTY LCUT FREQ" both land on clean thematic
   breaks. No boundary adjustment needed; documented in `ExTheme`'s own doc
   comment with the item names as evidence, not just asserted.
4. [x] Confirmed the real per-theme item counts by extracting every `p1`
   from `EX_MENU_TABLE` (151 landed rows, `grep`+`sed` over the source,
   cross-checked against the file's own item count): 45/33/11/19/26/17
   across the six ranges (sums to 151), with the two previously-known
   permanent gaps — `027` "TIME ZONE" (falls in `GeneralAgcCw`, hence 45
   not 46) and `087` "RADIO ID" (falls in `Mixed`, hence 11 not 12) — both
   still absent, exactly as `radio`'s own module docs record. No other
   gaps found.
5. [x] Built `ex_theme_items(theme) -> Vec<&'static ExMenuItem>`,
   filtering `EX_MENU_TABLE` by `theme`'s `(lo, hi)` range **at call time**
   — genuinely computed from the real table per §11.4's explicit
   instruction, not a hand-copied parallel list. Same table-driven
   principle already established by `ctcss_tone_options`/`dcs_code_options`
   for their own tables (Wave 4 Task 3).
6. [x] **Top-level `EX` group UI shape, judgment call**: `CommandGroup::
   ExMenu`'s own `GroupMenu` screen (the "theme picker") lists all 6
   `ExTheme`s (`CommandKind::ExSubGroup`, keys G/T/X/R/S/B) **plus** the
   number-entry escape hatch folded in as a 7th entry
   (`CommandKind::EnterExNumberEntry`, key `N` — same key as
   `EX_NUMBER_ENTRY_KEY`'s existing dedicated `Menu`-level binding from
   Task 8, left unchanged and still reachable both ways). Chose "fold in
   AND keep the top-level shortcut" over picking one, so users who already
   know a `p1` keep the fast path while users browsing discover the escape
   hatch without needing to already know `[N]` exists from `Menu`.
7. [x] Added `ControlState::ExSubGroupMenu { theme, cursor }` as a new,
   genuinely-scrollable state — confirmed first (per the task's own read-
   list item 4) that `GroupMenu`'s own `cursor` field is still vestigial/
   unwired in this codebase (never mutated outside its `cursor: 0`
   initializers) and in `ts570d`'s reference precedent. Since the largest
   sub-group (`GeneralAgcCw`, 45 items) can't be single-char-keyed (>36
   possible chars) or usefully shown unscrolled on a typical terminal,
   `ExSubGroupMenu` is the state that gets real `Up`/`Down` (plus `j`/`k`)
   cursor movement, clamped at both ends, with `Esc` returning to the
   theme picker (`GroupMenu { group: ExMenu, .. }`).
8. [x] `ui/src/layout.rs` gained `draw_ex_sub_group_menu`, a genuinely
   scrolling render (sliding window centered on `cursor` when the sub-group
   is longer than the available height) — the first scrolling list in this
   crate, since `GroupMenu`'s own command column never needed one. Header
   line shows the theme label + "item N of M"; footer shows the
   `Up/Down`/`Enter`/`Esc` hint.
9. [x] **Convergence**: both paths fork through the exact same
   `enter_ex_value_entry` helper Task 8 built (not duplicated) — extended
   its signature with an `origin: ExValueEntryOrigin` parameter
   (`NumberEntry` vs. `Theme(theme, cursor)`) so `Esc` from the fork can
   route back correctly per §11.4's own pseudocode ("`Esc` -> back to
   `ExNumberEntry` (path (b)) or the `EX` group screen (path (a))") — the
   *only* behavioral difference between paths. Implemented via a new
   `SetExMenuItemFromTheme(u16, ExTheme, usize)` variant on both
   `InputAction`/`SelectAction`, sharing every other match arm with the
   existing `SetExMenuItem(u16)` variant via `|`-combined patterns (no
   validation/execution logic duplicated). Two new tests
   (`test_path_a_and_path_b_converge_for_enumerated_item_60`/
   `..._range_item_1`) drive both paths for the same `p1` and assert
   identical `ListSelect` options/`TextInput` prompts and identical
   final `ExecuteAction`s.
10. [x] Added 20 new tests to `ui/src/control.rs`: theme-range bucketing
    (partition/no-overlap/gap spot-checks), theme-picker keys reachable
    and unique, sub-group cursor movement (Up/Down/j/k, both clamps),
    item-selection forks (`Enumerated`/`Range`), the full `Esc` chain
    (value-entry -> `ExSubGroupMenu` -> theme picker -> `Menu`), and the 2
    path-(a)-vs-path-(b) convergence tests. Also replaced 2 now-stale
    tests (`test_stub_groups_have_no_commands_yet`/
    `test_stub_group_char_key_is_a_no_op`, both premised on `ExMenu` still
    being an empty stub) with `test_all_twelve_groups_are_populated`/
    `test_ex_menu_group_unbound_char_key_is_a_no_op` — net test count
    change is +20, not +22.
11. [x] `cargo build -p ui` clean. `cargo test -p ui`: **425 passed, 0
    failed** (up from 405 — net +20, zero regressions). `cargo clippy -p ui
    --all-targets -- -D warnings` clean on the first pass. `cargo fmt -p ui
    -- --check` found one reflow spot (a `Span::styled` call in
    `draw_ex_sub_group_menu` over the line-length limit) — fixed with
    `cargo fmt -p ui`, clean after.
12. [x] Did not touch `radio/`, `emulator/`, or `src/main.rs` — confirmed
    via `git status --short` that this task's own diff is confined to
    `ui/src/control.rs` and `ui/src/layout.rs` (other files showing as
    modified in the working tree are pre-existing, uncommitted output from
    earlier Wave 4 tasks in this same session, not touched here).
13. [x] Report results back — this completes Wave 4's entire dispatch
    queue (all 12 `ui` groups populated, both `EX` access paths landed).
