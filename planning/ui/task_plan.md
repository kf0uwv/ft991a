# UI Agent Task Plan

## Goal
Build the ratatui/crossterm terminal interface for FT-991A control,
depending only on the shared `cat-framework` crate (transitively) and this
repo's `radio` crate (its `Radio` trait and domain types) — never on a
transport or session crate.

## Current Status: Wave 2 — real TUI implementation (unblocked)

Preconditions cleared: `radio` crate's 11-command first slice landed and
committed (`e3698cf`); architect Wave 2 planning pass designed the
right-sized TUI in `planning/architect/task_plan.md` §6 (read in full).
`emulator` does not exist yet (a different agent is building it in
parallel) — do not touch it.

## Spec source of truth

`planning/architect/task_plan.md` §6 (lines ~434-664). Where the plan's
prose and the actual `radio` crate source (`radio/src/radio_trait.rs`,
`radio/src/ft991a.rs`) differ, the crate source wins — flag discrepancies,
don't silently improvise.

## Design decisions made while reading source (before writing code)

1. **Squelch range confirmed 0-100**, not 0-255 — `ft991a.rs`'s
   `get_squelch`/`set_squelch` doc comments say "range 000-100". AF/RF gain
   confirmed 0-255 per their own doc comments. Frequency range confirmed
   `Frequency::MIN_HZ` (30_000) / `MAX_HZ` (470_000_000) in
   `radio_trait.rs`. Power (PC) confirmed 5-100 watts per `ft991a.rs`'s
   `get_power`/`set_power` doc comments ("005-100").
2. **`run` signature**: current placeholder (`ui/src/lib.rs` line 49) is
   already `run<R: Radio + 'static>(radio: R) -> UiResult<()>` — takes by
   value, not `&mut`. Task prompt confirms this is correct and must not
   change (matches `app`'s later `main.rs` call site expectation). Note:
   root `CLAUDE.md`'s architecture diagram prose says
   `ui::run<R: Radio>(radio: &mut R)` — this is stale/aspirational prose
   describing an earlier sketch, not binding; the actual placeholder
   signature (by value) is what's real and what the task prompt directs me
   to preserve. Flagging this as a discrepancy in my final report, not
   silently resolving it either direction beyond what the task explicitly
   directs.
3. **Polling loop architecture — judgment call**: `ts570d/ui/src/terminal.rs`
   splits into two `monoio::spawn`-ed tasks (radio task + UI task) linked by
   `Rc<RefCell<VecDeque<T>>>` channels, specifically so key events stay
   responsive while a slow poll cycle is in flight. Architect §6.6 cites
   specific line numbers in that file (130-160 signature, 2101/2143
   poll-cadence, 2209/2254 event-poll/sleep cadence) but does not mandate
   reproducing the two-task/channel plumbing itself — only the timing
   shape. Given this slice's much smaller poll set (10 calls vs. ts570d's
   ~21) and the "right-sized, not a port" mandate from §6.1, I'm
   implementing a **single sequential loop** (no `monoio::spawn`, no
   channels): poll every 200ms, redraw, `event::poll` for up to 10ms,
   execute any resulting action inline, sleep 5ms. This is simpler and
   still meets every explicitly-specified requirement (signature, 200ms
   poll cadence, 10ms/5ms event timing, 3-consecutive-failed-cycles
   disconnect logic). Flagging this as a judgment call in my report.
4. **Fail-cycle threshold**: ts570d uses `FAIL_THRESHOLD = 10` errors out of
   ~21 poll calls (roughly half) to mark one cycle "failed", then 3
   consecutive failed cycles → `connected = false`. Scaling proportionally
   for this slice's 10 poll calls, I'm using `FAIL_THRESHOLD = 5` (a
   cycle counts as failed if at least half the 10 polls errored). Not
   specified explicitly in §6 — flagged as a judgment call.

## File layout to produce (per §6.7)

- `ui/src/lib.rs` — `UiError`/`UiResult` (keep), add `Ft991aDisplay` (§6.2),
  re-export `run`, declare `mod control; mod layout; mod terminal;`.
- `ui/src/layout.rs` — `split_areas`, `draw_header`, `draw_errors`,
  `draw_disconnected` (near-verbatim port from ts570d), `draw_status` (new,
  collapsed 2-row body per §6.3), `draw_control_panel` (simplified,
  `Normal`/`TextInput`/`ListSelect`/`Feedback` only).
- `ui/src/control.rs` — `ControlState`/`InputAction`/`SelectAction`/
  `ExecuteAction`/`handle_key` per §6.4, flat 9-entry keybinding table per
  §6.5.
- `ui/src/terminal.rs` — `run`/`poll_radio_state`/terminal setup-teardown
  per §6.6 (single-loop design, see decision 3 above).
- `ui/Cargo.toml` — add real `ratatui`/`crossterm` deps per §6.8.
- No `diag.rs` (§6.1 — out of scope this wave).

## Verification plan

- Unit tests in `control.rs` (state transitions, validation ranges — mirrors
  `ts570d/ui/src/control.rs`'s test module shape) and small pure rendering-
  decision helpers (e.g. TxState → label/color mapping) — no live
  terminal/serial port needed.
- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt --check` all clean before reporting done.

## Constraints reaffirmed

- Do not touch `radio/`, `emulator/` (doesn't exist), `src/main.rs`, root
  `Cargo.toml` `[dependencies]`/`[[bin]]` (workspace `members` already lists
  `ui`, confirmed by reading it — no edit needed there either).
- Do not touch `ts570d` or `radio-cat-rs` (read-only reference).
- Do not commit.

## Session: Wave 4 Task 2 — grouped-menu skeleton

### Spec source of truth

`planning/architect/task_plan.md` §11.2 (12 groups), §11.3 (trait-bound
widening), §11.6 dispatch queue item 2. `radio/src/radio_trait.rs` (lines
1842-2200, real `Ft991aExtras`/`CwKeying` definitions) confirmed to match
the plan's prose closely — both `#[async_trait(?Send)]`-annotated,
`Ft991aExtras`'s ~20 re-export methods + `get_ex_menu_item`/
`set_ex_menu_item`, `CwKeying`'s 5 plain sync `&self` methods. Both
re-exported from `radio::{Ft991aExtras, CwKeying}` (`radio/src/lib.rs`).
`ts570d/ui/src/control.rs` read in full for the `CommandGroup`/`GroupMenu`/
`CommandKind` structural reference (it lives at
`/home/mattfranklin/src/github.com/kf0uwv/ts570d`, a sibling checkout, NOT
nested inside this repo — the architect plan's relative-path phrasing was
initially ambiguous about this, resolved by `find`).

### Design decisions made while reading source (before writing code)

1. **`ts570d`'s `GroupMenu{group,cursor}.cursor` field is vestigial in the
   actual reference code**, not a working scroll implementation — grepped
   the whole `ts570d/ui/src` tree: `cursor` is initialized to `0` on every
   `GroupMenu` construction, pattern-matched with `..` everywhere else, and
   never modified by any key handler (`ts570d` selects group commands via
   direct digit keys `1`-`9`/`a`-`c`, not cursor movement) or read by any
   render function. The architect plan's phrase "`GroupMenu{group,cursor}`'s
   scrolling behavior" describes an aspiration for larger groups (e.g. the
   future `EX` sub-groups), not code that exists today. Porting "in spirit"
   here means: keep the `cursor: usize` field for structural/future parity,
   initialize it to `0`, but do NOT invent scroll-key handling that has no
   working precedent to port from. Flagged as a discrepancy in my report.
2. **Group 1's internal command keys stay the original letters
   (F/B/M/T/A/R/S/P/O), not `ts570d`'s digit-based `1`-`9`/`a`-`c` scheme.**
   §11.2's explicit text: "the existing flat 9-command screen becomes
   literally `CommandGroup::FrequencyLevels`'s contents, unchanged in
   validation/**keys**" — read literally, not just "same semantics." Since
   `Menu`-state group-selection keys and `GroupMenu`-state command-selection
   keys are different `match` arms on `ControlState`, there is no namespace
   collision even where a group's top-level entry letter reuses a letter
   also used as an internal command key in a different group (e.g. `M` is
   both the `MemoryChannels` group's entry key and `FrequencyLevels`'s
   internal "Set mode" key) — verified this compiles and behaves correctly
   before finalizing the key table below.
3. **Top-level group keys chosen** (13 total incl. `Q`=Quit, all unique):
   `F`=Frequency & Levels, `O`=VFO/Memory Quick-Ops, `M`=Memory Channels,
   `C`=Clarifier/Tone/IF-Shift, `K`=Keyer/CW/Break-In, `X`=Scan/VOX/Busy
   (`S` reserved for System/Tuner/DVS), `A`=Attenuator/Noise/AGC/Notch/
   Filter, `P`=Speech/Mic/Monitor, `B`=Band/Step/Encoder, `T`=Meters/Status,
   `S`=System/Tuner/DVS, `E`=`EX` Menu. Not specified by the architect
   plan (only group names/order were) — a judgment call, documented in
   `control.rs`'s `group_key` doc comment.
4. **Groups 2-11 stub bodies are empty `Vec::new()`s**, one dedicated
   function per group (mirrors `ts570d`'s one-function-per-theme pattern),
   each doc-commented with which Wave 4 dispatch-queue item (§11.6) will
   populate it — not `todo!()`/`unimplemented!()`, satisfying "genuinely
   testable end-to-end" (entering an empty group renders a "no commands
   yet" placeholder and `Esc` still returns to `Menu`).
5. **`ui::run`'s bound widens exactly as specified**: `Radio + Ft991aExtras
   + CwKeying + 'static`. Confirmed by building: `src/main.rs`'s call site
   (`ui::run(radio).await`, `radio: Ft991a<SerialCatSession<SerialPort>>`)
   needed zero changes — `SerialPort` and `SerialCatSession<T: Transport +
   ModemControlLines>` both implement `ModemControlLines` unconditionally
   in `radio-cat-rs` (grepped `cat-transport-serial/src/{io_uring,
   session}.rs` to confirm directly, not just trusted the plan's assertion).
6. **Splitting skeleton-plumbing from group-1-porting**: per the task
   brief's own suggestion, tracked as two logical passes within this one
   task (not two separate dispatched tasks) — (a) `CommandGroup`/
   `ControlState`/`run` bound/`MockRadio` plumbing, (b) the group-1 content
   port — but delivered together since they touch the same few files and
   splitting the actual commits would not reduce review surface here.

### File layout touched

- `ui/src/control.rs` — `CommandGroup` (12 variants + key/label lookup
  tables), `ControlState::{Menu,GroupMenu,TextInput,ListSelect,Feedback}`
  (no `Diagnostic`), `GroupCommand`/`CommandKind` (renamed from the old flat
  `Command`), 12 `{group}_commands()` functions (1 populated, 11 stubs),
  `handle_key` restructured for the `Menu`->`GroupMenu` hop.
- `ui/src/layout.rs` — `draw_control_panel` gains `Menu`/`GroupMenu`
  render arms (replacing the old flat `Normal` arm); `TextInput`/
  `ListSelect`/`Feedback` 3-line layout unchanged.
- `ui/src/terminal.rs` — `run`'s bound widens; `MockRadio` gains
  `impl Ft991aExtras for MockRadio {}` / `impl CwKeying for MockRadio {}`.
- `ui/src/lib.rs` — module docs updated to disclose the FT-991A-only
  scope narrowing; `MockRadio` gains the same two impls; bound-check helper
  updated.
- `src/main.rs` — read-only verification, no edit (confirmed above).

### Verification plan

- `cargo test -p ui`, `cargo clippy -p ui --all-targets -- -D warnings`,
  `cargo fmt -p ui -- --check`, plus `cargo build --workspace` to confirm
  `main.rs` genuinely needs no change.
- New tests: all 12 groups reachable from `Menu` + `Esc` back; group 1's
  9 commands behave identically to the old flat screen (validation ranges
  preserved) — old flat-screen tests updated in place (state changed from
  `ControlState::Normal` to `ControlState::GroupMenu{group:
  FrequencyLevels,..}`, documented per-test, not silently dropped); a
  compile-time+runtime check that `MockRadio` satisfies the widened
  `Radio + Ft991aExtras + CwKeying + 'static` bound.

## Session: Wave 4 Task 3 — populate groups 3/4/6 (Memory Channels,
## Clarifier/Tone/IF-Shift, Scan/VOX/Busy)

### Spec source of truth

`planning/architect/task_plan.md` §11.2 (group definitions/table, confirms
groups 3/4/6 are "100% `Radio`") and §11.6 dispatch queue item 3 ("Populate
groups 3 (Memory Channels), 4 (Clarifier/Tone/IF-Shift), 6 (Scan/VOX/Busy)
— the three groups that are 100% `Radio`-trait-backed... Depends on task
2."). `radio/src/radio_trait.rs`'s `Radio` trait body read directly (lines
~1093-1443, the memory-channel/clarifier/RIT-XIT/IF-shift/tone/scan/VOX/BY
sections) to confirm every method these groups need really is on `Radio`,
not `Ft991aExtras` — **confirmed, no discrepancy**: `get/set_memory_channel`,
`read/write_memory_channel`, `read/write_memory_channel_tag`,
`get/set_rx_clarifier_on`, `get/set_tx_clarifier_on`, `clarifier_clear`,
`clarifier_down`/`up`, `get/set_if_shift_hz`, `get/set_tone_squelch_mode`,
`get/set_ctcss_tone_hz`, `get/set_dcs_code`, `get/set_scan_state`,
`get/set_vox_on`, `get/set_vox_gain`, `get/set_vox_delay`, `get_rx_busy` are
all plain `Radio` trait methods with `NotImplemented` default bodies, same
as group 1's. `radio/src/ft991a.rs`'s doc comments (the actual client
implementation, not just the trait's prose) read for every validation range
used below — see `control.rs`'s own inline citations (line-range comments)
for the exact source lines. `ts570d/ui/src/control.rs` (sibling checkout)
read for structural reference on `on_off()`'s cursor convention and the
`ReadMemoryChannel`/`WriteMemoryChannelFromVfoA` "read/write sources its
data from elsewhere, not a single Text buffer" pattern — FT-991A's own
field set/ranges used throughout, not ts570d's values.

### Design decisions made while reading source (before writing code)

1. **Group 3's `MemoryChannelEntry` writes (`MW`/`MT`) source their
   frequency/mode from VFO A at *execution* time**, not from a
   `Ft991aDisplay` snapshot captured when the key was pressed —
   `terminal.rs`'s `execute_action` calls `radio.get_vfo_a()`/
   `radio.get_mode()` fresh, mirroring `ts570d::ui::terminal`'s own
   `WriteMemoryChannelFromVfoA` precedent exactly (confirmed by reading its
   source, not assumed). The entry's other fields (clarifier offset/on,
   tone status, offset type) have no dedicated UI input in this task and
   are written as their documented defaults (`0`/`false`) — flagged as a
   judgment call in the final report, not silently absorbed. This also
   meant `validate_text_input`'s signature did **not** need to grow a
   `display: &Ft991aDisplay` parameter (unlike `initial_list_cursor`, which
   already had one) — a smaller diff than the alternative.
2. **Group 3's "6 commands" vs. the architect table's "item count (approx)
   4"**: the table bundles `get/set_memory_channel` as one conceptual item
   and `read/write_memory_channel`/`read/write_memory_channel_tag` as two
   more, i.e. 4 *wire-command* groupings — but this task exposes all 6
   underlying methods as 6 distinct keys (`C` select, `G` get, `R` read
   `MR`, `W` write `MW`, `T` read `MT`, `V` write `MT`) rather than
   collapsing any of them, since the task brief's own "check the actual
   method signatures" instruction and full-coverage goal outweighed hitting
   the table's "approx" count exactly. Flagged as an intentional deviation,
   not an oversight.
3. **`WriteMemoryChannelTagFromVfoA`'s "channel:tag" text format is a
   judgment call**, not specified anywhere — chosen over a second prompt/
   multi-field input (no precedent for multi-field `TextInput` exists yet
   in this crate) because `ControlState::TextInput` only carries one
   buffer. Split on the *first* `:` only (`splitn(2, ':')`), so a tag that
   itself legally contains a `:` (see `MemoryTag`'s allowed character set)
   still round-trips — covered by
   `test_write_memory_tag_colon_inside_tag_preserved`.
4. **Groups 4/6's boolean toggles (RX/TX clarifier, VOX) and enumerated
   selects (tone squelch mode, CTCSS tone, DCS code, scan state) use
   `CommandKind::List`, not group 1's `CommandKind::Immediate(fn(&
   Ft991aDisplay) -> ExecuteAction)` toggle-off-last-known-state pattern.**
   Read `ts570d/ui/src/control.rs` directly to confirm this is *its* own
   convention too (`on_off()` + explicit-choice `ListSelect`, not a display-
   state-dependent toggle) for every analogous on/off field there
   (RIT/XIT/NB/preamp/attenuator/VOX/scan) — adopted the same convention
   here rather than inventing a third pattern, and because it avoids
   needing to extend `Ft991aDisplay`/`poll_radio_state` with new fields
   this task was not asked to add (memory channel/clarifier/tone/IF-shift/
   scan/VOX state is not currently polled or displayed anywhere).
   Consequence: `initial_list_cursor` cannot live-pre-select these new
   `SelectAction`s' cursors (defaults to `0` for all of them) — documented
   inline as a known limitation, not silently absorbed; a natural follow-up
   for whichever future task extends polling/display for these groups.
5. **CTCSS/DCS options built at runtime from `radio::CTCSS_TONES_DECIHZ`/
   `radio::DCS_CODES`** (both re-exported at the crate root, confirmed by
   reading `radio/src/lib.rs`'s `pub use` list), not a hand-maintained
   parallel list in `ui` — avoids drift, same principle §11.4 calls out for
   the future `EX` sub-groups' own table-driven design. `ListSelect`'s
   cursor indexes directly into these tables (`select_action_to_execute`),
   so an invalid tone/code is structurally unrepresentable — no free-text
   validation needed for these two fields, unlike every other numeric input
   in this task.
6. **`cargo clippy`'s `enum_variant_names` lint fired on `SelectAction`**
   once it grew past `SetMode` (all-`Set*`-prefixed with only one variant
   never triggers it; 8 variants sharing the prefix does). Fixed by renaming
   the three boolean-toggle variants to `Toggle*` (`ToggleRxClarifier`,
   `ToggleTxClarifier`, `ToggleVox`) rather than suppressing the lint —
   matches `ts570d::ui::control::SelectAction`'s own `Toggle`-vs-`Set`
   naming split (confirmed by reading it), not an arbitrary rename.
7. **`terminal.rs`'s `execute_action` needed a real signature change**
   (`(&'static str, RadioResult<()>)` -> `(&'static str,
   RadioResult<String>)`), not just new match arms — the read-type actions
   (`GetMemoryChannel`, `ReadMemoryChannel`, `ReadMemoryChannelTag`) need to
   surface a fetched value in the `Feedback` message, which the old
   unit-only return type couldn't carry. Adopted `ts570d::ui::terminal`'s
   own exact shape and its `run_loop`'s "non-empty extra text wins, else
   `OK: {desc}`" convention (confirmed by reading both files directly, not
   reconstructed from memory) rather than inventing a different one. This
   was necessarily a `terminal.rs` change even though the task brief's
   "read first" list only named `control.rs` — `ExecuteAction` is matched
   exhaustively there, so any new variant added in `control.rs` requires a
   corresponding arm; flagged explicitly in the final report as a task-
   brief-adjacent file this task also had to touch, not silently expanded
   scope.

### File layout touched

- `ui/src/control.rs` — `InputAction`/`SelectAction`/`ExecuteAction`
  variants for groups 3/4/6; `memory_channels_commands()`,
  `clarifier_tone_if_shift_commands()`, `scan_vox_busy_commands()` now
  return real command lists (were `Vec::new()` stubs); `validate_text_input`,
  `select_action_to_execute`, `initial_list_cursor` grown with the new
  arms; new helper functions (`on_off_options`, `ctcss_tone_options`,
  `dcs_code_options`, `tone_squelch_mode_options`/`TONE_SQUELCH_MODE_ORDER`,
  `scan_state_options`/`SCAN_STATE_ORDER`, `get_memory_channel_immediate`,
  `clarifier_clear_immediate`, `parse_memory_channel`); ~52 new tests.
- `ui/src/terminal.rs` — `execute_action`'s return type widened to carry
  extra feedback text (decision 7 above); new match arms for every new
  `ExecuteAction` variant; `run_loop`'s feedback-message construction
  updated for the new `Ok(String)` shape; new imports
  (`MemoryChannelEntry`, `MemoryTag`, `TaggedMemoryChannel`).
- `ui/src/layout.rs`, `ui/src/lib.rs` — **not touched**; groups 3/4/6 don't
  need new render logic (reuse `Menu`/`GroupMenu`/`TextInput`/`ListSelect`/
  `Feedback` verbatim) or new `Ft991aDisplay` fields (decision 4 above).
- `radio/`, `emulator/`, `src/main.rs` — not touched, per constraints.

### Verification plan

- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt -p ui -- --check`, `cargo build --workspace`
  (confirm no breakage of concurrently-landed `radio`/`emulator` work from
  other agents).
- New tests per group: reachability from `Menu` + `Esc` back; key-count +
  uniqueness; one transition test per key confirming the exact
  `ControlState`/`InputAction`/`SelectAction` produced; at least one
  min/max-boundary-accepted + one just-past-boundary-rejected test per
  numeric `InputAction` (channel 1-117, clarifier offset 0-9999, IF-shift
  -1200..=1200 step 20, VOX gain 0-100, VOX delay 30-3000 step 10, plus the
  memory-tag length/format cases); cursor<->table-value lockstep tests for
  CTCSS/DCS/tone-squelch-mode/scan-state list selections; an explicit
  "`ScanVoxBusy` has no `BY` key" regression guard.

## Session: Wave 4 Task 4 — populate group 5 (Keyer/CW/Break-In) including
## the RTS real-time CW-keying keybinding

### Spec source of truth

`planning/architect/task_plan.md` §11.3 point 6 in full (the exact RTS
keybinding design — key, `CommandKind::Immediate`, toggle semantics not
hold-to-key, `rts_asserted: bool` tracked locally not polled, optimistic
set + rollback-on-`Err`) and §11.6 item 4 ("Populate group 5 ... including
the RTS CW-keying keybinding ... Depends on tasks 1-3", confirming this
task also covers group 5's non-RTS keyer/CW/break-in content). §11.2's
table row for group 5: `KM KP KR KS KY CS ZI BI SD` + RTS keying, "mixed:
break-in/keyer-speed/pitch/enabled/zero-in/cw-spot on `Radio`; `KM`/`KY`
(keyer memory store/play) on `Ft991aExtras`; new RTS CW-key toggle on
`CwKeying`". `radio/src/radio_trait.rs`'s `Radio` (lines ~1298-1361),
`Ft991aExtras` (lines ~1906-1930), and `CwKeying` (lines ~2174-2179)
bodies read directly to confirm every method's real trait home and
signature, not just trusted the table. `radio/src/ft991a.rs`'s client
implementation (lines ~1259-1461) read for every validation range used
below (doc comments + the actual range checks in the method bodies, not
just the doc comments alone — `SD`/`KS` turned out to have no step
constraint despite `VD`'s precedent having one, confirmed by reading the
implementation, not assumed by analogy).

### Design decisions made while reading source (before writing code)

1. **`CwKeying::assert_rts`'s signature confirmed exactly as the task brief
   expected**: `fn assert_rts(&self, asserted: bool) -> RadioResult<()>`
   (radio_trait.rs line 2177) — plain sync `&self`, not `&mut self`, not
   `async`. This is why `execute_action`'s new `ToggleRts` arm calls it with
   no `.await`, unlike every other new group-5 arm (which are all `Radio`/
   `Ft991aExtras` async methods).
2. **The optimistic-set + rollback-on-error logic cannot live in
   `control.rs`'s pure state machine** — `handle_key`/`CommandKind::
   Immediate` only ever produce an `ExecuteAction` from an *immutable*
   `&Ft991aDisplay` (mirroring `toggle_tx`/`toggle_power_on`'s existing
   shape exactly: `fn toggle_rts(display: &Ft991aDisplay) -> ExecuteAction`
   just reads `display.rts_asserted` and carries it on
   `ExecuteAction::ToggleRts(bool)`, same as those two). The actual mutate-
   then-maybe-roll-back has to happen where the radio call itself happens —
   `terminal.rs`'s `execute_action`. This required a real, disclosed
   signature change: `execute_action` gained a `display: &mut
   Ft991aDisplay` parameter (previously `radio`/`action` only), and its
   generic bound widened from `R: Radio` to `R: Radio + Ft991aExtras +
   CwKeying` (needed anyway for the non-RTS group-5 `Ft991aExtras` calls).
   `run_loop`'s bound widened to match, and its one call site now passes
   `&mut display`. This is the only action variant that touches `display`
   directly — every other arm leaves it alone, since every other polled
   field is refreshed by the next `poll_radio_state` cycle instead, but
   `rts_asserted` is deliberately never polled (§11.3 point 6 — no
   "read back what I asserted" ioctl exists; `read_cts` reads the *status*
   line CTS, not RTS's own asserted state).
3. **Key choice: `K`, as `planning/architect/task_plan.md` §11.3 point 6
   specifies exactly**, not a substitute — checked it against all 11 of
   group 5's other command keys before finalizing (`B`/`D`/`S`/`E`/`W`/`P`/
   `Z`/`R`/`M`/`Y`/`J`, chosen below) and confirmed no collision within
   group 5's own internal keymap (the only kind of collision that would
   matter, per Wave 4 Task 2's established namespace-separation precedent:
   `GroupMenu`-level command keys are a different `ControlState` match arm
   from `Menu`-level group-entry keys, so `K` being *both* group 5's
   top-level entry key *and* its own internal RTS-toggle key is not a
   collision, exactly like `M` already being both `MemoryChannels`'s entry
   key and `FrequencyLevels`'s internal "Set mode" key).
4. **Full group 5 keybinding set (12 keys) — a judgment call on scope and
   letters, since §11.2's table gives only an approximate "9 + 1" item
   count**: following Wave 4 Task 3's own precedent (group 3 was
   deliberately expanded from an "approx 4" table count to 6 real keys,
   "full-coverage goal outweighed hitting the table's 'approx' count
   exactly"), this task exposes every one of group 5's 9 underlying wire
   commands as distinct keys, using groups 4/6's established
   `CommandKind::List` + `on_off_options` convention for the 3 booleans
   (`BI`/`CS`/`KR`) rather than group 1's display-state-dependent
   `Immediate` toggle (matches decision 4 from the Task 3 section above —
   same reasoning: no `Ft991aDisplay` polling exists yet for these fields,
   so `initial_list_cursor` defaults all three to cursor 0, a documented
   limitation not silently absorbed):
   - `K` Toggle RTS CW key (real-time) — `Immediate`, `CwKeying`
   - `B` Break-in on/off — `List`, `Radio::{get,set}_break_in_on`
   - `D` Semi break-in delay (30-3000 ms, **no step constraint** — confirmed
     by reading `set_semi_break_in_delay`'s body directly, unlike `VD`'s
     10ms-step precedent which does have one) — `Text`, `Radio`
   - `S` CW spot on/off — `List`, `Radio::{get,set}_cw_spot_on`
   - `E` Electronic keyer on/off (`KR`) — `List`, `Radio::{get,set}_
     keyer_enabled`
   - `W` Keyer speed, WPM, 4-60 (`KS`, no step) — `Text`, `Radio`
   - `P` Keyer pitch, Hz, 300-1050 step 10 (`KP`) — `Text`, `Radio`
   - `Z` CW auto zero-in (`ZI`) — `Immediate` trigger (write-only,
     zero-width, no persisted state — same shape as group 4's `ClarifierClear`)
   - `R` Read keyer memory (`KM` read, channel 1-5) — `Text`,
     `Ft991aExtras::read_keyer_memory`, surfaces the fetched message as
     `Feedback` text (same "extra text wins" convention Task 3 established
     for `GetMemoryChannel`/`ReadMemoryChannel`)
   - `M` Write keyer memory message (`KM` write) — `Text`, "channel:message"
     format (see decision 5 below), `Ft991aExtras::write_keyer_memory`
   - `Y` Play keyer memory (`KY`, [`radio::KeyerPlaybackMode::KeyerMemory`])
     — `Text`, channel only, `Ft991aExtras::play_keyer_memory`
   - `J` Play message keyer (`KY`, [`radio::KeyerPlaybackMode::MessageKeyer`])
     — `Text`, channel only, `Ft991aExtras::play_keyer_memory`

   All 12 keys verified programmatically unique
   (`test_keyer_cw_break_in_group_has_twelve_commands_all_unique_keys`).
5. **`KY`'s two playback families got two separate keys (`Y`/`J`) instead
   of one key with a compound "channel+mode" text format** — a judgment
   call, flagged explicitly. `radio::KeyerPlaybackMode` has exactly 2
   variants (`KeyerMemory`/`MessageKeyer`); both only need a plain 1-5
   channel number once the mode is fixed by which key was pressed, so
   `parse_keyer_channel`'s existing single-field validator covers both
   without inventing a second multi-field text syntax. The alternative
   (one key, buffer format like `"3m"` for MessageKeyer) was considered and
   rejected as needless complexity for a 2-valued distinction that a second
   key states more plainly. `KM` write, by contrast, genuinely needs two
   independently-typed fields (a channel number *and* free-text), so it
   reuses group 3's `WriteMemoryChannelTagFromVfoA` "channel:text" (split
   on the *first* `:` only) convention rather than adding a 13th key —
   confirmed a message containing a colon still round-trips
   (`test_write_keyer_memory_colon_inside_message_preserved`).
6. **`Ft991aDisplay` gains exactly one new field**, `rts_asserted: bool`
   (default `false`), documented as locally-tracked/never-polled per
   §11.3 point 6 — `poll_radio_state` in `terminal.rs` was **not** touched
   (no `RS`-equivalent read exists to poll it from, by design).
7. **`layout.rs`'s `draw_status` row 1 gains a small `RTS: ON/OFF`
   indicator**, immediately after the existing `TX`/`RX` indicator and
   before the `ID` field — reused the exact `tx_state_label`-style
   `(&'static str, Color)` helper pattern via a new `rts_label(bool)`
   function (red+bold "ON" / dark-gray "OFF") rather than inventing a new
   rendering shape. No new render *function* needed, matching the task
   brief's "check whether this needs new rendering code or can reuse an
   existing pattern" instruction — the answer was "reuse the pattern, not
   the function" (it's a different boolean than `TxState`'s 3-valued
   enum, so `tx_state_label` itself couldn't be reused directly, but its
   shape could).
8. **Test double for the rollback path**: `terminal.rs`'s existing in-crate
   `MockRadio` test double (already had a `fail_all: bool` field driving
   every other overridden `Radio` method's success/failure branching, and
   already had a Wave-4-Task-2-added but until-now-default-bodied
   `impl CwKeying for MockRadio {}`) just needed its `CwKeying` impl block
   filled in with a real `assert_rts` override branching on the same
   `fail_all` flag — no new struct/type needed, per the task brief's
   "check if one already exists" instruction. `MockRadio::ok()`
   (`fail_all: false`) exercises the optimistic-set-stays-set path;
   `MockRadio::failing()` (`fail_all: true`) exercises the rollback path;
   a third test starts from `rts_asserted: true` (via
   `Ft991aDisplay { rts_asserted: true, ..Default::default() }`, per
   `cargo clippy`'s `field_reassign_with_default` lint) to confirm rollback
   restores the *carried* prior value, not just unconditionally `false`.
9. **One existing test's premise broke and needed a documented fix, not a
   silent drop**: `test_stub_group_char_key_is_a_no_op` (from Wave 4 Task 2)
   pressed `'z'` inside `CommandGroup::KeyerCwBreakIn`'s `GroupMenu` to
   prove empty stub groups are safe to navigate. Since `'Z'` is now group
   5's real zero-in key (`find_group_command` uppercases before matching),
   that specific key+group combination no longer produces a no-op.
   Retargeted the test at `CommandGroup::VfoMemoryQuickOps` (still a
   genuine empty stub) instead of deleting the coverage, and added a
   group-5-specific replacement (`test_keyer_cw_break_in_q_key_is_a_no_op`,
   using `'q'`, which is not one of group 5's 12 keys) plus
   `test_stub_groups_have_no_commands_yet`'s `populated` array updated to
   include `KeyerCwBreakIn`.

### File layout touched

- `ui/src/control.rs` — `InputAction`/`SelectAction`/`ExecuteAction`
  variants for group 5; real `keyer_cw_break_in_commands()` (12 keys, was
  `Vec::new()` stub); `validate_text_input`, `select_action_to_execute`,
  `initial_list_cursor` grown with the new arms; new helpers (`toggle_rts`,
  `zero_in_immediate`, `parse_keyer_channel`); ~70 new tests (reachability,
  key-count+uniqueness, per-key transitions, RTS carry-both-directions,
  validation-range boundaries for `SD`/`KS`/`KP`/keyer-memory-channel,
  "channel:message" parsing incl. colon-inside-message preservation,
  cursor<->value lockstep for the 3 boolean toggles); two pre-existing
  Wave-4-Task-2 tests updated in place (decision 9 above), documented, not
  silently changed.
- `ui/src/terminal.rs` — `execute_action`'s bound widened to `Radio +
  Ft991aExtras + CwKeying` and it gained a `display: &mut Ft991aDisplay`
  parameter (decision 2); new match arms for all 12 group-5
  `ExecuteAction` variants, including the `ToggleRts` optimistic-set/
  rollback arm; `run_loop`'s bound widened to match, its one
  `execute_action` call site updated; every pre-existing `execute_action`
  test call site updated for the new parameter (mechanical, no behavior
  change to those); `MockRadio`'s `CwKeying` impl filled in with a real
  `assert_rts` override (decision 8); 3 new RTS-specific tests.
- `ui/src/lib.rs` — `Ft991aDisplay` gains `rts_asserted: bool` (default
  `false`), documented as locally-tracked/never-polled; 1 new test.
- `ui/src/layout.rs` — new `rts_label(bool) -> (&'static str, Color)`
  helper + its use in `draw_status`'s row 1; module doc updated; 2 new
  tests.
- `radio/`, `emulator/`, `src/main.rs` — not touched, per constraints.

### Verification plan

- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt -p ui -- --check`, `cargo build --workspace`.
- Result: **175 passed, 0 failed** (up from 120 — net +55 new tests, zero
  regressions, all 120 pre-existing tests still pass, 2 of them updated
  in place per decision 9 and documented, not silently changed). `cargo
  clippy` hit one failure (`field_reassign_with_default` on a test
  constructing `Ft991aDisplay` then mutating `rts_asserted` — fixed with
  struct-update syntax) then clean. `cargo fmt` reformatted 3 spots
  (2 long `assert_eq!`/match-arm bodies, 1 long call chain) then clean.
  `cargo build --workspace` clean.

## Session: Wave 4 Task 5 — populate group 2 (VFO/Memory Quick-Ops) and
## group 9 (Band/Step/Encoder)

### Spec source of truth

`planning/architect/task_plan.md` §11.2 (group definitions/table, group 2
= batch 1 `AB BA AM VM MA CH QI QR QS SV`, group 9 = batch 8 `BS BU BD FS
ED EU EK DN UP`) and §11.6 dispatch queue item 5 ("Populate group 2
(VFO/Memory Quick-Ops) and group 9 (Band/Step/Encoder) — both mixed
`Radio`/`Ft991aExtras`, moderate complexity. Depends on task 4
(file-sequencing, not a real design dependency)."). `radio/src/
radio_trait.rs`'s `Radio` trait body (batch-1 section lines ~1129-1177,
batch-8 section lines ~1654-1710) and `Ft991aExtras`'s VFO/memory
quick-ops section (lines ~1881-1904) and encoder/ENT-key section (lines
~2002-2019) read directly to confirm every method's real trait home and
signature, not just trusted the table. `radio/src/ft991a.rs`'s client
implementations (batch 1 lines ~1005-1083, batch 8 lines ~2064-2156) read
for every validation range/doc comment used below. `ui/src/control.rs`'s
landed `vfo_memory_quick_ops_commands`/`band_step_encoder_commands` stubs
and groups 3/4/5/6's populated implementations read as structural
templates, per the task brief's explicit instruction to use group 5's
implementation as the freshest example of mixing `Radio` and
`Ft991aExtras` methods in one group.

### Design decisions made while reading source (before writing code)

1. **§11.2's trait-mix claim confirmed exactly for both groups, no
   discrepancy** — see `findings.md`'s new section for the full method-by-
   method confirmation. Group 2: `copy_vfo_a_to_b`/`copy_vfo_b_to_a`/
   `swap_vfos`/`store_vfo_to_memory`/`recall_memory_to_vfo`/
   `memory_channel_up`/`memory_channel_down` on `Radio`;
   `toggle_vfo_memory_mode`/`qmb_store`/`qmb_recall`/`quick_split` on
   `Ft991aExtras`. Group 9: `set_band`/`band_up`/`band_down`/
   `get_fine_step`/`set_fine_step`/`mic_up`/`mic_down` on `Radio`;
   `encoder_down`/`encoder_up`/`ent_key` on `Ft991aExtras`.
2. **Group 2 is all-`Immediate`, zero `Text`/`List`** — every one of its 11
   underlying wire commands (`AB BA SV AM MA CH0 CH1 VM QI QR QS`) is a
   zero-argument write-only trigger with no persisted state to snapshot on
   the way in, per both the `Radio` trait's and `Ft991aExtras`'s own
   method signatures (`&mut self) -> RadioResult<()>` throughout, no
   parameters). This is the first group in the crate with *no* numeric or
   enumerated input at all — flagged since it means this group needed zero
   new `validate_text_input`/`select_action_to_execute` arms, only new
   `ExecuteAction` variants and one small named `_immediate` helper
   function per trigger (matching the established one-function-per-command
   convention, not a shared generic closure — consistent with how
   `get_memory_channel_immediate`/`clarifier_clear_immediate`/
   `zero_in_immediate`/`toggle_rts` are each their own function today).
3. **11 keys for group 2, judgment call on letters** (architect table gives
   only an approximate "10" item count, this task exposes all 11
   underlying methods as distinct keys, following Wave 4 Task 3's own
   "full-coverage goal outweighed hitting the table's 'approx' count
   exactly" precedent): `A` Copy VFO A→B, `B` Copy VFO B→A, `W` Swap VFOs,
   `S` Store VFO A to memory, `R` Recall memory to VFO A, `U` Memory
   channel up, `D` Memory channel down, `M` Toggle VFO/Memory mode, `I`
   QMB store, `Q` QMB recall, `P` Quick split toggle. All 11 verified
   programmatically unique
   (`test_vfo_memory_quick_ops_group_has_eleven_commands_all_unique_keys`).
   `Q` being both this group's internal "QMB recall" key and the
   `Menu`-level global Quit key is confirmed safe by
   `test_vfo_memory_quick_ops_lowercase_q_inside_group_menu_is_not_global_quit`
   — same namespace-separation precedent group 1's `M`/group 5's `K`
   already established (different `ControlState` match arms).
4. **Group 9's band select uses `ListSelect` over `radio::Band`, not
   `TextInput` over a raw wire code** — resolved the task brief's explicit
   "check what's more natural given the method's actual parameter type"
   question by reading `set_band`'s real signature
   (`fn set_band(&mut self, _band: Band) -> RadioResult<()>`): it takes a
   16-variant enum, not a string/int, so `ListSelect` is the type-driven
   choice, matching Mode/CTCSS/DCS's existing precedent (an invalid band is
   structurally unrepresentable). `BAND_ORDER`/`band_options` mirror
   `MODE_ORDER`/`mode_options`'s cursor<->value pairing pattern exactly,
   excluding the documented gap at wire value `13` (`Band` itself has no
   variant for it).
5. **`SetBand`'s `initial_list_cursor` always defaults to cursor 0, for a
   *different* reason than groups 4/5/6's existing "not polled yet"
   entries** — `BS` has **no** `Read`/`Answer` form on the wire at all
   (confirmed in `radio_trait.rs`'s own doc comment on `set_band`: "no
   `Read`/`Answer` form exists for `BS` at all"), so there is no live value
   to poll even in principle, not just "not implemented yet." Documented as
   a structurally distinct case in both `band_step_encoder_commands`'s doc
   comment and `initial_list_cursor`'s own match arm, rather than folding
   it into the existing "not polled yet" bucket where a future reader might
   think adding `Ft991aDisplay` polling would fix it.
6. **The encoder-nudge (`ED`/`EU`) dead-end question — decided to include,
   not omit, with an honest doc comment**: full reasoning in `findings.md`.
   Summary: confirmed Wave 3's "validates structurally but mutates no
   persisted `Ft991aState`" finding is still accurate for `ED`/`EU`/`EK` by
   re-reading `ft991a_radio.rs`'s `handle_command` directly; judged this to
   be a test-emulator modeling limitation, not evidence the real wire
   commands are meaningless (the client `ft991a.rs` sends the genuine wire
   frames in all three cases); found this crate already has an accepted
   "write-only trigger, no persisted state to observe" category
   (`ClarifierClear`/`ZeroIn`, both already real keys in groups 4/5) that
   `EK` fits cleanly and `ED`/`EU` fit less cleanly (their effect is
   context-dependent on the radio's current front-panel state, which this
   UI cannot observe or display) — included all three anyway as a genuine
   "remote knob/button" feature, with the `Text` prompt and doc comment
   both stating the context-dependency honestly rather than implying a
   predictable effect. `UP`/`DN` (mic buttons) were checked separately and
   found to have a real, emulator-observable effect (step `vfo_a_hz`), so
   they carry no such caveat.
7. **9 keys for group 9, matching the architect table's "approx 9" count
   exactly** (unlike group 2's "expanded past approx" precedent, this one
   landed at exactly 9 without needing to decide any collapsing/splitting
   trade-off): `B` Select band (`List`), `U` Band up, `D` Band down, `F`
   Fine step on/off (`List`, `on_off_options` convention per groups
   4/5/6), `P` Mic UP button, `N` Mic DOWN button, `J` Encoder down
   (`Text`, "encoder:steps"), `K` Encoder up (`Text`, same format), `E`
   ENT key press. All 9 verified programmatically unique
   (`test_band_step_encoder_group_has_nine_commands_all_unique_keys`).
8. **`ED`/`EU`'s "encoder:steps" text format spells out the encoder name in
   full** (`main`/`sub`/`multi`, case-insensitive), not the raw `0`/`1`/`8`
   wire digits `EncoderSelector::as_wire_digit`/`from_wire_digit` already
   provide — judgment call, prioritizing legibility for someone typing into
   a `TextInput` with no on-screen legend for what a bare digit means,
   consistent with group 3/5's existing "channel:tag"/"channel:message"
   compound-field convention (split on the *first* `:` only, reusing that
   established pattern rather than inventing a new one). Step count
   validated `1..=99` per `Ft991aExtras::encoder_down`/`encoder_up`'s own
   range check (`radio/src/ft991a.rs` lines ~2110-2135,
   `RadioError::InvalidEncoderSteps`).
9. **No new `terminal.rs` tests added** — both groups' `execute_action` arms
   are plain 1:1 `Radio`/`Ft991aExtras` passthroughs with no
   executor-side branching logic (unlike group 5's RTS optimistic-set/
   rollback), so per Wave 4 Task 3's own established division of test
   coverage (special executor logic gets `terminal.rs` tests; plain
   passthrough arms are covered by `control.rs`'s keybinding/validation
   tests plus the fact that the match arm must compile), no new
   `terminal.rs` tests or `MockRadio` overrides were needed. Flagged
   explicitly so this isn't mistaken for an oversight.
10. **One pre-existing test's premise broke and was fixed, not silently
    left green-by-accident**: Wave 4 Task 4's
    `test_stub_group_char_key_is_a_no_op` targeted
    `CommandGroup::VfoMemoryQuickOps` (genuinely stubbed at the time) with
    key `'z'`. Since `'Z'` is not one of group 2's 11 keys, this test would
    have kept *passing* even after this task populated group 2 — but its
    doc comment ("must not panic... proves the skeleton is safe to
    navigate even before content lands") would have become misleading
    (the group is no longer "before content lands"). Retargeted to
    `CommandGroup::AttenuatorNoiseAgcNotchFilter` (still genuinely
    stubbed) and gave groups 2/9 their own dedicated no-op regression
    tests (`test_vfo_memory_quick_ops_z_key_is_a_no_op`,
    `test_band_step_encoder_z_key_is_a_no_op`), matching Task 4's own
    precedent for the analogous situation with `KeyerCwBreakIn`.

### File layout touched

- `ui/src/control.rs` — `InputAction` gains `EncoderDown`/`EncoderUp`;
  `SelectAction` gains `SetBand`/`ToggleFineStep`; `ExecuteAction` gains 11
  group-2 zero-arg variants + 9 group-9 variants; real
  `vfo_memory_quick_ops_commands()` (11 keys, was `Vec::new()` stub) and
  `band_step_encoder_commands()` (9 keys, was `Vec::new()` stub);
  `validate_text_input`/`select_action_to_execute`/`initial_list_cursor`
  grown with the new arms; new helpers (`BAND_ORDER`/`band_options`, 11
  group-2 `_immediate` functions, 5 group-9 `_immediate` functions,
  `parse_encoder_selector`, `parse_encoder_steps`); module doc comment
  updated to list groups 1-6 and 9 as populated, 7/8/10/11/12 as remaining
  stubs; ~43 new tests (reachability, key-count+uniqueness, per-key
  transitions, `SetBand`/`ToggleFineStep` cursor<->value lockstep,
  `ED`/`EU` "encoder:steps" validation boundaries incl. case-insensitive
  selector names, missing-colon/invalid-selector/non-numeric-steps
  rejection, no-op regression guards); 1 pre-existing test retargeted in
  place (decision 10 above), documented, not silently changed.
- `ui/src/terminal.rs` — `execute_action` gains 20 new match arms (11
  group-2 + 9 group-9), all plain `ok_unit(radio.<method>(...).await)`
  passthroughs; no signature change, no new tests, no `MockRadio` changes
  needed (decision 9 above).
- `ui/src/layout.rs`, `ui/src/lib.rs` — **not touched by this task**
  (confirmed by `git diff --stat`); both already carried uncommitted
  changes from Wave 4 Task 4's `rts_label`/`rts_asserted` work at the start
  of this session, per that task's own `progress.md` — flagged in
  `findings.md` so this isn't mistaken for scope creep by this task.
- `radio/`, `emulator/`, `src/main.rs` — not touched, per constraints
  (confirmed clean — this session's edits are `git diff --stat`-confirmed
  to be limited to `ui/src/control.rs` and `ui/src/terminal.rs`; other
  uncommitted changes present in the working tree pre-date this session,
  see `findings.md`).

### Verification plan

- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt -p ui -- --check`, `cargo build --workspace`.
- Result: **218 passed, 0 failed** (up from 175 — net +43 new tests, zero
  regressions, all 175 pre-existing tests still pass, 1 of them
  retargeted in place per decision 10, documented, not silently changed).
  `cargo clippy -p ui --all-targets -- -D warnings` clean on the first
  pass (no new lint findings this task, unlike Tasks 3/4's
  `enum_variant_names`/`field_reassign_with_default` fixes). `cargo fmt -p
  ui -- --check` found 4 reflow spots (one long match-arm line in
  `select_action_to_execute`, two long single-line `assert_eq!` calls in
  the new tests, one long match-arm group in `execute_action`) — fixed
  with `cargo fmt -p ui`, clean after. `cargo build --workspace` clean.

## Session: Wave 4 Task 6 — populate group 7 (Attenuator/Noise/AGC/Notch/
## Filter) and group 8 (Speech/Mic/Monitor)

### Spec source of truth

`planning/architect/task_plan.md` §11.2 (group definitions/table, group 7 =
batch 6 `RA PA NB NL NR RL GT CO BP BC NA SH`, group 8 = batch 7
`PL PR MG ML`) and §11.6 dispatch queue item 6 ("Populate group 7
(Attenuator/Noise/AGC/Notch/Filter) and group 8 (Speech/Mic/Monitor).
Depends on task 5."). `radio/src/radio_trait.rs`'s `Radio` trait body
(batch-6 section lines ~1444-1583, batch-7 section lines ~1586-1652) and
`Ft991aExtras`'s contour/APF/manual-notch section (lines ~1932-1986) and
parametric-mic-EQ section (lines ~1988-2000) read directly to confirm every
method's real trait home and signature, not just trusted the table's prose.
`radio/src/ft991a.rs`'s client implementations (lines ~1578-2064) read for
every validation range/doc comment used below, cross-checked against
`RadioError`'s own variant messages (`radio_trait.rs` lines ~85-106).
`radio/src/ft991a_radio.rs`'s `SH_BANDWIDTH_TABLE`/`filter_bandwidth_hz`/
`mode_family_for`/`ModeFamily` read for the `SH` filter-width table, and
confirmed re-exported from `radio`'s crate root (`radio/src/lib.rs` lines
~126-133). `ui/src/control.rs`'s landed
`attenuator_noise_agc_notch_filter_commands`/`speech_mic_monitor_commands`
stubs and groups 5/2/9's populated implementations read as structural
templates, per the task brief's instruction to use the freshest populated
groups as the mixing-`Radio`-and-`Ft991aExtras` template.

### Design decisions made while reading source (before writing code)

1. **§11.2's trait-mix claim confirmed for both groups, with one addition
   beyond the table's prose** — group 7: 10 plain `Radio` methods
   (attenuator, preamp, noise blanker on/level, noise reduction on/level,
   AGC, auto notch, narrow, filter width) plus 6 `Ft991aExtras`-only
   methods for contour on/frequency and manual notch on/frequency, exactly
   as the table's prose named. **Addition, not in the table's own item
   count**: `CO` also carries APF (on/frequency), which the table
   mentioned only in passing ("...manual-notch (BC/BP?...)/APF on
   Ft991aExtras") without listing it as its own contributor to the group's
   item count. Included as 2 more real keys since it's `CO`'s own third
   sub-field, same command family as contour — omitting it while including
   contour/manual-notch would have been an arbitrary, undisclosed
   carve-out. Full method-by-method citation in `findings.md`. Group 8: 5
   plain `Radio` methods (mic gain, speech processor level/on, monitor
   on/level) plus 1 `Ft991aExtras`-only method (parametric mic EQ on/off),
   exactly matching the table.
2. **16 keys for group 7 (not the table's "approx 12"), 6 for group 8
   (matching the table exactly)** — same "full-coverage goal outweighs
   hitting the table's approx count" precedent Task 5 established for
   group 2 (11 vs. "approx 10"). Group 7 keys: `A` Attenuator on/off, `P`
   Pre-amp/IPO mode, `B` Noise blanker on/off, `L` Noise blanker level, `N`
   Noise reduction on/off, `R` Noise reduction level, `G` AGC mode, `U`
   Auto notch on/off, `W` Narrow filter on/off, `F` Filter width, `C`
   Contour on/off, `H` Contour frequency, `X` APF on/off, `Y` APF
   frequency, `M` Manual notch on/off, `Z` Manual notch frequency. Group 8
   keys: `G` Mic gain, `L` Speech processor level, `S` Speech processor
   on/off, `M` Monitor on/off, `V` Monitor level, `E` Parametric mic EQ
   on/off. All verified programmatically unique per group
   (`test_attenuator_noise_agc_notch_filter_group_has_sixteen_commands_all_unique_keys`,
   `test_speech_mic_monitor_group_has_six_commands_all_unique_keys`).
3. **The task brief's central open question — `CO`/`BP`'s "shared P3 field
   whose meaning depends on a P2 selector" — resolved with *no* new
   `ControlState` variant**: `radio/src/ft991a.rs`'s actual method bodies
   already split every `P2`-selected sub-field into its own independent,
   cleanly-typed `get_*`/`set_*` pair at the client-API boundary (Wave 3's
   own work) — the raw `P2`/`P3` wire fields are never exposed to a
   caller. Each pair is therefore just another independent boolean
   (`CommandKind::List` + `on_off_options`) or numeric (`CommandKind::Text`)
   control, the same shape `NR`/`RL`'s on/level split already established
   in this file. Full reasoning for why this genuinely isn't the
   "two-dimensional" case the task brief anticipated (and why the
   `EncoderDown`/`EncoderUp` compound-field convention would have been the
   wrong tool even if it were) is in `findings.md`.
4. **`SH` filter width (`F`) is `ListSelect`, per the task brief's own
   suggestion, but its 22 labels come from the static `SH_BANDWIDTH_TABLE`
   constant, not live radio state** — `get_filter_width_index`'s own doc
   comment states the actual Hz meaning depends on current mode and `NA`
   (narrow) state, neither part of `SH`'s own wire bytes.
   `CommandKind::List`'s `options: fn() -> Vec<String>` has no
   `&Ft991aDisplay` parameter to read live mode from regardless, and `NA`
   isn't a polled `Ft991aDisplay` field anyway (this task does not extend
   polling, matching Tasks 3/5's "not polled yet" precedent). Rather than a
   bare "index 00..21" list (the `ED`/`EU` context-dependent-trigger
   precedent), each label shows all three families' narrow/wide Hz values
   straight from the compile-time table (e.g. `"05 SSB1100/- CW250/-
   RTTY250/-"`) — real information, just not narrowed to the current mode.
5. **AGC mode (`G`) offers 5 `ListSelect` options, not `AgcMode`'s full
   7-valued reported domain** — `GT`'s Set command genuinely only accepts 5
   values; the 2 extra reported `AUTO-MID`/`AUTO-SLOW` variants exist only
   as possible *read* results, not settable choices. `AGC_ORDER`'s "Auto"
   entry maps to `AgcMode::AutoFast` on selection, reusing the same
   arbitrary-but-documented sub-variant `AgcMode`'s own doc comment already
   names as this emulator's default report choice, and consistent with
   `AgcMode::set_wire_value`'s own collapsing behavior.
6. **Preamp mode (`P`) is `ListSelect` over the 3-valued `PreampMode`, not
   `TextInput`** — type-driven, same "structurally unrepresentable invalid
   value" principle as Mode/Band/CTCSS/DCS/ScanState's existing precedent,
   not a judgment call.
7. **No new `terminal.rs` tests added** — all 22 new `execute_action` arms
   (16 group-7 + 6 group-8) are plain 1:1 `Radio`/`Ft991aExtras`
   passthroughs with no executor-side branching logic (unlike group 5's
   RTS optimistic-set/rollback), so per Tasks 3/5's established division of
   test coverage, no new `terminal.rs` tests or `MockRadio` overrides were
   needed.
8. **Two pre-existing tests' premises broke and were fixed, not silently
   left green-by-accident**: `test_stub_groups_have_no_commands_yet`'s
   `populated` list (and its stale header comment) needed both new groups
   added. `test_stub_group_char_key_is_a_no_op` (Task 5's own retarget to
   `CommandGroup::AttenuatorNoiseAgcNotchFilter` using key `'z'`) had its
   premise broken by `'Z'` becoming this task's real manual-notch-frequency
   key — retargeted to `CommandGroup::MetersStatus` (still genuinely
   stubbed), and gave groups 7/8 their own dedicated `'Q'`-key no-op
   regression tests. Full reasoning in `findings.md`.

### File layout touched

- `ui/src/control.rs` — `InputAction` gains 5 group-7 + 3 group-8
  `Text`-backed variants (`SetNoiseBlankerLevel`, `SetNoiseReductionLevel`,
  `SetContourFrequencyHz`, `SetApfFrequencyHz`,
  `SetManualNotchFrequencyHz`, `SetMicGain`, `SetSpeechProcessorLevel`,
  `SetMonitorLevel`); `SelectAction` gains 11 group-7 + 3 group-8
  `List`-backed variants; `ExecuteAction` gains 16 group-7 + 6 group-8
  variants; real `attenuator_noise_agc_notch_filter_commands()` (16 keys,
  was `Vec::new()` stub) and `speech_mic_monitor_commands()` (6 keys, was
  `Vec::new()` stub); `validate_text_input`/`select_action_to_execute`/
  `initial_list_cursor` grown with the new arms; new helpers
  (`PREAMP_ORDER`/`preamp_options`, `AGC_ORDER`/`agc_options`,
  `filter_width_options`); module doc comment updated to list groups 1-9 as
  populated, 10/11/12 as remaining stubs; 69 new tests (reachability,
  key-count+uniqueness, per-key transitions,
  `SetPreampMode`/`SetAgcMode`/`SetFilterWidthIndex` cursor<->value
  lockstep, validation-range boundaries for every new numeric field
  including step-constraint and no-step-constraint cases, no-op regression
  guards); 2 pre-existing tests retargeted/updated in place (decision 8
  above), documented, not silently changed.
- `ui/src/terminal.rs` — `execute_action` gains 22 new match arms (16
  group-7 + 6 group-8), all plain `ok_unit(radio.<method>(...).await)`
  passthroughs; no signature change, no new tests, no `MockRadio` changes
  needed (decision 7 above).
- `ui/src/layout.rs`, `ui/src/lib.rs` — **not touched by this task**
  (confirmed by `git diff --stat`); both already carried uncommitted
  changes pre-dating this session (from earlier Wave 4 tasks and other
  agents' work) — flagged in `findings.md` so this isn't mistaken for
  scope creep by this task.
- `radio/`, `emulator/`, `src/main.rs` — not touched, per constraints
  (confirmed clean via `git diff --stat` — this session's edits are
  limited to `ui/src/control.rs` and `ui/src/terminal.rs`; other
  uncommitted changes present in the working tree pre-date this session,
  see `findings.md`).

### Verification plan

- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt -p ui -- --check`, `cargo build --workspace`.
- Result: **287 passed, 0 failed** (up from 218 — net +69 new tests, zero
  regressions, all 218 pre-existing tests still pass, 2 of them
  updated/retargeted in place per decision 8, documented, not silently
  changed). `cargo clippy -p ui --all-targets -- -D warnings` found 1
  `doc_lazy_continuation` lint (a doc-comment line starting with `+` read
  as an unindented markdown list continuation) — fixed by rewording, clean
  after. `cargo fmt -p ui -- --check` found reflow spots in the new
  `terminal.rs` match arms — fixed with `cargo fmt -p ui`, clean after.
  `cargo build --workspace` clean.

## Session: Wave 4 Task 7 — populate group 10 (Meters/Status) and group 11
## (System/Tuner/DVS), including the date/time multi-field entry design

### Spec source of truth

`planning/architect/task_plan.md` §11.2 (group definitions/table, group 10 =
batch 9 `IF RM RI RS MS UL`, group 11 = batch 10 `AC AI DA DT LK OI OS FT TS
MX LM PB`) and §11.6 dispatch queue item 7 ("Populate group 10 (Meters/
Status, read-heavy/display-focused) and group 11 (System/Tuner/DVS, the most
`Ft991aExtras`-heavy group — date/time read/write needs its own small
multi-field `TextInput` design, flagged for this task specifically).
Depends on task 6."). `radio/src/radio_trait.rs`'s `Radio` trait body (meters
section lines ~1036-1053, batch-10 section lines ~1730-1801) and
`Ft991aExtras`'s composite-status/meters section (lines ~1841-1879) and
antenna-tuner/dimmer/date-time/TXW/DVS section (lines ~2021-2125) read
directly to confirm every method's real trait home and signature, not just
trusted the table's prose. `radio/src/ft991a.rs`'s client implementations
(meters lines ~781-896, batch 10 lines ~2158-2496) read for every validation
range/doc comment used below. `ui/src/terminal.rs`'s `poll_radio_state` read
directly to confirm none of group 10's 8 fields are part of the passive
200ms polling loop or `Ft991aDisplay` (it only polls the original Wave 2
10-field set) — so, per the task brief's own instruction, this group needed
real keys for everything, not a mostly-display-only screen with a couple of
keys.

### Design decisions made while reading source (before writing code)

1. **Group 10's trait mix confirmed, with one discrepancy found and
   flagged, not silently resolved**: `select_meter`/`get_selected_meter`
   (`MS`)/`get_meter` (`RM` direct-select) are plain `Radio` methods;
   `get_active_meter_reading` (`RM` `P1=0`)/`get_radio_indicator`
   (`RI`)/`get_menu_mode_active` (`RS`)/`get_pll_unlocked`
   (`UL`)/`get_information` (`IF`) are `Ft991aExtras`-only — confirmed by
   reading both files directly. **Discrepancy**: §11.2's table prose
   parenthetically says "PLL-unlock (`RS`→`get_pll_unlocked`)," but the
   actual source has this backwards — `RS` backs `get_menu_mode_active` and
   `UL` backs `get_pll_unlocked` (both `radio_trait.rs`'s doc comments and
   `ft991a.rs`'s implementation agree on this, read directly, not assumed).
   The method names bound to keys `N`/`U` below follow the real source, not
   the table's swapped prose — documented inline in
   `meters_status_commands`'s doc comment so a future reader isn't misled
   by the architect table alone.
2. **Group 10 has zero `Text`-backed commands** — all 8 keys are `List`
   (structurally-typed meter/indicator selection) or `Immediate`
   (zero-argument reads), matching the group's own "read-heavy" framing
   from the task brief. This is a first for this crate: every other
   populated group has at least one numeric `TextInput`.
3. **Date/time (`DT`) design — confirmed no new `ControlState` variant
   needed, per the task brief's own STOP-and-report condition for the
   alternative**: `DT` is a 3-shape wire command where `P1` selects
   date/time/offset, but `radio/src/ft991a.rs` (lines ~2236-2320) already
   splits this into 3 fully independent method pairs at the client-API
   boundary (`read_date`/`write_date`, `read_time`/`write_time`,
   `read_time_zone_offset`/`write_time_zone_offset`), each taking/returning
   its own plain tuple, not a raw `P1`+wire-string pair. This is exactly the
   "genuinely can't be split into 3 independent calls" condition the task
   brief said to check *before* forcing a design — checked, and it turned
   out the `radio` crate had *already* done the splitting in Wave 3, so the
   natural, cleanest fit really is **3 separate keybindings** (`D`/`H`/`Z`
   for set, `Y`/`N`/`F` for read), each with its own `TextInput` and
   format-specific parser (`parse_date`/`parse_time`/
   `parse_time_zone_offset`), consistent with how every other multi-field
   command in this crate (memory tag, keyer memory, encoder nudge, dimmer)
   is already exposed as a single delimited `TextInput` buffer per key
   rather than a combined multi-step flow. No STOP was needed — flagged as
   the considered-and-ruled-out alternative in `system_tuner_dvs_commands`'s
   doc comment, not silently skipped.
4. **Date/time gets 6 keys, not 3** — one write key plus one dedicated read
   key per field (`D`/`Y` date, `H`/`N` time, `Z`/`F` offset), rather than
   only exposing writes. Not specified by the task brief (which only
   flagged the write-side 3-shape design explicitly), but consistent with
   this task's own framing of group 11 as needing full read+write coverage
   like every other `Ft991aExtras`-heavy field here (dimmer, DVS status) —
   a plain read has no format-parsing question to design, so it was
   included as an obvious complement, not a scope expansion needing its own
   justification.
5. **Date validation deliberately does not add calendar-correctness
   checking (leap years, days-per-month) beyond `write_date`'s own `month
   1-12`/`day 1-31` range check** — `parse_date` mirrors exactly what
   `radio::Ft991a::write_date` itself validates (confirmed by reading its
   body, `radio/src/ft991a.rs` lines ~2248-2259: no leap-year/month-length
   calendar validation is performed there either), so e.g. `20260230`
   (Feb 30) is accepted by this UI layer and would be rejected (or silently
   miswritten) only at whatever point the real radio's firmware handles it
   — not invented stricter validation this task wasn't asked to add and the
   underlying client doesn't itself enforce.
6. **Time zone offset parser (`parse_time_zone_offset`) combines sign +
   HH + MM into one signed minutes value before range/step-checking** —
   `write_time_zone_offset`'s own validated range is `-720..=840` minutes
   in 30-minute steps (`radio/src/ft991a.rs` lines ~2308-2320,
   `RadioError::InvalidTimeZoneOffset`), which does not map onto a naive
   "validate HH and MM separately" scheme (e.g. `+1400`=840 min is legal
   but `+1430`=870 min is not, even though both have "valid-looking"
   2-digit HH/MM fields) — the parser must compute the combined value
   first, exactly mirroring what the underlying `write_time_zone_offset`
   itself does internally.
7. **DVS record/playback (`LM`/`PB`) kept as 3 independent keys each
   (start/stop/status) rather than collapsed into one stateful toggle** —
   unlike group 5's `ToggleRts` (which carries the prior boolean state on
   the `ExecuteAction` itself, no arguments needed), `start_dvs_recording`/
   `start_dvs_playback` require a channel argument `stop_*` doesn't have, so
   a single toggle key would need a `Ft991aDisplay`-tracked "currently
   active channel" this group does not poll — 3 plain, independently-typed
   commands per LM/PB was simpler and avoided inventing new display-state
   tracking this task wasn't asked to add. Documented as a considered
   alternative in `system_tuner_dvs_commands`'s doc comment, not a silent
   default.
8. **22 keys for group 11, judgment call on scope, matching this crate's
   established "full-coverage goal outweighs hitting the table's 'approx'
   item count" precedent** (groups 2/3/7) — the table gives only an
   approximate "12" item count for this group's *conceptual* wire-command
   groupings, but this task exposes all ~29 underlying `Radio`/
   `Ft991aExtras` methods (5 get/set pairs on `Radio`, ~19 methods on
   `Ft991aExtras` incl. the 6-method date/time/tz split and the 6-method
   DVS record/playback split) as 22 distinct keys — the largest single
   group in this crate to date (previous largest: group 7's 16). Justified
   by this group's own "most `Ft991aExtras`-heavy" framing in the task
   brief, not scope creep for its own sake.
9. **All group 10/11 `List`-backed fields default `initial_list_cursor` to
   cursor 0** — same "not polled yet" documented limitation groups 4-9
   already carry (`Ft991aDisplay` was not extended with new fields this
   task, matching every populated group's precedent since group 3).
   `RI`'s indicator picker gets its own distinct doc-comment framing (not
   just "not polled yet"): unlike `MS`'s meter selection (which
   *could* in principle be polled since `get_selected_meter` exists), `RI`
   has no "currently selected indicator" concept at all — each of the 7
   selectors is an independent, stateless read — so cursor 0 there is an
   arbitrary starting point, not a stand-in for a value this crate could
   ever pre-select even with more polling.
10. **No new `terminal.rs` tests added** — all 30 new `execute_action` arms
    (8 group-10 + 22 group-11) are plain 1:1 `Radio`/`Ft991aExtras`
    passthroughs (including the "read and format" arms like
    `GetSelectedMeter`/`ReadDate`/`GetDimmer`, which follow the exact same
    shape Task 3 already established for `GetMemoryChannel`/
    `ReadMemoryChannel` — no executor-side branching/rollback logic like
    group 5's `ToggleRts`), so per Tasks 3/5/6's established division of
    test coverage, no new `terminal.rs` tests or `MockRadio` overrides were
    needed.
11. **Two pre-existing tests' premises broke and were fixed, not silently
    left green-by-accident** (same category of fix Tasks 5/6 made):
    `test_stub_groups_have_no_commands_yet`'s `populated` list (and its
    stale header comment) needed both new groups added.
    `test_stub_group_char_key_is_a_no_op` previously used key `'z'` against
    `CommandGroup::MetersStatus` (Task 6's own retarget, "still genuinely
    stubbed" at the time) — `MetersStatus` is now populated by this task
    (though `'Z'` happens not to collide with any of its 8 real keys, so the
    test would not have started *failing*, its doc comment claiming the
    group is "still genuinely stubbed" would have become false). Retargeted
    proactively to `CommandGroup::ExMenu` (the one group still genuinely
    stubbed after this task — Wave 4 dispatch queue items 8-9, not this
    task's job), and gave groups 10/11 their own dedicated `'Q'`-key no-op
    regression tests (`Q` is not one of either group's real keys — group
    10's are `M C D F I R N U`, group 11's are `A B C D E F G H I K L N P R
    S T U V W X Y Z`).

### File layout touched

- `ui/src/control.rs` — `InputAction` gains 6 group-11 `Text`-backed
  variants (`SetDimmer`, `SetDate`, `SetTime`, `SetTimeZoneOffset`,
  `StartDvsRecording`, `StartDvsPlayback`); `SelectAction` gains 3 group-10
  + 7 group-11 `List`-backed variants; `ExecuteAction` gains 8 group-10 + 22
  group-11 variants; real `meters_status_commands()` (8 keys, was
  `Vec::new()` stub) and `system_tuner_dvs_commands()` (22 keys, was
  `Vec::new()` stub); `validate_text_input`/`select_action_to_execute`/
  `initial_list_cursor` grown with the new arms; new helpers (`METER_ORDER`/
  `meter_options`, `RADIO_INDICATOR_ORDER`/`radio_indicator_options`,
  `REPEATER_SHIFT_ORDER`/`repeater_shift_options`, `tx_vfo_options`,
  `antenna_tuner_options`, 5 group-10 `_immediate` functions, 9 group-11
  `_immediate` functions, `parse_dimmer`, `parse_date`, `parse_time`,
  `parse_time_zone_offset`, `parse_dvs_channel`); module doc comment updated
  to list groups 1-11 as populated, 12 as the remaining stub; 87 new tests
  (reachability, key-count+uniqueness, per-key transitions,
  `SelectMeter`/`ReadMeterDirect`/`ReadRadioIndicator`/`SetRepeaterShift`/
  `SetTxVfo`/`SetAntennaTunerState`/boolean-toggle cursor<->value lockstep,
  validation-range boundaries for dimmer/date/time/time-zone-offset/DVS-
  channel incl. valid+invalid cases for every date/time/offset format, no-op
  regression guards); 2 pre-existing tests updated/retargeted in place
  (decision 11 above), documented, not silently changed.
- `ui/src/terminal.rs` — `execute_action` gains 30 new match arms (8
  group-10 + 22 group-11), all plain `Radio`/`Ft991aExtras` passthroughs
  (some formatting a fetched value into feedback text, same shape Task 3
  established); no signature change, no new tests, no `MockRadio` changes
  needed (decision 10 above).
- `ui/src/layout.rs`, `ui/src/lib.rs` — **not touched by this task**
  (confirmed by `git diff --stat`); both already carried uncommitted
  changes pre-dating this session (same pre-existing working-tree state
  Tasks 5/6 already flagged) — noted here so this isn't mistaken for scope
  creep by this task.
- `radio/`, `emulator/`, `src/main.rs` — not touched, per constraints
  (confirmed via `git diff --stat` — this session's edits are limited to
  `ui/src/control.rs` and `ui/src/terminal.rs`; other uncommitted changes
  present in the working tree pre-date this session).

### Verification plan

- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt -p ui -- --check`, `cargo build --workspace`.
- Result: **374 passed, 0 failed** (up from 287 — net +87 new tests, zero
  regressions, all 287 pre-existing tests still pass, 2 of them
  updated/retargeted in place per decision 11, documented, not silently
  changed). `cargo clippy -p ui --all-targets -- -D warnings` clean on the
  first pass. `cargo fmt -p ui -- --check` found reflow spots (import list,
  a `const` array literal, several chained `.parse()` calls, a couple of
  long match arms/`vec!`/`assert_eq!` lines) — fixed with `cargo fmt -p ui`,
  clean after. `cargo build --workspace` clean.

## Session: Wave 4 Task 8 — `EX` menu number-entry escape hatch (path (b))

### Design decisions made while reading source

1. **Prerequisite confirmed already landed.** `radio::Ft991aExtras::
   get_ex_menu_item(&mut self, p1: u16) -> RadioResult<i32>` /
   `set_ex_menu_item(&mut self, p1: u16, value: i32) -> RadioResult<()>`
   exist exactly as §11.4 specified (`radio/src/radio_trait.rs` lines
   ~2135-2144, `radio/src/ft991a.rs` lines ~2510-2559/~2729-2733).
   `ExMenuValueKind::Enumerated` is `&'static [(&'static str /* wire */,
   &'static str /* label */)]` (`radio/src/ft991a_radio.rs` lines
   ~2167-2189) — the label extension §11.4 flagged as a recommended
   addition, not the pre-Task-1 bare-wire-string shape. Both re-exported at
   the `radio` crate root (`ExMenuItem`, `EX_MENU_TABLE`); `ExMenuValueKind`
   itself is not re-exported at the root but is reachable via the public
   `radio::ft991a_radio::ExMenuValueKind` path (the `ft991a_radio` module
   is `pub mod`).
2. **Used `radio::ex_menu_item(p1) -> Option<&'static ExMenuItem>`**
   (`radio/src/ft991a_radio.rs` line ~4020, re-exported at the crate root)
   instead of inlining `EX_MENU_TABLE.iter().find(|i| i.p1 == p1)` as
   §11.4's pseudocode literally writes — this helper already exists and is
   exactly that expression; using it is strictly DRY-er and behaviorally
   identical. Not a deviation from the design, just from its pseudocode's
   literal spelling.
3. **`ExMenuValueKind::parse`/`format` are `pub(crate)` to `radio`**, not
   visible from `ui`. `select_action_to_execute`'s new
   `SelectAction::SetExMenuItem` arm therefore parses the `Enumerated`
   item's wire string directly with `str::parse::<i32>()` rather than
   calling `ExMenuValueKind::parse` — confirmed safe by reading every
   landed `Enumerated` row in `EX_MENU_TABLE`: all wire values are plain
   unsigned decimal digit strings (no `+`/`-` sign character — that's a
   `Range`-only concept via `ExMenuValueKind::Range::signed`), so a bare
   `str::parse` is equivalent for this subset. If a future `Enumerated` row
   ever used a signed or otherwise non-trivial wire encoding, this would
   need revisiting — flagged here, not assumed permanent.
4. **Key collision found and resolved (real discrepancy vs. §11.4, not a
   reinterpretation).** §11.4's own state-machine pseudocode literally
   writes `'['X']'` for the top-level entry key. `'X'` is already
   `CommandGroup::ScanVoxBusy`'s `group_key` (`ui/src/control.rs`, assigned
   during Wave 4 Task 2 — a judgment call left open by the architect's plan
   text itself, since single-letter key assignments were explicitly not
   specified by `planning/architect/task_plan.md`, only group names/order
   were). The architect's §11.4 section was written in the same planning
   session as §11.2's group table but necessarily before Task 2's actual
   key choices existed, so this collision could not have been caught at
   design time. Resolution: `'N'` ("Number entry"), verified unique against
   `ALL_GROUPS`'s 12 keys + `Q` via a dedicated test
   (`test_ex_number_entry_key_is_unique`). Declared as `pub(crate) const
   EX_NUMBER_ENTRY_KEY: char = 'N'` for a single source of truth, matching
   `group_key`'s own pattern.
5. **`ExValueEntry` is not a third `ControlState` variant.** Re-read §11.4's
   pseudocode carefully: it introduces `ExValueEntry { item, .. }` as a
   *conceptual* fork point, then immediately says each branch "reuse[s]
   `ControlState::ListSelect`/`TextInput` verbatim" — i.e. the fork is
   physically realized by transitioning directly into one of the two
   already-existing states (carrying a new `SetExMenuItem` action), not by
   adding a distinct `ControlState::ExValueEntry` variant that would then
   itself need its own render/key-handling logic duplicating `ListSelect`/
   `TextInput`'s. Implemented as a small `enter_ex_value_entry(item:
   &'static ExMenuItem) -> ControlState` helper that matches on `item.kind`
   and returns the appropriate `ControlState::{ListSelect, TextInput}`
   directly. `ControlState::ExNumberEntry`, by contrast, genuinely is a new
   variant — the task brief states its shape explicitly
   (`{ buffer: String, error: Option<String> }`), and unlike the
   `ExValueEntry` fork, its behavior (3-digit-max entry, EX_MENU_TABLE
   lookup) isn't expressible by reusing any existing state's shape (it's
   not a general-purpose numeric `TextInput` — `TextInput`'s existing
   validation dispatch is keyed by `InputAction`, one static prompt/range
   per variant, not a dynamic per-entered-number lookup).
6. **Esc redirect scoped narrowly to the new action variant, not by
   `ControlState` shape.** `TextInput`/`ListSelect`'s `Esc` handling now
   branches on whether `action` is `InputAction::SetExMenuItem`/
   `SelectAction::SetExMenuItem` specifically (redirecting to
   `ExNumberEntry` with `buffer` restored to the entered `p1`) — every
   other action of either state still falls through to `Menu` unchanged.
   Verified both directions with dedicated tests
   (`test_ex_value_entry_{list_select,text_input}_esc_returns_to_ex_number_
   entry_with_p1` and `test_ex_value_entry_other_{text_input,list_select}_
   esc_still_returns_to_menu`), since a shape-based branch (e.g. "any
   `TextInput` reached via `ExNumberEntry`") wasn't distinguishable from
   the state alone — only the carried action tells the two apart.
7. **Read-first-then-edit: evaluated, explicitly skipped, not silently
   dropped.** §11.4 frames this as "recommended... not load-bearing... a
   should-have, not a hard requirement," with an explicit escape valve in
   this task's own brief for exactly this case. Traced the concrete
   blocker: `handle_key(key: KeyEvent, state: &mut ControlState, display:
   &Ft991aDisplay) -> KeyResult` is synchronous and only holds `&
   Ft991aDisplay`, never `&mut R: Radio` — confirmed by reading
   `ui/src/terminal.rs`'s `run_loop`, where `execute_action` (the only
   function with radio access) runs strictly *after* `handle_key` returns
   a `KeyResult::Execute`, one event-loop iteration too late to influence
   the value-entry state `handle_key` just built. `Ft991aDisplay` also has
   no per-`p1` `EX` item cache (`ui/src/lib.rs`'s field list checked
   directly) unlike `SetMode`'s `initial_list_cursor`, which reads an
   already-polled `display.mode`. Making this work would require either
   (a) polling all 151 `EX` items into `Ft991aDisplay` every 200ms cycle —
   wasteful, since `EX` items are rarely-changed settings, not live
   operating state, or (b) making `handle_key` itself async so it could
   issue a one-off `get_ex_menu_item` call mid-transition — a materially
   larger architecture change than this task's scope (would ripple into
   every other synchronous `handle_key` call site). Both rejected as
   disproportionate to a "should-have." Documented in
   `enter_ex_value_entry`'s own doc comment, not just here.
8. **`ui/src/layout.rs` needed a real change, unlike Task 7's "confirmed no
   changes needed."** `draw_control_panel`'s two `match state { ... }`
   blocks are exhaustive over `ControlState` (no wildcard arm), so adding
   `ControlState::ExNumberEntry` was a compile-breaking change until a
   render arm existed. Per §11.4's own wording ("reuses the `TextInput`
   rendering shell"), folded `ExNumberEntry` into the existing outer-match
   group alongside `TextInput`/`ListSelect`/`Feedback` (the "3-line layout"
   branch), then added an inner-match arm rendering a fixed prompt line
   ("EX menu item number (001-153):") plus the same buffer/error rendering
   `TextInput`'s own arm uses. This file wasn't in the task brief's "read
   first" list (only `control.rs`/`terminal.rs` were) — flagged here as an
   unavoidable mechanical knock-on of adding an enum variant to an already-
   exhaustively-matched type, not scope creep.
9. **`terminal.rs`'s `execute_action` arm is a plain passthrough**, same
   shape as every `Radio`/`Ft991aExtras` passthrough arm already
   established (Tasks 3/5/6/7) — `ExecuteAction::SetExMenuItem(p1, value)
   => ("EX menu item set", ok_unit(radio.set_ex_menu_item(p1, value).await))`.
   No executor-side branching logic (unlike Task 4's RTS optimistic-set/
   rollback), so no `display: &mut Ft991aDisplay` mutation needed here —
   confirmed by re-reading `execute_action`'s own doc comment, which states
   `display` is mutated by exactly one existing action (`ToggleRts`) and
   left untouched by everything else; this new action doesn't change that.
10. **`MockRadio` test-double extension**: added an `ex_menu_calls:
    Vec<(u16, i32)>` field and overrode `Ft991aExtras::set_ex_menu_item`
    (rather than leaving it at its inherited `NotImplemented` default) to
    record `(p1, value)` call arguments — the same "override, don't rely
    on the blanket default" precedent `CwKeying::assert_rts`'s override
    already established in Task 4, needed here because the round-trip test
    must verify *what* was called, not just that some `Result` came back.

### File layout touched

- `ui/src/control.rs` — `EX_NUMBER_ENTRY_KEY` constant; `ControlState`
  gains `ExNumberEntry { buffer, error }`; `InputAction` gains
  `SetExMenuItem(u16)`; `SelectAction` gains `SetExMenuItem(u16)`;
  `ExecuteAction` gains `SetExMenuItem(u16, i32)`; new
  `enter_ex_value_entry` helper; `validate_text_input`/
  `select_action_to_execute`/`initial_list_cursor` each gain one new arm;
  `handle_key` gains the `Menu`-level `'N'` dispatch, the whole
  `ExNumberEntry` match arm, and the EX-specific `Esc` redirects in
  `TextInput`/`ListSelect`; 27 new tests (key uniqueness/reachability,
  digit-buffer growth/cap/backspace, empty/unknown-`p1` errors,
  `Enumerated`/`Range` fork correctness incl. label text and prompt
  content, leading-zero `p1` parsing, `Esc` redirect both ways incl. a
  non-EX regression guard, `Range` boundary/step/non-numeric rejection,
  `Enumerated` confirm producing the right `ExecuteAction`, defensive
  fallback paths in `select_action_to_execute`/`initial_list_cursor`).
- `ui/src/terminal.rs` — `execute_action` gains one new match arm
  (`SetExMenuItem`, plain passthrough); `MockRadio` gains `ex_menu_calls`
  + an overridden `set_ex_menu_item`; 4 new tests (2 direct
  `execute_action` tests — success recording the call, failure recording
  nothing — and 2 full state-machine-to-radio round-trip tests, one per
  `ExMenuValueKind` fork).
- `ui/src/layout.rs` — one new render arm for `ControlState::ExNumberEntry`
  (decision 8 above) — not in the task brief's file list, but required for
  the crate to compile once the new `ControlState` variant existed.
- `radio/`, `emulator/`, `src/main.rs`, `ts570d/`, `radio-cat-rs` — not
  touched, per constraints.
- Path (a) (themed browsing sub-groups, `ex_menu_commands()`'s `Vec::new()`
  stub) — **not touched**, per constraint 6 of the task brief. Task 9's
  job; will consume `enter_ex_value_entry`/`SetExMenuItem` unchanged, per
  §11.4's "the two paths converge on the same value-entry state" design.

### Verification plan

- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt -p ui -- --check`, `cargo build --workspace`.
- Result: **405 passed, 0 failed** (up from 374 — net +31 new tests, zero
  regressions, all 374 pre-existing tests still pass unmodified). `cargo
  clippy -p ui --all-targets -- -D warnings` clean on the first pass.
  `cargo fmt -p ui -- --check` found reflow spots (a `match` arm, a
  let-else binding, a couple of `assert_eq!`/`execute_action` calls
  spanning the 100-col limit) — fixed with `cargo fmt -p ui`, clean after.
  `cargo build --workspace` clean.

## Session: Wave 4 Task 9 — `EX` menu themed browsing (path (a)) — Wave 4's
final task

### Plan

1. Read `planning/architect/task_plan.md` §11.4's "(a) Themed browsing
   sub-groups" paragraph in full — the authoritative spec: bucket
   `EX_MENU_TABLE`'s real `p1` values at runtime, not a hand-maintained
   list; sub-groups by `p1` range; each a `GroupMenu`-style scrollable list
   showing `"{p1:03} {name}"`; `Enter` on an item forks into the same
   value-entry flow Task 8 built.
2. **Resolve the "five sub-groups" vs. six-listed-ranges discrepancy** by
   counting the ranges actually given, not the stated number — six
   `ExTheme` variants.
3. Read `ui/src/control.rs` for Task 8's exact `enter_ex_value_entry`
   name/signature and the `ExMenu` stub (`ex_menu_commands() -> Vec::new()`
   placeholder) to confirm the exact reuse point.
4. Read enough of `radio/src/ft991a_radio.rs`'s `EX_MENU_TABLE` (151
   entries) to confirm the real `p1` distribution across the architect's
   proposed ranges, including the documented `027`/`087` gaps, and verify
   the flagged 080-091 boundary against the table's actual item names.
5. Read `ts570d/ui/src/control.rs`'s `GroupMenu{group,cursor}` shape and
   confirm whether `cursor` is wired to scroll keys anywhere in either
   codebase — it is not, in either — and decide whether a genuinely
   scrollable state is needed for the largest sub-group (45 items). It is;
   design a new `ControlState::ExSubGroupMenu { theme, cursor }` with real
   Up/Down handling, rather than trying to retrofit `GroupMenu`'s cursor
   or shipping an unscrollable 45-item list.
6. Design the top-level `EX` group UI shape: `CommandGroup::ExMenu`'s own
   `GroupMenu` command list becomes the "theme picker" — 6 `ExTheme`
   entries (`CommandKind::ExSubGroup`) plus the number-entry escape hatch
   folded in as a 7th entry (`CommandKind::EnterExNumberEntry`), alongside
   its existing dedicated `Menu`-level key from Task 8 (kept, not removed)
   — both access points stay reachable, judgment call on discoverability.
7. Extend `enter_ex_value_entry`'s signature (not duplicate it) with an
   `origin: ExValueEntryOrigin` parameter so `Esc` from the value-entry
   fork can route back to either `ExNumberEntry` (path (b)) or the
   originating `ExSubGroupMenu` (path (a), cursor restored) — the one
   behavioral difference §11.4's own pseudocode specifies between paths.
   Requires a new `SetExMenuItemFromTheme(u16, ExTheme, usize)` variant on
   `InputAction`/`SelectAction`, sharing every other match arm with the
   existing `SetExMenuItem(u16)` variant via `|`-combined patterns (no
   validation/execution logic duplicated).
8. Add `draw_ex_sub_group_menu` to `ui/src/layout.rs` — a sliding-window
   scroll render, the first genuinely scrolling list in this crate.
9. Write unit tests: theme-range bucketing (partition/no-overlap/boundary
   spot-checks/known-gap exclusion), sub-group cursor movement and clamps,
   item-selection forks (`Enumerated`/`Range`), the full `Esc` navigation
   chain (value-entry -> sub-group list -> theme picker -> `Menu`), and a
   path-(a)-vs-path-(b) convergence test comparing rendered content and
   final `ExecuteAction` for the same `p1` via both paths.
10. Verify: `cargo test -p ui`, `cargo clippy -p ui --all-targets -- -D
    warnings`, `cargo fmt -p ui -- --check`. Confirm zero regressions
    against the 405-test baseline and confirm (via `git diff --stat`) this
    task's own diff is confined to `ui/src/control.rs` and
    `ui/src/layout.rs`.

### Decisions and rationale

1. **Six `ExTheme` variants, not five** — §11.4's prose says "five
   sub-groups" but lists six labeled ranges (001-046/047-079/080-091/
   092-110/111-136/137-153). Counted the ranges given, flagged the
   discrepancy in the final report rather than guessing which of the six
   to drop or silently picking "five."
2. **080-091 boundary confirmed clean, no adjustment**: `079` "FM PKT
   MODE" (last of the TX audio chain) is immediately followed in the real
   table by `080` "RPT SHIFT 28MHz" (first of the mixed grab-bag), and
   `091` "STANDBY BEEP" is immediately followed by `092` "RTTY LCUT FREQ".
   Both boundaries the architect flagged for verification land on genuine
   thematic breaks — checked against `EX_MENU_TABLE`'s actual item names,
   not assumed from the manual's rough page layout.
3. **`ExSubGroupMenu` is a new state, not a repurposed `GroupMenu`** —
   `GroupMenu`'s `cursor` field stays exactly as vestigial as it was
   before this task (confirmed unwired in both this codebase and
   `ts570d`'s reference precedent); `ExMenu`'s own theme-picker list is
   only 7 entries (no scrolling need), so retrofitting `GroupMenu`'s
   cursor would have been solving a problem that list doesn't have, while
   leaving the list that *does* need it (up to 45 items) unaddressed. A
   dedicated new state with genuinely-wired Up/Down + `j`/`k` navigation
   and a sliding-window render directly addresses the task brief's "don't
   ship an unusable unscrollable list" instruction.
4. **Reuse via extended signature, not a duplicate function** —
   `enter_ex_value_entry` is the same function both paths call; adding an
   `origin` parameter (rather than writing `enter_ex_value_entry_themed`
   as a near-copy) satisfies "reuse it, don't duplicate" literally.
5. **Both `EX` access points kept reachable** — folded the escape hatch
   into `ExMenu`'s own command list *in addition to* keeping its existing
   `Menu`-level key, rather than choosing one. Discoverability judgment
   call, not specified by §11.4.
6. **Convergence tested at the behavioral level, not raw enum equality** —
   path (a)'s `action` necessarily carries more data (origin routing) than
   path (b)'s, so the two produced `ControlState`s are never going to be
   `==`. The two convergence tests instead assert what §11.4 actually
   means by "converge": identical rendered content (options/prompt) and
   identical final `ExecuteAction` after confirming, for the same `p1` via
   either path.

### File layout touched

- `ui/src/control.rs` — new `ExTheme` enum (6 variants) + `ALL_EX_THEMES`
  + `ex_theme_range`/`ex_theme_label`/`ex_theme_key`/`ex_theme_items`;
  `CommandKind` gains `ExSubGroup(ExTheme)`/`EnterExNumberEntry`;
  `ControlState` gains `ExSubGroupMenu { theme, cursor }`; `InputAction`/
  `SelectAction` each gain `SetExMenuItemFromTheme(u16, ExTheme, usize)`;
  new `ExValueEntryOrigin` enum; `enter_ex_value_entry` signature extended
  with an `origin` parameter; `validate_text_input`/
  `select_action_to_execute`/`initial_list_cursor` each gain a
  `|`-combined pattern for the new action variant (no new logic);
  `handle_key` gains the `ExSubGroupMenu` match arm and two new
  `CommandKind` arms inside `GroupMenu`'s handler, plus `FromTheme`-aware
  `Esc` routing in the `TextInput`/`ListSelect` arms; `ex_menu_commands()`
  rewritten from the `Vec::new()` stub to 7 real entries; 20 new tests
  (replacing 2 now-stale stub-group tests, net +20).
- `ui/src/layout.rs` — `draw_control_panel` gains an `ExSubGroupMenu` arm
  (plus the exhaustive-match no-op arm for it in the shared 3-line-layout
  block); new `draw_ex_sub_group_menu` helper (sliding-window scroll
  render).
- `radio/`, `emulator/`, `src/main.rs`, `ts570d/`, `radio-cat-rs` — not
  touched, per constraints.

### Verification plan

- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt -p ui -- --check`.
- Result: **425 passed, 0 failed** (up from 405 — net +20 new tests, zero
  regressions, all 405 pre-existing tests still pass unmodified). `cargo
  clippy -p ui --all-targets -- -D warnings` clean on the first pass.
  `cargo fmt -p ui -- --check` found one reflow spot (a `Span::styled` call
  in `draw_ex_sub_group_menu`) — fixed with `cargo fmt -p ui`, clean after.
  `git diff --stat` confirms this task's own changes are confined to
  `ui/src/control.rs` and `ui/src/layout.rs`.
- **This completes Wave 4's entire dispatch queue.**
