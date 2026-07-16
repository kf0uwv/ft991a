# Emulator Agent Task Plan

## Goal
Build an FT-991A protocol emulator (virtual TTY via PTY, plus optional
physical-port support) that faithfully simulates the FT-991A's CAT responses
for testing, using the `radio` crate's `FT991A_COMMAND_TABLE` and
`Ft991aRadio` state machine as the source of truth for behavior.

## Current Status: blocked — no implementation yet

No `emulator/` crate exists, and there is no `radio` crate or command table
for it to simulate against yet. Blocked on:
1. `radio-cat-rs` publishing a consumable `cat-framework` crate.
2. The `radio` crate's `FT991A_COMMAND_TABLE` and `Ft991aRadio` existing (in
   turn blocked on the FT-991A manual — see `../yaesu/task_plan.md`).
3. An explicit architect/user go-ahead.

See `../../docs/adr/0001-second-radio-on-shared-cat-framework.md`.

## Dispatch Queue

None. No task has been assigned yet.
