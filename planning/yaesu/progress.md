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
