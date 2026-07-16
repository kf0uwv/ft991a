# Architect Task Plan — Overall Coordination

## Goal
Coordinate the eventual FT-991A radio control application: emulator for
testing, transport I/O (likely via `radio-cat-rs`'s `cat-transport-serial`),
CAT protocol layer (`radio`), and terminal UI — all built on the shared
`cat-framework` engine once it is consumable.

## Current Status: blocked — no implementation yet

Blocked on:
1. `radio-cat-rs` publishing a consumable `cat-framework` crate (and ideally
   `cat-client`/`CatSession`) — see
   `../../docs/adr/0001-second-radio-on-shared-cat-framework.md`.
2. The official Yaesu FT-991A CAT operation reference manual being added to
   this repository.
3. An explicit architect/user go-ahead to begin implementation.

## Dispatch Queue

None. No subagent has been dispatched. Do not dispatch implementation work
until the blockers above are confirmed cleared — check
`../../docs/adr/README.md` for the current status before assuming otherwise.

## Next Steps (once unblocked)
1. Confirm `radio-cat-rs` crate names/versions and add them as dependencies
   once a `Cargo.toml` is created (this itself is a dispatchable task, likely
   to the `app` agent).
2. Confirm the FT-991A manual's path and have the `yaesu` agent begin a
   command-table audit mirroring `ts570d`'s ADR 0003 process.
3. Decide the `serial` open question (consume `cat-transport-serial` vs.
   local implementation) — see `.claude/agents/serial.md`.
4. Stand up the workspace skeleton (crate layout) as its own reviewed task,
   not bundled into first-command-implementation work.
