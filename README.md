# Yaesu FT-991A Radio Control

Terminal-based CAT control for the Yaesu FT-991A HF/VHF/UHF transceiver. This is
the **second** radio built on top of a shared, radio-independent CAT
(Computer Aided Transceiver) engine — the first being
[`ts570d`](https://github.com/kf0uwv/ts570d) (Kenwood TS-570D/S).

## Status: scaffolding only — no implementation yet

This repository currently contains **no Rust code**. There is no `Cargo.toml`,
no crates, and no CAT command implementation. What exists is planning and
agent scaffolding: ADRs recording the intended architecture, subagent
definitions adapted from `ts570d`'s working discipline, and per-agent
planning directories — so that once the shared library is ready,
implementation can begin immediately with the same conventions `ts570d` uses.

See [`docs/adr/0001-second-radio-on-shared-cat-framework.md`](docs/adr/0001-second-radio-on-shared-cat-framework.md)
for the recorded decision and its current blocked status.

## Why this repo exists

`ts570d` was refactored (see its `docs/adr/0001`–`0005`) to separate a
generic, radio-independent CAT engine from Kenwood-TS-570D-specific command
tables, state machines, and domain types, specifically so that a second
radio could reuse the generic engine "unchanged" — providing only its own
`CommandId` enum, command table, state machine, `Event`/`Error` types, and a
`CatRadio` implementation (`ts570d` ADR 0004). This repository is that second
radio, targeting the Yaesu FT-991A.

## Relationship to sibling repositories

```text
radio-cat-rs   shared library (cat-framework, cat-client, cat-transport-*, cat-server)
                    ▲                              ▲
                    │ depends on                   │ depends on
                    │                               │
                ts570d                          ft991a  (this repo)
        (Kenwood TS-570D/S, first radio)   (Yaesu FT-991A, second radio)
```

- **`ts570d`** — sibling application, same architecture pattern. It is the
  template this repo's structure follows: same crate shapes (`radio`, `ui`,
  `serial`, `emulator`, application wiring), same planning-with-files and
  subagent discipline, same "framework knows how to process a command, the
  radio crate knows what a command means" split. It is **not** a dependency
  of this repo — the shared engine is being extracted from it into
  `radio-cat-rs` instead, precisely so neither radio depends on the other.
- **`radio-cat-rs`** — the shared library this repo will depend on once
  extraction from `ts570d` happens: `cat-framework` (generic command table,
  parser, dispatch, response builder), `cat-client`/`CatSession`
  (transport-independent request/response abstraction), and
  `cat-transport-serial`/`-tcp`/`-udp` (transport implementations). As of
  this writing, `radio-cat-rs` is itself being scaffolded in parallel and its
  crates do not yet exist. This repo cannot build against it yet.

## The FT-991A is not the TS-570D

The FT-991A is a Yaesu radio, not a Kenwood radio. Its CAT command set,
framing conventions, parameter encodings, and response layouts are Yaesu's
own and differ from Kenwood's TS-570D command table — different command
codes, different parameter widths, different response formats. Nothing about
the TS-570D command table transfers over by assumption. When implementation
begins, the FT-991A command table must be derived from the official Yaesu
FT-991A CAT operation reference manual, page by page, the same discipline
`ts570d`'s `kenwood` agent applied to the Kenwood manual (see
`.claude/agents/yaesu.md`).

## What this repo will look like, once unblocked

Following `ts570d`'s shape (see its ADR 0004 and `CLAUDE.md`), this repo is
expected to end up with:

- `radio/` — `Ft991aCommandId`, a static `FT991A_COMMAND_TABLE`, the
  `Ft991aRadio` state machine (a `cat_framework::CatRadio` implementation),
  `Ft991aState`/`Ft991aEvent`/`Ft991aError`, a `Radio` trait plus FT-991A
  domain types, and a controller client generic over the shared library's
  session/transport abstraction.
- `ui/` — a ratatui/crossterm terminal interface, depending on `radio` and
  the shared library's domain-independent types only.
- `serial/` — likely a thin consumer of `radio-cat-rs`'s
  `cat-transport-serial` rather than a from-scratch io_uring implementation;
  see `.claude/agents/serial.md` for why this is recorded as an open
  decision rather than settled.
- `emulator/` — an FT-991A protocol simulator for testing, mirroring
  `ts570d`'s emulator role.
- Application wiring (`src/main.rs` equivalent) — the only place concrete
  types are assembled.

None of this exists yet. See `docs/adr/` and `CLAUDE.md` for the binding
rules that will govern this work once it starts, and `.claude/agents/` for
the subagent roster.

## License

Intended to be licensed under the Apache License, Version 2.0, matching the
sibling `ts570d` project. A `LICENSE.txt` will be added alongside the first
real implementation commit.
