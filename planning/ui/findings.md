# UI Agent Findings

(none from Wave 2)

## Wave 4 Task 2 — grouped-menu skeleton

- `ts570d` is a **sibling checkout**
  (`/home/mattfranklin/src/github.com/kf0uwv/ts570d`), not nested inside
  this repo — `find` confirms no `ts570d/` directory exists under this
  repo's own tree. The architect plan's relative-path phrasing for
  `ts570d/ui/src/control.rs` reads naturally either way; worth flagging so a
  future reader doesn't waste time looking for it inside `ft991a/`.
- `ts570d/ui/src/control.rs`'s `ControlState::GroupMenu{group,cursor}`
  `cursor` field is **vestigial in the actual reference code** — grepped the
  whole `ts570d/ui/src` tree: `cursor` is always initialized to `0`, never
  mutated by any key handler (group commands are selected by direct digit
  keys `1`-`9`/`a`-`c`, independent of `cursor`), and never read by any
  render function (`layout.rs` pattern-matches `GroupMenu{group,..}`,
  discarding `cursor`). The architect plan's phrase "`GroupMenu{group,
  cursor}`'s scrolling behavior" describes an intended future capability
  (for larger groups, e.g. `EX` sub-groups), not code that exists to port
  today. Ported the field for structural/future parity, left it unwired,
  same as the reference.
- `radio/src/radio_trait.rs`'s real `Ft991aExtras`/`CwKeying` definitions
  (lines 1842-2200) match the architect plan's §11.3 prose closely — no
  discrepancy found. Both `#[async_trait(?Send)]`-annotated where
  applicable (`Ft991aExtras` is; `CwKeying` is plain sync, matching
  `ModemControlLines`'s own precedent); both re-exported from
  `radio::{Ft991aExtras, CwKeying}`.
- Confirmed by grep in `radio-cat-rs` (not just trusted the plan's
  assertion): `cat-transport-serial/src/io_uring.rs`'s
  `impl ModemControlLines for SerialPort` and `session.rs`'s
  `impl<T: Transport + ModemControlLines> ModemControlLines for
  SerialCatSession<T>` are both unconditional (no feature flag/generic
  bound gates them off) — so `Ft991a<SerialCatSession<SerialPort>>`
  (`src/main.rs`'s only concrete wiring) satisfies `CwKeying` today with no
  code change required anywhere.

## Wave 4 Task 3 — populate groups 3/4/6

- **Architect's "100% `Radio`-trait-backed" claim for groups 3, 4, and 6
  confirmed exactly, no discrepancy** — read every method's actual
  definition in `radio/src/radio_trait.rs`'s `Radio` trait body directly
  (not just the §11.2 table's prose): the memory-channel section (`MC MR
  MW MT`, lines ~1093-1127), the clarifier/RIT-XIT + IF-shift + tone
  section (`RT RC RD RU XT IS CT CN`, lines ~1179-1279), and the scan/VOX/
  busy section (`SC VX VD VG BY`, lines ~1363-1443) are all plain `Radio`
  trait methods with `NotImplemented` default bodies, same shape as group
  1's own methods — none of them needed `Ft991aExtras`. This is the first
  of the three "prove the skeleton works" groups to actually verify the
  claim against source rather than propagate it forward untested.
- `CTCSS_TONES_DECIHZ`/`DCS_CODES` (the 50/104-entry standard-tone/code
  tables) and the `ctcss_tone_hz`/`ctcss_tone_index`/`dcs_code_index`/
  `dcs_code_number` conversion functions all live in `ft991a_radio.rs` but
  are re-exported at the crate root (`radio/src/lib.rs`'s `pub use`
  block) — reachable as `radio::CTCSS_TONES_DECIHZ`/`radio::DCS_CODES`
  without `ui` needing to name the `ft991a_radio` module path directly.
  `ui`'s `ctcss_tone_options`/`dcs_code_options` build their `ListSelect`
  option strings straight from these tables at runtime (not a
  hand-maintained duplicate), so an invalid CTCSS Hz/DCS code is
  structurally unrepresentable through the UI — no free-text validation
  needed for those two fields, unlike every other numeric input added this
  task.
- `radio::MemoryChannelEntry`, `MemoryTag`, `TaggedMemoryChannel` are all
  plain structs with `pub` fields (confirmed by reading
  `radio_trait.rs` lines ~907-987) — directly constructible from `ui`/
  `terminal.rs` with no builder or FT-991A-crate-internal helper needed.
  `MemoryTag::new(&str) -> Result<Self, RadioError>` already implements
  exactly the validation this task needed (<=12 chars, printable ASCII
  space-tilde excluding `;`) — reused directly rather than re-implementing
  the character-set check in `ui`.
- `ts570d/ui/src/control.rs`'s `on_off()` helper (`vec!["On", "Off"]`,
  cursor 0 = On) and its `WriteMemoryChannelFromVfoA`/`ReadMemoryChannel`
  `execute_action` arms (in `ts570d/ui/src/terminal.rs`) were read directly
  and reused as *conventions* (not values) for this task's groups 4/6
  on/off toggles and group 3's VFO-A-sourced writes — confirms these are
  genuine, precedented patterns in the sibling codebase, not invented here.
  `ts570d`'s `execute_action` return-type shape (`(&'static str,
  RadioResult<String>)`, extra text wins over the generic "OK: {desc}"
  when non-empty) was likewise read directly and adopted verbatim in
  `terminal.rs`, since group 3's read-type commands (`GetMemoryChannel`/
  `ReadMemoryChannel`/`ReadMemoryChannelTag`) need to surface a fetched
  value that the old unit-only return type had no way to carry.
- **`cargo clippy`'s `enum_variant_names` lint fired** on `SelectAction`
  once it grew from 1 variant (`SetMode`) to 8, all but 3 sharing the
  `Set*` prefix in a way clippy read as "all variants have the same
  prefix." Not previously visible since a 1-variant enum never triggers
  this lint. Fixed by renaming the three boolean-toggle variants to
  `Toggle*` (`ToggleRxClarifier`, `ToggleTxClarifier`, `ToggleVox`) —
  checked `ts570d/ui/src/control.rs`'s own `SelectAction` first and
  confirmed it already uses exactly this `Toggle`-vs-`Set` split for the
  same reason, so this is precedented, not an arbitrary workaround.

## Wave 4 Task 4 — populate group 5 (Keyer/CW/Break-In) + RTS keying

- **`CwKeying::assert_rts`'s signature confirmed exactly as
  `planning/architect/task_plan.md` §11.3 point 6 describes**: `fn
  assert_rts(&self, asserted: bool) -> RadioResult<()>`
  (`radio/src/radio_trait.rs` line 2177) — sync, `&self`, default body
  `Err(RadioError::NotImplemented)`. No discrepancy from the plan.
- **§11.2's table claim for group 5's trait mix confirmed exactly**:
  `BI`/`SD`/`CS`/`KR`/`KS`/`KP`/`ZI` are 100% `Radio`-trait-backed
  (`radio_trait.rs` lines ~1298-1361, plain `NotImplemented`-default
  methods same shape as every other `Radio` method); `KM`
  (`read_keyer_memory`/`write_keyer_memory`) and `KY`
  (`play_keyer_memory`) are `Ft991aExtras`-only (lines ~1906-1930),
  deliberately kept off `Radio` per that trait's own doc comment
  ("FT-991A-inherent, not a generic CW-operating concept" — tightly
  coupled to the FT-991A-specific `KM` message store). The new RTS toggle
  is the crate's first key backed by `CwKeying` rather than `Radio`/
  `Ft991aExtras`.
- **`SD` (semi break-in delay) and `KS` (keyer speed) have *no* step
  constraint**, unlike `VD` (VOX delay, group 6)'s 10ms-step precedent —
  confirmed by reading `set_semi_break_in_delay`/`set_keyer_speed`'s
  actual implementation bodies in `radio/src/ft991a.rs` (only a range
  check, no modulus check), not assumed by analogy to `VD`. `KP` (keyer
  pitch) *does* have a 10 Hz step, confirmed the same way. This is
  exactly the kind of "check the actual method signatures, don't guess
  ranges" the task brief asked for — the assumption-by-analogy would have
  been wrong for 2 of the 3 numeric fields.
- **The RTS toggle's optimistic-set + rollback-on-error logic could not
  live in `control.rs`'s pure state machine** — `handle_key` only ever
  sees an immutable `&Ft991aDisplay`, so the actual display mutation has
  to happen in `terminal.rs`'s `execute_action`, at the same point the
  real `assert_rts` call happens. This forced a real, disclosed signature
  change to `execute_action` (gained a `display: &mut Ft991aDisplay`
  parameter, bound widened to `Radio + Ft991aExtras + CwKeying`) — not
  something the task's own "read first" list of files called out
  explicitly, but necessary and flagged here the same way Task 3 flagged
  its own unlisted `terminal.rs` touch for an analogous reason (new
  `ExecuteAction` variants are matched exhaustively there).
- **One pre-existing Wave 4 Task 2 test's premise silently broke** once
  group 5 stopped being an empty stub:
  `test_stub_group_char_key_is_a_no_op` pressed lowercase `'z'` inside
  `CommandGroup::KeyerCwBreakIn` expecting a no-op, but `'Z'` is now group
  5's real zero-in key (`find_group_command` uppercases before matching).
  Caught by running the full test suite after adding group 5's content
  (test failed as expected, not silently green) — retargeted at
  `CommandGroup::VfoMemoryQuickOps` (still genuinely stubbed) and gave
  group 5 its own no-op regression test using `'q'` instead. Documented
  in `task_plan.md`'s decision 9, not silently fixed.

## Wave 4 Task 5 — populate group 2 (VFO/Memory Quick-Ops) and group 9
## (Band/Step/Encoder)

- **§11.2's table claim for group 2's trait mix confirmed exactly, no
  discrepancy** — read `radio_trait.rs`'s batch-1 section (lines
  ~1129-1177) and `ft991a.rs`'s client implementations (lines ~1005-1083)
  directly: `copy_vfo_a_to_b` (`AB`), `copy_vfo_b_to_a` (`BA`), `swap_vfos`
  (`SV`), `store_vfo_to_memory` (`AM`), `recall_memory_to_vfo` (`MA`),
  `memory_channel_up`/`memory_channel_down` (`CH0`/`CH1`) are plain `Radio`
  methods; `toggle_vfo_memory_mode` (`VM`), `qmb_store` (`QI`), `qmb_recall`
  (`QR`), `quick_split` (`QS`) are `Ft991aExtras`-only, each with its own
  doc comment explaining why (`VM`'s meaning rests on a documented
  manual-heading ambiguity between `VM`'s and `AM`'s per-command box
  headings; `QI`/`QR`/`QS` are FT-991A-named "Quick" features distinct from
  the generic memory-channel/split concepts `Radio` already exposes). All
  11 are zero-argument, write-only wire triggers — no numeric or
  enumerated input exists anywhere in this group, so it needed zero new
  `validate_text_input`/`select_action_to_execute` arms, only new
  `ExecuteAction` variants and one `_immediate` helper function per
  trigger (matching `get_memory_channel_immediate`/`clarifier_clear_immediate`/
  `zero_in_immediate`'s established one-function-per-trigger convention
  rather than a shared generic closure).
- **§11.2's table claim for group 9's trait mix confirmed exactly** — read
  `radio_trait.rs`'s batch-8 section (lines ~1654-1710) and
  `Ft991aExtras`'s encoder/ENT-key section (lines ~2002-2019) directly:
  `set_band` (`BS`), `band_up`/`band_down` (`BU`/`BD`), `get_fine_step`/
  `set_fine_step` (`FS`), `mic_up`/`mic_down` (`UP`/`DN`) are plain `Radio`
  methods; `encoder_down`/`encoder_up` (`ED`/`EU`) and `ent_key` (`EK`) are
  `Ft991aExtras`-only, with `radio_trait.rs`'s own comment stating they are
  "FT-991A-specific concept[s] with no `ts570d::Radio` precedent and no
  clean generic abstraction."
- **Band select uses `ListSelect`, not `TextInput`, per the task's own
  "check what's more natural given the method's actual parameter type"
  instruction** — `Radio::set_band` takes a `radio::Band` enum (16
  variants, confirmed by reading `radio_trait.rs` lines ~538-636), not a
  raw wire code, so an invalid band is structurally unrepresentable through
  the UI, same principle as the existing Mode/CTCSS/DCS lists. `BS` has
  **no** `Read`/`Answer` form at all (manual p.3, confirmed in
  `radio_trait.rs`'s own doc comment on `set_band`) — this is a
  structurally different limitation from groups 4/5/6's "not polled into
  `Ft991aDisplay` *yet*" documented gaps, and `band_step_encoder_commands`'s
  doc comment calls that distinction out explicitly so a future reader
  doesn't conflate "no live value exists to poll" with "not polled yet."
- **The encoder-nudge (`ED`/`EU`) "confusing dead end" question, worked
  through concretely rather than deferred**: re-read `ft991a_radio.rs`'s
  `handle_command` directly and confirmed Wave 3's finding is still
  accurate word-for-word — `ED`/`EU` are "structurally and semantically
  validated... but mutate no persisted `Ft991aState` field... no
  Hz-per-step mapping is knowable from this manual page alone," and `EK` is
  a "zero-width Action trigger, no persisted-state effect." **Decision:
  included all three as real keys (`J`/`K`/`E`), not omitted**, because:
  (1) this is a limitation of this crate's own test emulator's state
  model, not evidence the real wire commands do nothing — `ft991a.rs`'s
  client implementation sends the genuine `ED`/`EU`/`EK` wire frames in all
  three cases, exactly like every other command in this file, and on real
  hardware they nudge whichever front-panel parameter the selected encoder
  currently controls / press the real ENT key; (2) this crate already has
  an accepted "write-only trigger, no persisted state to observe" category
  — `ClarifierClear` (`RC`) and `ZeroIn` (`ZI`) are both already bound to
  real keys in groups 4/5 on exactly that basis, and `EK` fits that
  category as cleanly as they do (a single, well-understood button press).
  `ED`/`EU` are the harder case and are disclosed as such, not glossed
  over: their real-world effect is genuinely **context-dependent** on the
  physical radio's current front-panel state, so this UI cannot show *what*
  changed, only that the trigger was sent — the `Text` prompt itself says
  so ("effect depends on the radio's current front-panel context"), and
  `band_step_encoder_commands`'s doc comment spells out the full reasoning
  as the place a future reader should revisit this call if they disagree,
  rather than a silently-made judgment.
- **Mic UP/DOWN (`UP`/`DN`) are not in the same "opaque effect" category as
  `ED`/`EU`** — confirmed by reading `ft991a_radio.rs`'s `handle_command`
  for `Dn`/`Up` directly: each press steps `self.state.vfo_a_hz` by a fixed
  `MIC_STEP_HZ`, saturating at `FA`'s documented range, so pressing these
  keys against the emulator produces a real, observable change on the next
  `poll_radio_state` cycle (VFO A's displayed frequency moves). No
  dead-end concern for these two; included without the caveat `ED`/`EU`
  carry.
- **`Band`/`EncoderSelector` are both `#[derive(..., PartialEq, Eq)]`**
  (confirmed by reading their definitions directly), so `ExecuteAction`'s
  existing blanket `#[derive(Debug, Clone, PartialEq)]` needed no change to
  carry `SetBand(Band)`/`EncoderDown(EncoderSelector, u8)`/
  `EncoderUp(EncoderSelector, u8)` and remain usable with `assert_eq!` in
  tests, unlike some other crates' domain enums that might need an explicit
  opt-in.
- **The `ED`/`EU` "encoder:steps" text format's encoder name is spelled out
  in full (`main`/`sub`/`multi`), not the raw `0`/`1`/`8` wire digits** —
  judgment call: `EncoderSelector::as_wire_digit`/`from_wire_digit` exist
  and could have been reused directly for a terser format, but raw digits
  with no on-screen legend would be far less legible to someone typing into
  a `TextInput` than group 3/5's existing "channel:tag"/"channel:message"
  compound-field precedent, which always uses human-meaningful text on both
  sides of the `:`. `parse_encoder_selector` converts case-insensitively;
  the wire digit is produced only at the `Ft991aExtras::encoder_down`/
  `encoder_up` call boundary in `terminal.rs`, same as every other
  domain-to-wire conversion in this crate.
- **No new `terminal.rs` tests were added for groups 2/9's `execute_action`
  arms**, matching the established precedent from Wave 4 Task 3 (groups
  3/4/6): `terminal.rs`'s existing test suite only exercises
  `execute_action` for the handful of actions with special executor-side
  logic (VFO set validation, the 3-valued `TxState` toggle, the RTS
  optimistic-set/rollback) — plain 1:1 `Radio`/`Ft991aExtras` passthrough
  arms (which is all of groups 2/9) are exercised structurally by the match
  arm existing and compiling, with the actual keybinding/validation
  behavior covered in `control.rs`'s tests instead, same division of
  coverage groups 3/4/6 already established. `MockRadio` was left
  unmodified (no new trait-method overrides needed) for the same reason.
- **Pre-existing uncommitted working-tree state observed, not caused by
  this task**: `git status` at the start of this session already showed
  uncommitted changes in `radio/src/*.rs`, `emulator/src/tui.rs`, and
  `ui/src/{layout.rs,lib.rs}` (the latter two from Wave 4 Task 4's
  `rts_label`/`rts_asserted` work, per that task's own `progress.md`,
  never committed since no `ui` task commits per the standing constraint).
  None of these were touched by this task — confirmed by `git diff --stat`
  showing only `ui/src/control.rs` and `ui/src/terminal.rs` changed by this
  session's edits — flagged here only so a future reader doesn't mistake
  this task for having touched `radio/`/`emulator/`.

## Wave 4 Task 6 — populate group 7 (Attenuator/Noise/AGC/Notch/Filter) and
## group 8 (Speech/Mic/Monitor)

- **§11.2's table claim for group 7's trait mix confirmed exactly, with one
  addition beyond what the table's prose named** — read `radio_trait.rs`'s
  batch-6 section (lines ~1444-1583) directly: `get`/`set_attenuator_on`
  (`RA`), `get`/`set_preamp_mode` (`PA`), `get`/`set_noise_blanker_on`/
  `_level` (`NB`/`NL`), `get`/`set_noise_reduction_on`/`_level` (`NR`/`RL`),
  `get`/`set_agc_mode` (`GT`), `get`/`set_auto_notch_on` (`BC`),
  `get`/`set_narrow_on` (`NA`), `get`/`set_filter_width_index` (`SH`) are
  all plain `radio::Radio` methods (10 keys). `radio_trait.rs`'s own batch-6
  doc comment states `CO`/`BP` are "deliberately not on this trait...kept
  `Ft991a`-inherent-only" — confirmed at lines ~1932-1986 (`Ft991aExtras`):
  `get`/`set_contour_on`, `get`/`set_contour_frequency_hz`, `get`/
  `set_manual_notch_on`, `get`/`set_manual_notch_frequency_hz` (6 methods,
  as §11.2's table named). **Addition**: `CO` also carries **APF**
  (`get`/`set_apf_on`, `get`/`set_apf_frequency_hz`, lines ~1953-1968),
  which §11.2's table prose mentioned only in passing ("...manual-notch
  (BC/BP?...)/APF on Ft991aExtras") without listing it as its own group-7
  item count contributor. Included APF as 2 more real keys (`X`/`Y`) since
  it is `CO`'s own third sub-field, same command family as contour, and
  omitting it while including contour/manual-notch would have been an
  arbitrary, undisclosed carve-out. This pushes group 7 to **16** keys
  total, well past the architect table's "approx 12" — same "full-coverage
  goal outweighs hitting the table's approx count" precedent Task 5 already
  established for group 2 (11 vs. "approx 10").
- **§11.2's table claim for group 8's trait mix confirmed exactly** — read
  `radio_trait.rs`'s batch-7 section (lines ~1586-1652) and the parametric
  mic EQ section (lines ~1988-2000) directly: `get`/`set_mic_gain` (`MG`),
  `get`/`set_speech_processor_level` (`PL`), `get`/`set_speech_processor_on`
  (`PR` `P1=0`), `get`/`set_monitor_on`/`_level` (`ML`) are plain
  `radio::Radio` methods (5 keys); `get`/`set_parametric_mic_eq_on` (`PR`
  `P1=1`) is `Ft991aExtras`-only, per `set_speech_processor_on`'s own doc
  comment: "an FT-991A-named parametric-EQ feature with no generic concept
  precedent, same treatment batch 6 gave `CO`/`BP`" (1 key). 6 keys total,
  matching §11.2's table exactly.
- **The task brief's central open question — how to handle `CO`/`BP`'s
  "shared P3 field whose meaning depends on a P2 selector" — resolved
  without any new `ControlState` machinery**: read `radio/src/ft991a.rs`'s
  actual method bodies (lines ~1727-1861) and confirmed Wave 3 already did
  the work of resolving the wire-level `P2` selector into 6 independent,
  cleanly-typed `get_*`/`set_*` method pairs (contour on/off, contour
  frequency, APF on/off, APF frequency, manual notch on/off, manual notch
  frequency) at the client-API boundary — `Ft991a<S>`'s own inherent
  methods (and therefore `Ft991aExtras`, which just re-exports them) never
  expose a raw `P2`/`P3` pair to a caller at all. This means the "genuinely
  two-dimensional" concern the task brief raised does not actually apply at
  the UI layer: each pair is just another independent boolean
  (`CommandKind::List` + `on_off_options`) or numeric (`CommandKind::Text`)
  control, the exact same shape `NR`/`RL`'s on/level split (landed in the
  Wave 3 `radio` work, reused as UI precedent since Task 3) already
  established in this file. No new `ControlState` variant, no 2-step
  select-then-enter flow, and no coordinate-splitting parsing (like the
  `EncoderDown`/`EncoderUp` "encoder:steps" compound-field convention) was
  needed or would have been the right shape here — that machinery exists in
  this crate for cases where a *single* wire command's *single* input truly
  carries two pieces of information the UI must jointly collect (e.g.
  `ED`/`EU`'s encoder selector + step count), which is not what `CO`/`BP`
  present to a caller once the `radio` crate's own abstraction is used as
  designed, rather than re-deriving the wire protocol's shape by hand.
- **`SH` filter width (`F`) uses `ListSelect`, per the task brief's own
  suggestion, but its 22 option labels are derived from the static
  `SH_BANDWIDTH_TABLE` constant, not from live radio state** — read
  `radio_trait.rs`'s doc comment on `get_filter_width_index` directly: "the
  actual bandwidth in Hz this index represents depends on the radio's
  current mode and narrow/wide (`NA`) state, neither of which is part of
  `SH`'s own wire bytes." Two structural facts rule out a display-driven
  label: (1) `CommandKind::List`'s `options` field is a bare `fn() ->
  Vec<String>`, not `fn(&Ft991aDisplay) -> Vec<String>`, so it has no way to
  read live mode even in principle; (2) `NA`'s (narrow/wide) state is not a
  polled `Ft991aDisplay` field anyway (this task does not extend polling,
  per the established "not polled yet" precedent from Tasks 3/5). Rather
  than falling back to a bare, uninformative "index 00..21" list (the
  `ED`/`EU` "context-dependent, no persisted state" precedent), each of the
  22 static labels shows **all three families' narrow/wide Hz values**
  straight from the compile-time `SH_BANDWIDTH_TABLE` constant (e.g. `"05
  SSB1100/- CW250/- RTTY250/-"`) — real, accurate, complete information from
  the manual's own table, just not narrowed to "what this index means for
  the radio's mode right now." `radio::SH_BANDWIDTH_TABLE`,
  `radio::filter_bandwidth_hz`, `radio::mode_family_for`, and
  `radio::ModeFamily` are all confirmed re-exported from the `radio` crate
  root (`radio/src/lib.rs` lines ~126-133) — no visibility gap to work
  around.
- **AGC mode (`G`) offers 5 `ListSelect` options, not `AgcMode`'s full
  7-valued domain** — `AgcMode`'s own doc comment (`radio_trait.rs` lines
  ~460-475) documents a genuine write/report domain mismatch: `GT`'s Set
  command (`P2`) only ever accepts 5 values (`OFF`/`FAST`/`MID`/`SLOW`/
  `AUTO`), while its Answer (`P3`) can report 7 (`AUTO` resolved into one of
  `AUTO-FAST`/`AUTO-MID`/`AUTO-SLOW`). Since this UI's `G` key only ever
  *sets* the mode (never displays a live-polled current value — this
  group's state is not polled, see above), offering all 7 reported variants
  as settable options would misrepresent what `GT`'s Set command can
  actually request. `AGC_ORDER`'s 5th entry ("Auto") maps to
  `AgcMode::AutoFast` on selection — the same arbitrary-but-documented
  sub-variant `AgcMode`'s own doc comment already names as this emulator's
  default report choice for a plain "AUTO" set, reused here rather than
  inventing a second arbitrary choice, and consistent with
  `AgcMode::set_wire_value`'s own collapsing behavior (`AutoMid`/`AutoSlow`
  wire-encode identically to `AutoFast` on Set).
- **Preamp mode (`P`) is `ListSelect` over the 3-valued `PreampMode`, not
  `TextInput`** — `set_preamp_mode` takes `PreampMode` (`Ipo`/`Amp1`/
  `Amp2`), a small closed enum, same "structurally unrepresentable invalid
  value" principle as Mode/Band/CTCSS/DCS/ScanState's existing `ListSelect`
  precedent in this file — not a judgment call, a type-driven one.
- **All 22 new `ExecuteAction`/`terminal.rs::execute_action` arms are plain
  1:1 `Radio`/`Ft991aExtras` passthroughs, matching Tasks 3/5's established
  division of test coverage** — no executor-side branching logic (unlike
  group 5's RTS optimistic-set/rollback), so no new `terminal.rs`-level
  tests were added; keybinding/validation behavior is covered by
  `control.rs`'s own 69 new tests instead. `MockRadio` needed no new
  overrides for the same reason (all default `NotImplemented` bodies
  already satisfy the widened `Radio + Ft991aExtras + CwKeying` bound).
- **Two pre-existing tests' premises broke and were fixed, not silently
  left green-by-accident** (same category of fix Task 5 made for groups
  2/9): `test_stub_groups_have_no_commands_yet`'s `populated` list (and its
  stale header comment, which still said "groups 1, 3, 4, 5, 6 are
  populated") needed both new groups added — otherwise the test would have
  started *failing* (correctly) once group 7/8 stopped returning empty
  command lists, so this was caught by running the suite, not missed.
  `test_stub_group_char_key_is_a_no_op` previously used key `'z'` against
  `CommandGroup::AttenuatorNoiseAgcNotchFilter` (Task 5's own retarget,
  chosen because it was "still genuinely stubbed" at the time) — `'Z'`
  became this task's real manual-notch-frequency key, so leaving the test
  as-is would have made it start asserting the *opposite* of the truth
  once group 7 had real content (it would have started failing once `'z'`
  actually transitioned state, since the test asserts `KeyResult::Continue`
  and no state change). Retargeted proactively, before running the suite,
  to `CommandGroup::MetersStatus` (still genuinely stubbed, confirmed via
  `group_command_labels`), and gave groups 7/8 their own dedicated
  `'Q'`-key no-op regression tests (`Q` is not one of either group's real
  keys — group 7's are `A P B L N R G U W F C H X Y M Z`, group 8's are
  `G L S M V E`).
- **One new clippy finding, distinct in kind from Tasks 3/4's
  `enum_variant_names`/`field_reassign_with_default` findings**: a
  `doc_lazy_continuation` lint on a doc-comment paragraph where a line
  began with `+ [...]` (intended as ordinary prose, "boolean... + numeric
  ([...]) control") — clippy's markdown-aware doc lint parsed the leading
  `+` as an unindented continuation of a list item. Fixed by rewording to
  avoid a line-initial `+`, not by suppressing the lint.
- **Pre-existing uncommitted working-tree state observed, not caused by
  this task** (same category of note Task 5 made): `git status` at the
  start of this session already showed uncommitted changes in
  `radio/src/*.rs`, `emulator/src/tui.rs`, and `ui/src/{layout.rs,lib.rs}`.
  None of these were touched by this task — confirmed by `git diff --stat`
  showing only `ui/src/control.rs` and `ui/src/terminal.rs` changed by this
  session's edits.

## Wave 4 Task 7 — group 10 (Meters/Status) + group 11 (System/Tuner/DVS)

Full design/decision record is in `planning/ui/task_plan.md`'s "Session:
Wave 4 Task 7" section — summarized here for cross-reference:

- **Trait-mix citation, group 10**: `select_meter`/`get_selected_meter`
  (`MS`)/`get_meter` (`RM` direct-select) are plain `radio::Radio` methods
  (`radio_trait.rs` lines ~1036-1053). `get_active_meter_reading` (`RM`
  `P1=0`)/`get_radio_indicator` (`RI`)/`get_menu_mode_active`
  (`RS`)/`get_pll_unlocked` (`UL`)/`get_information` (`IF`) are all
  `radio::Ft991aExtras`-only (`radio_trait.rs` lines ~1841-1879).
- **Discrepancy found in §11.2's table, not silently fixed**: the table's
  prose maps "PLL-unlock (`RS`→`get_pll_unlocked`)" — but reading both
  `radio_trait.rs`'s doc comments and `ft991a.rs`'s implementation directly
  shows `RS` actually backs `get_menu_mode_active` and `UL` backs
  `get_pll_unlocked`. `control.rs`'s `meters_status_commands` doc comment
  states this explicitly so a future reader following only the table isn't
  misled; the code itself follows the real source.
- **Trait-mix citation, group 11**: `get`/`set_auto_info_on` (`AI`),
  `get`/`set_frequency_lock` (`LK`), `get`/`set_repeater_shift` (`OS`),
  `get`/`set_tx_vfo` (`FT`), `get`/`set_mox_on` (`MX`) are plain
  `radio::Radio` methods (`radio_trait.rs` lines ~1730-1801).
  `get`/`set_antenna_tuner_state` (`AC`), `get`/`set_dimmer` (`DA`),
  `read`/`write_date`, `read`/`write_time`, `read`/`write_time_zone_offset`
  (all `DT`), `get_opposite_band_information` (`OI`, read-only),
  `get`/`set_txw_on` (`TS`), and the 6 DVS record/playback methods (`LM`/
  `PB`) are all `radio::Ft991aExtras`-only (`radio_trait.rs` lines
  ~2021-2125) — exactly matching §11.2's table for this group, no
  discrepancy found here.
- **Validation ranges cited directly from `radio/src/ft991a.rs`, not
  guessed**: dimmer LED `1`-`2`/TFT `0`-`15` (lines ~2221-2231,
  `RadioError::InvalidDimmerLevel`); date month `1`-`12`/day `1`-`31`, no
  calendar validation (lines ~2248-2259, `RadioError::InvalidDate`); time
  hour `0`-`23`/minute+second `0`-`59` (lines ~2275-2289,
  `RadioError::InvalidTime`); time zone offset `-720..=840` minutes in
  30-minute steps (lines ~2308-2320, `RadioError::InvalidTimeZoneOffset`);
  DVS channel `1`-`5` (lines ~2453-2461/~2483-2491,
  `RadioError::InvalidDvsChannel`).
- **Date/time (`DT`) multi-field design**: confirmed `radio/src/ft991a.rs`
  already splits the wire-level 3-shape command (`P1` selects date/time/
  offset) into 3 fully independent method pairs at the client boundary —
  so the task brief's flagged design question resolved to "3 separate
  `TextInput` keybindings with format-specific parsers"
  (`parse_date`="YYYYMMDD", `parse_time`="HHMMSS",
  `parse_time_zone_offset`="+HHMM"/"-HHMM"), no new `ControlState` variant,
  consistent with this crate's existing single-delimited-buffer convention
  for every other multi-field command (memory tag, keyer memory, encoder
  nudge, dimmer). The STOP-and-report alternative did not apply — the
  underlying `radio` methods were already independently callable.
- **Group 10 fields confirmed genuinely absent from passive display**: read
  `ui/src/terminal.rs`'s `poll_radio_state` directly — it polls exactly the
  original Wave 2 10-field set (VFO A/B, mode, TX state, S-meter, power on,
  AF/RF gain, squelch, TX power) and nothing from `IF`/`RM`/`RI`/`RS`/`MS`/
  `UL`. So, per the task brief's conditional instruction, every one of
  group 10's 8 fields needed its own real key — no key was invented for
  something already passively visible, and none was omitted for something
  that should have gotten a key.
- **Two pre-existing tests' premises broke and were fixed** (same category
  Tasks 5/6 hit): `test_stub_groups_have_no_commands_yet`'s `populated`
  list needed both new groups. `test_stub_group_char_key_is_a_no_op`
  (Task 6's retarget to `CommandGroup::MetersStatus` with key `'z'`) had
  its doc comment's premise ("still genuinely stubbed") invalidated once
  this task populated that group — even though `'Z'` doesn't collide with
  any of group 10's 8 real keys (so the test wouldn't have started
  *failing*), the doc comment would have become misleading. Retargeted to
  `CommandGroup::ExMenu` (the one group genuinely still stubbed after this
  task), and gave groups 10/11 their own dedicated `'Q'`-key no-op tests.
- **Pre-existing uncommitted working-tree state observed, not caused by
  this task** (same note as prior sessions): `git status` at the start of
  this session already showed uncommitted changes in `radio/src/*.rs`,
  `emulator/src/tui.rs`, and `ui/src/{layout.rs,lib.rs}`. None of these
  were touched by this task — confirmed by `git diff --stat` showing only
  `ui/src/control.rs` and `ui/src/terminal.rs` changed by this session's
  edits.

## Wave 4 Task 8 — `EX` menu number-entry escape hatch (path (b))

Full design/decision record is in `planning/ui/task_plan.md`'s "Session:
Wave 4 Task 8" section — summarized here for cross-reference:

- **Prerequisite confirmed already landed**: `radio::Ft991aExtras::
  get_ex_menu_item`/`set_ex_menu_item` exist exactly as §11.4 specified
  (`radio/src/radio_trait.rs` lines ~2135-2144); `ExMenuValueKind::
  Enumerated` already carries `(wire, label)` pairs, not bare wire strings.
- **Real discrepancy vs. §11.4, flagged not silently resolved**: the design
  text's own `'[X]'` top-level key notation collides with `CommandGroup::
  ScanVoxBusy`'s already-assigned `group_key` ('X', a Wave 4 Task 2 choice
  the architect's plan text predates). Used `'N'` instead, verified unique
  against all 12 group keys + `Q`.
- **`ExValueEntry` is not a third `ControlState` variant**: §11.4's own
  pseudocode says the fork "reuse[s] `ControlState::ListSelect`/
  `TextInput` verbatim" — implemented as a small `enter_ex_value_entry`
  helper that transitions directly into one of those two existing states
  (carrying a new `SetExMenuItem` action), not a new third variant.
  `ControlState::ExNumberEntry` itself *is* a genuinely new variant, per
  the task brief's explicit shape requirement.
- **Read-first-then-edit skipped, not silently dropped**: `handle_key` is
  synchronous with only `&Ft991aDisplay` access (confirmed via
  `terminal.rs`'s `run_loop` — `execute_action`, the only radio-touching
  call, runs strictly after `handle_key` returns), and `Ft991aDisplay` has
  no per-`p1` `EX` value cache. Forcing it would mean polling all 151 `EX`
  items every cycle or making `handle_key` async — both disproportionate
  to a documented "should-have, not a hard requirement." Documented in
  `enter_ex_value_entry`'s own doc comment and in the task's final report.
- **`ui/src/layout.rs` needed a real change** (unlike Task 7's "confirmed
  no changes needed"): `draw_control_panel`'s `match state` blocks are
  exhaustive over `ControlState`, so the new `ExNumberEntry` variant forced
  a render arm — added, reusing the `TextInput` rendering shell per §11.4's
  own wording. Not in the task brief's "read first" file list, flagged as
  an unavoidable mechanical knock-on, not scope creep.
- **`ExMenuValueKind::parse`/`format` are `pub(crate)` to `radio`**, not
  reachable from `ui` — `select_action_to_execute`'s `SetExMenuItem` arm
  parses `Enumerated` wire strings with a plain `str::parse::<i32>()`
  instead, confirmed safe since every landed `Enumerated` row uses
  unsigned plain-decimal wire values (signed encoding is a `Range`-only
  concept).
- Path (a) (themed browsing sub-groups) untouched, per the task's explicit
  constraint — Task 9's job, will consume `enter_ex_value_entry`/
  `SetExMenuItem` unchanged once built.
- Verification: `cargo test -p ui` **405 passed, 0 failed** (374 baseline +
  31 new, zero regressions). `cargo clippy -p ui --all-targets -- -D
  warnings` clean. `cargo fmt -p ui -- --check` clean after one `cargo fmt`
  pass. `cargo build --workspace` clean.

## Wave 4 Task 9 — `EX` menu themed browsing (path (a)) — Wave 4's final
task

Full step-by-step is in `planning/ui/progress.md`'s "Session: Wave 4 Task 9"
section — summarized here for cross-reference:

- **Off-by-one in §11.4's own prose, flagged not silently resolved**: text
  says "five sub-groups" but lists six labeled `p1` ranges. Implemented
  six `ExTheme` variants (`GeneralAgcCw`/`TxAudioChain`/`Mixed`/
  `RttySsbTxChain`/`MeterScope`/`BandLimitVox`), counting the ranges given
  rather than trusting the stated number.
- **Boundary verification (§11.4's own explicit ask for the 080-091
  range)**: checked against `radio::EX_MENU_TABLE`'s real item names at
  both flagged edges — `079` "FM PKT MODE" -> `080` "RPT SHIFT 28MHz" and
  `091` "STANDBY BEEP" -> `092` "RTTY LCUT FREQ". Both are clean thematic
  breaks in the actual table; no boundary adjustment was needed, but the
  check was real (grep'd the source, not assumed from the manual's rough
  page layout alone).
- **Actual per-theme counts, computed not assumed**: 45/33/11/19/26/17
  across the six ranges (sums to the 151 landed rows). `GeneralAgcCw` is
  45 not 46 (missing `027` "TIME ZONE"); `Mixed` is 11 not 12 (missing
  `087` "RADIO ID") — both are the two permanent, already-documented gaps
  in `radio`'s own `EX_MENU_TABLE`, not new gaps found by this task.
- **Runtime bucketing, not a hand-maintained list**: `ex_theme_items(theme)`
  filters `EX_MENU_TABLE` by range at call time, per §11.4's explicit
  instruction — same principle as `ctcss_tone_options`/`dcs_code_options`
  (Task 3). Confirmed via `test_ex_theme_ranges_partition_ex_menu_table_
  with_no_overlap`, which sums every theme's bucket size and compares
  against `EX_MENU_TABLE.len()` directly (151), not a hardcoded expected
  total — the test would catch drift if a future sub-batch changes the
  table without anyone updating a parallel list, because there is no
  parallel list to fall out of sync.
- **Top-level `EX` group UI shape, judgment call**: `ExMenu`'s own
  `GroupMenu` screen lists the 6 themes plus the number-entry escape hatch
  folded in as a 7th entry (same `EX_NUMBER_ENTRY_KEY`/`'N'` as Task 8's
  existing dedicated `Menu`-level binding, left unchanged and still
  reachable both ways) — chose "both, not either/or" for discoverability:
  users who already know a `p1` keep the fast top-level shortcut; users
  browsing discover the escape hatch without needing to already know it
  exists.
- **`GroupMenu`'s `cursor` field stays vestigial, confirmed, not
  repurposed**: re-verified (per this task's own read-list item 4) that
  neither this codebase's `GroupMenu` nor `ts570d`'s reference precedent
  ever wires that field to scrolling. Since `ExMenu`'s own theme-picker
  list is only 7 entries (fits fine, single-char-keyed, no scrolling
  need), a **new** state — `ControlState::ExSubGroupMenu { theme, cursor }`
  — was added specifically for the one list that does need scrolling (up
  to 45 items), rather than trying to retrofit `GroupMenu`'s cursor. This
  is the first genuinely cursor-driven, Up/Down-scrolled list in this
  crate; `ui/src/layout.rs` gained a matching `draw_ex_sub_group_menu` with
  a sliding window centered on `cursor` — addresses the task brief's "don't
  ship an unusable unscrollable 45-item list" concern directly, not
  deferred or hand-waved.
- **Path convergence implemented via `enter_ex_value_entry`'s signature,
  not a duplicate function**: added an `origin: ExValueEntryOrigin`
  parameter (`NumberEntry` vs. `Theme(theme, cursor)`) so the *same* helper
  Task 8 built serves both paths — satisfies the task brief's "reuse it,
  don't duplicate" instruction literally, not just in spirit. The one
  real behavioral difference between paths — where `Esc` from the
  value-entry fork returns to, per §11.4's own pseudocode giving each path
  a different target — required a new `SetExMenuItemFromTheme(u16,
  ExTheme, usize)` variant on `InputAction`/`SelectAction` (carrying enough
  to route back to the right sub-group/cursor); every other match arm
  (`validate_text_input`, `select_action_to_execute`, `initial_list_cursor`)
  shares its existing arm with the plain `SetExMenuItem(u16)` variant via
  `|`-combined patterns, so no validation/execution logic was duplicated
  either.
- **Convergence test design choice**: rather than asserting full
  `ControlState` equality between path (a)'s and path (b)'s output states
  (which would be false by construction, since path (a)'s `action` carries
  extra routing data path (b)'s doesn't), the two convergence tests assert
  the parts that actually matter for "the two paths converge" — identical
  rendered content (`ListSelect` options/`TextInput` prompt) and identical
  final `ExecuteAction` after confirming — which is the behavioral
  guarantee §11.4 actually cares about, not incidental enum-shape equality.
- **Pre-existing uncommitted working-tree state observed, not caused by
  this task** (same note as prior sessions): `git status --short` at the
  start of this session already showed uncommitted changes in
  `radio/src/*.rs`, `emulator/src/tui.rs`, and `ui/src/{lib.rs,
  terminal.rs}`. None of these were touched by this task — confirmed via
  `git diff --stat` showing only `ui/src/control.rs` and `ui/src/layout.rs`
  changed by this session's edits.
- Verification: `cargo test -p ui` **425 passed, 0 failed** (405 baseline +
  20 new, zero regressions). `cargo clippy -p ui --all-targets -- -D
  warnings` clean. `cargo fmt -p ui -- --check` clean after one `cargo fmt`
  pass.
- **This completes Wave 4's entire dispatch queue** — all 12 `CommandGroup`
  variants are populated and both `EX` access paths (number-entry escape
  hatch, themed browsing) are landed and verified to converge.
