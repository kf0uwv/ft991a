# Yaesu FT-991A — task plan (Wave 1 Task 1: `radio` crate first slice)

## Scope

Per `planning/architect/task_plan.md` §5 Task 1: deliver `radio/Cargo.toml`
+ `radio/src/*` — `Ft991aCommandId`/`FT991A_COMMAND_TABLE` (11 commands),
`Ft991aState`/`Ft991aEvent`/`Ft991aRadio` (`CatCommandCatalog`+`CatRadio`),
`Ft991a<S: CatSession>` controller client (`SharedSession<S>` copied from
`ts570d/radio/src/ts570d.rs`), a first-slice-only `Radio` trait +
`Frequency`/`Mode`/`RadioError`/`RadioResult`, and in-process unit tests via
`cat_framework::CatFramework::process_frame`.

Root `Cargo.toml`, `src/`, `ui/`, `emulator/` are explicitly out of scope
(the `app` agent's task). Since no root `Cargo.toml` exists in this repo
yet and `cargo test -p radio` needs one, a **minimal placeholder**
`[workspace]` manifest (members = ["radio"] only, plus the
`workspace.dependencies` `radio/Cargo.toml` needs) is added at the repo
root — no `[package]`/`[[bin]]`/`ui`/`emulator` members, since that's the
`app` agent's job to expand.

## Manual verification — see findings relayed in the final report

All 11 commands (Fa, Fb, Md, Tx, Sm, Ps, Ag, Rg, Sq, Pc, Id) were
individually re-verified against their own per-command tables in
`docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` (not just the p.3 master
table), reading page images directly rather than trusting OCR or the
task_plan.md summary. **Note**: PDF page number = printed footer number + 1
(unprinted cover page), so page reads must target `footer + 1`, not the
footer number itself, or the wrong page is pulled.

**DISCREPANCY FOUND AND FLAGGED, per architect instructions to trust the
manual over the summary but not silently resolve conflicts**: `AG`, `RG`,
`SQ` are NOT "selector reads" like `MD`/`SM`. Their Read row in the manual
(p.4, p.15, p.17 respectively) is genuinely zero-width (`AG;`, `RG;`,
`SQ;` — no `P1` selector digit), unlike `planning/architect/task_plan.md`
§2's transcription (`AG0;`/`RG0;`/`SQ0;`). Only their **Set** form carries
the fixed-`0` selector ahead of the 3-digit level. Full details in the
final report to the architect. **Resolution taken**: implemented the
manual-verified (corrected) forms — `query_forms = QUERY0`, single
`set_forms = SET_4`, no length-based read/write disambiguation needed for
these three (ordinary `Query`/`Set`, like `PC` but with a baked-in
selector byte in the set param string). Only `Md` and `Sm` retain the
"selector read" treatment `planning/architect/task_plan.md` describes (two
`set_forms` widths / explicit `readable`/`writable` / length-based
dispatch for `Md`; single `set_forms` width + `readable:true,writable:false`
for `Sm`).

## Corrected command table (11 commands)

| Id | Code | Query form | Set form(s) | readable/writable | Notes |
|----|------|-----------|-------------|--------------------|-------|
| Fa | FA | `FA;`→9-digit answer | `FA<9digits>;` | derived (both) | range 30,000–470,000,000 Hz |
| Fb | FB | same as Fa | same as Fa | derived | |
| Md | MD | none | `MD0;` (w1, read) + `MD0<mode>;` (w2, write) | explicit true/true | dispatch by `parameters.raw().len()`: 1=read,2=write |
| Tx | TX | `TX;`→0/1/2 answer | `TX<0/1>;` (w1) | derived | `get_tx_state()` for 3-valued answer; state models only CAT-driven TX (never emits 2) |
| Sm | SM | none | `SM0;` (w1, read only) | explicit true/false | no set at all in manual |
| Ps | PS | `PS;`→0/1 | `PS<0/1>;` (w1) | derived | wake-sequence quirk (dummy data + 1-2s delay) documented, not baked into `set_power_on` |
| Ag | AG | `AG;`→`AG0<3digits>;` | `AG0<3digits>;` (w4, selector baked in) | derived | **corrected from selector-read to plain query** |
| Rg | RG | `RG;`→`RG0<3digits>;` | `RG0<3digits>;` (w4) | derived | **corrected** |
| Sq | SQ | `SQ;`→`SQ0<3digits>;` | `SQ0<3digits>;` (w4) | derived | **corrected**, range 000-100 (not 255) |
| Pc | PC | `PC;`→3-digit answer | `PC<3digits>;` (w3, no selector) | derived | range 005-100 watts |
| Id | ID | `ID;`→4-char answer | none | derived (read-only) | fixed `"0670"`, treated as opaque string |

## `"?;"` protocol-error assumption (explicit open item)

The manual's 20 pages never state a protocol-error response format.
`write_protocol_error` uses `"?;"`, following `ts570d`'s Kenwood convention
and the general (non-manual-cited) Yaesu CAT convention. Documented as a
doc comment directly on `write_protocol_error` in `ft991a_radio.rs` and
flagged here as unverified against real hardware.

## Structure (mirrors `ts570d/radio/src/*`)

- `radio/Cargo.toml` — deps: `cat-framework`, `cat-client`,
  `cat-transport-core` (workspace git deps), `monoio`, `async-trait`,
  `thiserror`; dev-deps `tempfile`, `mockall`, `cat-transport-serial`
  (test-only).
- `radio/src/lib.rs` — module wiring + re-exports, mirrors `ts570d/radio/src/lib.rs`.
- `radio/src/ft991a_radio.rs` — `Ft991aCommandId`, `FT991A_COMMAND_TABLE`,
  `Ft991aState`, `Ft991aEvent`, `Ft991aRadio` (`CatCommandCatalog`+`CatRadio`
  impls), inline `handle_command` (no separate handlers file needed at this
  scale — 11 commands vs. `ts570d`'s ~80).
- `radio/src/radio_trait.rs` — `RadioError`/`RadioResult`, `Frequency`,
  `Mode`, first-slice-only `Radio` trait (vfo a/b, mode, ptt incl.
  `get_tx_state`, smeter, power on/off, af/rf gain, squelch, tx power, id).
- `radio/src/ft991a.rs` — `SharedSession<S>` (copied near-verbatim from
  `ts570d/radio/src/ts570d.rs` lines 65-131, only type-parameter-adjacent
  names changed), `Ft991a<S: CatSession>` wrapping `CatClient`, typed
  get/set methods, `Radio` trait impl.
- Tests: in-module `#[cfg(test)]` in `ft991a_radio.rs` (table integrity +
  `CatFramework::process_frame` round-trips for FA/MD/TX + explicit
  `MD0;`/`MD01;` selector-read-vs-write test) and in `ft991a.rs`
  (`SharedSession`/client wire-format tests using
  `cat_transport_core::test_support::ScriptedCatSession`, mirroring
  `cat-client`'s own test module).

## Verification

`cargo test -p radio`, `cargo clippy -p radio --all-targets -- -D
warnings`, `cargo fmt --check` — all run from repo root against the
placeholder workspace `Cargo.toml`.

---

# Wave 3 — CAT batch 9 (Meters/status: `IF RM RI RS MS UL`)

Per `planning/architect/task_plan.md` §10.5's batch 9 row and the
cross-batch finding above the batch table (§10.5, "`IF` (p.10), `MR`/`MT`
(p.12), and `OI` (p.13) all share the identical composite payload shape").
Dispatched early (before batch 2/the tail of batch 10) because those
batches depend on this task's `IF` field-parsing finding.

## Scope

Implement `IF RM RI RS MS UL` in `radio/src/ft991a_radio.rs` (command
table + state machine + `CatRadio::handle_command`) and
`radio/src/ft991a.rs` (controller client methods), extending
`Ft991aCommandId`/`FT991A_COMMAND_TABLE`/`Ft991aState`/`Ft991aEvent`, plus
`radio/src/radio_trait.rs` growth where the concept is generic enough
(`CLAUDE.md`'s "Radio trait scope" explicitly lists "meters" as
trait-worthy).

`radio/`-crate-only task: `ui/`, `emulator/`, `src/main.rs`, root
`Cargo.toml`, `ts570d`, `radio-cat-rs` not touched.

## Manual re-verification (the actual task, most of the effort)

Re-read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` pages 10-19 (PDF page
numbers; printed-page = PDF-page − 1, per Wave 1's documented offset),
working from the rendered page images (not extracted text), for all six
commands' own per-command Set/Read/Answer boxes — not just the p.3 master
table.

**`IF` (the highest-risk item, per Wave 1's explicit deferral)**: manual
printed p.10. Column-by-column re-verification of the Answer row (which
spans three 10-column sub-rows, terminator at column 28) resolved the
exact field layout unambiguously — no residual ambiguity was found this
time (Wave 1's deferred ambiguity was about the P2/P3 boundary and P9's
fixed segment; reading the column-numbered sub-rows directly, rather than
inferring boundaries from prose, resolves both). Settled layout (25-byte
body after the `IF` code): P1 channel(3) + P2 freq(9) + P3 clarifier
sign+offset(5) + P4 rx-clar(1) + P5 tx-clar(1) + P6 mode(1) + P7 select(1)
+ P8 tone-status(1) + P9 fixed"00"(2) + P10 offset-type(1) = 25. Factored
into `ChannelStatusFields` (`parse`/`to_wire_string`, full doc-commented
column table with citation) for reuse by batch 2 (`MR`/`MT`) and the tail
of batch 10 (`OI`), per the cross-batch finding — those tasks share the
identical trailing P1-P10 field sequence.

**`RM`/`MS` relationship (confirmed against the manual, not assumed)**:
`MS` (manual p.12) selects one of 6 physical meters (P1 0-5:
COMP/ALC/PO/SWR/ID/VDD). `RM` (manual p.15) is a selector read with 9
legal P1 values (0-8): `1`=S-meter (independent of `MS`, duplicates `SM`'s
field), `3`-`8`=direct meter select (also independent of `MS`), and `0`
and `2` **both** mean "depends on the front panel METER" — i.e. resolve
through whatever `MS` currently has selected. This is narrower than the
architect's summary implied ("RM's meaning depends on MS") — only 2 of
RM's 9 selector values actually depend on MS; the other 7 are
self-contained. Implemented as `Ft991aState::meter_reading`/
`selected_meter_reading`, tested end-to-end (`MS` set followed by `RM0`/
`RM2`/`RM6` reads) in `framework_rm_selectors_0_and_2_depend_on_current_ms_selection`.

**`RI`**: manual p.15, independent selector read, **not** related to
`MS`/`RM`. P1 legend has a documented gap: only `0`, `3`-`7`, `A` are
listed (`1`,`2`,`8`,`9`,`B`-`F` absent from the manual) — transcribed
exactly, including the gap, rather than guessed. This emulator has no CAT
command (in this or any prior batch) that drives the underlying VFO
TX/RX/DVS-REC/DVS-PLAY/Hi-SWR/TX-LED state RI reports, so all 7 legal
selectors report OFF (`0`) — documented simplification, not a manual
default.

**`RS`**: manual p.16 (not "p.15-16" as the architect's summary cited —
the detail box is entirely on p.16; the p.3 master-table row is on p.3, a
separate page, which is likely the source of the "p.15-16" range).
Zero-width query, single-bit answer (0=NORMAL MODE, 1=MENU MODE),
read-only, no CAT-reachable way to enter MENU MODE — always reports 0.

**`MS`**: manual p.12 (not "p.12-13" — detail box entirely on p.12).
Zero-width query / 1-digit set, both read and write, values 0-5.

**`UL`**: manual p.18. Zero-width query, single-bit answer (0=Lock,
1=Unlock), read-only, monitoring-only on real hardware too — always
reports Lock (0).

**Minor manual inconsistency flagged, not silently resolved**: `IF`'s own
P6 (MODE) legend labels values 3/7 "CW"/"CW-R", while `MD`'s P2 legend
(same page) labels the identical numeric values "CW-U"/"CW-L" — a
labeling mismatch between the two tables' prose, but the *numeric hex
encoding* is identical either way, so `ChannelStatusFields.mode` reuses
`Ft991aState::mode`'s existing raw-nibble representation without
incident. Documented in `ChannelStatusFields`'s doc comment, not resolved
in favor of one label.

**Judgment call, not a manual fact**: `IF`'s P1 (memory channel) is
documented as `001`-`117`; the manual does not state what appears there
when the radio is on a VFO (no memory channel selected). This
implementation accepts `000`-`117` and uses `0` as the VFO-mode sentinel —
documented as an open item in `ChannelStatusFields`'s doc comment, not
treated as the core Wave-1-flagged ambiguity (which was about column
boundaries, now fully resolved).

## Implementation notes

- Added `Ft991aRadio::from_state(state: Ft991aState) -> Self` (alongside
  the existing `new()`/`state()`) — needed because most of `IF`'s fields
  (clarifier, VFO/memory select, tone status, offset type) and all of
  `RM`'s direct-select meter fields have no `Set` command in any batch
  landed so far, so `CatFramework::process_frame`-level tests can't reach
  non-default values without a way to seed starting state directly.
  Documented as also useful for a future `emulator` crate's scripted
  starting scenarios.
- `radio_trait.rs` grew: `Meter` enum (COMP/ALC/PO/SWR/ID/VDD, matches
  `MS`'s P1 table) plus `Radio::select_meter`/`get_selected_meter`/
  `get_meter` trait methods — added to the trait (not just as `Ft991a`
  inherent methods) because `CLAUDE.md`'s "Radio trait scope" section
  explicitly names "meters" as a trait-worthy generic concept, alongside
  gain/squelch which are already there. `RadioIndicator` (`RI`'s 7-value
  selector set) was **not** added to the trait — kept a plain domain type
  used only by `Ft991a::get_radio_indicator`, since it's an FT-991A CAT
  protocol artifact (a fixed list with a documented gap), not a concept
  `CLAUDE.md`'s list names. `IF`'s `ChannelStatusFields` and `RS`/`UL`'s
  booleans were likewise kept off the trait (composite status dump /
  monitoring-only flags, not generic radio concepts) — these are judgment
  calls, flagged here for review rather than silently decided.

## Verification

- `cargo test -p radio`: **90 unit tests + 1 doctest, all passing** (up
  from Wave 1-2's 51+1 — all original tests still pass unmodified except
  the table-integrity count assertion, updated from 11 to 17, and the
  master-flags test, extended with new assertions rather than replaced).
- `cargo clippy -p radio --all-targets -- -D warnings`: clean after fixing
  5 `field_reassign_with_default` findings in new tests (switched to
  struct-update syntax).
- `cargo fmt --check -p radio`: clean after one `cargo fmt -p radio` pass.
- Not touched (per task constraints): `ui/`, `emulator/`, `src/main.rs`,
  root `Cargo.toml`, `ts570d/`, `radio-cat-rs/`. No commits made.

---

# Wave 3 — `EX` menu, second sub-batch (items 001-046, minus 027) (2026-07-19)

Per `planning/architect/task_plan.md` §10.6's sub-batching-by-menu-number-
range recommendation and §10.8 dispatch item 7 ("the other ~144 `EX`
items, sub-batched by menu-number range"). Continuation of the first `EX`
sub-batch (below) — this task's charge was "as many items in 001-079 as
can be cleanly transcribed in one session," with explicit latitude to stop
at a reasonable boundary rather than complete the full range.

## Scope decision

Read the full 153-row menu table (both `pdftotext -layout` and the
rendered page image, printed p.7-9 / PDF p.8-10) up front to see the whole
001-079 range before committing to a stopping point, rather than reading
incrementally. Landed **001-046, minus 027** (45 items) this session —
covering the manual's own printed-page-7 grouping (items 001-042, plus
043-046 which start the immediately-following AM-audio-chain group whose
047/048 members the first sub-batch already landed) — and left **049-079**
(the remainder of the "TX audio chain" theme: CW/DATA/FM PKT/SSB
LCUT/HCUT filters, out levels, PTT/port items not yet done, etc.) for the
next sub-batch. This was a judgment call, not a manual-dictated boundary —
see progress.md for the reasoning (roughly: 46 items was already a
substantial, carefully-verified batch; extending to 79 in one pass risked
rushing the value-encoding transcription the architect's brief explicitly
warned against).

`radio/src/ft991a_radio.rs` only — no other file touched. `Radio` trait
(`radio_trait.rs`) intentionally not touched, same reasoning as the first
`EX` sub-batch (menu access is FT-991A-specific, not `Radio`-trait-worthy).

## Manual re-verification (the actual task)

Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.7-8 (PDF pages
8-9) via `pdftotext -layout` first (covering the full 001-153 table in one
pass, to see the whole range before scoping), then the rendered page image
directly, column-by-column, for every row in 001-046 — both extractions
agreed on all 46 rows in this range (no OCR misread like the first
sub-batch's item 072). One misread **was** caught, immediately adjacent to
this range but not in it: `pdftotext -layout`'s first pass swapped items
068/069's Digits column (068 "DATA HCUT FREQ" shown as Digits=1, 069 "DATA
HCUT SLOPE" as Digits=2 — backwards vs. every sibling `*HCUT FREQ`/`*HCUT
SLOPE` pair, always 2/1). The page-image read confirmed the correct 2/1
order. Both items are in the *next* sub-batch's scope (049-079), not this
one — flagged here so the next sub-batch doesn't re-derive it from
scratch, and to demonstrate the cross-check caught a real error even
outside the landed range.

## Domain-modeling growth: `ExMenuValueKind` (the actual new "plumbing")

The first sub-batch's 9 items were all small named pick-lists — a flat
`legal_values: &[&str]` field sufficed. This sub-batch's first 46 manual
rows include genuine continuous numeric ranges (e.g. 001 "AGC FAST DELAY,"
0020-4000 msec in 20 msec steps — enumerating ~200 legal wire strings by
hand was not viable). Added `ExMenuValueKind` (`ft991a_radio.rs`), an enum
with two variants:

- `Enumerated(&'static [&'static str])` — the first sub-batch's original
  shape, unchanged in behavior, all 9 existing rows converted to this
  variant with zero value-semantics change.
- `Range { min, max, step, signed }` — a numeric range at a fixed step;
  `signed` marks a leading explicit `+`/`-` wire character (items 035
  "QUICK SPLIT FREQ," 039 "REF FREQ ADJ" — both in this sub-batch), needed
  because the manual explicitly allows **both** `"+00"` and `"-00"` for
  zero on these two items — parsing sign+magnitude handles both without a
  special case, but the read-back is always canonically `"+00"` (a real,
  tested behavior, not an oversight — see
  `framework_ex_signed_zero_collapses_to_canonical_plus_zero_on_read`).

`ExMenuItem.legal_values` was replaced with `kind: ExMenuValueKind`
(mechanical rename across the 9 existing rows + `handle_command`'s `Ex`
arm, no behavior change for those 9). `ExMenuItem::digits` unchanged in
meaning. This is genuinely new capability (the task brief anticipated
this: "the real cost is domain modeling... 153 items each need a
correctly-typed value/range/encoding"), not a rewrite of the selector-read
dispatch mechanism (`handle_command`'s params.len()==3-vs-write branching,
`EX_SET_FORMS`' width list) — that plumbing is untouched.

**Storage type widened `u8` → `i32`**: `Ft991aState::ex_menu_value`/
`set_ex_menu_value` now take/return `i32` (was `u8`) to hold this
sub-batch's signed values and 4-digit unsigned values (e.g. 017 "CONTEST
NUMBER," 0000-9999). The first sub-batch's 9 state fields stay `u8`
(unchanged, still hold 0-3) and are cast at the `ex_menu_value`/
`set_ex_menu_value` boundary only — no behavior change, confirmed by the
existing 9-item tests passing unmodified.

**Latent formatting bug fixed as a necessary side-effect, not scope
creep**: the first sub-batch's read-response formatting
(`format!("EX{p1_str}{value};")`) did not zero-pad — invisible with only
digit-width-1 items, but would have silently produced wrong wire output
(e.g. `"5"` instead of `"005"`) for any digit-width>1 item. Replaced with
`ExMenuValueKind::format(value, digits)`, which zero-pads correctly for
both variants and both signed/unsigned `Range`. Caught before landing any
multi-digit item, not discovered via a failing test in production.

## Value-encoding judgment calls (manual gives no explicit formula for some items)

Items without a full `X: LABEL` legend for every value (005, 008, 010,
011, 014, 015, 017, 025, 026, 035, 036, 039, 041, 043, 046) are modeled as
`Range` with `step` taken from the manual's own "N msec/step"/"N Hz
steps" wording where stated, else `1` where the manual gives only a bare
`min ~ max` — a documented assumption for those specific items, not a
manual-stated fact. Full per-item table and citations in
`EX_MENU_TABLE`'s doc comment in `ft991a_radio.rs`.

**027 "TIME ZONE" explicitly skipped, not guessed** — same treatment as
item 087 "RADIO ID." Unlike every other range-valued item in this
sub-batch, 027's P2 cell gives only the display-unit range (`UTC -12:00 ~
+14:00`) with no `"(P2 = ...)"` wire-encoding formula — every sibling
signed-range item (035, 039) does state one explicitly. Real-world UTC
offsets aren't uniformly stepped across this range either (`+05:30`,
`+05:45`, `+12:45` are real non-uniform zones), so guessing risked being
wrong in a way this manual alone cannot resolve. Left for a future
sub-batch/hardware verification, per the task's explicit instruction to
skip rather than guess.

**Default-value policy** (manual states no factory default for any item
in this table): `Enumerated` items default to their legend's first listed
value; unsigned `Range` items default to `min`; signed `Range` items (035,
039) default to `0` — chosen as the natural "no adjustment" neutral point
(itself a legal, manual-shown value), not an arbitrary endpoint. Same
category of open item as `meter_select`'s default (batch 9) and the first
`EX` sub-batch's PTT-select defaults — flagged, not silently decided.

## Tests added

Following the existing 9-item test shapes: `ex_menu_item_finds_all_second_
sub_batch_items`, `ex_menu_item_027_time_zone_is_explicitly_absent`,
`framework_ex_read_returns_default_values_for_second_sub_batch_items` (all
45 items' defaults), `framework_ex_write_round_trips_for_second_sub_batch_
valid_and_invalid_values` (data-driven, all 45 items: valid boundary
values + at least one invalid value each — satisfies both the
"round-trip" and "invalid-value rejection" requirement per item in one
parameterized test, same shape as the first sub-batch's equivalent test),
`framework_ex_signed_zero_collapses_to_canonical_plus_zero_on_read` (the
`-00`→`+00` canonicalization, new behavior this sub-batch introduces),
`framework_ex_rejects_wrong_digit_width_for_a_multi_digit_second_sub_
batch_item` (a width-mismatch case that specifically exercises a
digit-width>1 item — the first sub-batch's equivalent test only covered
width-1, since all 9 of its items were width-1), and
`framework_ex_second_sub_batch_items_do_not_clobber_first_sub_batch_or_
each_other` (cross-sub-batch isolation, not just within-sub-batch).
Existing `ex_menu_table_has_exactly_nine_entries` renamed to
`ex_menu_table_has_exactly_fifty_four_entries` and its assertion updated
(9→54); `ex_menu_item_returns_none_for_any_unlanded_p1` and
`framework_ex_out_of_table_p1_fails_cleanly_not_a_panic` had their
now-landed example `P1`s (1, 46, 001) swapped for still-unlanded ones (27,
49, 61) — both were **necessary** updates (the old examples would now
silently pass for the wrong reason, or fail outright), not incidental
churn.

## Verification

`cargo build -p radio`: clean. `cargo test -p radio`: **486 unit tests + 1
doctest, all passing** (up from 479+1 — 7 net new tests [2 existing tests
renamed/re-scoped, not counted as new], zero regressions — every prior
test name still passes, either unmodified or with only its literal `P1`
examples swapped as described above). `cargo clippy -p radio --all-targets
-- -D warnings`: clean, no fixes needed. `cargo fmt --check -p radio`:
clean, no reformatting needed. Only `radio/src/ft991a_radio.rs` touched —
confirmed by this session's own edit history (no other file opened for
writing). `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched. No commits made.

## Next sub-batch should pick up at 049

Remaining `EX` items: **049-079** (rest of the "TX audio chain" theme —
CW/DATA/FM PKT/SSB LCUT/HCUT filter pairs, out/gain levels, the DATA MODE/
PSK TONE/OTHER DISP-SHIFT items, RPT SHIFT items technically starting at
080), then **080-153** (repeater shift, ARS, DCS polarity, GM/AMS/standby,
RTTY/SSB chains, APF/contour/notch, spectrum-scope/waterfall colors,
parametric EQ ×6, TX max power, VOX family, WiRES/DG-ID), **minus 087**
(RADIO ID, permanently unresolvable from this manual). No cross-item
dependencies were found within 049-079 that would force a particular
sub-split; ordinary numeric-range sizing (e.g. "049-079" as one batch, or
split further) is a free choice for whoever picks this up next.

---

# Wave 3 — `EX` menu, first sub-batch (shared plumbing + 9 PTT/keying items)

Per `planning/architect/task_plan.md` §10.6 and its "Priority carve-out"
paragraph, and §10.8 dispatch item 3.

## Scope

`radio/src/ft991a_radio.rs` only. This task's own numbered instructions
listed only `ft991a_radio.rs` deliverables (`Ft991aCommandId::Ex`,
`CommandForm` plumbing, `EX_MENU_TABLE`, the `Ex` dispatch arm, tests) —
unlike Wave 3 batch 9's task, which explicitly listed `ft991a.rs`
(controller client methods) and `radio_trait.rs` growth as deliverables.
No `Ft991a<S>` controller client method was added for `EX`, and
`radio_trait.rs` was not touched — deliberate scope-matching decision, not
an oversight; see findings.md for the full reasoning and the "menu access
is FT-991A-specific, not `Radio`-trait-worthy" citation from `yaesu.md`
that independently supports it.

Delivered:
- `Ft991aCommandId::Ex` variant.
- `EX_SET_FORMS`: 7 `CommandForm` entries — the width-3 selector-read form
  (`EX<P1>;`) plus all 6 distinct write widths (4,5,6,7,8,11) found by
  re-transcribing the **entire** 153-row manual table's "Digits" column
  (not just this sub-batch's 9 items), confirming the architect's "~6"
  estimate exactly. Built now, ahead of need, so later `EX` sub-batches
  "just add table rows" per §10.6's own stated design.
- `ExMenuItem` struct + `EX_MENU_TABLE: &[ExMenuItem]` (exactly 9 rows) +
  `ex_menu_item(p1: u16) -> Option<&'static ExMenuItem>` lookup.
- 9 new `Ft991aState` fields (one `u8` per landed item, e.g.
  `ex_pc_keying`) + `Ft991aState::ex_menu_value`/`set_ex_menu_value`
  private helpers (same "caller validates via lookup first,
  `unreachable!()` otherwise" precondition pattern as
  `meter_reading`/`ri_status` above).
- `Ft991aRadio::handle_command`'s new `Ex` arm: dispatches by
  `params.len() == 3` (read) vs. write, looks up `P1` via `ex_menu_item`,
  validates the actual P2 width against that item's own `digits` and the
  actual P2 value against `legal_values`, only then mutates state. Any
  unregistered `P1`, or any structurally-valid-but-semantically-wrong P2
  width/value, falls through to `"?;"` — never a panic.

## Manual re-verification (the actual task)

Re-read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.7-9 (PDF
pages 8-10, per the documented "+1" offset) via **two independent
extraction methods** — the rendered page image (read directly via `Read
... pages=`, as this crate's established practice for `IF`) and
`pdftotext -layout` (used here as an explicit cross-check, new to this
task) — for: (a) `EX`'s own Set/Read/Answer box (p.7/PDF p.8), confirming
the architect's §10.6 summary exactly; (b) the full 153-row table's
"Digits" column, transcribed for **every row** (not just the 9 target
items), to determine the true count of distinct `CommandForm` widths
rather than trusting the architect's "~6" estimate; (c) the 9 target rows'
Function/P2/Digits cells individually, cross-checked against both
extraction methods.

Full citations, the digit-count cross-check that caught one image-render
OCR misread, and the genuine 048/109-vs-072/077 manual inconsistency are
in `EX_MENU_TABLE`'s doc comment in `ft991a_radio.rs` and repeated in
`findings.md` below.

## Verification

`cargo test -p radio`: **100 unit tests + 1 doctest, all passing** (up
from 90+1 — 10 new `EX` tests added, zero regressions; all 90 prior tests
pass unmodified except the table-integrity command-count assertion,
17→18). `cargo clippy -p radio --all-targets -- -D warnings`: clean, no
fixes needed. `cargo fmt --check -p radio`: clean after one `cargo fmt -p
radio` pass (line-length wrapping in new tests only, no logic changes).
Confirmed via `find -newer` that only `radio/src/ft991a_radio.rs` was
touched — `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/`, and this crate's other `.rs` files untouched. No commits
made.

---

# Wave 3 — CAT batch 2 (Memory channel records: `MC MR MW MT`)

Per `planning/architect/task_plan.md` §10.5's batch 2 row and the
cross-batch finding above the batch table. Dispatched after batch 9
(Meters/status, `IF` et al.) specifically because `MR`/`MT` share `IF`'s
composite-payload shape, per the architect's explicit ordering rationale.

## Scope

`radio/src/ft991a_radio.rs` (command table + state + `CatRadio::
handle_command`), `radio/src/ft991a.rs` (controller client methods), and
`radio/src/radio_trait.rs` (`Radio` trait growth) — all three named as
deliverables in the task prompt, matching batch 9's precedent (unlike the
`EX` first sub-batch, which was deliberately narrower). `ui/`, `emulator/`,
`src/main.rs`, root `Cargo.toml`, `ts570d`, `radio-cat-rs` not touched.

## Manual re-verification (the actual task)

Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.11-12 (PDF
pages 12-13, per the documented "+1" offset) via both `pdftotext -layout`
and the rendered page image, for `MC`'s own box (p.11) and `MR`/`MS`/`MT`/
`MW`'s own boxes (p.12) — not just the p.3 master table.

**`MC` (MEMORY CHANNEL), confirmed exactly as expected**: Set `MC<3
digits>;`, Read `MC;`, Answer `MC<3 digits>;`. P1 001-117 (001-099
regular, 100=P-1L, 101=P-1U .. 116=P-9L, 117=P-9U). Both readable and
writable (manual p.3: Set O Read O Ans O).

**`MR` (MEMORY CHANNEL READ) and `MW` (MEMORY CHANNEL WRITE), confirmed to
share `IF`'s exact P1-P10 field shape** (25-byte body, terminator at
column 28 relative to the command code) — column-by-column re-verification
against the rendered page image found **zero** field-boundary differences
from `ChannelStatusFields` (built in the batch-9 task for `IF`). Two
narrow *semantic* divergences, not shape divergences: (1) `MR`/`MW`'s P1
(channel) legend is `001-117` only — no `000` VFO-mode sentinel, unlike
`IF`. `ChannelStatusFields::parse` itself is unchanged (still accepts
`0..=117`, shared with `IF`); `handle_command`'s `Mr`/`Mw`/`Mt` arms add
their own `channel >= 1` check on top. (2) `MR`'s own P7 legend is
narrower than `IF`'s (`"0: VFO 1: Memory"` only, vs. `IF`'s full 0-6
7-value legend) — since `MR` only ever addresses an explicit memory
channel, this emulator always reports `1` (Memory), a documented judgment
call. `MW`'s P7 legend text reads `"00: (Fixed)"`, but its own column
diagram gives P7 only 1 wire column (matching `IF`/`MR`'s P7 width) — very
likely a copy-paste artifact from the adjacent P9 legend line (also `"00:
(Fixed)"`, genuinely 2 columns) — treated as fixed `"0"` per the column
diagram, not the legend prose; `handle_command`'s `Mw` arm rejects any
other P7 value. `MR` is read-only (Set=X), `MW` is write-only (Read=X,
Ans=X) — confirmed against both the master table and each command's own
box.

**`MT` (MEMORY CHANNEL WRITE/TAG), confirmed as a genuine superset, not a
duplicate**: same 25-byte P1-P10 body as `MR`/`MW`, plus **P11** (1 byte,
"0: (Fixed)", not stored — same "reserved, unvalidated" treatment
`ChannelStatusFields` already gives `IF`/`MR`/`MW`'s P9) plus **P12** (up
to 12 ASCII characters, the tag — genuinely new, no overlap with
`ChannelStatusFields` at all), for a 38-byte total body (terminator at
column 41). `MT`'s P7 legend explicitly distinguishes direction — `"Set:
0: (Fixed) / Read: 0: VFO 1: Memory"` — confirming the same fixed-on-write/
reported-as-Memory-on-read treatment applied to `MW`/`MR` above is correct
for `MT` too, not just consistent by analogy. Both readable and writable
(manual p.3: Set O Read O Ans O); its Read row is a genuine "selector
read" (`MT<3-digit channel>;`), same treatment as `Md`/`Sm`/`Rm`/`Ri`/`Ex`.

**Tag character-set/padding, an explicit judgment call, not manual-cited
on `MT`'s own page**: `MT`'s legend gives only `"TAG Characters (up to 12
characters) (ASCII)"` — no character-set restriction or padding
convention. Applied the CAT Operation section's *general* parameter rule
instead (manual p.2: "the parameter digits should be filled using any
character except the ASCII control codes (00 to 1Fh) and the terminator
(;)") as the character-set restriction (printable ASCII space through
tilde, excluding `;`), and — since the column diagram fixes P12 at exactly
12 wire columns regardless of "up to 12" content length — pad shorter tags
with trailing spaces on the wire, trimming back off on parse. Documented
in full on `MemoryChannelRecord`'s doc comment and `MemoryTag`'s doc
comment; not manual-cited beyond the general p.2 rule.

## Implementation

- `ChannelStatusFields` (from batch 9) reused **unmodified** — no changes
  needed to its struct, `parse`, or `to_wire_string`. `MR`'s answer and
  `MW`'s Set both round-trip through it directly; `MT`'s Set/Answer
  round-trips its first 25 bytes through it and handles P11/P12
  separately.
- New `MemoryChannelRecord` (emulator-internal storage, `ft991a_radio.rs`):
  holds the per-channel fields `ChannelStatusFields` doesn't cover as
  per-record state (frequency, clarifier, mode, tone status, offset type,
  tag) — deliberately excludes `channel` (the array index) and `select`
  (contextual, not stored — see above). `Ft991aState::memory_channels:
  Vec<MemoryChannelRecord>` (117 entries, index `channel - 1`) +
  `memory_channel`/`memory_channel_mut` helpers (same
  caller-validates-range-first precondition pattern as `meter_reading`/
  `ex_menu_value`).
- New `Ft991aCommandId::{Mc,Mr,Mw,Mt}` + table entries (`SET_25` for
  `MW`'s single write width, `MT_SET_FORMS` for `MT`'s two widths {3,38}).
  `is_valid_tag_wire` helper validates P12 content; used defensively via
  `.get()`-based byte-range slicing (not direct indexing/`split_at`) in
  the `Mt` write arm, to avoid a panic if stray multi-byte UTF-8 content
  ever lands off a char boundary — `CommandForm`'s width check is a byte
  length, not a char count, so this is a real (if unlikely) risk with
  direct indexing; `ChannelStatusFields::parse` already uses the same
  defensive `.get()` style, this task's new code matches it.
- Controller client (`ft991a.rs`): `get_memory_channel`/
  `set_memory_channel` (`MC`), `read_memory_channel`/`write_memory_channel`
  (`MR`/`MW`, using trait-facing `MemoryChannelEntry`),
  `read_memory_channel_tag`/`write_memory_channel_tag` (`MT`, using
  `TaggedMemoryChannel` = `MemoryChannelEntry` + `MemoryTag`). Two shared
  helpers, `memory_entry_from_fields`/`channel_status_fields_for_write`,
  convert between the wire-level `ChannelStatusFields` and the
  trait-facing `MemoryChannelEntry` (upgrading/downgrading the mode field
  between the raw nibble and the domain `Mode` type at this boundary
  only — `ChannelStatusFields` itself stays untyped, matching its
  existing `IF`-era precedent).
- `radio_trait.rs`: `MemoryChannelEntry`, `MemoryTag` (validated
  constructor + wire padding), `TaggedMemoryChannel`, plus 6 new `Radio`
  trait methods (`get_memory_channel`/`set_memory_channel`/
  `read_memory_channel`/`write_memory_channel`/`read_memory_channel_tag`/
  `write_memory_channel_tag`) — added to the trait itself (not just
  `Ft991a` inherent methods), since `CLAUDE.md`'s "Radio trait scope"
  section explicitly names "memory channels" as trait-worthy, and the
  task prompt named `radio_trait.rs` growth as in-scope. New
  `RadioError::InvalidMemoryChannel`/`InvalidMemoryTag` variants.

## Verification

`cargo test -p radio`: **130 unit tests + 1 doctest, all passing** (up
from 100+1 — 30 new tests, zero regressions; every prior test name still
present and passing unmodified except the table-integrity command count,
18→22, and the master-flags test, extended with new assertions rather than
replaced). `cargo clippy -p radio --all-targets -- -D warnings`: clean, no
fixes needed. `cargo fmt --check -p radio`: clean after one `cargo fmt -p
radio` pass (line-wrapping only, no logic changes). Confirmed via `find
-newer` that only `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs,
lib.rs}` were touched — `ui/`, `emulator/`, `src/main.rs`, root
`Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched; `Cargo.lock` unchanged.
No commits made.

---

# Wave 3 — CAT batch 1 (VFO/split/memory quick-ops: `AB BA AM VM MA CH QI
QR QS SV`)

Per `planning/architect/task_plan.md` §10.5's batch 1 row and its flagged
`VM`/`AM` manual heading inconsistency.

## Scope

`radio/src/ft991a_radio.rs` (command table + state + `CatRadio::
handle_command`), `radio/src/ft991a.rs` (controller client methods), and
`radio/src/radio_trait.rs` (`Radio` trait growth for the concepts generic
enough to belong there) — all three named as deliverables, matching batch
9/batch 2's precedent. `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d`, `radio-cat-rs` not touched.

## Manual re-verification (the actual task)

Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.4-5, p.11,
p.14-15, p.17-18 (PDF pages 5-6, 12, 15-16, 18-19, per the documented "+1"
cover-page offset) via the rendered page images directly, plus
`pdftotext -layout` to locate each command's own per-command box page
number ahead of the image reads (cross-checked against the documented
offset, all agreed).

**All ten commands confirmed as write-only triggers** (manual p.3 master
table: Set O, Read X, Ans X, AI X for every one of the ten rows) — verified
against each command's own per-command Set/Read/Answer box too, not just
the master table. **Nine of the ten are genuinely zero-width** (`A B ;`,
`B A ;`, `A M ;`, `V M ;`, `M A ;`, `Q I ;`, `Q R ;`, `Q S ;`, `S V ;` —
blank Read row, blank Answer row, no P1 column at all). `CH` is the sole
exception: `C H P1 ;`, P1 one digit, `0`=Memory Channel "UP", `1`=Memory
Channel "DOWN" (manual p.5).

This is the first use of `cat-framework`'s `CommandOperation::Action` /
`action_forms` in this crate's command table — added a new `ACTION` const
and extended the `definition!` macro with two new arities (mirroring
`ts570d`'s own `ACTION` const/macro-arm for its analogous `TX`/`RX`/`RC`/
`RU`/`RD`/`UP`/`DN` triggers) rather than trying to force these nine
commands through the existing `Query`/`Set` forms, since none of them have
a real Read/Answer row for `cat-framework`'s parser to key a `Query`
classification off of.

## `VM`/`AM` heading inconsistency — resolved via corroborating evidence,
not from the wire-format box (flagged, not silently picked)

Confirmed the architect's flag exactly: master table (printed p.3) names
`VM` `"[V/M] KEY FUNCTION"`, but `VM`'s own per-command box (printed p.18)
is headed **"VFO-A TO MEMORY CHANNEL"** — byte-for-byte identical to `AM`'s
own heading (printed p.4). Per the task brief's instruction to check
whether the wire-format boxes disambiguate the two despite the heading
clash: **they do not**. Both boxes are structurally identical — `A M ;`
and `V M ;`, zero-width Set, blank Read, blank Answer, no P1/P2 columns for
either. There is no column table to read for a definitive answer here,
unlike `IF`'s Wave-1/batch-9 ambiguity, which genuinely did resolve from
column numbers alone.

Resolved instead via three independent, corroborating signals (documented
in full in `ft991a_radio.rs`'s module doc comment and repeated in
`findings.md` below):
1. A full-manual `grep` for `[` finds **exactly one** bracketed entry in
   all 20 pages — `VM`'s own master-table name. This bracket convention
   appears nowhere else, suggesting it specifically marks "emulates a
   physical front-panel key press."
2. It would be redundant for the manual to define two byte-identical CAT
   commands with the same purpose — `AM` already unambiguously covers
   "store VFO-A into memory" via its own non-bracketed heading.
3. Well-established real-world Yaesu operating knowledge (outside this
   manual): the physical `[V/M]` key on Yaesu transceivers including the
   FT-991A toggles between VFO operation and Memory-channel (recall)
   operation — it does not store anything.

**Implemented `VM` as toggling `Ft991aState::channel_select`** (reusing
`IF`'s own P7 VFO/Memory/QMB select field from batch 9, rather than adding
new state) between `0` (VFO) and `1` (Memory); any other `channel_select`
value is treated as "not VFO" and toggled back to `0`. **This is a
documented judgment call, not a manual-proven fact** — flagged for
architect/hardware review, not silently assumed correct. `AM` itself is
implemented as the unambiguous "store VFO-A (frequency, mode, clarifier,
tone, offset) into the `MC`-selected channel" per its own clear heading.

## Other findings/judgment calls

- **State-model constraint inherited from Wave 1, not new**: `Ft991aState`
  has one `mode: u8` field, not one per VFO — `AB`/`BA`/`SV` therefore only
  copy/swap `vfo_a_hz`/`vfo_b_hz`, since there's no separate VFO-B mode.
  `AM`/`MA` copy/restore the full set (frequency, mode, clarifier, tone,
  offset) since `MemoryChannelRecord` does carry its own `mode`.
- **`CH`'s wrap-around at 1/117 is a documented judgment call** — the
  manual states no boundary behavior. Implemented as wrap (117→1 on UP,
  1→117 on DOWN), not clamp.
- **`QI`/`QR`'s dedicated QMB slot, confirmed not assumed**: `IF`'s own P7
  legend (batch 9, same manual page) already lists `3`=QMB and `4`=QMB-MT
  as select values distinct from `1`=Memory — confirming the Quick Memory
  Bank is a genuinely separate single-slot storage location, not one of
  the 117 numbered channels. Modeled as `Ft991aState::qmb:
  MemoryChannelRecord`.
- **`QS`'s toggle semantics, a documented judgment call**: no dedicated
  "split on"/"split off" command exists anywhere in the 91-command master
  table, and `QS` itself has no Read/Answer row. Modeled as a plain boolean
  toggle (`Ft991aState::split`).
- **Radio trait scope**: `copy_vfo_a_to_b`/`copy_vfo_b_to_a`/`swap_vfos`
  (`AB`/`BA`/`SV`) and `store_vfo_to_memory`/`recall_memory_to_vfo`/
  `memory_channel_up`/`memory_channel_down` (`AM`/`MA`/`CH`) were added to
  the `Radio` trait — generic dual-VFO and memory-channel concepts per
  `CLAUDE.md`'s "Radio trait scope" section, rounding out the
  `get_vfo_a`/`get_vfo_b`/memory-channel family already there. `VM`
  (residual meaning uncertainty), `QI`/`QR` (FT-991A-named "Quick Memory
  Bank," a distinct feature from the generic numbered-memory-channel
  concept already on the trait), and `QS` (FT-991A-named "Quick Split," no
  generic on/off split concept exists to hang a trait method off of) were
  deliberately kept `Ft991a`-inherent-only, not trait methods — flagged
  here as judgment calls, not silently decided.

## Verification

`cargo test -p radio`: **157 unit tests + 1 doctest, all passing** (up
from 130+1 — 27 new tests, zero regressions; every prior test name still
passes unmodified except the table-integrity command count, 22→32, and the
master-flags test, extended with a new loop over all ten batch-1 codes
rather than replaced). `cargo clippy -p radio --all-targets -- -D
warnings`: clean, no fixes needed. `cargo fmt --check -p radio`: clean
after one `cargo fmt -p radio` pass (line-wrapping only — one `if`/`else`
expression reformatted, two test call sites collapsed onto one line each,
no logic changes). Confirmed via directory scan that only
`radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs, lib.rs}` were
touched — `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched; `Cargo.lock` unchanged (no dependency changes).

## Wave 3 — CAT batch 3: clarifier/RIT-XIT + tone + IF-shift (`RT RC RD RU
XT CN CT IS`) (2026-07-18)

Per `planning/architect/task_plan.md` §10.5's batch 3 row. Scope: exactly
these 8 commands — `RT`/`RC`/`RD`/`RU`/`XT` (clarifier/RIT-XIT), `CN`/`CT`
(CTCSS/DCS tone number + mode), `IS` (IF-shift). No other CAT batch, no `EX`
items, `radio`-crate-only, matching every prior batch's constraints.

### Plan (pre-implementation)

1. Read manual pages for all 8 commands directly (page images, not just
   extracted text) — confirmed PDF page = printed footer + 1 still holds
   (master table printed p.3 = PDF page 4, re-verified against this task's
   own read of PDF page 4).
2. Resolve the two flagged ambiguities from the architect's brief *before*
   writing any command-table code:
   - The RX/TX clarifier relationship (single shared offset + two
     independent gates, or two independent offsets?) — resolve from `RT`'s
     and `XT`'s own wire-format boxes plus `IF`'s already-landed P3/P4/P5
     field layout (batch 9), not by analogy to another radio.
   - The full 50-entry CTCSS table and 104-entry DCS table (manual p.6) —
     transcribe completely from the page image, cross-check against the
     well-known industry-standard CTCSS/DCS lists as an independent
     verification pass (not a substitute for reading the manual itself).
3. Reuse `Ft991aState`'s already-landed `clarifier_offset_hz`/
   `rx_clarifier_on`/`tx_clarifier_on`/`tone_status` fields (batch 9/2,
   explicitly held back for "batch 3" per their own doc comments) rather
   than adding new ones — only `ctcss_tone_number`/`dcs_code_number`/
   `if_shift_hz` are genuinely new state.
4. Follow the established `definition!`/`CommandForm` const/
   `handle_command` width-dispatch patterns exactly (no new framework
   capability needed) — `RT`/`XT` are plain `QUERY0`/`SET_1` like `TX`/`PS`;
   `RC` is `ACTION` like the batch-1 triggers; `RD`/`RU` are explicit
   write-only `SET_4` like `MW`; `CT`/`CN`/`IS` are all "selector read"
   two-width `Set` shapes like `MD`/`EX`/`MT`.
5. `Radio` trait growth: clarifier/RIT-XIT and IF-shift are explicitly
   generic per `CLAUDE.md`'s trait-scope list ("RIT/XIT" named directly);
   CTCSS/DCS tone *selection* is generic (a `ToneSquelchMode` enum +
   Hz/code-number-typed get/set), but the specific 50/104-entry lookup
   tables stay `ft991a_radio`-internal FT-991A data, converted to/from
   domain values only at the `Ft991a` client boundary — mirrors how `Mode`
   already keeps its raw wire nibble internal to `ChannelStatusFields`
   while the trait-facing type is the domain `Mode` enum.

See `findings.md` for the manual citations and resolved discrepancies, and
below in this file's "Verification" section (this task added a second
`## Verification` heading further down, not a replacement of the batch-1
one above) for test/clippy/fmt results.

### Verification (batch 3)

`cargo test -p radio`: **225 unit tests + 1 doctest, all passing** (up from
157+1 — 68 new tests, zero regressions; every prior test name still passes
unmodified except the table-integrity command count, 32→40, and the
master-flags test, extended with new batch-3 read/write and write-only
loops rather than replaced). `cargo clippy -p radio --all-targets -- -D
warnings`: clean, no fixes needed. `cargo fmt --check -p radio`: clean
after two `cargo fmt -p radio` passes (line-wrapping only, across both the
implementation edit and the later test-addition edit — no logic changes).
Confirmed only `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs,
lib.rs}` were touched via this session's own tool-call history (not a git
repo, so no `git status` — every `Edit`/`Write` call this task made
targeted exactly these four files) — `ui/`, `emulator/`, `src/main.rs`,
root `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched; `Cargo.lock`
unchanged (no dependency changes). No commits made.
No commits made.

---

# Wave 3 — CAT batch 4 (Keyer/CW/break-in: `KM KP KR KS KY CS ZI BI SD`)

Per `planning/architect/task_plan.md` §10.5's batch 4 row. Scope: exactly
these 9 commands, `radio/`-crate-only, matching every prior batch's
constraints. Explicit instruction to keep `KY` (stored-memory playback)
conceptually and code-wise separate from the not-yet-consumed RTS/DTR
real-time CW-keying feature (§10.2-10.4) — not implemented here, not
conflated with `KY`.

## Manual re-verification (the actual task)

Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.5-6, p.10-11,
p.16, p.18 (PDF pages 6-7, 11-12, 17, 19, per the documented "+1"
cover-page offset) via both `pdftotext -layout` (used first, per page-index
`csplit` on the full extracted text, to locate each command's own
per-command box page number) and the rendered page image directly (to
column-verify each box, matching this crate's established practice) — no
discrepancies found between the two extraction methods for any of the nine
commands, and no OCR misreads like the `EX` sub-batch's item 072.

All nine wire shapes confirmed exactly as the architect's summary described
(no corrections needed against `planning/architect/task_plan.md` §10.5's
batch 4 row this time):

- **`KM` (KEYER MEMORY)**: manual p.10. Set `KM<1-digit channel 1-5><1-50
  char message>;`, Read `KM<channel>;`, Answer same shape as Set. Set O
  Read O Ans O, AI X (manual p.3). A genuine variable-width "selector
  read" — used `cat-framework`'s `CommandForm::variable(Set, 2, 51)` for
  the write form (a range) rather than 50 discrete widths, unlike `EX`'s
  handful of fixed widths.
- **`KP` (KEY PITCH)**: p.10. Set `KP<2 digits>;` (00-75, 300-1050 Hz,
  10 Hz steps), Read `KP;`, Answer same. All O.
- **`KR` (KEYER)**: p.10. Set `KR<0/1>;`, Read `KR;`, Answer same. All O.
- **`KS` (KEY SPEED)**: p.11. Set `KS<3 digits>;` (004-060 WPM), Read
  `KS;`, Answer same. All O.
- **`KY` (CW KEYING)**: p.11. Set `KY<1 char>;` only — P1 legend `1`-`5`:
  "Keyer Memory 'N' Playback", `6`-`9`,`A`: "Message Keyer 'N' Playback".
  Set O Read X Ans X, AI X (manual p.3) — write-only, confirming this is
  the stored-memory-playback trigger, not a real-time arbitrary-text
  keying command (unlike `ts570d`'s own, unrelated `KY` command, which
  sends free text directly — a same-code-different-manufacturer trap
  flagged and avoided, not copied).
- **`CS` (CW SPOT)**: p.6. Set `CS<0/1>;`, Read `CS;`, Answer same. All O.
- **`ZI` (ZERO IN)**: p.18. Set `ZI;` zero-width, "(CW AUTO ZERO IN
  Function)" — Set O Read X Ans X, AI X (manual p.3). Genuine
  `CommandOperation::Action`, same shape as batch 1's nine triggers.
- **`BI` (BREAK-IN)**: p.5. Set `BI<0/1>;`, Read `BI;`, Answer same. All O.
- **`SD` (CW BREAK-IN DELAY TIME)**: p.16. Set `SD<4 digits>;` (0030-3000
  msec), Read `SD;`, Answer same. All O.

## `KY`/RTS-DTR distinction (the task's explicit flag) — confirmed, not
just asserted

Cross-referenced `KY`'s P1 legend against the already-landed `EX` menu
items 018-022 "CW MEMORY 1"-"5" (`EX` first sub-batch, manual p.7-9), each
of which selects `0: TEXT` / `1: MESSAGE` playback mode for the *same*
numbered `KM` channel. This independently confirms `KY`'s `1`-`5`/`6`-`A`
split is two **playback modes** for one 5-channel `KM` store, not two
separate message stores — implemented as `KeyerPlaybackMode` (a tag, not a
second `Ft991aState` array). `KY` is entirely CAT-driven and only ever
addresses pre-stored `KM` content; the RTS/DTR feature (§10.2-10.4,
`ModemControlLines`, landed in `radio-cat-rs` but not yet consumed on this
repo's side) is real-time, PC-driven keying of arbitrary Morse timing over
serial control lines, with no CAT command and no pre-stored message
involved at all. Implemented and documented as two unrelated features;
`KY`'s implementation does not reference or touch anything RTS/DTR-shaped,
and no RTS/DTR consumption work was done (out of this task's scope, per
§10.8 dispatch item 6, a separate future `yaesu` task).

## `KM`'s variable-length-message design

`KM_SET_FORMS` uses `CommandForm::variable(CommandOperation::Set, 2, 51)`
for the write form (1-digit channel + 1-50 message chars) alongside a
fixed-width-1 selector-read form — reusing `cat-framework`'s existing
`CommandForm::variable` constructor (already present in the framework,
unused by this crate until now) rather than enumerating discrete widths
the way `EX_SET_FORMS` does, since `KM`'s P2 is genuinely free-length, not
one of a handful of enumerated widths. Message content validated via a
renamed, reused helper (`is_valid_tag_wire` → `is_valid_ascii_wire_content`,
call site in `MT`'s write arm updated) against the same general
p.2-parameter-rule character set `MT`'s tag already uses. **Documented
consequence, not manual-stated**: since the read form is 1 character wide,
the write form's minimum width must be ≥2 to stay structurally
distinguishable from a read — this implementation cannot write a
0-character (empty/"clear") message via `KM`.

## Verification

`cargo test -p radio`: **278 unit tests + 1 doctest, all passing** (up from
225+1 — 53 new tests, zero regressions; every prior test name still passes
unmodified except the table-integrity command count, 40→49, and the
master-flags test, extended with new batch-4 read/write and write-only
loops rather than replaced). `cargo clippy -p radio --all-targets -- -D
warnings`: clean, no fixes needed. `cargo fmt --check -p radio`: clean
after one `cargo fmt -p radio` pass (line-wrapping only, across
`ft991a_radio.rs`, `ft991a.rs`, and `radio_trait.rs` — no logic changes).
Confirmed via `find -newer` that only `radio/src/{ft991a_radio.rs,
ft991a.rs, radio_trait.rs, lib.rs}` were touched — `ui/`, `emulator/`,
`src/main.rs`, root `Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched;
`Cargo.lock` unchanged (no dependency changes). No commits made.

Judgment calls / flagged items (full detail in findings.md, none blocked
the task):
- `KY`/`ZI` mutate no persisted `Ft991aState` field — both are modeled as
  event-only acknowledgments (`CommandOutcome::events`), since neither has
  any simulated audio/RF playback or received-signal frequency in this
  emulator to represent state changes against. A documented judgment call,
  tested via the returned `CommandOutcome` rather than a follow-up read.
- Per-field arbitrary defaults for all 7 new `Ft991aState` fields (manual
  states no factory default for any of `KM`/`KP`/`KR`/`KS`/`CS`/`BI`/`SD`),
  same category of open item as `meter_select`'s default (batch 9).
- `Radio` trait scope: `get/set_break_in_on` (`BI`),
  `get/set_semi_break_in_delay` (`SD`), `get/set_cw_spot_on` (`CS`),
  `get/set_keyer_speed` (`KS`), `get/set_keyer_pitch_hz` (`KP`),
  `get/set_keyer_enabled` (`KR`), and `zero_in` (`ZI`) were added to the
  trait — generic CW-operating concepts per `CLAUDE.md`'s "Radio trait
  scope" section, directly mirroring `ts570d::Radio`'s own precedent
  (`get_keyer_speed`/`set_keyer_speed`,
  `get_semi_break_in_delay`/`set_semi_break_in_delay` already exist there
  for the analogous Kenwood concepts). `KM` (`read_keyer_memory`/
  `write_keyer_memory`) and `KY` (`play_keyer_memory`) were deliberately
  kept `Ft991a`-inherent-only, per the task's explicit instruction that the
  keyer-memory-message system is FT-991A-specific.

Not touched (per task constraints): `ui/`, `emulator/`, `src/main.rs`, root
`Cargo.toml`, `ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 5 (Scan/VOX/busy: `SC VX VD VG BY`) (2026-07-19)

Per `planning/architect/task_plan.md` §10.5's batch 5 row. Scope: exactly
these 5 commands, `radio/`-crate-only, matching every prior batch's
constraints. Explicit instruction to document — not hide — that `VD`'s
parameter meaning depends on `EX` menu item 142 "VOX SELECT" (MIC vs DATA),
per the manual's own doc note on `VD`'s box, and to explicitly not
implement menu 142 itself (only 9 `EX` items have landed, in a different
sub-batch).

### Plan (pre-implementation)

1. Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.5 (`BY`),
   p.16 (`SC`), p.17 (`VD`), p.18 (`VG`/`VX`) — PDF pages 6, 17, 18, 19 per
   the documented "+1" cover-page offset — via the rendered page image
   directly, having first located the printed page numbers with
   `pdftotext -layout` page-by-page.
2. Cross-reference `VD`'s doc note against the already-landed `EX_MENU_TABLE`
   and the full 153-row menu table's text (`pdftotext -layout`) to confirm
   menu item 142 is a real, distinct row, and note whether any adjacent
   items (143/144/146/147) suggest a wider MIC/DATA pattern extending to
   `VG` as well.
3. Follow the established `definition!`/`CommandForm` const/`handle_command`
   width-dispatch patterns exactly — all 5 commands reuse existing
   `QUERY0`/`SET_1`/`SET_3`/`SET_4`/`NONE` consts, no new `CommandForm`
   consts needed.
4. `Radio` trait growth: scan and VOX are explicitly named as trait-worthy
   per `CLAUDE.md`'s "Radio trait scope" section, and `ts570d::Radio` has a
   direct precedent for all of scan/VOX-on/VOX-gain/VOX-delay/busy
   (`get_scan`/`set_scan`, `get_vox`/`set_vox`, `get_vox_gain`/
   `set_vox_gain`, `get_vox_delay`/`set_vox_delay`, `is_busy`) — checked
   `ts570d/radio/src/radio_trait.rs` directly before designing, per this
   crate's established practice of checking `ts570d::Radio`'s surface before
   assuming trait-worthiness. `SC`'s three legal values (unlike `ts570d`'s
   plain bool `get_scan`/`set_scan`) needed a dedicated `ScanState` enum
   (mirroring `TxState`'s `TryFrom<u8>` shape) rather than a bool, since a
   bool would lose the FT-991A's genuine UP/DOWN scan direction.

### Manual re-verification (the actual task)

**`SC` (SCAN)**: manual p.16. Set `SC<1 digit>;` (`0`=OFF, `1`=ON UP-ward,
`2`=ON DOWN-ward), Read `SC;`, Answer same. All O (Set/Read/Ans) per p.3.
Unlike `TX`'s answer-only `2`, all three `SC` values are legally settable —
a plain three-way enumerated Set/Query pair, not an "answer can report more
than Set can express" situation.

**`VX` (VOX STATUS)**: manual p.18. Set `VX<0/1>;`, Read `VX;`, Answer same.
All O. Shaped exactly like `BI`/`CS`.

**`VG` (VOX GAIN)**: manual p.18. Set `VG<3 digits>;` (000-100), Read `VG;`,
Answer same. All O. Shaped exactly like `KS`'s 3-digit range (`SQ`'s
000-100 range, not `AG`/`RG`'s 000-255).

**`VD` (VOX DELAY TIME / DATA VOX DELAY TIME)**: manual p.17. Set
`VD<4 digits>;` (0030-3000 msec, 10 msec multiples), Read `VD;`, Answer
same. All O. Shaped exactly like `SD`'s 4-digit range, but with an
additional 10 msec step constraint `SD` doesn't have. **The doc note under
`VD`'s wire diagram is transcribed verbatim**: "VD command has different
parameters to be changed according to the setting of Menu item '142 VOX
SELECT'. 'MIC': VOX DELAY. 'DATA': DATA VOX DELAY." Cross-checked
`EX_MENU_TABLE`/the full 153-row table text and confirmed menu item 142
"VOX SELECT" (`0: MIC 1: DATA`, 1 digit, manual p.18/PDF p.19) is a real
row, plus found two **further** items that echo the same MIC/DATA split for
the adjacent gain/delay settings: 143 "VOX GAIN" and 144 "VOX DELAY" (the
MIC-side pair, matching `VG`/`VD`'s own ranges exactly) vs. 146 "DATA VOX
GAIN" and 147 "DATA VOX DELAY" (the DATA-side pair) — i.e. the front-panel
menu system stores MIC/DATA as **two separate settings** for gain too, not
just delay. Menu 142 (and 143/144/146/147) are explicitly **not**
implemented by this task (out of scope, a different `EX` sub-batch) — this
emulator has no CAT-reachable way to select MIC vs. DATA, so
`Ft991aState::vox_delay_ms` (and `vox_gain`) each model **one** shared
value, addressed unconditionally regardless of what menu 142 would (if
implemented) currently select. **`VG`'s own manual box carries no
equivalent doc note** (only `VD`'s does) — flagged as an observation (menu
143/146 suggest the same duality *could* apply to VOX gain) but not
implemented as an assumed dependency, since nothing on `VG`'s own page
states it — a "manual is asymmetric in what it documents" finding, not
silently smoothed over.

**`BY` (BUSY)**: manual p.5. Read-only (`Set` row in the manual has no wire
diagram, only legend text — confirmed against p.3: `Set X Read O Ans O`).
Read `BY;`, Answer `BY<P1><P2>;` (`P1`: 0/1 RX busy off/on, `P2`: fixed
`0`). No CAT command in any landed batch drives a simulated
received-signal/squelch-open condition (same gap `RI`'s status bits
already flagged, batch 9) — `Ft991aState::rx_busy` always reports `false`,
a documented simplification, not a manual-specified default.

### Implementation

- `Ft991aCommandId::{Sc,Vx,Vd,Vg,By}` + 5 new `DEFINITIONS` rows, all
  reusing existing `QUERY0`/`SET_1`/`SET_3`/`SET_4`/`NONE` consts — no new
  `CommandForm` const needed (unlike most prior batches, which added at
  least one).
- `Ft991aState::{scan_state: u8, vox_on: bool, vox_gain: u8,
  vox_delay_ms: u16, rx_busy: bool}` + 5 new `handle_command` match arms,
  following the established zero-width-query/fixed-width-set pattern
  exactly (`Sc`/`Vx`/`Vg`/`Vd` mirror `Tx`/`Bi`/`Ks`/`Sd`; `By` mirrors
  `If`/`Rs`/`Ul`'s read-only shape, with no `Set` arm at all).
- `radio_trait.rs`: new `ScanState` enum (`Off`/`Up`/`Down`,
  `TryFrom<u8>`/`as_u8`, mirroring `TxState`'s shape) + 2 new `RadioError`
  variants (`InvalidVoxGain`, `InvalidVoxDelay`) + 5 pairs of `Radio` trait
  methods (`get_scan_state`/`set_scan_state`, `get_vox_on`/`set_vox_on`,
  `get_vox_gain`/`set_vox_gain`, `get_vox_delay`/`set_vox_delay`,
  `get_rx_busy`) — all added to the trait (not `Ft991a`-inherent-only),
  directly informed by `ts570d::Radio`'s own precedent for the analogous
  Kenwood concepts (checked before deciding, same practice batch 4 used).
  `get_vox_delay`'s doc comment states the `EX` menu 142 dependency
  explicitly, drawing the parallel to `TxState::RadioKeyedNonCat`'s
  own "meaning depends on something outside this command's wire bytes"
  category of open item, per the task's explicit instruction not to hide
  it.
- `ft991a.rs`: 10 new client methods (`get_scan_state`/`set_scan_state`,
  `get_vox_on`/`set_vox_on`, `get_vox_gain`/`set_vox_gain`,
  `get_vox_delay`/`set_vox_delay`, `get_rx_busy`) + `Radio` trait impl
  delegations for all 5 pairs, `get_vox_delay`'s doc comment repeating the
  menu-142 dependency.
- `lib.rs`: `ScanState` re-export + module-doc scope update.

### Verification

`cargo test -p radio`: **302 unit tests + 1 doctest, all passing** (up from
278+1 — 24 new tests: 10 `CatFramework::process_frame` round-trip/rejection
tests in `ft991a_radio.rs` (`SC` all-three-values round trip + illegal-value
rejection, `VX`/`VG`/`VD` round trip + range/step rejection, `BY` default
read + read-only confirmation + a `from_state`-seeded busy-true read), 2
`ScanState` unit tests in `radio_trait.rs`, 12 wire-byte-level client tests
+ 1 `Radio`-trait-delegation test in `ft991a.rs` — zero regressions, every
prior test name still passes unmodified except the table-integrity command
count (49→54) and the master-flags test, extended with a new batch-5
read/write loop plus `BY`'s explicit read-only assertion, not replaced).
`cargo clippy -p radio --all-targets -- -D warnings`: clean after fixing one
`field_reassign_with_default` finding in a new test (struct-update syntax,
same fix category batch 9 needed). `cargo fmt --check -p radio`: clean
after one `cargo fmt -p radio` pass (one `if`/`match`-arm block
reformatting, no logic changes). Confirmed via `find -newer` that only
`radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs, lib.rs}` were
touched (plus this task's own `planning/yaesu/*` updates) — `ui/`,
`emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`, `radio-cat-rs/`
untouched; `Cargo.lock` unchanged (no dependency changes). No commits made.

Judgment calls / flagged items (none blocked the task):
- `VD`'s menu-142 dependency documented per the task's explicit instruction,
  including the newly-found observation that menu 143/146 suggest the same
  MIC/DATA duality could extend to `VG` too, even though `VG`'s own manual
  box states no such dependency — flagged, not assumed.
- `SC` modeled with a dedicated `ScanState` enum rather than `ts570d`'s
  plain bool `get_scan`/`set_scan`, since the FT-991A's `SC` genuinely has
  3 legal values (a deliberate shape divergence from the Kenwood precedent,
  documented on the trait method).
- Per-field arbitrary defaults (`vox_on`=false, `vox_gain`=0,
  `vox_delay_ms`=30, `scan_state`=0) — manual states no factory default for
  any of these, same category of open item as `meter_select`'s default
  (batch 9); `rx_busy`'s `false` default is a documented emulator
  simplification (no simulated RX condition exists), not an open item.

Not touched: `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`,
`ts570d/`, `radio-cat-rs/`. No commits made.

## Wave 3 — CAT batch 6 (Attenuator/preamp/noise/AGC/notch/filter-width: `RA
PA NB NL NR RL GT CO BP BC NA SH`) (2026-07-19)

Per `planning/architect/task_plan.md` §10.5's batch 6 row. Scope: exactly
these 12 commands — the largest remaining batch — `radio/`-crate-only,
matching every prior batch's constraints. Explicit instruction to transcribe
`SH`'s p.16 six-column bandwidth table (P2 00-21, SSB/CW/RTTY-PSK ×
narrow/wide) in full, not approximate or sample it.

### Plan (pre-implementation)

1. Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.4-5, p.10,
   p.13-16 (PDF pages 5-6, 11, 14-17 per the documented "+1" cover-page
   offset) via the rendered page images directly, having first located each
   command's printed page with `pdftotext -layout` (page-indexed via
   `csplit`).
2. Confirm each command's actual meaning from its own Set/Read/Answer box,
   not the abbreviation alone — per the task brief's explicit instruction to
   verify `CO`/`BP`/`BC`/`NA` precisely. Found `CO` = CONTOUR (a parametric
   audio-EQ feature + APF), not "carrier" as the 2-letter code might
   suggest.
3. Checked `ts570d::Radio`'s trait surface for `attenuator`/`preamp`/`noise
   blanker`/`noise reduction`/`agc` before designing this batch's trait
   methods (same practice batches 4/5 used) — found direct precedent for
   all five concepts, informing naming and confirming trait-worthiness,
   while diverging in method *shape* where the FT-991A's own wire format
   genuinely differs (`PreampMode` 3-valued vs. a bool, `AgcMode` 7-valued
   vs. a raw time-constant, noise reduction split into on/off + level
   instead of one combined field).
4. Follow the established `definition!`/`CommandForm` const/`handle_command`
   width-dispatch patterns exactly — all twelve commands are "selector
   read" shapes (1- or 2-char read carrying a fixed `P1=0`, wider write),
   structurally identical to batch 3's `CT`/`CN` precedent; no new
   `cat-framework` capability needed.

### Manual re-verification (the actual task)

All twelve wire shapes were individually re-verified against their own
per-command boxes (not just the p.3 master table), transcribed in full in
`ft991a_radio.rs`'s module docs' "Wire formats used (batch 6...)" section.
Two items required resolving genuine manual discrepancies rather than
transcribing at face value:

- **`GT` (AGC FUNCTION)**: Set's `P2` (5-valued: OFF/FAST/MID/SLOW/AUTO)
  and Answer's `P3` (7-valued: same four, plus AUTO-FAST/AUTO-MID/
  AUTO-SLOW) are genuinely different domains at the *same* wire width — a
  real asymmetry in the manual's own diagram, confirmed via both the
  rendered image and `pdftotext -layout`. Resolved by storing the wider
  `P3` domain in state and defining `P2=4` ("AUTO") → `P3=4` ("AUTO-FAST")
  as this implementation's own resolution — a documented judgment call, not
  provable from the manual alone (AUTO-MID/AUTO-SLOW are consequently
  unreachable via any `Set`, only seedable via `Ft991aRadio::from_state`).
- **`NA` (NARROW)**: the master table (p.3) and the per-command box's own
  heading both unambiguously name this command `NA`, correctly placed in
  the alphabetical `N`-block — but the box's own wire-diagram cells
  literally spell `M A P1 P2 ;` (not `N A P1 P2 ;`), confirmed via both
  extraction methods (not an artifact). Resolved in favor of `NA` via three
  corroborating signals (master table naming, `MA` already being a
  different landed batch-1 command, and byte-identical shape to the
  adjacent `NB` box strongly suggesting a copy-paste template error) — same
  category of resolution as batch 1's `VM`/`AM` heading clash and batch 2's
  `MW` P7 diagram-vs-legend mismatch.

`SH`'s full six-column bandwidth table (22 rows, P2 00-21 — SSB/CW/
RTTY-PSK × Narrow/Wide) was transcribed completely into
`SH_BANDWIDTH_TABLE`, cross-checked via both the rendered page image and
`pdftotext -layout` (both agreed exactly, no discrepancy this time). `SH`'s
own wire format carries no mode or narrow/wide parameter at all — which
column a given P2 applies to depends entirely on state outside `SH`'s own
command bytes, the same category of open item `VD`'s menu-142 dependency
(batch 5) and `RM`'s `MS` dependency (batch 9) already established. Modeled
as pure, non-I/O lookup functions (`mode_family_for`/`filter_bandwidth_hz`),
not wired into `handle_command`'s write-time validation.

Full citations and reasoning for all twelve commands are in
`ft991a_radio.rs`'s module docs and repeated in `findings.md` below.

### Verification

`cargo test -p radio`: **368 unit tests + 1 doctest, all passing** (up from
302+1 — 66 new tests: 34 `CatFramework::process_frame` round-trip/rejection
tests in `ft991a_radio.rs` (including `SH`'s boundary tests — first/last
valid P2 per mode family for SSB-narrow/CW-narrow/RTTY-PSK-wide, plus an
out-of-range-P2 case — and `GT`'s AUTO-resolution/wider-domain tests), 7
direct `SH_BANDWIDTH_TABLE`/`mode_family_for`/`apf_raw_to_hz` unit tests, 2
`PreampMode` + 3 `AgcMode` unit tests in `radio_trait.rs`, 20 `NopRadio`
`RadioResult::NotImplemented` assertions, and 33 wire-byte-level client
tests + 1 `Radio`-trait-delegation test in `ft991a.rs` — zero regressions,
every prior test name still passes unmodified except the table-integrity
command count (54→66) and the master-flags test, extended with a new
batch-6 loop asserting all twelve are read/write, not replaced).
`cargo clippy -p radio --all-targets -- -D warnings`: clean, no fixes
needed. `cargo fmt --check -p radio`: clean after one `cargo fmt -p radio`
pass (line-wrapping only, across `ft991a_radio.rs`/`ft991a.rs`/`lib.rs` — no
logic changes). Confirmed via this session's own tool-call history that
only `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs, lib.rs}` were
touched — `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched. No commits made.

---

# Wave 3 — CAT batch 7: speech processor/mic/monitor (`PL PR MG ML`) (2026-07-19)

Per `planning/architect/task_plan.md` §10.5's batch 7 row — the architect's
own deliberately smallest batch (4 commands), described only as a coherent
"audio chain" theme with no further per-command notes, unlike most other
batches' rows. Scope: exactly these 4 commands, `radio/`-crate-only,
matching every prior batch's constraints.

## Plan (pre-implementation)

1. Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.11 (`MG`),
   p.12 (`ML`), p.14 (`PL`/`PR`) — PDF pages 12, 13, 15 per the established
   "+1" cover-page offset, re-confirmed via `pdftotext -layout`'s own page
   footer markers before reading each page image — via the rendered page
   images directly, not extracted text alone, per this crate's established
   practice. Confirm each command's actual meaning from the manual (not the
   abbreviation alone), per the task's explicit instruction.
2. Follow established `definition!`/`CommandForm` const/`handle_command`
   width-dispatch patterns — `MG`/`PL` are plain `QUERY0`/`SET_3` like `PC`;
   `PR`/`ML` are two-width "selector read" shapes like `CT`/`NL`.
3. Check `ts570d::Radio`'s own trait surface before deciding `Radio` trait
   scope, per this crate's established practice.

## Manual re-verification (the actual task)

**`MG` (MIC GAIN)**: manual p.11. Set `MG<3-digit P1>;` (`000`-`100`), Read
`MG;` (zero-width — not a selector read), Answer `MG<3-digit P1>;`. All O
(Set/Read/Ans/AI) per p.3. Simplest of the four — no selector byte anywhere.

**`PL` (SPEECH PROCESSOR LEVEL)**: manual p.14. Set `PL<3-digit P1>;`
(`000`-`100`), Read `PL;` (zero-width), Answer `PL<3-digit P1>;`. Same shape
as `MG`. All O.

**`PR`, a genuine manual heading typo — resolved via the master table and
the command's own wire content, not followed literally**: `PR`'s own
per-command box (manual p.14, immediately below `PL`'s) is headed "SPEECH
PROCESSOR LEVEL" — byte-for-byte identical to `PL`'s own heading, confirmed
present verbatim via the rendered page image (not a `pdftotext` extraction
artifact). Same category of manual self-inconsistency as batch 1's `VM`/`AM`
heading clash and batch 6's `NA` wire-cell typo. The master table (manual
p.3) independently names this command just "SPEECH PROCESSOR" (no "LEVEL"),
and `PR`'s own wire content is unambiguous: Set `PR<P1><P2>;` (2 chars) —
`P1` selects **which** feature (`0`: Speech Processor, `1`: Parametric
Microphone Equalizer), `P2` is that feature's on/off state, with an unusual,
explicitly non-zero-based encoding (`1`: "OFF", `2`: "ON" — the only on/off
command in this crate's table so far that doesn't use `0`/`1`, transcribed
exactly, not normalized). Read is `PR<P1>;` (1 char, selector read, same
shape as `CT`/`RA`/`PA`); Answer mirrors Set. All O. Resolved as an on/off
toggle command (not a level command) via the master table's own heading plus
the wire content itself — flagged, not silently picked.

**`ML` (MONITOR LEVEL), the batch's only composite command**: manual p.12.
Set `ML<P1><P2 P2 P2>;` (4 chars) — `P1` selects which sub-value `P2`
represents (`0`: MONI "ON/OFF", `1`: MONI Level), `P2` is always 3 wire
digits regardless of `P1`, but its *meaning* depends on `P1`: `P1=0` → `P2`
is `000` (OFF) or `001` (ON); `P1=1` → `P2` is `000`-`100` (the level). Read
is `ML<P1>;` (1 char, selector read); Answer mirrors Set. All O. Same
two-width shape as `NL`/`RL`/`SH` (batch 6), but with a genuine, non-fixed
`P1` selector (unlike those three, whose `P1` is always the fixed byte
`"0"`) — closer in spirit to `CN`'s two-item `P1` selector (batch 3), just
with a fixed-width `P2` instead of `CN`'s selector-dependent lookup-table
index.

## `Radio` trait scope

Checked `ts570d::Radio` first. Direct precedent found for three of the four
concepts: `get_mic_gain`/`set_mic_gain` (u8) and
`get_speech_processor`/`set_speech_processor` (bool) — added
`get_mic_gain`/`set_mic_gain` (`MG`),
`get_speech_processor_level`/`set_speech_processor_level` (`PL`), and
`get_speech_processor_on`/`set_speech_processor_on` (`PR`'s `P1=0` branch)
to the trait. `PR`'s `P1=1` branch (Parametric Mic EQ) has no
`ts570d::Radio` precedent and no `CLAUDE.md`-listed generic concept — kept
`Ft991a`-inherent-only (`get_parametric_mic_eq_on`/
`set_parametric_mic_eq_on`), same treatment batch 6 gave `CO`/`BP`. `ML`'s
monitor on/off and level have **no** `ts570d::Radio` precedent at all (that
trait has zero "monitor" concept) — added to the trait anyway as an explicit
**judgment call**: an audio monitor is a near-universal transceiver concept,
not FT-991A-named the way CONTOUR/APF are, and `CLAUDE.md`'s "gain
controls"/"etc." language plausibly covers it — flagged for architect
review, not silently decided, since unlike the other three this one has no
direct `ts570d::Radio` method to point to.

New state: `Ft991aState::{mic_gain, speech_processor_level,
speech_processor_on, parametric_mic_eq_on, monitor_on, monitor_level}`. No
manual-stated factory default for any of the six; all use this crate's
established `0`/`false` arbitrary-default convention.

## Verification

`cargo test -p radio`: **404 unit tests + 1 doctest, all passing** (up from
368+1 — 36 new tests: 19 `CatFramework::process_frame` round-trip/rejection
tests in `ft991a_radio.rs` covering all 4 commands (including `PR`'s
independent-selector test and its illegal-encoding rejection, `ML`'s
independent-selector test and its illegal-on-off/out-of-range/illegal-
selector rejections), 17 wire-byte-level client tests + 1
`Radio`-trait-delegation test in `ft991a.rs` — zero regressions, every prior
test name still passes unmodified except the table-integrity command count,
66→70, and the master-flags test, extended with a new batch-7 loop rather
than replaced). `cargo clippy -p radio --all-targets -- -D warnings`: clean
after fixing 1 `clippy::chars_next_cmp` finding (`body.chars().next() !=
Some(x)` → `!body.starts_with(x)`) in `parse_pr_answer`. `cargo fmt --check
-p radio`: clean after one `cargo fmt -p radio` pass (one blank-line removal
before a comment block, no logic changes). Confirmed via file mtimes
(`stat`) that only `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs,
lib.rs}` were touched this session (clustered timestamps distinct from and
later than every other tracked `.rs`/`Cargo.toml` file in the repo) — `ui/`,
`emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`, `radio-cat-rs/`
untouched. No commits made.

# Wave 3 — CAT batch 8: band/step/encoder front-panel controls (`BS BU BD
FS ED EU EK DN UP`)

Per `planning/architect/task_plan.md` §10.5's batch 8 row.

## Manual re-verification

All nine wire shapes confirmed from their own per-command boxes:
`BS`/`BU`/`BD` manual p.4-5 (PDF pages 5-6), `FS` p.9 (PDF p.10),
`ED`/`EK`/`EU` p.7 (PDF p.8), `DN` p.6 (PDF p.7), `UP` p.17 (PDF p.18).
Located via `pdftotext -layout` page-by-page (`csplit` on form-feed
boundaries, confirming 20 PDF pages = 20 extracted page files) before
reading each command's own box — the established "PDF page = printed
footer + 1" offset held once more.

**`BS`'s full 16-band table, transcribed exactly, including the documented
gap at index `13`** (manual p.5):

| P1 | Band | P1 | Band | P1 | Band |
|----|------|----|------|----|------|
| `00` | 1.8 MHz | `06` | 18 MHz | `12` | MW |
| `01` | 3.5 MHz | `07` | 21 MHz | `13` | *(no entry)* |
| `02` | 5 MHz | `08` | 24.5 MHz | `14` | AIR |
| `03` | 7 MHz | `09` | 28 MHz | `15` | 144 MHz |
| `04` | 10 MHz | `10` | 50 MHz | `16` | 430 MHz |
| `05` | 14 MHz | `11` | GEN | | |

`BS` is Set-only (`B S P1 P1 ;`, 2-digit P1, no Read/Answer at all —
manual p.3: `Set O Read X Ans X`). `BU`/`BD` are also Set-only, 1-digit
`P1` documented `"0: Fixed"` (a required literal, not real data).

## `DN` heading mismatch — resolved, not silently picked

The flagged item: master table (p.3) names `DN`/`UP` plainly `"DOWN"`/
`"UP"`. `UP`'s own per-command box (p.17) agrees exactly. `DN`'s own box
(p.6) is headed **"MIC DWN"** instead — confirmed the only "MIC" +
UP/DOWN occurrence in the whole 20-page manual (full-text grep). Both
wire formats are structurally identical zero-width Action triggers (`D N
;` / `U P ;`), so the wire format alone cannot disambiguate — same
starting position as batch 1's `VM`/`AM`.

Resolved via cross-radio corroboration, per the task's instruction to
check symmetric commands/wire columns/master-table clues before guessing
from the heading text: `ts570d`'s Kenwood TS-570D CAT protocol has
**wire-identical** `UP`/`DN` commands (`ts570d/radio/src/ts570d_radio.rs`:
`definition!(Up, "UP", "Frequency Up", ...)`, `definition!(Dn, "DN",
"Frequency Down", ...)`), implemented via `ts570d::Radio::mic_up`/
`mic_down` under a `radio_trait.rs` section literally titled `"MIC
up/down (write-only momentary)"`, and `ts570d_radio_handlers.rs` steps
`vfo_a_hz` by a fixed 100 Hz per press ("Menu 02 default step"). A
different manufacturer's CAT protocol independently landing on the exact
same two 2-letter codes for the exact same "hand mic UP/DWN button"
concept is strong corroboration, not coincidence — resolved `DN`/`UP` as
mic-button commands, `DN`'s "MIC DWN" heading treated as the accurate
specific description (master table's "DOWN" is a generic label; `UP`'s
box just happens to already match the generic label). Implemented as
`Ft991a::mic_down`/`mic_up` (mirroring `ts570d`'s exact method names),
each stepping `vfo_a_hz` by a fixed `MIC_STEP_HZ` (10 Hz, a documented
arbitrary choice not tied to `FS`'s `fast_step_on`, mirroring `ts570d`'s
own unlinked design), saturating at `FA`'s `30_000..=470_000_000` Hz
range.

## `ED`/`EU`/`EK` — no simulate-able effect, documented not silently

`ED`/`EU` (manual p.7): Set `E D/U P1 P2 P2 ;` (3 total wire digits — 1
for `P1`, 2 for `P2`), no Read/Answer. `P1` selects MAIN(`0`)/SUB(`1`)/
MULTI(`8`) encoder; `P2` is `01`-`99` "Frequency Steps," with the
legend's own caveat that the actual Hz-per-step depends on what function
the encoder is currently assigned to (menu-driven state this emulator
doesn't model). Structural/range validation enforced, but neither
mutates persisted state — same "no simulate-able effect" treatment
`KY`/`ZI` (batch 4) established, and unlike `DN`/`UP`, `ts570d::Radio`
has no "encoder" concept at all to borrow a concrete effect from. `EK`
(ENT KEY, p.7) is a zero-width Action trigger, same treatment as `ZI`/
`RC`. All three kept `Ft991a`-inherent-only, per the task's own framing.

## `Radio` trait scope

`FS` (`get_fine_step`/`set_fine_step`) and `DN`/`UP` (`mic_up`/
`mic_down`) are direct `ts570d::Radio` precedent (checked
`ts570d/radio/src/radio_trait.rs` directly), added unchanged. `BS`/`BU`/
`BD` (`set_band`/`band_up`/`band_down`) have **no** `ts570d::Radio`
precedent (`ts570d` has no band concept anywhere) — added anyway per the
task's own framing ("band select/up/down are fairly generic") and this
crate's established "near-universal concept, flagged for review"
treatment (batch 5's `ScanState`, batch 7's `ML`). `set_band` has no
paired getter (`BS` has no Read/Answer wire form at all). `ED`/`EU`/`EK`
stay `Ft991a`-inherent-only (see above).

New state: `Ft991aState::{selected_band, fast_step_on}`. No manual-stated
factory default for either; `0` (1.8 MHz) and `false` are this
implementation's arbitrary choices, same category as prior batches'
undocumented defaults.

## Verification

`cargo test -p radio`: **442 unit tests + 1 doctest, all passing** (up
from 404+1 — 38 new tests: framework round-trip/boundary tests in
`ft991a_radio.rs` for all 9 commands including `BS`'s first/last-band and
documented-gap boundary tests and `BU`/`BD`'s wrap-and-skip-the-gap
tests, plus direct unit tests for `BAND_CODES`/`next_band`/`prev_band`/
`EncoderSelector`; `Band` unit tests (boundary + gap rejection + round
trip) in `radio_trait.rs`; wire-byte-level client tests + 1
`Radio`-trait-delegation test in `ft991a.rs` — zero regressions, every
prior test name still passes unmodified except the table-integrity
command count, 70→79, and the master-flags test, extended with a new
batch-8 loop rather than replaced). `cargo clippy -p radio --all-targets
-- -D warnings`: clean, no fixes needed. `cargo fmt --check -p radio`:
clean after one `cargo fmt -p radio` pass (line-wrapping only, across all
four touched files — no logic changes). Confirmed only
`radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs, lib.rs}` were
touched this session — `ui/`, `emulator/`, `src/main.rs`, root
`Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched. No commits made.

## Wave 3 — CAT batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS
(`AC AI DA DT LK OI OS FT TS MX LM PB`) (2026-07-19)

Per `planning/architect/task_plan.md` §10.5's batch 10 row — the last of
the 10 core CAT batches. Scope: exactly these 12 commands,
`radio/`-crate-only, matching every prior batch's constraints. After this
task, all 91 top-level (non-`EX`) CAT commands are implemented; the only
remaining scope is the `EX` menu's ~144 unimplemented items and the
RTS/DTR consumption wiring (§10.8).

### Plan (pre-implementation)

1. Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.4 (`AC`/`AI`),
   p.6 (`DA`/`DT`), p.9 (`FT`, cross-checked against `FA`/`FB`/`FS`), p.11
   (`LK`/`LM`), p.13 (`OI`/`OS`/`MX`), p.14 (`PB`), p.17 (`TS`, cross-checked
   against `UL`) — via `pdftotext -layout` first (to locate/confirm printed
   page numbers), then the rendered page image directly for each command's
   own box, per this crate's established practice.
2. Confirm/refute the architect's own two flagged items before writing any
   code: (a) `OI` sharing `IF`'s `ChannelStatusFields` shape exactly — did
   NOT assume from the cross-batch note alone, re-verified column-by-column
   against the image, cross-checked against `IF`'s own box and `FA`'s
   unambiguous 8+1 two-row frequency split to resolve an apparent (but
   ultimately spurious) "only 8 digits" rendering-quirk read; (b) `DT`'s
   3-shape variable-P2 design, confirmed exactly as described.
3. Follow established `definition!`/`CommandForm` const/`handle_command`
   width-dispatch patterns throughout — no new framework capability needed
   beyond `DT_SET_FORMS`' 4-width selector-read (a straightforward extension
   of `EX`'s already-landed pattern to a smaller, 3-shape case, exactly the
   "smaller rehearsal" the architect's own brief predicted).
4. `Radio` trait growth: checked `ts570d::Radio` first for each command, per
   established practice, before deciding trait-worthiness.

### Manual re-verification (the actual task)

**`TS`, a genuine architect-dispatch-prompt-vs-manual mismatch, resolved
from the manual, not guessed**: the task brief speculated "`TS`=tuning
step?". Its own per-command box (manual p.17) is unambiguously headed
"TXW", a plain boolean with no further elaboration anywhere in the 20 pages
of what "TXW" stands for. Implemented per the manual's actual wire box
(all that's needed for correctness); **not** modeled as "tuning step" —
flagged, not silently reconciled with the guess.

**`AC` (ANTENNA TUNER CONTROL)**: manual p.4. Set `AC<P1><P2><P3>;` (3
digits: `P1`/`P2` fixed "0", `P3` real: `0`=OFF `1`=ON `2`=Tuning
Start/Stop). Read `AC;` — genuinely **zero-width**, not a selector read
(confirmed via the image, unlike this crate's other fixed-`P1` commands).
Answer mirrors Set with the same `P3` label (no write/report domain split).
Kept `Ft991a`-inherent-only: both this repo's and `ts570d`'s own
`CLAUDE.md` explicitly name "antenna tuner" as a radio-specific feature,
never a `Radio`-trait concept — independently confirmed by `ts570d::Radio`
itself keeping its own antenna-tuner methods off its trait too.

**`AI` (AUTO INFORMATION)**: manual p.4. Plain bidirectional bool
(`0`/`1`), same shape as `RT`/`XT`/`CS`/`FS`. The manual's own note that
this resets to `0` on power-off is **not** enforced (consistent with this
crate's established practice of not inventing cross-command interactions
beyond what a command's own wire format states — e.g. `SH` not
cross-validating against `NA`). Direct `ts570d::Radio::set_auto_info(mode:
u8)` precedent — added to the trait as a plain bool
(`get_auto_info_on`/`set_auto_info_on`), a deliberate divergence from
`ts570d`'s 4-valued `0`-`3` domain since the FT-991A's own `AI` is only
2-valued.

**`DA` (DIMMER)**: manual p.6. Set `DA<P1 P1><P2 P2><P3 P3>;` (6 digits:
`P1`="00" fixed, `P2` "01"-"02" LED brightness, `P3` "00"-"15" TFT
brightness — all three counts re-verified against the image's column
diagram, not just the extracted legend text, since `P2`'s narrow 2-value
range looked suspicious at first; the image confirms it exactly). Read
`DA;` (zero-width). FT-991A display-hardware-specific — kept
`Ft991a`-inherent-only.

**`DT` (DATE AND TIME), the batch's rehearsal of `EX`'s variable-P2-shape
pattern, confirmed exactly as the architect's brief described**: manual
p.6. `P1` (`0`=Date, `1`=Time UTC, `2`=Time zone) selects `P2`'s shape:
8-digit `yyyymmdd`, 6-digit `hhmmss` (24hr), or 1-sign+4-digit `hhmm`
(`-12:00`..`+14:00`, 30-min steps) respectively. Read `DT<P1>;` (1 char,
selector read). Four `DT_SET_FORMS` widths (`1`,`6`,`7`,`9`), all
structurally distinguishable by length; `handle_command`'s `Dt` arm
additionally checks `P1` against the width actually used (e.g. a 6-char
frame claiming `P1="0"` is structurally legal but semantically wrong for a
date) — same "structural match succeeded, semantic validation still
per-item" pattern `EX`/`FA` established. Month/day/hour/minute/second are
range-checked, no calendar (leap-year/month-length) validation (manual
silent, no other command in this crate enforces cross-field calendar
rules either). Kept `Ft991a`-inherent-only — a system/utility clock
setting, not `CLAUDE.md`-listed and with no `ts570d::Radio` precedent
(an older HF-only rig), closer in kind to `EX`'s menu settings (also
off-trait) than to a core operating concept.

**`LK` (LOCK)**: manual p.11. Plain bidirectional bool ("VFO-A DIAL
Lock"). Direct `ts570d::Radio::get_frequency_lock`/`set_frequency_lock`
precedent — added under those same names.

**`OI` (OPPOSITE BAND INFORMATION), confirmed to share `IF`'s
`ChannelStatusFields` shape exactly — verified, not assumed from the
cross-batch note**: manual p.13. Read `OI;` (zero-width query — `OI` is
read-only, manual p.3: `Set X Read O Ans O`). The Answer's `P1`-`P10`
sequence is byte-for-byte identical in field order/width/legend to `IF`'s
own (batch 9). One apparent discrepancy was investigated and resolved
during this task, not silently accepted: `OI`'s own column diagram's first
10-column sub-row appears (at first read) to show only 4 `P2`
(VFO-B-frequency) digit cells, one fewer than the 5 needed for a clean 5+4
split of a 9-digit field. Cross-checked against `IF`'s own box (which
shows the **identical** apparent 4-cell pattern) and, independently,
against `FA`'s own unambiguous two-row 8+1 split for its 9-digit frequency
field — both signals agree the missing 5th cell is a low-resolution
page-render rendering quirk, not a genuinely narrower field; `OI`'s `P2`
is the same full 9-digit frequency `IF`'s is. Reused
[`ChannelStatusFields::to_wire_string`] directly, **zero** changes to that
struct. The **sole** documented difference from `IF`'s payload is which
frequency it reports (`vfo_b_hz`, not `vfo_a_hz` — confirmed directly from
each command's own legend text). **Judgment call, inherited from Wave 1's
single-shared-non-per-VFO-fields state model, not new**: `OI`'s
`P1`/`P3`-`P10` all read from the exact same shared state `IF` already
uses (no separate per-VFO clarifier/mode/select/tone/offset-type exists in
this emulator) — flagged for architect/hardware review. Kept
`Ft991a`-inherent-only, following `IF`'s own established off-trait
precedent (composite status dump, not a generic concept).

**`OS` (OFFSET / REPEATER SHIFT)**: manual p.13. Selector-read shape
(`OS0;` / `OS0<P2>;`), `P2`: `0`=Simplex `1`=Plus `2`=Minus. The
"*only with an FM mode*" front-panel-context caveat is not enforced,
consistent with `AI`'s similar unenforced note. **Reuses
`Ft991aState::offset_type` directly** — that field was declared back in
batch 9 with a doc comment explicitly deferring its `Set` side to "batch
10"; no new state field needed. Added to the trait
(`get_repeater_shift`/`set_repeater_shift`, new `RepeaterShift` enum) as a
documented judgment call — no `ts570d::Radio` precedent (HF-only rig, no
FM-repeater concept), added anyway as a near-universal VHF/UHF-transceiver
concept, same treatment `Band`/`ScanState`/`ML` already received.

**`FT` (FUNCTION TX), a genuine write/report domain mismatch, confirmed
via the image**: manual p.9. Set `FT<P1>;` (`P1`∈{`2`,`3`} for VFO-A/VFO-B
TX respectively); Answer `FT<P2>;` (`P2`∈{`0`,`1`} for the **identical**
two states) — a direct 1:1 remap (`2`→`0`, `3`→`1`), not a widening like
`GT`'s batch-6 finding but the same category of write/report domain split.
State stores the Answer-domain value directly; the Set arm translates.
Direct `ts570d::Radio::get_tx_vfo`/`set_tx_vfo` precedent (`ts570d`'s own
signature: `0`=VFO A, `1`=VFO B, `2`=Memory) — added under the same trait
method names, narrowed to the FT-991A's own 2-value domain (no
memory-channel-TX option for `FT`, a documented, deliberate narrowing).

**`TS`**: see above.

**`MX` (MOX SET)**: manual p.13. Plain bidirectional bool. No
`ts570d::Radio` precedent, but structurally/conceptually adjacent to the
already-trait-level PTT concept — added to the trait
(`get_mox_on`/`set_mox_on`) as a documented judgment call, same
near-universal-concept treatment as `OS`/`AI`.

**`LM` (LOAD MESSAGE / DVS RECORD) and `PB` (PLAY BACK / DVS PLAYBACK),
direct structural analog to `KM`/`KY` (batch 4), kept
`Ft991a`-inherent-only for the same reason**: manual p.11/p.14. Both
`<CMD><P1="0"><P2 0-5>;`, selector read, same two-width shape as
`CT`/`OS`. **Genuinely different `P2` semantics, transcribed exactly, not
assumed symmetric**: `LM`'s legend phrases every non-zero value as a
per-channel Recording **Start/Stop** toggle (same channel stops an
in-progress recording; a different non-zero channel starts a new one);
`PB`'s legend phrases its non-zero values as Playback **Start** only, no
toggle wording (sending `N` always (re)starts channel `N`, unconditionally
— `P2=0` stops both). Modeled as raw wire-shaped
`dvs_recording_channel`/`dvs_playback_channel` state (`0`=stopped,
`1`-`5`=active channel), `Lm`'s write arm implementing the toggle, `Pb`'s
always overwriting. No audio is simulated (same "no simulate-able effect"
category as `KY`/`ZI`/`ED`/`EU`). Kept `Ft991a`-inherent-only, directly
mirroring `KM`/`KY`'s exclusion (FT-991A-specific stored-message system,
applied here to stored audio instead of stored CW text).

### Verification

`cargo test -p radio`: **473 unit tests + 1 doctest, all passing** (up
from 442+1 — 31 new tests, zero regressions; every prior test name still
passes unmodified except the table-integrity command count, 79→91, and
the master-flags test, extended with a new batch-10 loop rather than
replaced). New tests cover: command-table integrity, all 12 commands'
`CatFramework::process_frame` round-trips, `DT`'s three P1-selected shapes
(date/time/offset) each independently round-tripped and range-rejection
tested (including a dedicated "structurally-legal-width-with-wrong-P1"
rejection test mirroring `EX`/`FA`'s established pattern), `OI`'s reuse of
`ChannelStatusFields` tested three ways (default-state string match,
cross-command diff-against-`IF`-from-index-14-onward, and a
`from_state`-seeded non-default-value match against the identical formula
already validated for `IF` in batch 9), `LM`'s toggle-vs.-`PB`'s
unconditional-start semantics each explicitly tested and contrasted, and
`FT`'s Set-domain-vs-Answer-domain translation round-tripped both
directions. `cargo clippy -p radio --all-targets -- -D warnings`: clean
after fixing 1 `clippy::if_same_then_else` finding in `LM`'s write arm
(collapsed two identical `0`-arms into one `||`-guarded condition).
`cargo fmt --check -p radio`: clean after one `cargo fmt -p radio` pass
(line-wrapping only, no logic changes). Confirmed via `find -newer` that
only `radio/src/{ft991a_radio.rs, ft991a.rs, radio_trait.rs, lib.rs}` were
touched — `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched; `Cargo.lock` unchanged (no dependency changes).
No commits made.

**Running total confirmed**: all 91 top-level (non-`EX`) CAT commands are
now implemented in `radio` — this was the last of the 10 core CAT batches
per §10.5. Only remaining scope: the `EX` menu's ~144 unimplemented items
(one sub-batch of 9 landed) and the RTS/DTR consumption wiring (§10.2-10.4
landed in `radio-cat-rs`, not yet consumed here).

---

# Wave 3 — RTS/DTR modem-control-lines consumption (§10.4) (2026-07-19)

Per `planning/architect/task_plan.md` §10.4 in full. Scope: `radio/src/
ft991a.rs` only — a `SharedSession<S: ModemControlLines>` blanket
delegation (mirroring the existing `CatSession` delegation) plus a new,
additive `impl<S> Ft991a<S> where S: CatSession<Error = TransportError> +
ModemControlLines` block with `assert_rts`/`assert_dtr`/`read_cts`/
`read_dsr`/`read_dcd`. No CAT command or `EX` menu item implemented —
purely the modem-control-line wiring `radio-cat-rs`'s new
`ModemControlLines` trait makes available.

## Plan (pre-implementation)

1. Read `cat-transport-core/src/modem.rs` and `cat-transport-serial/src/
   session.rs` directly (both in the sibling `radio-cat-rs` checkout) rather
   than trusting §10.4's sketch verbatim, per the task's own instruction.
2. Locate `radio/src/ft991a.rs`'s existing `SharedSession<S>` +
   `CatSession for SharedSession<S>` delegation (found at lines ~277-343
   pre-edit) and the main `impl<S> Ft991a<S> where S: CatSession<Error =
   TransportError>` inherent-method block (474-2457 pre-edit, closing right
   before the `Radio` trait impl) — mirror the former's take/call/put_back
   shape exactly for the new `ModemControlLines` delegation, and insert the
   new additive `Ft991a<S>` block right after the latter closes, before the
   `Radio` trait impl section, per §10.4's explicit "additive only, does not
   touch the existing block" instruction.
3. Map `ModemControlLines`'s `Result<_, TransportError>` into `RadioResult`
   via the existing `RadioError::Transport(#[from] TransportError)` variant
   (`radio_trait.rs`) — already gives a blanket `From<TransportError> for
   RadioError`, so `.map_err(Into::into)` is sufficient, matching every
   other `Ft991a` method's own error-mapping idiom in this file. No new
   `RadioError` variant needed.
4. Test via the existing `FakeTransport`/`SerialCatSession<FakeTransport>`
   wire-level test fixture already in `ft991a.rs`'s test module, rather than
   inventing a new fake session type — giving `FakeTransport` a
   `ModemControlLines` impl (via `Cell`s, mirroring `cat-transport-serial`'s
   own test-module `FakeTransport` precedent exactly) is enough, since
   `SerialCatSession<T: Transport + ModemControlLines>: ModemControlLines`
   already exists as a blanket impl upstream — no second test double
   needed.

## Real-API verification against §10.4's sketch — exact match, no shape
deviation

`cat-transport-core/src/modem.rs`'s `ModemControlLines` trait is
byte-for-byte what §10.4 sketched: `set_rts`/`set_dtr`/`read_cts`/
`read_dsr`/`read_dcd`, all `&self` (not `&mut self`), all returning
`Result<_, TransportError>`, no `#[async_trait]`. `cat-transport-serial`'s
`impl<T: Transport + ModemControlLines> ModemControlLines for
SerialCatSession<T>` blanket delegation and `SerialPort`'s own
`ModemControlLines` impl (`io_uring.rs`, using `TIOCMBIS`/`TIOCMBIC`/
`TIOCMGET`) both match §10.3's description exactly. No STOP-worthy API
mismatch was found — the implementation in this task's report is a direct,
faithful translation of §10.4's own sketch, not a reworked shape.

## BLOCKING DISCREPANCY FOUND — not an API mismatch, a commit/push gap

`radio-cat-rs`'s `ModemControlLines` work (`cat-transport-core/src/
modem.rs`, plus the `cat-transport-serial/src/{session.rs,io_uring.rs}`
changes) exists only as **uncommitted working-tree changes** in the sibling
`radio-cat-rs` checkout — confirmed via `git status` (shows `modem.rs` as
untracked, `session.rs`/`io_uring.rs`/`lib.rs` as modified-not-staged) and
`git fetch origin && git log origin/main` (top commit is still
`0c13844`, the same commit `ft991a`'s `Cargo.lock` already pins — the
extraction commit, predating any `ModemControlLines` work entirely). This
means `ft991a`'s `cat-transport-core = { git = "...", branch = "main" }`
dependency **cannot** see this code no matter how `cargo build`/`cargo
update` is invoked — there is no commit containing it to fetch. `cargo
build -p radio` against the real, unmodified dependency state fails with
`unresolved import cat_transport_core::ModemControlLines`.

This directly contradicts this task's own briefing ("the `ModemControlLines`
trait is already landed there, done") — flagged here per the task's
explicit instruction to trust the real crate source over the plan's prose,
and to STOP and report rather than force a mismatched shape. This isn't a
shape mismatch (the API matches perfectly, see above) but a **cannot-build
until `radio-cat-rs` actually commits and pushes** blocker, which sits
outside this task's constraints (`radio-cat-rs` is explicitly read-only for
this agent — committing there is not this agent's call to make).

**How this was verified without permanently violating the "don't hand-edit
root `Cargo.toml`'s `[dependencies]`" constraint**: added a temporary
`[patch."https://github.com/kf0uwv/radio-cat-rs"]` table (not a
`[dependencies]`/`[[bin]]` edit) pointing the four affected crates at the
sibling checkout's local paths, ran the full verification suite
(`cargo build -p radio --all-targets`, `cargo test -p radio`, `cargo clippy
-p radio --all-targets -- -D warnings`, `cargo fmt --check -p radio`,
`cargo build --workspace`) against that patched state — all clean, see
progress.md — then **reverted** `Cargo.toml` to a byte-identical match of
its pre-task state (`diff` confirmed) and restored `Cargo.lock` from a
pre-task backup (`diff` confirmed byte-identical). Re-ran `cargo build -p
radio --all-targets` against the reverted, real dependency state to confirm
it fails exactly as described above — this is the true, honest state of the
repo as delivered, not the patched one.

**Consequence for this task's own deliverable**: the code added to
`radio/src/ft991a.rs` is complete, correct, and fully verified (see
progress.md for the full clean test/clippy/fmt/build results obtained under
the temporary patch) — it will build and pass the moment `radio-cat-rs`
commits and pushes its already-written `ModemControlLines` work to
`origin/main`. Until then, `cargo build -p radio` on this repo's real,
unpatched dependency state fails on this new code specifically (previously
it built cleanly, since `ModemControlLines` wasn't referenced at all).
**This needs architect/coordinating-session action**: either dispatch a
task to `radio-cat-rs`'s own agents to commit+push the existing (correct,
already-verified-by-this-task) working-tree changes, or otherwise get that
commit onto `origin/main` — not something this agent can do itself per its
explicit "read-only reference" constraint on `radio-cat-rs`.

## `main.rs` zero-change confirmation (step 3) — confirmed, not just
asserted

Under the temporary local patch, `cargo build --workspace` succeeded with
zero edits to `src/main.rs`, confirming §10.4's claim: `SerialPort:
ModemControlLines` (real, `io_uring.rs`) composes with `SerialCatSession<T:
Transport + ModemControlLines>: ModemControlLines` (real, `session.rs`) to
give `Ft991a<SerialCatSession<SerialPort>>` (exactly what `main.rs`
constructs today) the new bound for free — `assert_rts`/`read_cts`/etc.
become callable with no CLI flag, no second constructor, no `main.rs` edit.
This part of §10.4's design is verified correct, independent of the
commit/push blocker above (which affects buildability against the *real*
current dependency state, not the correctness of the design itself).

## Verification

Under the temporary local `[patch]` (reverted before this task ended, see
above): `cargo build -p radio --all-targets` clean; `cargo test -p radio`
— **479 unit tests + 1 doctest, all passing** (up from 473+1 — 6 new tests,
zero regressions, no pre-existing test changed); `cargo clippy -p radio
--all-targets -- -D warnings` — clean, no fixes needed; `cargo fmt --check
-p radio` — clean after one `cargo fmt -p radio` pass (line-wrapping only
in the new tests, no logic changes); `cargo build --workspace` — clean,
confirming `main.rs` needs zero changes. Confirmed via file mtimes that
only `radio/src/ft991a.rs` was modified as this task's actual deliverable;
`Cargo.toml`/`Cargo.lock` are back to byte-identical matches of their
pre-task state (temporary patch fully reverted, `diff` confirmed both
ways); `ui/`, `emulator/`, `src/main.rs`, `radio/Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched. No commits made (not a git repo).

# Wave 3 — `EX` menu, third sub-batch (items 049-079) (2026-07-19)

Per the architect's dispatch: implement as much of `EX` menu items 049-079
as can be transcribed cleanly, with special attention to the 068/069
digit-width discrepancy the second sub-batch flagged as suspect but did not
land.

## Plan (pre-implementation)

1. Read `EX_MENU_TABLE`/`ExMenuItem`/`ExMenuValueKind`/`ex_menu_item`/
   `EX_SET_FORMS`/the `Ex` dispatch arm in `ft991a_radio.rs` — confirmed all
   working, additive-only infrastructure (no redesign needed): `EX_SET_FORMS`
   already covers every digit-width (1,2,3,4,5,8) the full 153-row table
   uses, and the `Ex` dispatch arm is fully generic over `EX_MENU_TABLE`'s
   contents — new rows need zero dispatch-code changes.
2. Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.7-8 (PDF
   pages 8-9) via `pdftotext -layout`, transcribing the full 001-153 table
   (not just 049-079) for cross-reference against sibling patterns.
3. Re-render PDF page 9 at 300 DPI (`pdftoppm -png -r 300`) and crop/zoom
   directly on the 068/069 rows to definitively resolve the digit-width
   question the second sub-batch flagged — the explicit special-attention
   item in this task's brief.
4. Add new `EX_MENU_TABLE` rows for 049-079 minus 060/071/072/076/077
   (already landed by the first sub-batch), new `Ft991aState` fields (`i32`,
   matching the second sub-batch's widened-field convention), new
   `ex_menu_value`/`set_ex_menu_value` match arms, and round-trip +
   rejection unit tests per item, following the second sub-batch's exact
   test patterns.

## 068/069 resolution — re-verification overturned the prior sub-batch's own conclusion

`pdftotext -layout` on printed p.8 shows `068 DATA HCUT FREQ` Digits=**1**
and `069 DATA HCUT SLOPE` Digits=**2**. The second sub-batch's findings
claimed "the page-image read confirmed the correct 2/1 order" without
landing the items. This task re-rendered the actual page at 300 DPI
(`pdftoppm -r 300`), cropped tightly on rows 066-072, and zoomed 2x — the
rendered image **also** shows 068=1/069=2, the same order `pdftotext`
already reported. The prior sub-batch's stated image-based resolution was
itself mistaken, not a `pdftotext`-only artifact.

However, the manual's own literal printed Digits column is **functionally
impossible** for item 068: its P2 legend is `00: OFF, 01: 700 Hz ~ 67: 4000
Hz` — representing values up to 67 requires 2 ASCII digits, which cannot
fit in a 1-digit field. Every other `*HCUT FREQ`/`*HCUT SLOPE` pair on the
same page, without a single exception (043/044 AM, 052/053 CW, 066/067's
own immediately-preceding sibling pair DATA LCUT, 092/093 and 094/095
RTTY, 102/103 and 104/105 SSB), is Digits=2/1 — FREQ always wider than
SLOPE. **Implemented as 068 Digits=2, 069 Digits=1** — the functionally-
necessary, sibling-pattern-corroborated reading, treating the manual's
printed `1`/`2` for this one row pair as a genuine typesetting error (most
likely an adjacent-row Digits-column swap), not a real protocol
difference. This is corroborated by two independent kinds of evidence
(legend arithmetic + universal sibling pattern across 6 other pairs), not
a guess — so it was implemented rather than skipped as unresolvable. Full
citation in `EX_MENU_TABLE`'s doc comment and a dedicated regression test
(`framework_ex_068_069_digit_width_resolution_matches_functionally_
necessary_2_1_order`) locks this reading in.

## Items landed vs. skipped

**Landed (26 new rows)**: 049, 050, 051, 052, 053, 054, 055, 056, 057, 058,
059, 061, 062, 063, 064, 065, 066, 067, 068, 069, 070, 073, 074, 075, 078,
079.

**Already landed, not re-added (5)**: 060 (PC KEYING), 071/072 (DATA
PTT/PORT SELECT), 076/077 (FM PKT PTT/PORT SELECT) — all fall inside
049-079 numerically but were landed by the first sub-batch; confirmed no
duplicate `EX_MENU_TABLE` rows were added for these.

**Nothing in 049-079 was skipped as unresolvable** — every item in range
transcribed cleanly (including 068/069, resolved above via corroborated
judgment call, not left unresolved).

`EX_SET_FORMS` needed zero changes — this sub-batch's digit widths (1, 2,
3, 4, 5) were all already present.

## Verification

`cargo build -p radio` clean. `cargo test -p radio`: **493 unit tests + 1
doctest, all passing** (up from 486+1 — 7 net new tests, zero regressions;
2 existing tests updated because their "unlanded P1" examples — 49/61 in
one, 049 in another — became landed by this task, same necessary-update
category the second sub-batch's own progress notes documented for an
analogous case). `cargo clippy -p radio --all-targets -- -D warnings`:
clean, no fixes needed. `cargo fmt --check -p radio`: clean after one
`cargo fmt -p radio` pass (line-wrapping only in the new tests, no logic
changes).

Confirmed via `find -newer` that only `radio/src/ft991a_radio.rs` was
touched. `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched. No commits made.

**Next sub-batch should resume at 080** ("RPT SHIFT 28MHz"). No
cross-item dependency was found that would force any particular further
split of 080-153 (minus already-skipped 087) — a free choice for whoever
picks it up.

---

# Wave 3 — `EX` menu, fourth sub-batch (items 080-153, minus 087, 108/109) (2026-07-19)

Per the architect's dispatch: implement `EX` menu items 080 onward, no
fixed upper bound — cover as much as can be transcribed cleanly and
confidently in one session, reporting exactly where the work stops. Apply
the third sub-batch's 300-DPI-re-render + sibling-pattern-corroboration
methodology to any ambiguous item hit in this range, rather than
reflexively skipping it.

## Scope

`radio/src/ft991a_radio.rs` only — same scope discipline as every prior
`EX` sub-batch. `Radio` trait (`radio_trait.rs`) intentionally not
touched (menu access is FT-991A-specific, not `Radio`-trait-worthy, same
reasoning as every prior `EX` sub-batch). `ui/`, `emulator/`,
`src/main.rs`, root `Cargo.toml`, `ts570d`, `radio-cat-rs` not touched.

## Plan (pre-implementation)

1. Read `EX_MENU_TABLE`/`ExMenuItem`/`ExMenuValueKind`/`ex_menu_item`/
   `EX_SET_FORMS`/the `Ex` dispatch arm in `ft991a_radio.rs` — confirmed
   all working, additive-only infrastructure, no redesign needed (same
   confirmation the third sub-batch made).
2. Read `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.7-9 (PDF
   pages 8-10) via `pdftotext -layout -f 8 -l 10` in one pass, transcribing
   the full 080-153 range before committing to a stopping point.
3. Re-render PDF pages 9 and 10 (printed p.8-9) at 300 DPI
   (`pdftoppm -png -r 300`), crop into top/mid/bottom sections, and read
   each directly via the `Read` tool — column-by-column, cross-checked
   against the `pdftotext` pass — for every row in 080-153, not just the
   ones expected to be ambiguous (the task's explicit "apply this rigor to
   any ambiguous item you hit" instruction, applied proactively rather
   than reactively).
4. Add new `EX_MENU_TABLE` rows for 080-153 minus 087 (skip) and 108/109
   (already landed), new `Ft991aState` fields (`i32`, matching the
   established convention), new `ex_menu_value`/`set_ex_menu_value` match
   arms, and round-trip + rejection unit tests per item, following the
   third sub-batch's exact test patterns and data-driven table shape.

## Result: the entire remaining range transcribed cleanly — no further sub-batch needed

Unlike the second/third sub-batches (each of which deliberately stopped
partway through the remaining range), this session's up-front full-range
read found nothing that forced a stopping point before 153. Two items
needed a documented judgment call via corroboration (100, 147 — see
below), one item has a documented legend gap transcribed as-is (116), and
one item remains permanently skipped (087, already known unresolvable) —
none of these required stopping the batch early. `EX_MENU_TABLE` now has
151 entries (80 prior + 71 new), covering every one of the 153 manual
menu numbers except 027 and 087.

## 100 "RTTY SHIFT FREQ" — genuine manual typo, resolved via corroboration

The manual's own printed P2 legend reads `1: 170 Hz  1: 200 Hz  2: 425
Hz  3: 850 Hz` — a duplicate `1:` label. Confirmed identical on both
`pdftotext -layout` and a tight 300 DPI crop zoomed 2x directly on the
row (not a rendering/OCR artifact — the manual is printed this way).
Resolved to 0-based (`0:170Hz 1:200Hz 2:425Hz 3:850Hz`, the first printed
`1:` treated as a typesetting error for `0:`) via two independent
corroborating signals: (1) every other 4-value single-digit selector in
the full 153-row table, without exception, is 0-based — the only
documented exceptions are 2-value "PORT SELECT"-family fields (072, 077,
and this table's own 101 "RTTY MARK FREQ"), a narrower, already-documented
pattern that does not extend to a 4-value field; (2) 170 Hz is the
well-established real-world default/standard amateur-radio RTTY shift,
matching this table's convention of `0` as the neutral/default option
elsewhere. Implemented as `Enumerated(&["0", "1", "2", "3"])`, locked in
by `framework_ex_100_rtty_shift_freq_typo_resolution_is_zero_based`.

## 147 "DATA VOX DELAY" — step assumption via sibling corroboration, a documented judgment call

147's own P2 cell reads `30 ~ 3000 msec (P2 = 0030 ~ 3000)` — no step
note, confirmed not a column-truncation artifact via the 300 DPI
re-render (full row text visible, genuinely omits the note). Sibling item
144 "VOX DELAY" (identical quantity, MIC vs. DATA variant — a duality
batch 5's `VD` finding already ties to menu items 144/147 by manual
citation) states `10 msec/step` for the same range. Applied `step=10` to
147 by corroboration with 144 — a documented judgment call, not provable
from 147's own row alone, flagged for hardware/architect review.

## 116 "SCP SPAN FREQ" — documented gap, transcribed exactly

Legal P2 values are `03`-`07` only (`00`-`02` absent from the manual's
own legend) — same treatment as `RI`'s selector gap (batch 9) and item
028's gap (second sub-batch). Modeled as
`Enumerated(&["03", "04", "05", "06", "07"])`, the first 2-digit-width
`Enumerated` item in this table — needed zero `ExMenuValueKind` code
changes.

## Parametric-EQ sextet (119-136)

6 structurally-identical FREQ/LEVEL/BWTH triples. FREQ items are
contiguous-from-`00` named-frequency-point selectors, modeled as unsigned
`Range` over the raw wire integers (same convention as `*LCUT`/`*HCUT
FREQ` items). LEVEL items are signed `-20..=+10`. BWTH items are unsigned
`01..=10`. The largest uniform block landed in one sub-batch so far;
needed zero new `ExMenuValueKind` capability.

## Implementation

`EX_SET_FORMS` needed zero changes — all digit widths this sub-batch uses
(1, 2, 3, 4, 5, 8) were already present, including the 8-digit outlier
(151 "PRESET FREQUENCY"), confirming no second 8-digit item exists
anywhere in the table. 71 new `Ft991aState` fields, `ex_menu_value`/
`set_ex_menu_value` match arms, and `EX_MENU_TABLE` rows added, following
the established per-field doc-comment and default-value-policy
conventions (`Enumerated`→first legend value, unsigned `Range`→`min`,
signed `Range`→`0`) exactly.

Two pre-existing tests needed updating (necessary churn, not incidental):
`ex_menu_item_returns_none_for_any_unlanded_p1`'s examples (80, 100)
became landed, reduced to just the two permanently-skipped numbers (27,
87) plus a bogus one (999); `framework_ex_out_of_table_p1_fails_cleanly_
not_a_panic`'s concrete example (EX080) became landed, swapped to EX027.
`ex_menu_table_has_exactly_eighty_entries` renamed to `..._151_entries`,
count and full expected-`P1`-set assertion both updated.

## Verification

`cargo build -p radio`: clean. `cargo test -p radio`: **501 unit tests +
1 doctest, all passing** (up from 493+1 — 8 new test functions, zero
regressions). `cargo clippy -p radio --all-targets -- -D warnings`:
clean, no fixes needed. `cargo fmt --check -p radio`: clean after one
`cargo fmt -p radio` pass (comment-column alignment only, no logic
changes).

Confirmed via `stat` mtime comparison (not a git repo) that only
`radio/src/ft991a_radio.rs` was touched — its mtime is far more recent
than every other tracked `.rs` file, which cluster around an earlier
bulk-checkout timestamp. `ui/`, `emulator/`, `src/main.rs`, root
`Cargo.toml`, `ts570d/`, `radio-cat-rs/` untouched. No commits made.

**This completes `EX_MENU_TABLE`.** No further `EX` sub-batch is needed —
151 of the 153 possible menu numbers are landed; 027 ("TIME ZONE") and
087 ("RADIO ID") are permanently unresolvable from this manual alone (no
wire-encoding formula stated for either), not deferred work.

---

# Wave 4 — Task 1: `Ft991aExtras`/`CwKeying` traits + `EX` label extension (2026-07-19)

Per `planning/architect/task_plan.md` §11.3 point 3 (trait design/bounds),
§11.3 point 4 (why this doesn't violate `CLAUDE.md`'s dependency rules),
§11.4 first two paragraphs (`get`/`set_ex_menu_item` +
`ExMenuValueKind::Enumerated` label extension), and §11.6 dispatch item 1
("the `radio`-crate prerequisite... blocks everything else"). Read all
three sections in full before starting, plus `radio/src/ft991a.rs` (to
find the actual complete inherent-only method set — the architect's "~20"
was explicitly flagged as an approximation not to be trusted), and
`radio/src/radio_trait.rs` (the `Radio` trait's default-body idiom to
mirror exactly).

## Scope

`radio/` crate only: `radio/src/{ft991a.rs, ft991a_radio.rs, radio_trait.rs,
lib.rs}`. Per task constraints, `ui/`, `emulator/`, `src/main.rs`, root
`Cargo.toml`, `ts570d`, `radio-cat-rs` not touched (confirmed via `git
status` at the end — only these four files are modified; `emulator/src/
tui.rs` and `planning/emulator/*` showed up as modified too, but that is a
different, concurrently-running agent's work, not this task's — this
task's own tool-call history never opened those files).

## Step 1: verify the actual inherent-only method count myself

Grepped every `pub async fn`/`pub fn` in both `impl<S> Ft991a<S> where S:
CatSession<Error = TransportError>>` blocks (the main one, lines ~522-2505
pre-edit, and the small `IF`-adjacent methods) and cross-referenced
against every method already on the `Radio` trait (`radio_trait.rs`).
Result: **48** inherent-only methods qualify for `Ft991aExtras` (not the
architect's approximate "~20") — the discrepancy is explained by the
architect's summary naming *categories* ("keyer memory, QMB, encoder
nudges, antenna tuner, dimmer, date/time, DVS, contour/APF/manual-notch,
`IF` composite") rather than counting every method within each category
(e.g. "date/time" alone is 6 methods: `read_date`/`write_date`/
`read_time`/`write_time`/`read_time_zone_offset`/`write_time_zone_offset`;
"DVS" is 6: record channel query + start/stop, playback channel query +
start/stop). Plus the 5 already-landed `ModemControlLines`-bound methods
(`assert_rts`/`assert_dtr`/`read_cts`/`read_dsr`/`read_dcd`) for
`CwKeying`, and the 2 genuinely new `get_ex_menu_item`/`set_ex_menu_item`
methods for `Ft991aExtras` — for a final count of **50 methods on
`Ft991aExtras`, 5 on `CwKeying`**. `flush_rx` was confirmed to already be
on the `Radio` trait itself (a sync default-no-op method, easy to miss
with an `async fn`-only grep) — correctly excluded from both new traits.

## Step 2: `ExMenuValueKind::Enumerated` label extension — done first, mechanically

Extended `Enumerated(&'static [&'static str])` to `Enumerated(&'static
[(&'static str, &'static str)])` — `(wire, label)` pairs, per the
architect's own suggested shape. Rather than hand-transcribing labels for
79 `Enumerated` `EX_MENU_TABLE` rows by eye (error-prone at this scale),
wrote a small Python script (scratchpad-only, not committed) that:
1. Parsed the live `EX_MENU_TABLE` array (151 `ExMenuItem` entries: 79
   `Enumerated`, 72 `Range` — confirmed by parsing, not assumed) to get
   each item's exact current wire-value list.
2. Parsed `EX_MENU_TABLE`'s own doc-comment legend tables (the `| P1 |
   Name | P2 legal values | Digits |` markdown rows already in the file,
   covering all four `EX` sub-batches) to get each item's manual-cited
   label legend.
3. Cross-referenced by P1, resolving two special cases already flagged in
   the doc comments: items 018-022 ("CW MEMORY 1-5") share one combined
   doc row; items 031/032 ("CAT RATE"/"CAT TOT") are documented as "same
   shape as" 029/030 and resolve through those rows' legends.
4. Asserted every resolved item's wire-value list matched the array's own
   list exactly (order and content) before writing anything back —
   **zero mismatches across all 79 items**, confirming every label traces
   to this file's own pre-existing manual-citation doc comments, per the
   task's explicit instruction not to guess or re-read the manual for
   already-documented items.

Applied via 79 in-place regex substitutions (verified against the exact
match count first), then `cargo build`/`cargo fmt -p radio` to confirm
compilation and let `rustfmt` reflow the now-longer array literals (some,
e.g. item 118 "WATER FALL COLOR" with 8 label pairs, wrap across multiple
lines). `ExMenuValueKind::parse` updated (`values.contains(&wire)` →
`values.iter().any(|(w, _)| *w == wire)`); `ExMenuValueKind::format`
unchanged (formats an already-validated integer, doesn't touch labels).
Added `ExMenuValueKind::label_for_value` as a small forward-looking
convenience (not consumed anywhere in this crate yet — flagged for a
future `ui` `ListSelect` consumer per §11.4). `parse`/`format` widened
from private to `pub(crate)` so `ft991a.rs`'s new
`get_ex_menu_item`/`set_ex_menu_item` can reuse them directly rather than
reimplementing menu-value parsing client-side, per the task's explicit
"reuse the existing table/logic" instruction.

## Step 3: traits + impls + `get`/`set_ex_menu_item`

- `Ft991aExtras` (async, `#[async_trait(?Send)]`, `radio_trait.rs`): 50
  methods (48 re-export forwards + 2 new), every default body `Err(
  RadioError::NotImplemented)`, identical idiom to `Radio`. `impl<S:
  CatSession<Error = TransportError>> Ft991aExtras for Ft991a<S>`
  (`ft991a.rs`) — every re-export method is a one-line forward to the
  pre-existing inherent method of the same name; `get_ex_menu_item`/
  `set_ex_menu_item` are new inherent methods (added to the main impl
  block) that look up `ex_menu_item(p1)` first (returning the new
  `RadioError::UnknownExMenuItem(u16)` variant if absent — nothing
  reusable fit, so a variant was added, mirroring every other
  `RadioError::Invalid*`/`Unknown*` variant's shape), then round-trip
  through the now-`pub(crate)` `ExMenuValueKind::parse`/`format`.
  `set_ex_menu_item` validates client-side (format then re-parse,
  matching the value) *before* sending, so an illegal value surfaces as
  `RadioError::InvalidProtocolString` attributable to the call rather
  than a generic rejected-wire-frame error.
- `CwKeying` (sync `&self`, plain `fn`s — matching
  `ModemControlLines`/the existing Wave-3 `assert_rts`-etc. precedent, NOT
  async): 5 thin forwarding methods. `impl<S: CatSession<Error =
  TransportError> + ModemControlLines> CwKeying for Ft991a<S>` — the
  *same* bound the existing (Wave 3, §10.4) inherent-method impl block
  already uses.
- **Coherence verified by actually compiling, not just trusting the
  architect's claim**: `cargo build -p radio` after adding both impl
  blocks succeeded on the first attempt with zero `E0119`
  overlapping-impl errors — confirming `Ft991aExtras`'s unconditional `S:
  CatSession` bound and `CwKeying`'s narrower `S: CatSession +
  ModemControlLines` bound genuinely don't collide, exactly as §11.3
  point 3/4 reasoned through (unlike the ruled-out alternative of folding
  these methods directly into `Radio`, which *would* need two
  overlapping `impl Radio for Ft991a<S>` blocks).
- `Ft991aExtras`/`CwKeying` re-exported from `lib.rs`'s crate root
  (needed — `ft991a.rs`'s `impl<S> crate::Ft991aExtras for ...` doesn't
  resolve otherwise; caught by the first `cargo build` attempt).
- Added `impl Ft991aExtras for NopRadio {}` / `impl CwKeying for NopRadio
  {}` (both trivial, inheriting the traits' own `NotImplemented`
  defaults) so this crate's existing `NopRadio` stays usable against the
  widened `ui::run<R: Radio + Ft991aExtras + CwKeying>` bound §11.3
  anticipates — not required by the task's own instructions, but cheap
  and keeps `NopRadio` from silently becoming stale, same category of
  mechanical addition the architect flagged for a future `ui::MockRadio`.

## Verification

`cargo build -p radio`: clean (confirms coherence, see above). `cargo test
-p radio`: **517 unit tests + 1 doctest, all passing** (up from 501+1 — 16
new tests, zero regressions: all 501 prior tests pass completely
unmodified, since the `Enumerated` label extension only changed the
enum's associated data, not its match-ability, and no existing test
constructed an `Enumerated(&[...])` literal directly — confirmed by grep
before editing). `cargo clippy -p radio --all-targets -- -D warnings`:
clean, no fixes needed. `cargo fmt --check -p radio`: clean after one
`cargo fmt -p radio` pass (line-wrapping of the newly-long `Enumerated`
tuple arrays and one over-long `play_keyer_memory` forwarding signature,
no logic changes). Confirmed via `git status`/`git diff --stat` that only
`radio/src/{ft991a.rs, ft991a_radio.rs, lib.rs, radio_trait.rs}` were
modified by this task (1188 insertions, 99 deletions across the four
files) — `ui/`, `emulator/`, `src/main.rs`, root `Cargo.toml`, `ts570d/`,
`radio-cat-rs/` untouched. No commits made.
