# App Agent Task Plan

## Goal
Own the application wiring layer (equivalent of `ts570d`'s `src/main.rs`):
CLI argument parsing, assembling concrete types (`Ft991a<SomeCatSession>`,
a chosen transport, the emulator or a real port), and handing them to
`ui::run`.

## Current Status: blocked — no implementation yet

No `Cargo.toml` or application code exists. Blocked on:
1. `radio-cat-rs` publishing a consumable `cat-framework` crate.
2. The architect confirming a workspace/crate layout to wire together.
3. An explicit architect/user go-ahead.

See `../../docs/adr/0001-second-radio-on-shared-cat-framework.md`.

## Dispatch Queue

None. No task has been assigned yet.
