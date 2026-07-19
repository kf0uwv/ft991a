# Architecture Decision Records

Decisions are recorded as [ADRs](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
(Michael Nygard format). Each file is one decision; numbers are stable and never reused.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-second-radio-on-shared-cat-framework.md) | Second radio on the shared CAT framework | Accepted |
| [0002](0002-rts-dtr-ptt-cw-keying.md) | RTS/DTR PTT and CW keying: RS-232C-only, DTR deferred | Accepted |

## Repository status

**Implementation underway (started 2026-07-17).** All three blockers
recorded in [ADR 0001](0001-second-radio-on-shared-cat-framework.md) have
cleared:

- `cat-framework`, `cat-client`, `cat-transport-core`, and
  `cat-transport-serial` are published and consumable from the
  `radio-cat-rs` shared-library repository (git dependencies, `branch =
  "main"`), per `ts570d` ADRs 0001, 0004, and 0005 and `ts570d`'s own
  post-remap `Cargo.toml`.
- The architect/user go-ahead to begin implementation has been given.
- The official Yaesu FT-991A CAT Operation Reference Manual is checked in at
  `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf`.

**Wave 1 landed** (`e3698cf`): an 11-command `radio` crate (`FA`/`FB`/`MD`/
`TX`/`SM`/`PS`/`AG`/`RG`/`SQ`/`PC`/`ID`), a real `Ft991a<S: CatSession>`
controller client, a first-slice `Radio` trait, workspace scaffold, and a
minimal `ui` placeholder stub.

**Wave 2 landed and integrated** (not yet committed): a right-sized
ratatui `ui` crate (flat single-screen control state, scoped to the 11
landed commands — deliberately not a scaled-down port of `ts570d/ui`'s
larger menu tree) and a PTY-hosted `emulator` crate mirroring
`ts570d/emulator`'s infrastructure closely. See
`planning/architect/task_plan.md` §6-§9 for the design and dispatch
reasoning, including why no new ADR was opened for the UI design (no
precedent for one in `ts570d`'s own ADR history — see
`ts570d/docs/adr/README.md`).

Task 5 (the sequential wiring/verification follow-up, §8) confirmed the two
land cleanly together: root `Cargo.toml` now lists `emulator` in both
`[workspace] members` and `[dev-dependencies]`, `src/main.rs`'s
`ui::run(radio).await` call site needed no change (the real `ui::run`
preserves the Wave 1 placeholder's `run<R: Radio + 'static>(radio: R) ->
UiResult<()>` signature exactly), and a genuine end-to-end smoke test — the
real `ft991a` binary against a `cargo run --bin emulator -- --background`
instance over a live PTY — confirmed the two processes actually interoperate:
the app's 200ms poll loop exchanged real FT-991A CAT wire traffic (`FA`/
`FB`/`MD`/`TX`/`SM`/`PS`/`AG`/`RG`/`SQ`/`PC`) with the emulator and rendered
a live TUI with no errors or panics. Full workspace build/test/clippy/fmt
all clean. This is the first point in the project's history the `ft991a`
and `emulator` binaries have run against each other.

See [ADR 0001](0001-second-radio-on-shared-cat-framework.md) for the full
decision record.
