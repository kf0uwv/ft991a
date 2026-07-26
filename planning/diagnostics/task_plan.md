# Task: Full-parity hand-coded diagnostics (replace cat-diagnostics-based [D] screen)

## Goal
Replace ft991a's read-only `[D]` diagnostics screen (radio/src/diagnostics.rs,
wrapping radio-cat-rs's cat_diagnostics::run_diagnostics_with) with a
hand-coded, ts570d-parity diagnostic living in `ui` crate, generic over
`R: Radio + Ft991aExtras + CwKeying`. User explicitly chose full parity
(exercises write/action commands incl. keying TX/CW) with snapshot/restore
safety net, matching ts570d/ui/src/terminal.rs's standard of care.

## Phases
1. [x] Read CLAUDE.md conventions
2. [ ] Read ts570d/ui/src/terminal.rs in FULL (snapshot/restore + run_diagnostics_task, all match arms)
3. [ ] Read ft991a radio/src/radio_trait.rs in full (Radio/Ft991aExtras/CwKeying)
4. [ ] Read ft991a radio/src/diagnostics.rs + current ui diagnostics screen + ADR 0004
5. [ ] Read planning/yaesu/findings.md for relevant protocol quirks (clarifier, QMB, keyer memory, tuner)
6. [ ] Read ft991a.rs/ft991a_radio.rs command defs, ADR 0002 (RTS/DTR CwKeying)
7. [ ] Design: RadioSnapshot struct + step list for all 91 commands (63 existing safe reads + 28 needing real judgment)
8. [ ] Implement in `ui`: diagnostics module (snapshot/restore/run steps/progress channel), remove radio/src/diagnostics.rs, drop cat-diagnostics dep from radio's Cargo.toml (ui already doesn't dep on it)
9. [ ] Wire into ui screen ([D] menu entry, live progress rendering)
10. [ ] cargo fmt/clippy/test
11. [ ] Windows cross-check
12. [ ] Manual emulator verification (before/after state diff)
13. [ ] Write ADR 0006
14. [ ] Final commits + report

## Decisions Log
- Single pass (no rounds), DIAG_STEP_COUNT=114, verified against real output via test.
- RD/RU confirmed absolute sets (not relative) -> exact clarifier restore possible.
- KY is keyer-memory playback, not raw text send -> gated behind DiagWarning +
  DiagCwCallsign prompt (mirrors ts570d's independently-landed ADR 0007), sends
  "TEST <CALLSIGN>", Skipped (not aborted) if blank.
- VM conditionally Skipped only when get_information().select not in {0,1}.
- AC/MX/DVS deliberately kept get-only (out of the 28-command scope, real
  RF/physical side effects).
- radio crate: removed diagnostics.rs, run_diagnostics_with, cat-diagnostics dep entirely.
- New ui/src/diagnostics.rs (data model) + terminal.rs (engine), mirroring ts570d's
  file split.

## Status: COMPLETE
All phases done: radio cleanup, engine implementation (114 steps covering all 91
commands incl. 28 previously-skipped), DiagWarning/DiagCwCallsign safety gate,
ADR 0006 written, fmt/clippy/test/windows-check all green, live emulator
verification done (114/114 with callsign, 113/1/0/114 blank, zero KY on wire when
blank, byte-for-byte state restore confirmed). Committed in 4 commits on main.
