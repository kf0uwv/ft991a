# Yaesu Agent Progress

## Wave 1 Task 1 — `radio` crate first slice (2026-07-17)

Status: **implementation complete, verification clean, ready for architect
review.**

Delivered:
- `Cargo.toml` (repo root) — minimal placeholder `[workspace]` (members =
  `["radio"]` only), flagged as superseded by the `app` agent's Wave 1
  Task 2.
- `radio/Cargo.toml`
- `radio/src/lib.rs`, `radio/src/ft991a_radio.rs` (command table + emulator
  state machine), `radio/src/ft991a.rs` (`SharedSession<S>` + `Ft991a<S>`
  controller client), `radio/src/radio_trait.rs` (`Radio` trait +
  `Frequency`/`Mode`/`TxState`/`RadioError`/`RadioResult`)

Verification (all from repo root, against the placeholder workspace):
- `cargo build -p radio --all-targets` — clean (git dependency on
  `radio-cat-rs` resolved from the existing local `~/.cargo/git` cache, no
  network needed at build time).
- `cargo test -p radio` — **51 unit tests + 1 doctest, all passing.**
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, zero
  warnings.
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (macro-arm and single-statement-block reformatting only, no logic
  changes).

Deviation from `planning/architect/task_plan.md` §2, found during
mandatory manual re-verification and flagged before implementing: `AG`,
`RG`, `SQ` are plain zero-width-query commands (`AG;`/`RG;`/`SQ;`), not
"selector reads" like `MD`/`SM` — see the final report to the architect and
`planning/yaesu/task_plan.md`'s "DISCREPANCY FOUND AND FLAGGED" section for
the full manual citation. Implemented per the manual (corrected), not per
the summary's guess, per the explicit "trust the primary source" 
instruction — needs architect sign-off.

Deferred (documented, not silently dropped):
- `Ft991a::wake_and_power_on()` helper (task_plan §4's "first-slice
  nice-to-have, not a hard blocker") — no `monoio` timer API precedent
  exists anywhere in this codebase or `ts570d` to build against safely
  within this task's scope; the wake-sequence quirk is documented as a doc
  comment on `Ft991a::set_power_on` instead. Follow-on wave.
- `"?;"` protocol-error-response format — explicit, documented assumption,
  not manual-cited. See doc comment on
  `Ft991aRadio::write_protocol_error`.

Not touched (per task constraints): `ui/`, `emulator/`, `src/main.rs`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 9: Meters/status (`IF RM RI RS MS UL`) (2026-07-18)

Status: **implementation complete, verification clean, ready for architect
review.** Dispatched early per `planning/architect/task_plan.md` §10.8
item 2 (batches 2 and 10 depend on this task's `IF` field-parsing
finding).

Delivered, `radio/` crate only:
- `radio/src/ft991a_radio.rs`: 6 new `Ft991aCommandId` variants + command
  table entries, 15 new `Ft991aState` fields, `ChannelStatusFields`
  (composite `IF`/future-`MR`/`MT`/`OI` payload parser, doc-commented
  column table with citation), `Ft991aState::meter_reading`/
  `selected_meter_reading`/`ri_status` helpers, `Ft991aRadio::from_state`
  constructor, 6 new `handle_command` match arms.
- `radio/src/ft991a.rs`: `get_information`, `select_meter`/
  `get_selected_meter`/`get_meter`/`get_active_meter_reading`,
  `get_radio_indicator`, `get_menu_mode_active`, `get_pll_unlocked`
  client methods, `parse_rm_answer` helper, `Radio` trait impl additions.
- `radio/src/radio_trait.rs`: `Meter` enum + `Radio::select_meter`/
  `get_selected_meter`/`get_meter` trait methods (added to the trait —
  `CLAUDE.md`'s "Radio trait scope" explicitly lists "meters"), plus
  `RadioIndicator` (kept as a plain domain type, not added to the trait —
  judgment call, see findings.md).
- `radio/src/lib.rs`: re-exports for `ChannelStatusFields`, `Meter`,
  `RadioIndicator`; module-doc scope note updated (no longer says "Wave 1,
  first slice" only).

Verification (all from repo root):
- `cargo test -p radio` — **90 unit tests + 1 doctest, all passing** (was
  51+1; the 51 original Wave 1-2 tests all still pass unmodified except
  the table-integrity count, updated 11→17, and the master-flags test,
  extended not replaced — confirmed zero regressions).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, after
  fixing 5 `field_reassign_with_default` findings in new tests (struct
  literals with `..Default::default()` instead of post-construction field
  assignment).
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio`
  pass.
- Confirmed via `find -newer` that only `radio/src/{ft991a_radio.rs,
  ft991a.rs, radio_trait.rs, lib.rs}` (plus the auto-regenerated
  `Cargo.lock`, no dependency changes) were touched — `ui/`, `emulator/`,
  `src/main.rs`, root `Cargo.toml` untouched, matching task constraints.

No ambiguity blocked this task — see `findings.md` for the full
column-by-column `IF` re-verification, the `RM`/`MS` relationship found,
two citation corrections against the architect's summary, and the
judgment calls made (IF's VFO-mode channel sentinel, RadioIndicator kept
off the `Radio` trait). None required stopping; all flagged for
architect review rather than silently decided.

Not touched (per task constraints): `ui/`, `emulator/`, `src/main.rs`,
root `Cargo.toml`, `ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — `EX` menu, first sub-batch: plumbing + 9 PTT/keying items (2026-07-18)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.6's "Priority
carve-out" paragraph and §10.8 dispatch item 3.

Delivered, `radio/src/ft991a_radio.rs` only (no other file touched — see
findings.md's "Deliberate scope narrowing" note on why `ft991a.rs`/
`radio_trait.rs` were not touched this task, unlike batch 9):
- `Ft991aCommandId::Ex` variant.
- `EX_SET_FORMS` (7 `CommandForm` entries: width-3 selector-read + all 6
  write widths {4,5,6,7,8,11} found across the full 153-row table, not
  just this sub-batch's items).
- `ExMenuItem` struct, `EX_MENU_TABLE: &[ExMenuItem]` (exactly 9 rows:
  047, 048, 060, 071, 072, 076, 077, 108, 109), `ex_menu_item(p1)` lookup.
- 9 new `Ft991aState` fields + `ex_menu_value`/`set_ex_menu_value` private
  helpers.
- `Ft991aRadio::handle_command`'s new `Ex` arm (P1-lookup dispatch,
  read-vs-write by parameter length, per-item digit-width + legal-value
  validation).
- 10 new tests: table-shape (`ex_menu_table_has_exactly_nine_entries`),
  lookup (`ex_menu_item_finds_all_nine_landed_items`/
  `..._returns_none_for_any_unlanded_p1`), default-read round trip for all
  9 items, a dedicated PC-KEYING (menu 060) read/write/reject test (the
  item this wave's RTS/DTR feature actually needs), a parameterized
  valid/invalid-value round trip across all 9 items (including the
  072/077 one-based-legend items), a wrong-digit-width rejection test, an
  out-of-table-P1 test (`EX001;`/`EX0010020;`/`EX999;` all resolve to
  `"?;"` cleanly, per the task's explicit "not a panic" requirement), and
  a cross-item state-independence test.

Verification (all from repo root):
- `cargo test -p radio` — **100 unit tests + 1 doctest, all passing** (was
  90+1; 10 new tests, zero regressions — the only change to a pre-existing
  test was the table-integrity command count, 17→18).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed.
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping only).
- Confirmed via `find -newer` that only `radio/src/ft991a_radio.rs` was
  touched.

Judgment calls / flagged items (see findings.md for full detail, none
blocked the task):
- One image-render OCR digit-count misread (item 072) caught and resolved
  via `pdftotext -layout` cross-check + sibling-row-pattern agreement.
- A genuine manual inconsistency (items 048/109 vs. 072/077 use different
  zero-based/one-based encodings for the identical DATA/USB concept)
  transcribed exactly, not normalized.
- Arbitrary (manual-silent) default values chosen for all 9 state fields,
  documented per-field, same category as `meter_select`'s precedent.
- Scope narrowed to `ft991a_radio.rs` only, per this task's own explicit
  instructions (no `ft991a.rs` client methods, no `radio_trait.rs`
  growth) — flagged as a deliberate deviation from batch 9's broader
  precedent, not an oversight.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`, and — within `radio/` — `ft991a.rs`,
`radio_trait.rs`, `lib.rs` (all unchanged this task). No commits made.

## Wave 3 — CAT batch 2: Memory channel records (`MC MR MW MT`) (2026-07-18)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 2 row,
dispatched after batch 9 (`IF` et al.) so `MR`/`MT` could reuse `IF`'s
already-verified composite payload shape.

Delivered:
- `radio/src/ft991a_radio.rs`: 4 new `Ft991aCommandId` variants + table
  entries, `MemoryChannelRecord` (per-channel emulator storage, reusing
  `ChannelStatusFields` for the shared P1-P10 wire shape rather than
  re-deriving it), `Ft991aState::{selected_memory_channel,
  memory_channels}` + `memory_channel`/`memory_channel_mut` helpers, 4 new
  `handle_command` match arms (`Mc`/`Mr`/`Mw`/`Mt`), `is_valid_tag_wire`
  helper.
- `radio/src/ft991a.rs`: `get_memory_channel`/`set_memory_channel`,
  `read_memory_channel`/`write_memory_channel`,
  `read_memory_channel_tag`/`write_memory_channel_tag` client methods +
  `memory_entry_from_fields`/`channel_status_fields_for_write` conversion
  helpers + `Radio` trait impl additions.
- `radio/src/radio_trait.rs`: `MemoryChannelEntry`, `MemoryTag`
  (validated up-to-12-ASCII-char tag type), `TaggedMemoryChannel`, 6 new
  `Radio` trait methods, 2 new `RadioError` variants
  (`InvalidMemoryChannel`/`InvalidMemoryTag`).
- `radio/src/lib.rs`: re-exports + module-doc scope update.

`ChannelStatusFields` (built in the batch-9 task for `IF`) was reused
**unmodified** — `MR`'s answer and `MW`'s Set round-trip through it
directly (confirmed column-by-column against the manual: zero shape
differences from `IF`, only two narrow semantic ones — channel range,
P7/select handling — both resolved as extra checks in `handle_command`,
not changes to the shared struct). `MT` extends it with 2 genuinely new
fields (a reserved byte + the 12-char tag) not covered by
`ChannelStatusFields` at all.

Verification (all from repo root):
- `cargo test -p radio` — **130 unit tests + 1 doctest, all passing** (was
  100+1; 30 new tests — command-table integrity, `MC` query/set round
  trip + range rejection, `MR` default-state read + range rejection +
  read-only confirmation, `MW`→`MR` write-then-read round trip +
  channel-zero/bad-P7 rejection, `MT` write-then-read round trip
  including tag, `MT` tag shortest/longest legal content, `MT` control-
  character tag rejection, `MT` channel range rejection, `MT` non-fixed-
  P7 rejection, plus 9 `MemoryTag` unit tests and 12
  wire-byte-level client tests in `ft991a.rs` — zero regressions, every
  pre-existing test name still passes unmodified except the
  table-integrity count, 18→22, and the master-flags test, extended not
  replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed (one panic-risk fix applied proactively during implementation —
  see findings.md — before this check, not found by clippy itself).
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping only).
- Confirmed via `find -newer` that only `radio/src/{ft991a_radio.rs,
  ft991a.rs, radio_trait.rs, lib.rs}` were touched; `Cargo.lock` unchanged
  (no dependency changes). `ui/`, `emulator/`, `src/main.rs`, root
  `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `MW`'s P7 legend text (`"00: (Fixed)"`) disagrees with its own column
  diagram (1 wire column, not 2) — resolved in favor of the diagram,
  flagged not silently picked.
- Tag character-set restriction and wire-padding convention are both
  explicit judgment calls, grounded in the manual's general p.2 parameter
  rule rather than anything stated on `MT`'s own page.
- A defensive `.get()`-based rewrite of the `Mt` write arm's byte slicing
  (vs. direct indexing) to avoid a theoretical panic on off-boundary
  multi-byte UTF-8 content — applied to this task's own new code; the
  pre-existing `Ex` arm has the same theoretical exposure and was left
  unchanged (out of scope), flagged for awareness.

Not touched (per task constraints): `ui/`, `emulator/`, `src/main.rs`,
root `Cargo.toml`, `ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 1: VFO/split/memory quick-ops (`AB BA AM VM MA CH QI
QR QS SV`) (2026-07-18)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 1 row.

Delivered:
- `radio/src/ft991a_radio.rs`: 10 new `Ft991aCommandId` variants + table
  entries, a new `ACTION` const (`cat-framework`'s zero-width-trigger
  `CommandOperation::Action`, first use in this crate) plus two new
  `definition!` macro arities to thread `action_forms` through, 2 new
  `Ft991aState` fields (`qmb: MemoryChannelRecord`, `split: bool`), 10 new
  `handle_command` match arms.
- `radio/src/ft991a.rs`: 10 new client methods (`copy_vfo_a_to_b`,
  `copy_vfo_b_to_a`, `store_vfo_to_memory`, `toggle_vfo_memory_mode`,
  `recall_memory_to_vfo`, `memory_channel_up`, `memory_channel_down`,
  `qmb_store`, `qmb_recall`, `quick_split`, `swap_vfos`).
- `radio/src/radio_trait.rs`: 7 new `Radio` trait methods
  (`copy_vfo_a_to_b`/`copy_vfo_b_to_a`/`swap_vfos`/`store_vfo_to_memory`/
  `recall_memory_to_vfo`/`memory_channel_up`/`memory_channel_down`) +
  `NopRadio` test coverage for all 7.
- `radio/src/lib.rs`: module-doc scope update (no new public type exports
  needed — no new domain types introduced this task).

**`VM`/`AM` manual heading inconsistency (the architect's flagged item)**:
confirmed exactly, and resolved via three corroborating signals (unique
bracket notation, redundancy heuristic, real-world V/M key behavior) since
the wire-format boxes themselves are byte-identical between the two
commands and cannot disambiguate — implemented `VM` as toggling
`Ft991aState::channel_select` (reusing `IF`'s existing P7 field) between
VFO/Memory, distinct from `AM`'s unambiguous "store" behavior. Documented
as a judgment call requiring architect/hardware review, not silently
assumed correct — full reasoning in `findings.md` above and in
`ft991a_radio.rs`'s module doc comment.

Verification (all from repo root):
- `cargo test -p radio` — **157 unit tests + 1 doctest, all passing** (was
  130+1; 27 new tests — command-table integrity extension, `AB`/`BA`/`SV`
  copy/swap round trips + parameter-rejection, `AM`/`MA` store/recall
  round trips including a combined `AM`→change VFO→`MA` round-trip test
  and an `MC`-selected-channel interaction test, `VM` toggle test (both
  direct state assertion and cross-checked through `IF`'s P7 field), `CH`
  up/down step + wrap-at-boundary + illegal-selector rejection, `QI`/`QR`
  round trip confirmed independent of the numbered `MC`/memory-channel
  state, `QS` toggle test using `CatFramework::radio()` to assert state
  directly, an Action-trigger-rejects-any-parameter test, plus 12
  wire-byte-level client tests and 1 `Radio`-trait-delegation test in
  `ft991a.rs`, and 7 new `NopRadio` `RadioResult::NotImplemented`
  assertions in `radio_trait.rs` — zero regressions, every pre-existing
  test name still passes unmodified except the table-integrity command
  count, 22→32, and the master-flags test, extended with a loop over the
  ten new codes rather than replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed.
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping only — no logic changes).
- Confirmed only `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs,
  lib.rs}` were touched (directory recent-file scan); `Cargo.lock`
  unchanged (no dependency changes); `ui/`, `emulator/`, `src/main.rs`,
  root `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `VM`'s meaning (see above) — the highest-risk judgment call of this
  task, since it rests on corroborating evidence rather than a
  crisply-specified wire format.
- `CH`'s wrap-around behavior at the 1/117 boundary (manual silent).
- `QI`/`QR`'s dedicated QMB slot, `QS`'s plain-toggle semantics (manual
  gives no on/off split command anywhere).
- `Radio` trait scope: 7 of the 10 commands' operations added as trait
  methods (dual-VFO/memory-channel concepts); `VM`/`QI`/`QR`/`QS` kept
  `Ft991a`-inherent-only, reasoning documented per-command.
- `AB`/`BA`/`SV` only copy/swap frequency, not mode — an inherited Wave-1
  state-model constraint (`Ft991aState` has one `mode` field, not
  per-VFO), not a new decision this task made.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 4: Keyer/CW/break-in (`KM KP KR KS KY CS ZI BI SD`) (2026-07-19)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 4 row.

Delivered:
- `radio/src/ft991a_radio.rs`: 9 new `Ft991aCommandId` variants + table
  entries, `SET_2`/`KM_SET_FORMS` (the latter using `cat-framework`'s
  `CommandForm::variable` — first use in this crate), `KeyerPlaybackMode`
  enum + `ky_selector_from_wire`/`ky_selector_to_wire` helpers, 7 new
  `Ft991aState` fields (`keyer_memories`, `key_pitch`, `keyer_on`,
  `key_speed`, `cw_spot_on`, `break_in_on`, `cw_break_in_delay_ms`) +
  `keyer_memory`/`set_keyer_memory` helpers, 9 new `handle_command` match
  arms, `is_valid_tag_wire` renamed to `is_valid_ascii_wire_content` and
  reused for `KM` (call site in `MT`'s arm updated, no behavior change).
- `radio/src/ft991a.rs`: `read_keyer_memory`/`write_keyer_memory`/
  `play_keyer_memory` (`Ft991a`-inherent only, per the task's explicit `KM`/
  `KY` scope instruction), `get/set_keyer_pitch_hz`, `get/set_keyer_enabled`,
  `get/set_keyer_speed`, `get/set_cw_spot_on`, `zero_in`,
  `get/set_break_in_on`, `get/set_semi_break_in_delay` client methods +
  `Radio` trait impl additions for the seven generic ones.
- `radio/src/radio_trait.rs`: 5 new `RadioError` variants
  (`InvalidKeyerMemoryChannel`, `InvalidKeyerMessage`, `InvalidKeyerSpeed`,
  `InvalidKeyerPitch`, `InvalidBreakInDelay`), 13 new `Radio` trait methods
  (break-in, semi break-in delay, CW spot, keyer speed/pitch/on-off,
  zero-in — directly mirroring `ts570d::Radio`'s own
  `get_keyer_speed`/`set_keyer_speed`/`get_semi_break_in_delay`/
  `set_semi_break_in_delay` precedent) + `NopRadio` test coverage for all 13.
- `radio/src/lib.rs`: re-exports (`KeyerPlaybackMode`,
  `ky_selector_from_wire`, `ky_selector_to_wire`) + module-doc scope update.

**`KY`/RTS-DTR distinction, the task's explicit flag, confirmed and kept
separate**: `KY` triggers playback of a pre-stored `KM` message (two
playback-mode families, confirmed via cross-reference to `EX` menu items
018-022, not two separate stores); the RTS/DTR feature (§10.2-10.4, already
landed in `radio-cat-rs`, not yet consumed here) is real-time PC-driven
Morse keying via serial control lines with no CAT command at all. Also
explicitly checked and avoided a same-code-different-semantics trap:
`ts570d::Radio::send_cw(message)` backs a *different*, arbitrary-free-text
`KY` command on the Kenwood side — the FT-991A's `KY` has no free-text
parameter, so no `send_cw`-shaped method was added here.

**`KM`'s variable-length message, the batch's highest-risk item**: used
`cat-framework`'s `CommandForm::variable(Set, 2, 51)` (present in the
framework, previously unused by this crate) for the write form, rather than
enumerating discrete widths like `EX`. Documented consequence (not
manual-stated): the read form's 1-character width forces the write form's
minimum to ≥2, so a 0-character "clear" message cannot be written via `KM`
in this implementation.

Verification (all from repo root):
- `cargo test -p radio` — **278 unit tests + 1 doctest, all passing** (was
  225+1; 53 new tests — command-table integrity extension, `KM` read/write
  round trips (short message, near-max-length 49-char message, exactly
  50-char message, out-of-range channel, control-character rejection,
  cross-channel independence), `KP`/`KR`/`KS`/`CS`/`BI`/`SD` query/set round
  trips + range rejections, `KY` all-ten-legal-selectors +
  channel/mode-reporting + illegal-selector-rejection + no-query-form tests,
  `ZI` trigger + parameter-rejection test, 2 `ky_selector_from_wire`/
  `ky_selector_to_wire` direct unit tests, plus 27 new wire-byte-level
  client tests and 1 `Radio`-trait-delegation test in `ft991a.rs`, and 13
  new `NopRadio` `RadioResult::NotImplemented` assertions in
  `radio_trait.rs` — zero regressions, every pre-existing test name still
  passes unmodified except the table-integrity command count, 40→49, and
  the master-flags test, extended with new batch-4 loops rather than
  replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed.
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping only, across `ft991a_radio.rs`/`ft991a.rs`/
  `radio_trait.rs` — no logic changes).
- Confirmed via `find -newer` that only `radio/src/{ft991a_radio.rs,
  ft991a.rs, radio_trait.rs, lib.rs}` were touched; `ui/`, `emulator/`,
  `src/main.rs`, root `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `KY`/`ZI` mutate no persisted `Ft991aState` field — both are event-only
  acknowledgments (no simulated audio/RF/received-signal state exists in
  this emulator to represent), tested via `CommandOutcome::events` rather
  than a follow-up read.
- Per-field arbitrary defaults for all 7 new `Ft991aState` fields (manual
  states no factory default for any of them), same category as
  `meter_select`'s default (batch 9).
- `Radio` trait scope: 7 of the 9 commands' operations added as trait
  methods (directly informed by checking `ts570d::Radio`'s own surface,
  which already has the Kenwood analogs of `KS`/`SD`); `KM`/`KY` kept
  `Ft991a`-inherent-only per the task's explicit instruction.
- `is_valid_tag_wire` renamed to `is_valid_ascii_wire_content` and reused
  for `KM` — a deliberate small refactor (not required, but the old
  tag-specific name was stale once a second unrelated field needed the same
  validation), `MT`'s existing call site updated to match, no behavior
  change.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 5: Scan/VOX/busy (`SC VX VD VG BY`) (2026-07-19)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 5 row.

Delivered:
- `radio/src/ft991a_radio.rs`: 5 new `Ft991aCommandId` variants + table
  entries (all reusing existing `QUERY0`/`SET_1`/`SET_3`/`SET_4`/`NONE`
  consts, no new `CommandForm` const needed), 5 new `Ft991aState` fields
  (`scan_state`, `vox_on`, `vox_gain`, `vox_delay_ms`, `rx_busy`), 5 new
  `handle_command` match arms.
- `radio/src/radio_trait.rs`: new `ScanState` enum (`TryFrom<u8>`/`as_u8`,
  mirroring `TxState`'s shape — a deliberate divergence from
  `ts570d::Radio::get_scan`/`set_scan`'s plain bool, since the FT-991A's
  `SC` genuinely has 3 legal values), 2 new `RadioError` variants
  (`InvalidVoxGain`, `InvalidVoxDelay`), 5 pairs of `Radio` trait methods
  (`get_scan_state`/`set_scan_state`, `get_vox_on`/`set_vox_on`,
  `get_vox_gain`/`set_vox_gain`, `get_vox_delay`/`set_vox_delay`,
  `get_rx_busy`) + `NopRadio` test coverage for all 10 + 2 `ScanState` unit
  tests.
- `radio/src/ft991a.rs`: 10 new client methods + `Radio` trait impl
  delegations for all 5 pairs.
- `radio/src/lib.rs`: `ScanState` re-export + module-doc scope update.

**`VD`'s `EX` menu item 142 "VOX SELECT" dependency, documented explicitly
per the task's instruction, not hidden**: `VD`'s own manual box (printed
p.17) states its parameter means "VOX DELAY" when menu 142 is "MIC" or
"DATA VOX DELAY" when "DATA" — transcribed verbatim on
`Ft991aState::vox_delay_ms`, `Radio::get_vox_delay`, and
`Ft991a::get_vox_delay`'s doc comments. Menu 142 itself is explicitly **not**
implemented (out of this task's scope, a different `EX` sub-batch); this
emulator models `vox_delay_ms` as one shared value, addressed
unconditionally. **New finding beyond the task brief**: cross-referencing
the full `EX` menu table found two further items (143 "VOX GAIN"/146 "DATA
VOX GAIN", alongside the already-known 144/147 delay pair) that suggest the
same MIC/DATA duality could extend to `VG` too — but `VG`'s own manual box
carries no equivalent doc note, so this implementation does **not** assume
that dependency for `VG`, flagged as an asymmetry in the manual's own
documentation rather than silently normalized. Full citation in
`ft991a_radio.rs`'s module docs' "VD, the batch's highest-risk item"
section and `findings.md` above.

Verification (all from repo root):
- `cargo test -p radio` — **302 unit tests + 1 doctest, all passing** (was
  278+1; 24 new tests — `SC` all-three-values round trip + illegal-value
  rejection, `VX`/`VG`/`VD` round trip + range/step rejection tests, `BY`
  default read + read-only confirmation + a `from_state`-seeded busy-true
  read, 2 `ScanState` unit tests, 12 wire-byte-level client tests + 1
  `Radio`-trait-delegation test in `ft991a.rs` — zero regressions, every
  prior test name still passes unmodified except the table-integrity
  command count, 49→54, and the master-flags test, extended with a new
  batch-5 loop plus `BY`'s explicit read-only assertion, not replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean after fixing
  one `field_reassign_with_default` finding in a new test (struct-update
  syntax, same fix category batch 9 needed).
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (one `match`-arm block reformatting, no logic changes).
- Confirmed via `find -newer` that only `radio/src/{ft991a_radio.rs,
  ft991a.rs, radio_trait.rs, lib.rs}` were touched (plus this task's own
  `planning/yaesu/*` updates); `ui/`, `emulator/`, `src/main.rs`, root
  `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched; `Cargo.lock`
  unchanged (no dependency changes).

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `SC` modeled with a dedicated `ScanState` enum, deliberately diverging
  from `ts570d::Radio`'s plain-bool `get_scan`/`set_scan` precedent, since
  the FT-991A's own manual gives `SC` 3 legal values, not 2.
- `VD`/menu-142 dependency documented per the task's explicit instruction;
  the newly-found menu 143/146 observation (same duality possibly applying
  to `VG`) flagged but not implemented, since `VG`'s own manual box states
  no such dependency.
- Per-field arbitrary defaults (`vox_on`, `vox_gain`, `vox_delay_ms`,
  `scan_state`) — manual states no factory default for any of these, same
  category of open item as `meter_select`'s default (batch 9); `rx_busy`'s
  `false` default is a documented emulator simplification (no simulated RX
  condition exists anywhere in this crate), not an open item.
- `get_rx_busy`'s naming deliberately follows this crate's own `get_*`
  convention rather than `ts570d::Radio::is_busy`'s naming — a documented
  small divergence from that otherwise-followed precedent.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 3: clarifier/RIT-XIT + tone + IF-shift (`RT RC RD RU
XT CN CT IS`) (2026-07-18)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 3 row.

Delivered:
- `radio/src/ft991a_radio.rs`: 8 new `Ft991aCommandId` variants + table
  entries, 3 new `CommandForm` consts (`CT_SET_FORMS`, `CN_SET_FORMS`,
  `IS_SET_FORMS`), the full `CTCSS_TONES_DECIHZ: [u16; 50]` /
  `DCS_CODES: [u16; 104]` lookup tables + 4 conversion helpers
  (`ctcss_tone_index`/`ctcss_tone_hz`/`dcs_code_index`/`dcs_code_number`),
  3 new `Ft991aState` fields (`ctcss_tone_number`, `dcs_code_number`,
  `if_shift_hz`) plus updated doc comments on the 4 already-landed
  clarifier/tone fields (`clarifier_offset_hz`/`rx_clarifier_on`/
  `tx_clarifier_on`/`tone_status`) now that batch 3's `Set` commands write
  them, 8 new `handle_command` match arms.
- `radio/src/ft991a.rs`: 16 new client methods (`get_rx_clarifier_on`/
  `set_rx_clarifier_on`/`get_tx_clarifier_on`/`set_tx_clarifier_on`/
  `clarifier_clear`/`clarifier_down`/`clarifier_up`/`get_if_shift_hz`/
  `set_if_shift_hz`/`get_tone_squelch_mode`/`set_tone_squelch_mode`/
  `get_ctcss_tone_hz`/`set_ctcss_tone_hz`/`get_dcs_code`/`set_dcs_code`)
  plus 2 new parsing helpers (`parse_is_body`, `parse_cn_index`).
- `radio/src/radio_trait.rs`: new `ToneSquelchMode` enum (+ `TryFrom<u8>`/
  `Display`), 4 new `RadioError` variants (`InvalidCtcssTone`,
  `InvalidDcsCode`, `InvalidToneSquelchMode`, `InvalidIfShift`), 16 new
  `Radio` trait methods (mirroring the client methods above) + `NopRadio`
  test coverage for all 16.
- `radio/src/lib.rs`: re-exports (`ToneSquelchMode`, `CTCSS_TONES_DECIHZ`,
  `DCS_CODES`, the 4 conversion functions) + module-doc scope update.

**RX/TX clarifier relationship (the architect's flagged item), resolved
from the manual, not assumed**: `RT`/`XT` are independent on/off gates on
a single shared `clarifier_offset_hz` value — confirmed by the absence of
any `XD`/`XU` command pair alongside `RD`/`RU` in the master table, and by
`IF`'s own already-landed P3/P4/P5 field layout modeling exactly this
shape. `RU`'s "RX CLARIFIER PLUS OFFSET" heading (the specific thing
flagged as a possible RX-only signal) was cross-checked against `IF`'s P3
legend and concluded to be a naming leftover, not evidence of a second
offset — full reasoning in `findings.md`.

**CTCSS/DCS tables, transcribed and independently cross-checked**: both
tables (50 CTCSS tones, 104 DCS codes, manual p.6) transcribed in full
from the page image and cross-checked against the well-known
industry-standard CTCSS/DCS lists — one single-digit transcription error
(DCS index 078: `465`→corrected to `466`) was caught this way and is
documented, not silently fixed.

**`IS`'s P2 width, a resolved manual discrepancy**: the per-command box's
column diagram literally shows 3 P2 digit-cells, but the manual's own p.2
worked example and error catalog both require 4 — resolved in favor of 4
digits (matching `"IS0+1000;"` exactly), the column diagram treated as a
drafting omission. Full citation in `ft991a_radio.rs`'s module docs.

Verification (all from repo root):
- `cargo test -p radio` — **225 unit tests + 1 doctest, all passing** (was
  157+1; 68 new tests — command-table integrity extension, `RT`/`XT`
  query/set round trips + independent-gate test, `RC` clear-offset-only
  test, `RD`/`RU` absolute-set + overwrite-not-accumulate tests, `CT`
  selector-read round trip over all 5 legal values + out-of-range
  rejection + cross-check against `IF`'s `tone_status`, `CN` CTCSS/DCS
  selector-read round trips at table boundaries (index 0 and the last
  index) + independent-selector test + 3 out-of-range/illegal-selector
  rejection tests, `IS` selector-read round trip (positive and negative,
  including the manual's own `"IS0+1000;"` worked example) + range/step
  rejection tests, 9 direct table-integrity/lookup-boundary tests
  (`ctcss_tones_table_has_fifty_unique_entries`,
  `dcs_codes_table_has_104_unique_entries`, first/last-entry and
  invalid-index/invalid-value lookups for both tables), plus 24 new
  wire-byte-level client tests and 1 `Radio`-trait-delegation test in
  `ft991a.rs`, and 16 new `NopRadio` `RadioResult::NotImplemented`
  assertions plus 4 `ToneSquelchMode` unit tests in `radio_trait.rs` —
  zero regressions, every pre-existing test name still passes unmodified
  except the table-integrity command count, 32→40, and the master-flags
  test, extended with new batch-3 loops rather than replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed.
- `cargo fmt --check -p radio` — clean after two `cargo fmt -p radio`
  passes (line-wrapping only, across the implementation edit and the
  later test-addition edit — no logic changes).
- Confirmed via this session's own tool-call history (not a git repo, so
  no `git status` available) that only `radio/src/{ft991a_radio.rs,
  ft991a.rs, radio_trait.rs, lib.rs}` were touched; `Cargo.lock` unchanged
  (no dependency changes); `ui/`, `emulator/`, `src/main.rs`, root
  `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `RD`/`RU` modeled as absolute sets (overwrite), not incremental steps —
  the most literal reading of the explicit magnitude field, but not
  100%-provable from text alone.
- `RC` zeroes the offset only, leaving `RT`/`XT`'s gates untouched — a
  documented judgment call (manual gives `RC` no parameter to say
  otherwise).
- `IS`'s "20 Hz steps" range restriction enforced literally
  (`magnitude % 20 == 0`) — not independently re-confirmed by a second
  manual signal the way the digit-width resolution was.
- `Radio` trait scope: clarifier/RIT-XIT and IF-shift added as trait
  methods per `CLAUDE.md`'s explicit "RIT/XIT" naming; CTCSS/DCS tone
  *selection* also added (domain-typed Hz/code values), but the raw
  50/104-entry lookup tables and their wire-index conversion stay
  `ft991a_radio`-internal, converted only at the client boundary — per
  the task brief's own framing that the tables are FT-991A data, not
  trait-level.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 6: Attenuator/preamp/noise/AGC/notch/filter-width (`RA
PA NB NL NR RL GT CO BP BC NA SH`) (2026-07-19)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 6 row — the
largest remaining batch (12 commands).

Delivered:
- `radio/src/ft991a_radio.rs`: 12 new `Ft991aCommandId` variants + table
  entries (all "selector read" shapes, reusing/extending the `CT_SET_FORMS`
  pattern with 12 new named `CommandForm` consts), 16 new `Ft991aState`
  fields, `ModeFamily` enum + `mode_family_for`/`filter_bandwidth_hz` pure
  lookup functions, `ShBandwidthRow` struct + `SH_BANDWIDTH_TABLE` (22 rows,
  the full six-column bandwidth table transcribed in full), `apf_raw_to_hz`/
  `apf_hz_to_raw` conversion helpers, 12 new `handle_command` match arms.
- `radio/src/radio_trait.rs`: 8 new `RadioError` variants, `PreampMode`
  (3-valued `TryFrom<u8>` enum) and `AgcMode` (7-valued domain, with
  `as_u8`/`set_wire_value` for the write/report domain split), 20 new
  `Radio` trait methods (10 pairs: attenuator, preamp mode, noise blanker
  on/level, noise reduction on/level, AGC mode, auto notch, narrow, filter
  width index) + `NopRadio` test coverage for all 20 + `PreampMode`/
  `AgcMode` unit tests.
- `radio/src/ft991a.rs`: 32 new inherent client methods (20 trait-backed +
  12 `Ft991a`-inherent-only for `CO`'s 4 items and `BP`'s 2 items) + `Radio`
  trait impl delegations for the 20 trait-backed ones.
- `radio/src/lib.rs`: re-exports (`PreampMode`, `AgcMode`, `ModeFamily`,
  `ShBandwidthRow`, `mode_family_for`, `filter_bandwidth_hz`,
  `SH_BANDWIDTH_TABLE`, `apf_raw_to_hz`, `apf_hz_to_raw`) + module-doc scope
  update.

**`GT`'s write/report domain mismatch** (Set's 5-valued `P2` vs. Answer's
7-valued `P3`) and **`NA`'s manual wire-diagram typo** (`M A` instead of
`N A` in its own per-command box) were both confirmed as genuine manual
findings, not extraction artifacts, and resolved via documented judgment
calls / corroborating evidence — full citations in `findings.md` above and
`ft991a_radio.rs`'s module docs.

**`SH`'s full six-column bandwidth table transcribed completely** (22 rows,
P2 00-21 × SSB/CW/RTTY-PSK × Narrow/Wide), cross-checked via both the
rendered page image and `pdftotext -layout` (both agreed exactly). Boundary
tests cover first/last valid P2 per mode family (SSB-narrow, CW-narrow,
RTTY-PSK-wide) plus an out-of-range-P2 case, per the task's explicit
requirement.

Verification (all from repo root):
- `cargo test -p radio` — **368 unit tests + 1 doctest, all passing** (was
  302+1; 66 new tests, zero regressions — every prior test name still
  passes unmodified except the table-integrity command count, 54→66, and
  the master-flags test, extended with a new batch-6 loop rather than
  replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed.
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping only, across `ft991a_radio.rs`/`ft991a.rs`/`lib.rs` — no
  logic changes).
- Confirmed via this session's own tool-call history that only
  `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs, lib.rs}` were
  touched — `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
  `radio-cat-rs/` untouched. No commits made.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `GT`'s `P2=4` ("AUTO") → `P3=4` ("AUTO-FAST") resolution — a documented
  judgment call, not provable from the manual; `AutoMid`/`AutoSlow` are
  unreachable via any CAT `Set`, only seedable via `from_state`.
- `NA`'s wire code resolved to `NA` (not the per-command box's literal
  `MA` wire cells) via three corroborating signals — flagged, not silently
  picked.
- `CO`'s APF frequency raw-to-Hz mapping is a linear-interpolation judgment
  call (endpoints/step count manual-stated, formula is not); `BP`'s manual
  notch frequency multiplier, by contrast, is directly manual-stated.
- `SH`'s mode-family mapping leaves `DATA-LSB`/`DATA-USB`/`DATA-FM`/`FM`/
  `AM`/`C4FM` unmapped rather than guessing; the narrow/wide column
  selector's connection to `NA`'s own state is well-evidenced but not
  literally stated on either command's page.
- `Radio` trait scope: 10 of 12 commands' operations (20 methods) added to
  the trait, directly informed by `ts570d::Radio`'s existing precedent for
  all five underlying concepts; `CO`/`BP` kept `Ft991a`-inherent-only
  (FT-991A-specific parametric-EQ/notch-detail features, no generic-concept
  precedent) — `BC` (plain bool) vs. `BP` (2-item selector) is a documented
  trait-inclusion asymmetry, not an inconsistency.
- Per-field arbitrary defaults for most of the 16 new `Ft991aState` fields
  (manual states no factory default for most of them), same category of
  open item as `meter_select`'s default (batch 9); `filter_width_index`'s
  default of `0` and `apf_freq_hz`'s default of `0` are the two
  manual-stated/unambiguous exceptions in this batch.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 7: Speech processor/mic/monitor (`PL PR MG ML`) (2026-07-19)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 7 row — the
architect's own deliberately smallest batch (4 commands).

Delivered:
- `radio/src/ft991a_radio.rs`: 4 new `Ft991aCommandId` variants + table
  entries (`MG`/`PL` plain `QUERY0`/`SET_3`; `PR_SET_FORMS`/`ML_SET_FORMS`
  two-width selector-read consts, `[1,2]` and `[1,4]` respectively), 6 new
  `Ft991aState` fields (`mic_gain`, `speech_processor_level`,
  `speech_processor_on`, `parametric_mic_eq_on`, `monitor_on`,
  `monitor_level`), 4 new `handle_command` match arms.
- `radio/src/ft991a.rs`: `get_mic_gain`/`set_mic_gain`,
  `get_speech_processor_level`/`set_speech_processor_level`,
  `get_speech_processor_on`/`set_speech_processor_on`,
  `get_parametric_mic_eq_on`/`set_parametric_mic_eq_on` (`Ft991a`-inherent
  only), `get_monitor_on`/`set_monitor_on`,
  `get_monitor_level`/`set_monitor_level` client methods +
  `parse_pr_answer`/`parse_ml_body` helpers + `Radio` trait impl additions
  for the 5 trait-backed pairs.
- `radio/src/radio_trait.rs`: 3 new `RadioError` variants (`InvalidMicGain`,
  `InvalidSpeechProcessorLevel`, `InvalidMonitorLevel`), 10 new `Radio`
  trait methods (5 pairs: mic gain, speech processor level, speech
  processor on/off, monitor on/off, monitor level) + `NopRadio` test
  coverage for all 10.
- `radio/src/lib.rs`: module-doc scope update (no new public types to
  re-export — `Ft991aState`'s new fields ride along with its existing
  re-export).

**`PR`'s manual heading typo, the batch's one real finding**: `PR`'s own
per-command box is headed "SPEECH PROCESSOR LEVEL" — identical to `PL`'s
heading — despite being a genuine on/off toggle command (`P1` selects
Speech Processor vs. Parametric Mic EQ, `P2` is that feature's on/off
state with an unusual `1`=OFF/`2`=ON encoding). Resolved via the master
table's own "SPEECH PROCESSOR" naming plus the wire content itself, not
the misleading per-command heading — same category as batch 1's `VM`/`AM`
clash and batch 6's `NA` typo, documented not silently picked.

Verification (all from repo root):
- `cargo test -p radio` — **404 unit tests + 1 doctest, all passing** (was
  368+1; 36 new tests — 19 `CatFramework::process_frame` round-trip/
  rejection tests covering all 4 commands (`MG`/`PL` round trip + range
  rejection, `PR` both selectors' round trips + independence test +
  illegal-selector/illegal-encoding rejections, `ML` both selectors' round
  trips + independence test + illegal-on-off/out-of-range-level/illegal-
  selector rejections), 17 wire-byte-level client tests + 1
  `Radio`-trait-delegation test in `ft991a.rs` — zero regressions, every
  prior test name still passes unmodified except the table-integrity
  command count, 66→70, and the master-flags test, extended with a new
  batch-7 loop rather than replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean after fixing
  1 `clippy::chars_next_cmp` finding in `parse_pr_answer`
  (`body.chars().next() != Some(x)` → `!body.starts_with(x)`).
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (one blank-line removal before a comment block, no logic changes).
- Confirmed via file `stat` mtimes that only `radio/src/{ft991a_radio.rs,
  ft991a.rs, radio_trait.rs, lib.rs}` were touched this session (clustered
  timestamps distinct from and later than every other tracked `.rs`/
  `Cargo.toml` file in the repo); `ui/`, `emulator/`, `src/main.rs`, root
  `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `PR`'s manual heading typo (see above) — resolved via corroborating
  evidence (master table naming + wire content), not silently picked.
- `PR`'s `1`=OFF/`2`=ON encoding — the only non-`0`/`1` on/off command in
  this crate's entire 70-command table, transcribed exactly.
- `ML`'s monitor on/off and level trait methods — the batch's one genuine
  judgment call: no `ts570d::Radio` precedent exists at all (checked
  directly), added anyway as a near-universal transceiver concept, flagged
  explicitly for architect review since every other trait addition this
  task made does have direct `ts570d::Radio` precedent to cite.
- Per-field arbitrary defaults (`0`/`false`) for all 6 new `Ft991aState`
  fields — manual states no factory default for any of them, same category
  of open item as `meter_select`'s default (batch 9).

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 8: band/step/encoder front-panel controls (`BS BU BD
FS ED EU EK DN UP`) (2026-07-19)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 8 row.

Delivered:
- `radio/src/ft991a_radio.rs`: 9 new `Ft991aCommandId` variants + table
  entries (all reusing existing `QUERY0`/`SET_1`/`SET_2`/`SET_3`/`ACTION`
  consts — no new `CommandForm` const needed), `EncoderSelector` enum +
  `as_wire_digit`/`from_wire_digit` helpers, `BAND_CODES` (16-entry array,
  the full band table excluding the documented `13` gap) +
  `next_band`/`prev_band` wrap-around helpers, `MIC_STEP_HZ` const, 2 new
  `Ft991aState` fields (`selected_band`, `fast_step_on`), 9 new
  `handle_command` match arms.
- `radio/src/ft991a.rs`: `set_band`/`band_up`/`band_down`,
  `get_fine_step`/`set_fine_step`, `encoder_down`/`encoder_up`/`ent_key`
  (`Ft991a`-inherent only), `mic_up`/`mic_down` client methods + `Radio`
  trait impl additions for the 7 trait-backed ones.
- `radio/src/radio_trait.rs`: `Band` enum (16-valued, `TryFrom<u8>`/
  `as_u8`, rejects the `13` gap), 2 new `RadioError` variants
  (`InvalidBand`, `InvalidEncoderSteps`), 7 new `Radio` trait methods
  (`set_band`/`band_up`/`band_down`, `get_fine_step`/`set_fine_step`,
  `mic_up`/`mic_down`) + `NopRadio` test coverage for all 7 + `Band` unit
  tests.
- `radio/src/lib.rs`: re-exports (`EncoderSelector`, `Band`) + module-doc
  scope update.

**`DN`'s manual heading mismatch (the architect's flagged item),
resolved via cross-radio corroboration**: `DN`'s own per-command box says
"MIC DWN" while the master table says "DOWN"; `UP`'s two headings agree.
Since the wire formats are byte-identical zero-width Action triggers
(same disambiguation problem as batch 1's `VM`/`AM`), resolved by
checking the sibling `ts570d` repo's own Kenwood CAT implementation
directly (present in this sandbox at
`/home/mattfranklin/src/github.com/kf0uwv/ts570d`): it has
**wire-identical** `UP`/`DN` commands implemented as
`ts570d::Radio::mic_up`/`mic_down`, stepping `vfo_a_hz` by a fixed 100 Hz
per press. This independent, different-manufacturer corroboration
resolved `DN`/`UP` as mic-button commands — implemented as
`Ft991a::mic_down`/`mic_up` (matching `ts570d`'s method names exactly),
stepping `vfo_a_hz` by a fixed `MIC_STEP_HZ` (10 Hz, documented arbitrary
choice), saturating at `FA`'s range. Full reasoning in `findings.md`.

**`BS`'s full 16-band table transcribed completely** (17 wire codes
`00`-`16`, 16 real bands, documented gap at `13`), cross-checked via
`pdftotext -layout` against the rendered page image (both agreed
exactly). Boundary tests cover the first (`00`)/last (`16`) valid bands,
the gap rejection (`13`), and an out-of-range rejection (`17`), plus
`BU`/`BD`'s wrap-around-while-skipping-the-gap behavior at both ends of
the table.

Verification (all from repo root):
- `cargo test -p radio` — **442 unit tests + 1 doctest, all passing** (was
  404+1; 38 new tests — command-table integrity extension, `BS`
  first/last/gap/out-of-range tests, `BU`/`BD` wrap-and-skip-gap tests +
  fixed-selector-rejection test, `FS` query/set round trip +
  illegal-value rejection, `ED`/`EU` validate-and-acknowledge tests
  (confirming no `vfo_a_hz` mutation) + illegal-P1/out-of-range-P2
  rejection test, `EK` trigger + parameter-rejection test, `DN`/`UP` step
  tests + range-saturation tests + parameter-rejection test, 6 direct
  unit tests for `BAND_CODES`/`next_band`/`prev_band`/`EncoderSelector`,
  plus 13 new wire-byte-level client tests + 1 `Radio`-trait-delegation
  test in `ft991a.rs`, and 4 new `Band` unit tests + `NopRadio`
  `RadioResult::NotImplemented` assertions for all 7 new trait methods in
  `radio_trait.rs` — zero regressions, every prior test name still passes
  unmodified except the table-integrity command count, 70→79, and the
  master-flags test, extended with a new batch-8 loop rather than
  replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed.
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping only, across all four touched files — no logic changes).
- Confirmed via this session's own tool-call history that only
  `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs, lib.rs}` were
  touched — `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
  `ts570d/`, `radio-cat-rs/` untouched (the `ts570d` repo was read for
  precedent-checking only, never modified). No commits made.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `DN`'s heading resolution (see above) — resolved via corroborating
  evidence found *outside* the FT-991A manual itself (a sibling repo's
  independently-arrived-at Kenwood CAT protocol), the strongest form of
  corroboration this crate has used for this category of manual
  inconsistency so far.
- `MIC_STEP_HZ`'s value (`10` Hz) and its independence from `FS`'s
  `fast_step_on` state are both documented arbitrary choices, mirroring
  `ts570d`'s own precedent in category (a fixed, undocumented Hz value)
  but not in exact number (`ts570d` picked `100`).
- `ED`/`EU`/`EK` mutate no persisted `Ft991aState` field — a documented
  judgment call, not an oversight, since no Hz-per-step mapping is
  knowable from this manual page and no cross-radio precedent exists to
  borrow one from (unlike `DN`/`UP`).
- `BU`/`BD`'s wrap-around behavior is manual-silent, same category of
  open item as batch 1's `CH`.
- Per-field arbitrary defaults (`selected_band = 0`, `fast_step_on =
  false`) — manual states no factory default for either, same category
  as `meter_select`'s default (batch 9).
- `Radio` trait scope: `set_band`/`band_up`/`band_down` added despite no
  `ts570d::Radio` precedent, a documented judgment call per the task's
  own "fairly generic" framing; `FS`/`mic_up`/`mic_down` have direct
  precedent; `ED`/`EU`/`EK` stay `Ft991a`-inherent-only.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS
(`AC AI DA DT LK OI OS FT TS MX LM PB`) (2026-07-19)

Status: **implementation complete, verification clean, ready for architect
review.** Per `planning/architect/task_plan.md` §10.5's batch 10 row — the
last of the 10 core CAT batches; **all 91 top-level (non-`EX`) CAT commands
are now implemented.**

Delivered:
- `radio/src/ft991a_radio.rs`: 12 new `Ft991aCommandId` variants + table
  entries (`SET_6`, `DT_SET_FORMS`, `OS_SET_FORMS`, `LM_SET_FORMS`,
  `PB_SET_FORMS` new `CommandForm` consts), 17 new `Ft991aState` fields (`OS`
  reuses the already-declared `offset_type` field, not new), 12 new
  `handle_command` match arms.
- `radio/src/ft991a.rs`: `get/set_antenna_tuner_state`,
  `get/set_auto_info_on`, `get/set_dimmer`, `read/write_date`,
  `read/write_time`, `read/write_time_zone_offset`,
  `get/set_frequency_lock`, `get_opposite_band_information`,
  `get/set_repeater_shift`, `get/set_tx_vfo`, `get/set_txw_on`,
  `get/set_mox_on`, `get_dvs_recording_channel`/`start_dvs_recording`/
  `stop_dvs_recording`, `get_dvs_playback_channel`/`start_dvs_playback`/
  `stop_dvs_playback` client methods + `Radio` trait impl additions for the
  5 trait-backed pairs (`AI`/`LK`/`OS`/`FT`/`MX`).
- `radio/src/radio_trait.rs`: 8 new `RadioError` variants
  (`InvalidRepeaterShift`, `InvalidDate`, `InvalidTime`,
  `InvalidTimeZoneOffset`, `InvalidAntennaTunerState`,
  `InvalidDimmerLevel`, `InvalidTxVfo`, `InvalidDvsChannel`), new
  `RepeaterShift` enum (`TryFrom<u8>`/`as_u8`), 10 new `Radio` trait
  methods (5 pairs: `get/set_auto_info_on`, `get/set_frequency_lock`,
  `get/set_repeater_shift`, `get/set_tx_vfo`, `get/set_mox_on`) +
  `NopRadio` test coverage for all 10.
- `radio/src/lib.rs`: `RepeaterShift` re-export + module-doc scope update.

**`TS`, the architect's-dispatch-prompt-guess-vs-manual mismatch, resolved
from the manual, not the guess**: manual p.17 headed unambiguously "TXW"
— implemented per the manual's own wire box, flagged not silently
reconciled with the "tuning step?" guess.

**`OI` confirmed to share `IF`'s `ChannelStatusFields` shape exactly**:
column-by-column re-verification against the manual image, cross-checked
against `IF`'s own box and `FA`'s unambiguous 8+1 frequency-field split to
resolve an apparent (but ultimately spurious) rendering-quirk digit-count
ambiguity. Reused `ChannelStatusFields::to_wire_string` **unmodified**; the
sole real difference from `IF`'s payload is the frequency source
(`vfo_b_hz`, not `vfo_a_hz`) — all other fields read from the same shared,
non-per-VFO state `IF` already uses (an inherited Wave-1 state-model
constraint, flagged for architect/hardware review).

**`FT`'s write/report domain mismatch** (Set `{2,3}` vs. Answer `{0,1}` for
identical states) and **`DT`'s 3-shape variable-P2 design** (the smaller
rehearsal of `EX`'s pattern the architect's brief predicted) both confirmed
exactly as expected/described — full citations in `findings.md` above and
`ft991a_radio.rs`'s module docs.

Verification (all from repo root):
- `cargo test -p radio` — **473 unit tests + 1 doctest, all passing** (was
  442+1; 31 new tests — command-table integrity extension, `AC`/`AI`/`DA`/
  `LK`/`TS`/`MX` round trips + rejection tests, `DT`'s three P1-selected
  shapes each independently round-tripped and range-rejection tested
  (including a structurally-legal-width-with-wrong-P1 rejection test), `OI`
  reuse of `ChannelStatusFields` tested three ways (default state, diff
  against `IF`, `from_state`-seeded non-default state), `OS` round trip +
  cross-check that it writes the same `offset_type` field `IF` reports,
  `FT`'s Set-domain-vs-Answer-domain translation round-tripped both
  directions + illegal-value rejection, `LM`'s toggle vs. `PB`'s
  unconditional-start semantics each explicitly tested and contrasted — zero
  regressions, every prior test name still passes unmodified except the
  table-integrity command count, 79→91, and the master-flags test, extended
  with a new batch-10 loop rather than replaced).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean after fixing
  1 `clippy::if_same_then_else` finding in `LM`'s write arm (collapsed two
  identical `0`-arms into one `||`-guarded condition).
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping only, no logic changes).
- Confirmed via `find -newer` that only `radio/src/{ft991a_radio.rs,
  ft991a.rs, radio_trait.rs, lib.rs}` were touched; `Cargo.lock` unchanged
  (no dependency changes); `ui/`, `emulator/`, `src/main.rs`, root
  `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `AC`'s Read row is genuinely zero-width (not a selector read), a
  structural exception worth flagging against this crate's many other
  fixed-`P1`-selector commands.
- `Radio` trait scope: `AI`/`LK`/`FT` have direct `ts570d::Radio` precedent
  (narrowed domains for `AI`/`FT`); `OS`/`MX` have no precedent, added
  anyway as near-universal concepts (flagged, same treatment as
  `Band`/`ScanState`/`ML`); `AC`/`DA`/`DT`/`OI`/`TS`/`LM`/`PB` kept
  `Ft991a`-inherent-only, each reasoned individually.
- Per-field arbitrary defaults for most of the 17 new `Ft991aState` fields
  (manual states no factory default for most of them), same category of
  open item as `meter_select`'s default (batch 9); `dvs_recording_channel`/
  `dvs_playback_channel`'s `0` default and `time_zone_offset_min`'s `0`
  default are the unambiguous, non-judgment-call exceptions in this batch.

**This was the last of the 10 core CAT batches** — running total confirmed:
all 91 top-level (non-`EX`) CAT commands are now implemented in `radio`.
Only remaining scope: the `EX` menu's ~144 unimplemented items (one
sub-batch of 9 landed so far) and the RTS/DTR consumption wiring
(§10.2-10.4, landed in `radio-cat-rs`, not yet consumed on this repo's
side).

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — RTS/DTR modem-control-lines consumption (§10.4) (2026-07-19)

Status: **implementation complete and fully verified (under a temporary,
now-reverted local patch) — BLOCKED for real, unpatched builds on a
radio-cat-rs commit/push gap. Needs architect/coordinating-session action,
not further work from this agent.** Per `planning/architect/task_plan.md`
§10.4 in full.

Delivered, `radio/src/ft991a.rs` only:
- `use cat_transport_core::ModemControlLines;` added to the existing
  `cat_transport_core` import line.
- `impl<S: ModemControlLines> ModemControlLines for SharedSession<S>` —
  blanket delegation immediately after the existing `CatSession for
  SharedSession<S>` impl, same take/call/put_back shape.
- New, additive `impl<S> Ft991a<S> where S: CatSession<Error =
  TransportError> + ModemControlLines` block (placed right after the main
  inherent-method block closes, before the `Radio` trait impl section) with
  `assert_rts`/`assert_dtr`/`read_cts`/`read_dsr`/`read_dcd`, each
  `.map_err(Into::into)` onto `RadioResult` via the existing
  `RadioError::Transport(#[from] TransportError)` variant. Does not touch
  the existing `impl<S: CatSession<...>> Ft991a<S>` block, the `Radio`
  trait, or `impl Radio for Ft991a<S>`.
- Test-module `FakeTransport` extended with a `ModemControlLines` impl
  (5 new `Cell` fields) — no new fake session type needed, since
  `cat-transport-serial`'s existing blanket `SerialCatSession<T: Transport
  + ModemControlLines>: ModemControlLines` delegation makes this sufficient.
- 6 new tests: `test_assert_rts_delegates_through_shared_session_to_transport`,
  `test_assert_dtr_delegates_through_shared_session_to_transport`,
  `test_read_cts_delegates_through_shared_session_to_transport`,
  `test_read_dsr_delegates_through_shared_session_to_transport`,
  `test_read_dcd_delegates_through_shared_session_to_transport`, and a
  combined `test_shared_session_modem_control_lines_all_delegate_independently`.

**BLOCKING FINDING**: `radio-cat-rs`'s `ModemControlLines` work is real,
correct, and matches §10.4's sketch exactly (verified by reading
`cat-transport-core/src/modem.rs` and `cat-transport-serial/src/session.rs`
directly) — but exists only as **uncommitted working-tree changes** in the
sibling checkout, not on `origin/main` (confirmed via `git fetch` + `git
log origin/main`, still at the pre-`ModemControlLines` extraction commit).
`ft991a`'s git dependency on `cat-transport-core`/`cat-transport-serial`
therefore cannot see this code — `cargo build -p radio` against the real,
unpatched dependency graph fails with `unresolved import
cat_transport_core::ModemControlLines`. Full detail and citations in
findings.md above and task_plan.md's matching section.

Verification, performed under a temporary, now-fully-reverted local
`[patch."https://github.com/kf0uwv/radio-cat-rs"]` (added to root
`Cargo.toml`, pointing at the sibling `radio-cat-rs` checkout's local
paths, solely to prove this task's own new code against the real trait —
not a `[dependencies]`/`[[bin]]` edit, and reverted before this task ended,
`diff`-confirmed byte-identical to the pre-task `Cargo.toml`/`Cargo.lock`):
- `cargo build -p radio --all-targets` — clean.
- `cargo test -p radio` — **479 unit tests + 1 doctest, all passing** (was
  473+1 — 6 new tests, zero regressions, no pre-existing test modified).
- `cargo clippy -p radio --all-targets -- -D warnings` — clean, no fixes
  needed.
- `cargo fmt --check -p radio` — clean after one `cargo fmt -p radio` pass
  (line-wrapping in the new tests only, no logic changes).
- `cargo build --workspace` — clean, **confirming `src/main.rs` needs zero
  edits** to satisfy the new bound (`Ft991a<SerialCatSession<SerialPort>>`
  gets `assert_rts`/`read_cts`/etc. for free, exactly as §10.4 claimed).

After verification, `Cargo.toml` reverted to its pre-task state (`diff`
against a pre-task backup: identical) and `Cargo.lock` restored from a
pre-task backup (`diff`: identical). Re-ran `cargo build -p radio
--all-targets` against this reverted, real state to confirm it fails with
the exact `unresolved import` error described above — the repo is left in
its true, honest, unpatched state, not a silently-working one.

Confirmed via file mtimes that only `radio/src/ft991a.rs` was modified as
this task's actual deliverable. `ui/`, `emulator/`, `src/main.rs`,
`radio/Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched (read-only
reference, no commits made there — committing radio-cat-rs's own
uncommitted work is explicitly outside this agent's authority per its
"read-only reference" constraint). No commits made in `ft991a` either (not
a git repo).

**Action needed from the architect/coordinating session**: get
`radio-cat-rs`'s already-written, already-verified-by-this-task
`ModemControlLines` work (`cat-transport-core/src/modem.rs` +
`cat-transport-serial/src/{session.rs,io_uring.rs}`) committed and pushed
to `origin/main` — e.g. by dispatching a task to `radio-cat-rs`'s own
agents. Once that lands, `ft991a`'s `radio` crate will build/test/lint
clean against the real dependency with zero further changes needed on this
side — this task's deliverable is otherwise complete and ready.

## Wave 3 — `EX` menu, second sub-batch (items 001-046, minus 027) (2026-07-19)

Landed 45 new `EX_MENU_TABLE` rows (menu numbers 001-046, minus 027 —
explicitly skipped, unresolvable, see findings.md), on top of the first
sub-batch's 9. New `ExMenuValueKind` enum (`Enumerated`/`Range`) added to
`ft991a_radio.rs` to model this sub-batch's continuous numeric ranges (the
first sub-batch's items were all small named pick-lists); the 9 existing
rows converted to the new shape with zero behavior change.
`Ft991aState::ex_menu_value`/`set_ex_menu_value` widened `u8`→`i32`
(existing 9 `u8` state fields unchanged, cast only at that boundary). 45
new `i32` state fields added with a documented, consistent default policy
(first legend value / range min / signed-range zero). Fixed a latent
zero-padding bug in the shared `Ex` read-response formatting (harmless
while every item was digit-width-1; would have broken on the first
multi-digit item) as a necessary side-effect, not scope creep.

**Only `radio/src/ft991a_radio.rs` touched** — confirmed by this session's
own edit history (no other file opened for writing). `ui/`, `emulator/`,
`src/main.rs`, root `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched. No
commits made.

**Verification**: `cargo build -p radio` clean. `cargo test -p radio`:
**486 unit tests + 1 doctest, all passing** (was 479+1 — 7 net new tests,
zero regressions; 2 existing tests renamed/re-scoped because their
"unlanded P1" examples became landed by this task — necessary updates,
not incidental churn, see task_plan.md for exactly which). `cargo clippy
-p radio --all-targets -- -D warnings`: clean, no fixes needed. `cargo fmt
--check -p radio`: clean, no reformatting needed.

**Menu numbers implemented**: 001-046, minus 027 (45 items) — see
`EX_MENU_TABLE`'s doc comment in `ft991a_radio.rs` for the full per-item
name/encoding table and citations.

**Menu numbers explicitly skipped (unresolvable, not deferred)**: 027
"TIME ZONE" — no wire-encoding formula given anywhere in the manual
(unlike signed-range siblings 035/039, which both state one), and
real-world UTC offsets aren't uniformly stepped, so guessing risked being
wrong in a way this manual alone can't resolve. Same treatment as item
087 "RADIO ID" (already skipped by the first sub-batch, still skipped —
out of range for this sub-batch's 001-046 window regardless).

**Menu numbers deferred to the next sub-batch (not unresolvable, just out
of this session's scope)**: 049-079 (rest of the architect's "TX audio
chain 045-079" theme — 047/048 already landed by the first sub-batch), and
080-153 beyond that. The next sub-batch should start at **049** ("AM DATA
GAIN"). No cross-item dependency was found that would force any
particular further split of 049-153 — a free choice for whoever picks it
up.

**Judgment calls flagged for review** (full detail in findings.md): (1)
~14 items with no per-value legend modeled as numeric `Range` rather than
`Enumerated`, with `step` assumed `1` where the manual states no step; (2)
default-value policy (first legend value / range min / signed-range zero)
is this implementation's consistent but arbitrary choice, no factory
default is stated in the manual for any of these; (3) signed items 035/039
canonicalize `"-00"` writes to a `"+00"` read-back — both are legal per
the manual, but only one canonical form is stored internally.

## Wave 3 — `EX` menu, third sub-batch (items 049-079) (2026-07-19)

Landed 26 new `EX_MENU_TABLE` rows (menu numbers 049-079, minus
060/071/072/076/077 — already landed by the first sub-batch, not
duplicated), on top of the 54 already landed (9 first sub-batch + 45
second sub-batch). `EX_MENU_TABLE` now has 80 entries. 26 new `i32`
`Ft991aState` fields added with the same default-value policy as the
second sub-batch (`Enumerated` → first legend value, unsigned `Range` →
`min`, signed `Range` (064/065) → `0`). No new `ExMenuValueKind` variant
or `EX_SET_FORMS` width was needed — the existing `Enumerated`/`Range`
shapes and all 6 digit widths (1,2,3,4,5,8) already covered every item in
this range.

**068/069 resolution (the task's special-attention item)**: re-rendered
the manual page at 300 DPI and zoomed directly on the rows — the actual
printed table shows Digits 068=1/069=2 (matching `pdftotext`, **not** what
the prior sub-batch's findings claimed to have confirmed via the image).
That literal reading is functionally impossible for 068 (legend needs
00-67, 2 digits) and contradicts all 6 sibling `*HCUT FREQ`/`*HCUT SLOPE`
pairs on the page (always 2/1). Implemented as **068 Digits=2, 069
Digits=1** — corroborated judgment call, not a guess, not a skip — with a
dedicated regression test locking the resolution in. Full citation in
`EX_MENU_TABLE`'s doc comment and `findings.md`.

**Only `radio/src/ft991a_radio.rs` touched** — confirmed via `find
-newer`. `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched. No commits made.

**Verification**: `cargo build -p radio` clean. `cargo test -p radio`:
**493 unit tests + 1 doctest, all passing** (was 486+1 — 7 net new tests,
zero regressions; 2 existing tests updated because their "unlanded P1"
examples, 49/61 and 049, became landed by this task — necessary updates,
not incidental churn, see task_plan.md/findings.md for exactly which).
`cargo clippy -p radio --all-targets -- -D warnings`: clean, no fixes
needed. `cargo fmt --check -p radio`: clean after one `cargo fmt -p
radio` pass (line-wrapping only in the new tests).

**Menu numbers implemented**: 049, 050, 051, 052, 053, 054, 055, 056,
057, 058, 059, 061, 062, 063, 064, 065, 066, 067, 068, 069, 070, 073, 074,
075, 078, 079 (26 items).

**Menu numbers already landed, not re-touched**: 060, 071, 072, 076, 077
(first sub-batch).

**Menu numbers skipped as unresolvable**: none in this range — every item
in 049-079 transcribed cleanly (068/069 needed a documented judgment call,
not a skip; see above).

**Next sub-batch should resume at 080** ("RPT SHIFT 28MHz"). No
cross-item dependency found that would force a particular further split
of 080-153 (minus already-skipped 087 "RADIO ID").

## Wave 3 — `EX` menu, fourth sub-batch (items 080-153, minus 087, 108/109) (2026-07-19)

Status: **implementation complete, verification clean, ready for architect
review. This completes `EX_MENU_TABLE`** — every one of the 153 manual
menu numbers now has a row except 027 and 087, both permanently
unresolvable from this manual. Per the architect's dispatch: implement as
much of items 080 onward as can be transcribed cleanly and confidently,
with special attention to applying the third sub-batch's 068/069
corroboration methodology (300 DPI re-render + sibling-pattern
cross-checking) to any ambiguous item hit in this range, rather than
skipping on first difficulty.

Landed **all 71 remaining items** (080-153, minus 087 "RADIO ID" —
permanently skipped, unresolvable — and minus 108/109 "SSB PTT/PORT
SELECT" — already landed by the first sub-batch). `EX_MENU_TABLE` grew
from 80 to 151 entries. No item was left for a further sub-batch; the
whole range transcribed cleanly in one pass after reading both `pdftotext
-layout` (PDF pages 8-10 in one pass) and a fresh 300 DPI rendered-image
read (PDF pages 9-10, column-by-column) up front, before writing any code.

**Two items needed the corroboration-methodology treatment the task
explicitly called for** (both resolved, neither skipped):
- **100 "RTTY SHIFT FREQ"**: manual prints a duplicate `1:` label (`1:
  170Hz 1:200Hz 2:425Hz 3:850Hz`), confirmed genuine (not an extraction
  artifact) via both `pdftotext` and a 300 DPI zoomed crop. Resolved to
  0-based (`0:170Hz 1:200Hz 2:425Hz 3:850Hz`) via two corroborating
  signals: the near-universal 0-based-numbering pattern for 4-value
  single-digit selectors elsewhere in this table, and the real-world
  amateur-radio convention that 170 Hz is the standard/default RTTY
  shift. Locked in by a dedicated regression test.
- **147 "DATA VOX DELAY"**: this item's own row omits the `10 msec/step`
  note that sibling item 144 "VOX DELAY" (identical quantity, MIC vs.
  DATA variant) states for the same `0030~3000` range — confirmed not a
  rendering truncation via the 300 DPI re-render. Applied `step=10` to
  147 by corroboration with 144, a documented judgment call (not provable
  from 147's own row alone), flagged for hardware/architect review.

**One further documented-gap item, same treatment as `RI`/028**: 116
"SCP SPAN FREQ" legal values are `03`-`07` only (`00`-`02` undocumented)
— transcribed exactly via `Enumerated(&["03",...,"07"])`, the first
2-digit-width `Enumerated` item in this table (needed zero
`ExMenuValueKind` code changes to support).

No new `ExMenuValueKind` variant or `EX_SET_FORMS` width was needed —
this sub-batch's digit widths (1, 2, 3, 4, 5, 8) were all already
present, including the 8-digit outlier (151 "PRESET FREQUENCY"),
confirming the first sub-batch's "sole 8-digit outlier" citation held for
the entire remaining table.

**Only `radio/src/ft991a_radio.rs` touched** — confirmed via `stat`
mtime comparison (this file's mtime is far more recent than every other
tracked `.rs` file, which cluster around an earlier bulk-checkout
timestamp). `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/` untouched. No commits made.

**Verification**: `cargo build -p radio` clean. `cargo test -p radio`:
**501 unit tests + 1 doctest, all passing** (up from 493+1 — 8 new test
functions, zero regressions; 2 existing tests updated because their
"unlanded P1" examples, 80/100 and 080, became landed by this task —
necessary updates, not incidental churn, same category the third
sub-batch's own progress notes documented for an analogous case; one
existing test renamed, `ex_menu_table_has_exactly_eighty_entries` →
`..._151_entries`, count and expected-`P1`-set assertion both updated).
`cargo clippy -p radio --all-targets -- -D warnings`: clean, no fixes
needed. `cargo fmt --check -p radio`: clean after one `cargo fmt -p
radio` pass (comment-column alignment in the new default-value-table
test only, no logic changes).

**Menu numbers implemented**: 080, 081, 082, 083, 084, 085, 086, 088,
089, 090, 091, 092, 093, 094, 095, 096, 097, 098, 099, 100, 101, 102, 103,
104, 105, 106, 107, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133,
134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147,
148, 149, 150, 151, 152, 153 (71 items).

**Menu numbers already landed, not re-touched**: 108, 109 (first
sub-batch).

**Menu numbers skipped as unresolvable**: 087 "RADIO ID" only (already
skipped by the first sub-batch's finding — literal dashes, no digit
count given anywhere in the manual; re-confirmed still unresolvable, not
re-derived from scratch). Every other item in 080-153 transcribed
cleanly (100 and 147 needed documented judgment calls via corroboration,
not skips; see above).

**Next steps**: none remaining for `EX` — `EX_MENU_TABLE` is complete
(151 of 153 possible rows; 027 and 087 are permanently unresolvable from
this manual alone, not deferred). Any future work on those two items
would require either hardware access or a different source document.
