# radio-cat-rs-sync Task Plan

## Goal
Consume radio-cat-rs's local (unpushed) ADR 0006/0007/0008 work — Windows
network transport, `cat-diagnostics`, shared `pin-test` binary,
`NoModemControlLines`, shared release workflow — into ft991a via a
temporary `[patch]` on the git dependency, then land six concrete
deliverables. See the caller's task list (reproduced below) for exact
acceptance criteria; not re-deriving it here.

## Constraints
- Work only in `/home/mattfranklin/src/github.com/kf0uwv/ft991a`.
- Commit locally, frequently, well-scoped. NEVER push, tag, or `gh release
  create`.
- Never write to `ts570d` or `radio-cat-rs` (read-only reference).
- `radio` never imports a transport crate. `ui` depends only on `radio`
  (verify `cat-diagnostics` doesn't count as a transport crate before
  wiring it into `ui` directly). `src/main.rs` is the only wiring layer.
  monoio is Linux-only. tokio never used.

## Tasks
1. **[patch] setup** — root Cargo.toml `[patch."https://github.com/kf0uwv/radio-cat-rs"]`
   pointing every `cat-*` crate at `../radio-cat-rs/<crate>` (path deps),
   marked temporary. Add `cat-diagnostics` as a new git dep (presumably in
   `ui`) + matching patch entry. Verify via `cargo tree -p cat-transport-tcp`.
2. **Diagnostics screen** — new `ui` screen invoking
   `cat_diagnostics::run_diagnostics`/`run_diagnostics_with` against
   `FT991A_COMMAND_TABLE` through `Ft991a<S>`, rendered ratatui-style,
   fit into the 12-group `CommandGroup` menu.
3. **pin-test packaging** — no new code; ship radio-cat-rs's
   `cat-transport-serial` `[[bin]] pin-test` in Task 4's packaging.
4. **Extend server/TCP-client to Windows** — remove now-stale
   `target_os = "linux"` gates in root Cargo.toml / src/main.rs / server/
   whose stated reason was "no Windows backend upstream" (now fixed by
   ADR 0006). Leave the *implementing* `#[cfg(target_os = "windows")]`
   blocks (windows_block_on etc.) untouched. Refactor `TcpClientSession`'s
   hand-rolled `ModemControlLines` impl to `NoModemControlLines` if it fits
   cleanly. Verify `cargo check --target x86_64-pc-windows-gnu -p ft991a -p
   server`, `cargo test --workspace`, and a live emulator+server+client run.
5. **Debian packaging** — `packaging/DEBIAN/control` +
   `packaging/build-deb.sh` (ts570d template), package name
   `ft991a-radio-control`, stage ft991a/emulator/pin-test, add root
   `LICENSE.txt` (full Apache-2.0 text), `--skip-build` support.
6. **Windows packaging** — `packaging/build-windows-package.ps1` per ADR
   0008 §3 (no args, runs after `cargo build --release`, zip in repo root).
   Cannot execute (no Windows sandbox) — review by hand.
7. **CI + release workflows** — `.github/workflows/ci.yml` (ts570d
   structure + a Windows cross-check job) and `.github/workflows/
   release.yml` (thin caller of radio-cat-rs's `release-app.yml@main`,
   per ADR 0008 §4's exact template).
8. ADRs (continuing past 0002) for: (a) consuming radio-cat-rs via
   `[patch]` pre-push, (b) diagnostics screen placement (ui vs radio),
   (c) Windows-enabling server/TCP-client + NoModemControlLines refactor,
   (d) packaging/CI additions. Update CLAUDE.md + README.md.

## Status
- [x] Read CLAUDE.md, root Cargo.toml, src/main.rs, server/, ui/lib.rs,
      radio-cat-rs ADRs 0006/0007/0008, ts570d packaging/CI templates.
- [ ] Cargo.toml patch + cargo tree verification
- [ ] Diagnostics screen
- [ ] Windows-enable server/TCP-client + NoModemControlLines refactor
- [ ] Debian packaging
- [ ] Windows packaging script
- [ ] CI + release workflows
- [ ] ADRs + CLAUDE.md/README.md updates
- [ ] Final verification pass (fmt/clippy/test) + final report

## Findings (folded in here — the Write tool refuses to create a file
literally named `findings.md` for this session, "subagents should return
findings as text"; keeping the repo's normal 3-file convention was not
possible this run, noted in the final report)

### radio-cat-rs state (local, unpushed, verified via git log there)
HEAD `cffd430`. ADR 0006 (Windows network transport): TCP/UDP/cat-server
gain Windows backends; `cat_transport_core::completion` (moved from
cat-transport-serial::oneshot) and `cat_transport_core::timeout` (portable
combinator, NOT safe under real monoio — only under block_on/std::thread
executors) added; `cat-transport-serial` gained `[[bin]] name = "pin-test"`;
`cat_transport_core::NoModemControlLines<S>` adapter added
(`cat-transport-core/src/modem.rs`). ADR 0007: new `cat-diagnostics` crate,
read-only/liveness-only, API: `DiagnosticConfig`, `CommandResult`,
`CommandOutcome<C>`, `DiagnosticReport<C>`, `run_diagnostics`/
`run_diagnostics_with`; depends only on cat-framework/cat-client/
cat-transport-core (+Linux-gated monoio) — not a transport crate. ADR 0008:
shared `release-app.yml` reusable workflow; consuming-repo contract =
`packaging/build-deb.sh [--skip-build]` + `packaging/build-windows-package.ps1`
(no args) + a `[[bin]]` matching main_binary; will not resolve until
radio-cat-rs main is pushed to GitHub (uses `@main` against remote).

### ft991a current state
Workspace: radio, ui, emulator, server. `server` crate + `--server
host:port` TCP client mode both gated `target_os = "linux"` throughout
(root Cargo.toml deps, src/main.rs ServerArgs/parse_server_args/
run_server_mode/run_over_tcp/TcpClientSession), reason recorded in
comments as "no Windows backend upstream in radio-cat-rs" — now stale per
ADR 0006. The ONLY genuinely Windows-specific code (`cfg(target_os =
"windows")`, not `not(linux)`) is main()'s Windows entry point +
windows_block_on module — must not touch. `ui` depends only on `radio`
(+thiserror/ratatui/crossterm, Linux-gated dev-dep monoio) — no transport
crate. `ui::run<R: Radio + Ft991aExtras + CwKeying + 'static>` is the
entry point; `Ft991aDisplay` is the rendered snapshot struct. Large files:
control.rs 8872 lines, ft991a_radio.rs 12945 lines, ft991a.rs 5827 lines —
use grep, not full reads.

## Key file locations (found during research)
- `Cargo.toml` (root) — workspace deps, `cat-*` git deps, target gating
- `src/main.rs` — Args/ServerArgs parsing, `run_over_serial`/`run_over_tcp`,
  `TcpClientSession` (~line 500-585), platform entry points
- `server/Cargo.toml`, `server/src/lib.rs` — Linux-only server crate
- `ui/src/lib.rs`, `ui/src/control.rs` (8872 lines!), `ui/src/layout.rs`
  (784 lines), `ui/src/terminal.rs` (1439 lines) — need targeted grep, not
  full reads, given size
- `radio/src/ft991a.rs` (5827 lines) — `Ft991a<S>` client
- `radio/src/ft991a_radio.rs` (12945 lines) — command table / CatRadio impl
- ts570d templates: `packaging/build-deb.sh`, `packaging/DEBIAN/control`,
  `.github/workflows/{ci,release}.yml`
