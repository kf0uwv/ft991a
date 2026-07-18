# Architecture Decision Records

Decisions are recorded as [ADRs](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
(Michael Nygard format). Each file is one decision; numbers are stable and never reused.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-second-radio-on-shared-cat-framework.md) | Second radio on the shared CAT framework | Accepted |

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

There is still no `Cargo.toml` or crate checked in as of this planning
update — see `planning/architect/task_plan.md` for the workspace design and
dispatch queue that will produce it.

See [ADR 0001](0001-second-radio-on-shared-cat-framework.md) for the full
decision record.
