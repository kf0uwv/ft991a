# Architecture Decision Records

Decisions are recorded as [ADRs](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
(Michael Nygard format). Each file is one decision; numbers are stable and never reused.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-second-radio-on-shared-cat-framework.md) | Second radio on the shared CAT framework | Accepted |

## Repository status

**No implementation.** This repository contains scaffolding only: ADRs,
`.claude/agents/` subagent definitions, `planning/` directories, and root
`README.md`/`CLAUDE.md`. There is no `Cargo.toml`, no crate, no CAT command
implementation, and no FT-991A manual checked in yet.

**Blocked on:**
- Extraction of `cat-framework` (and, ideally, `cat-client`/`CatSession` and
  at least one `cat-transport-*` crate) into the `radio-cat-rs` shared-library
  repository, per `ts570d` ADRs 0001, 0004, and 0005.
- An explicit architect/user go-ahead to begin implementation once the
  shared library is consumable.
- Acquisition of the official Yaesu FT-991A CAT operation reference manual
  into this repo (see `.claude/agents/yaesu.md`) — no command table work can
  start without it.

See [ADR 0001](0001-second-radio-on-shared-cat-framework.md) for the full
decision record.
