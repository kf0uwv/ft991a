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
