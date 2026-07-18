# Architect Progress

## 2026-07-17 — Unblocking review + first-implementation design

- Confirmed all three blockers in ADR 0001 have cleared: `radio-cat-rs` has
  published `cat-framework`/`cat-client`/`cat-transport-core`/
  `cat-transport-serial` (commit `0c13844`, consumed already by `ts570d`);
  `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` is present; user gave an
  explicit go-ahead.
- Updated status in `docs/adr/0001-second-radio-on-shared-cat-framework.md`,
  `docs/adr/README.md`, and `CLAUDE.md` (status header/body, resolved the
  `serial`-crate "OPEN DECISION" to "no local `serial` crate — depend on
  `cat-transport-serial` directly", removed stale "(once implementation
  starts)" qualifiers).
- Read `ts570d`'s post-remap reference implementation
  (`Cargo.toml`/`radio/Cargo.toml`/`src/main.rs`/`radio/src/ts570d.rs`/
  `radio/src/ts570d_radio.rs`) and the actual `radio-cat-rs` crates
  (`cat-framework/src/cat.rs`, `cat-client/src/client.rs`,
  `cat-transport-serial/src/lib.rs` + `io_uring.rs`) as ground truth.
- Read the full 20-page FT-991A CAT manual and derived an 11-command
  first-slice `FT991A_COMMAND_TABLE` scope (`FA`, `FB`, `MD`, `TX`, `SM`,
  `PS`, `AG`, `RG`, `SQ`, `PC`, `ID`), each cited to a manual page, plus an
  explicit non-goal list for follow-on waves (`IF`, `RM`/`RI`, memory
  channels, the 153-entry `EX` menu table, etc.).
- Found and documented a structural gotcha: several FT-991A read commands
  (`MD`, `SM`, `AG`, `RG`, `SQ`) take a selector parameter even on read,
  which `cat-framework`'s parser only classifies via `set_forms` (not a
  true zero-width `Query`) — the same pattern `ts570d`'s own `SM`/`MR`
  commands already use; documented the reusable fix shape.
- Designed `Ft991aState`/`Ft991aRadio`/`CatRadio` impl shape and the
  `Ft991a<S: CatSession>` controller client (reusing `ts570d`'s
  `SharedSession<S>` adapter near-verbatim) — see
  `planning/architect/task_plan.md` §3–4.
- Wrote the Wave 1 dispatch queue into `planning/architect/task_plan.md`:
  a `yaesu` task (radio crate, first slice) and an `app` task (workspace
  scaffold + minimal wiring binary against a placeholder `ui` stub).
  Explicitly deferred `ui` (needs a stable `Radio` trait first) and
  `emulator` (not needed to test Wave 1 — `CatFramework::process_frame` unit
  tests suffice, mirroring how `ts570d_radio.rs` tests itself) to later
  waves, with reasoning recorded.
- Did not write any `.rs`/`.toml` files or run `cargo init`, per the
  architect's code-editing prohibition. Did not touch `ts570d` or
  `radio-cat-rs`.
- Next: present this plan to the user for review; on approval, dispatch the
  `yaesu` and `app` tasks (one at a time per the Architect Review Workflow,
  or in parallel since their file scopes don't overlap — user's call).
