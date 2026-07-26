# Progress Log

## Session 2026-07-26

- Verified cwd/remote/log for ft991a per mandatory first step.
- Read CLAUDE.md, root Cargo.toml, src/main.rs, server/Cargo.toml,
  server/src/lib.rs, ui/src/lib.rs, ui/Cargo.toml in full.
- Read radio-cat-rs ADRs 0006, 0007, 0008 in full (read-only).
- Read ts570d packaging/build-deb.sh, packaging/DEBIAN/control,
  .github/workflows/{ci,release}.yml as templates (read-only).
- Created planning/radio-cat-rs-sync/{task_plan.md,progress.md}.
  `findings.md` creation was blocked by the harness (subagent report-file
  guard on that literal filename) — findings folded into task_plan.md
  instead; noted for the final report.
- Next: Cargo.toml `[patch]` setup + `cargo tree -p cat-transport-tcp`
  verification (Task 1 setup step).

- Added `[patch]` table, verified `cargo tree -p cat-transport-tcp` and
  `cargo build --workspace`. Committed `d29fa45`.
- Audited every `target_os = "linux"` gate (grep across Cargo.toml,
  src/main.rs, server/, emulator/, radio/, ui/). Found `cat-rigctl`
  (server's dependency) is NOT Windows-ported by radio-cat-rs ADR 0006 —
  confirmed via a real failing `cargo check --target x86_64-pc-windows-gnu
  -p cat-rigctl` run (11 unresolved-crate errors). Ungated only
  `--server`/`TcpClientSession` (genuinely fixed by ADR 0006);
  left `server` gated with corrected comments. Refactored
  `TcpClientSession` to compose `NoModemControlLines` instead of a
  hand-rolled impl. Verified: `cargo check --target x86_64-pc-windows-gnu
  -p ft991a` clean; same command `-p server` fails as expected;
  `cargo test --workspace` 1103/0; manual tmux run of
  `server --raw-tcp-port` + `--server host:port` against the emulator,
  clean connect/render/exit. Committed `3ecd3b5`.
- Diagnostics screen: found `ui::run`'s generic `R: Radio + Ft991aExtras +
  CwKeying` bound has no way to obtain a `CatClient`
  (`Ft991a<S>::client` is `pub(crate)`), so `cat_diagnostics::
  run_diagnostics_with` cannot be called from `ui` at all — settles the
  "should ui depend on cat-diagnostics" question by hard constraint, not
  judgment call. Added `radio/src/diagnostics.rs` (concrete, non-generic
  mirror types), `Ft991aExtras::run_diagnostics_with`, `[D]` escape hatch
  in `ui` (new `KeyResult::RunDiagnostics`, `ControlState::Diagnostics`),
  live progress via direct `Terminal::draw` calls from the (sync)
  `on_progress` callback. Added an `AutoAckSession` test fake so the new
  method could be tested against the real, full 91-command table without
  hand-scripting exchanges. Verified: `cargo test -p radio`/`-p ui` all
  green; manual tmux run against the emulator showed live progress
  landing on "63 passed / 0 failed / 28 skipped / 91 total" with working
  scroll/detail/Esc-back. Committed `b7dd8a4`.
- Packaging + CI: copied `ts570d/LICENSE.txt` (verified generic, no
  ts570d-specific text) to root. Wrote `packaging/DEBIAN/control` +
  `build-deb.sh` (ts570d template, package `ft991a-radio-control`).
  Verified `cargo build --release -p cat-transport-serial --bin
  pin-test` resolves and produces `target/release/pin-test` despite that
  crate not being a workspace member. Ran `./packaging/build-deb.sh
  --skip-build` end-to-end — produced and inspected a real `.deb` with
  all three binaries correctly staged. Wrote
  `packaging/build-windows-package.ps1` per ADR 0008 §3 — NOT executed,
  no pwsh in this sandbox (`which pwsh`/`which powershell` both empty).
  Wrote `.github/workflows/ci.yml` (ts570d structure + new
  `windows-check` job) and `release.yml` (thin caller per ADR 0008 §4,
  plus `apt_packages: libudev-dev` matching ADR 0008's own ts570d
  example). Validated both workflow YAML files parse with Python's
  `yaml.safe_load`. Committed `5a07079`.
- All six tasks complete. Final fmt/clippy/test pass: clean, 1115+ tests
  passing across the workspace, `cargo check --target
  x86_64-pc-windows-gnu -p ft991a` green.
