# Code Review Agent Task Plan

## Goal
Review implementation work (once it exists) for correctness, adherence to
recorded architectural decisions, and the shared-library dependency
boundary — in particular, that no crate in this repo vendors or reimplements
`cat-framework`, and that FT-991A-specific types never leak into what should
be shared-library code (and vice versa).

## Current Status: blocked — no implementation yet

No source code exists in this repository to review. Blocked on any other
agent producing implementation output.

See `../../docs/adr/0001-second-radio-on-shared-cat-framework.md`.

## Dispatch Queue

None. No review has been requested yet.
