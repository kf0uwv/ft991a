# Yaesu Agent Task Plan

## Goal
Implement the FT-991A-specific `radio` crate: `Ft991aCommandId`, the static
`FT991A_COMMAND_TABLE`, the `Ft991aRadio` state machine (a
`cat_framework::CatRadio` implementation), `Ft991aState`/`Ft991aEvent`, the
`Radio` trait and FT-991A domain types, and a controller client generic over
the shared library's `CatSession`.

## Current Status: blocked — no implementation yet

Blocked on:
1. `radio-cat-rs` publishing a consumable `cat-framework` crate.
2. **The official Yaesu FT-991A CAT operation reference manual being added
   to this repository.** No command table entry may be written before this
   — see `../../.claude/agents/yaesu.md`'s manual-first requirement.
3. An explicit architect/user go-ahead.

See `../../docs/adr/0001-second-radio-on-shared-cat-framework.md`.

## Dispatch Queue

None. No task has been assigned yet. The first real task, once unblocked, is
expected to be: acquire/confirm the manual's location, then perform a
command-by-command audit mirroring `ts570d` ADR 0003's process (single
authoritative table, `readable`/`writable` derived from the manual, not
assumed from the TS-570D table).
