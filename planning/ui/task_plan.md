# UI Agent Task Plan

## Goal
Build the ratatui/crossterm terminal interface for FT-991A control,
depending only on the shared `cat-framework` crate and this repo's `radio`
crate (its `Radio` trait and domain types) — never on a transport or session
crate.

## Current Status: blocked — no implementation yet

No `ui/` crate exists, and there is no `radio` crate's `Radio` trait for it
to depend on yet. Blocked on:
1. `radio-cat-rs` publishing a consumable `cat-framework` crate.
2. The `radio` crate reaching a stable-enough `Radio` trait to build against.
3. An explicit architect/user go-ahead.

See `../../docs/adr/0001-second-radio-on-shared-cat-framework.md`.

## Dispatch Queue

None. No task has been assigned yet.
