# Yaesu FT-991A Radio Control - Agent Guidelines

## Current Status: full command coverage, grouped UI, Windows-buildable

- `radio` implements all 91 top-level FT-991A CAT commands and 151 of 153
  `EX` menu items (two, "TIME ZONE" and "RADIO ID", have no resolvable wire
  encoding in the official manual — see `planning/yaesu/findings.md`).
  Includes RTS/DTR PTT/CW keying via `radio-cat-rs`'s `ModemControlLines`
  (see `docs/adr/0002-rts-dtr-ptt-cw-keying.md`).
- `ui` is a ratatui/crossterm terminal interface with a 12-group menu
  (`CommandGroup`) proportional to the full command surface, plus two `EX`
  menu access paths (number-entry escape hatch, themed browsing) that
  converge on the same value-entry flow.
- `emulator` is a PTY-hosted FT-991A protocol simulator, verified
  interoperating end-to-end with the real `ft991a` binary over a live PTY.
- This application cross-compiles cleanly for Windows
  (`cargo check --target x86_64-pc-windows-gnu -p ft991a`), riding on
  `radio-cat-rs`'s native Win32 COM serial backend and a hand-rolled
  Windows entry point (`#[monoio::main]` doesn't exist there).
- `radio::Profile` (`--profile <name>` CLI flag, `[L]` in-UI menu action)
  applies a named bundle of settings (mode, filter bandwidth, gain,
  attenuator, `EX` menu items, ...) from a TOML file in one shot — see
  `planning/architect/task_plan.md` §12.3.
- `server` (new workspace crate, cross-platform since `radio-cat-rs`
  docs/adr/0006's 2026-07-26 amendment gave `cat-rigctl` a Windows backend
  — see `docs/adr/0003`'s amendment) is a headless network server
  mode (`ft991a server --port <dev> --rigctl-port <n> [--raw-tcp-port <n>]
  [--raw-udp-port <n>]`): one process owns the physical serial port,
  shared by `radio-cat-rs`'s `cat-server` request broker with a new
  Hamlib rigctld-compatible TCP listener for WSJT-X's "Hamlib NET rigctl"
  rig type, plus the existing raw `cat-server` TCP/UDP protocols for other
  `radio-cat-rs`-aware clients. Verified end-to-end against the live
  `emulator` — see `planning/architect/task_plan.md` §12.2/§12.4.
  **`radio-cat-rs` bug found and fixed during this verification** (§12.4):
  `cat-server`'s broker silently dropped the response to any "selector
  read" command (`MD`, the whole `EX` menu, and ~23 others) that is also
  writable — fixed upstream via a new `CommandForm::selector_read`
  marker (`radio-cat-rs@889591b`, pushed to `main`) and picked up here as
  a plain `branch = "main"` git dependency update, no `[patch]` needed.
  The rigctld command translation (§12.2) has since been validated
  against a real Hamlib client (`rigctl -m 2`, the same `netrigctl.c`
  backend WSJT-X's "Hamlib NET rigctl" rig type uses), which surfaced and
  fixed two more bugs: `\dump_state`'s capability tail was two fields
  short (`has_get_parm`/`has_set_parm` missing, hanging the client's
  handshake), and `F` (set frequency) only parsed a bare integer where
  Hamlib always sends a `%f`-formatted float (e.g. `F 14074000.000000`).
  Not yet validated against WSJT-X itself, only against the Hamlib
  library it's built on.
- `ft991a --server <host:port>` (TCP client mode, **now Windows-buildable
  too** — see below) connects the normal control/TUI program to a *remote*
  `ft991a server`'s raw `--raw-tcp-port` listener instead of opening a
  local serial port — mutually exclusive with `--port`. `TcpClientSession`
  adapter in `src/main.rs` wraps `radio-cat-rs`'s
  `cat-transport-tcp::TcpCatSession` to satisfy `Ft991a<S>`'s
  `CatSession<Error = TransportError>` bound (mirrors
  `server/src/broker_session.rs::BrokerCatSession`); the separate "TCP has
  no RTS/DTR" concern is handled by wrapping that adapter in
  `radio-cat-rs`'s `cat_transport_core::NoModemControlLines` (`Ft991a::new
  (NoModemControlLines::new(TcpClientSession::new(session)))`) so
  `Ft991a<NoModemControlLines<TcpClientSession>>` still satisfies
  `ui::run`'s unconditional `CwKeying` bound, replacing this app's
  previous hand-rolled honest-error `ModemControlLines` impl (five
  near-identical `Err(...)` bodies) — see
  `docs/adr/0003-consume-radio-cat-rs-windows-network-transport.md`.
  `radio-cat-rs` ADR 0006 gave `cat-transport-tcp`/`cat-transport-core` a
  real Windows backend (same public API both platforms, no
  `cfg`-branching needed in this app's own code anymore) — confirmed via
  `cargo check --target x86_64-pc-windows-gnu -p ft991a`, green. Verified
  end-to-end on Linux: TUI driven live (via tmux) against `ft991a server
  --raw-tcp-port` backed by the `emulator`, connected cleanly with no
  errors. See `planning/app/task_plan.md`'s Wave 4 task and
  `planning/radio-cat-rs-sync/task_plan.md` for this round's changes.
- `ft991a server ...` (headless network server mode) is now **Windows-
  buildable too** — `radio-cat-rs` docs/adr/0006's 2026-07-26 amendment
  gave `cat-rigctl` (which `server/src/lib.rs` wraps) a real Windows
  backend, closing the gap that previously kept this Linux-only even after
  `cat-transport-tcp`/`cat-transport-udp`/`cat-server` themselves became
  cross-platform. `server::run`/`main.rs`'s `run_server_mode` are
  `#[cfg]`-selected per platform (`async fn` on Linux, a plain blocking
  `fn` on Windows) to match `cat_rigctl::run`'s own split. Confirmed:
  `cargo check --target x86_64-pc-windows-gnu -p ft991a -p server` clean.
  See `docs/adr/0003`'s amendment.
- The `[D]` diagnostics screen (`ui/src/terminal.rs`'s
  `run_diagnostics_task`) is now a **hand-coded, full-parity** engine,
  matching `ts570d`'s own standard of care — the user's explicit choice
  over the original read-only `cat-diagnostics`-wrapped design
  (`docs/adr/0004-shared-diagnostics-screen.md`, now superseded). Every one
  of the 91 top-level `FT991A_COMMAND_TABLE` commands' underlying
  `Radio`/`Ft991aExtras` methods is exercised via a real typed call
  (snapshot state up front, set, verify, restore at the end) — including
  all 28 commands the old read-only engine could only mark `Skipped`
  (S-Meter/Read Meter/Radio Information/Memory Channel Read as plain reads;
  Memory Channel Write/VFO-A-to-Memory/QMB Store/Channel Up-Down/Clarifier
  Clear-Down-Up with real snapshot+restore; Swap VFO/Quick Split with
  symmetric self-undo; Band Select/Up/Down/Encoder Down/Up/Ent Key/Zero
  In/Down/Up as verified actions; `[V/M]` Key Function and CW Keying each
  conditionally `Skipped` with an honest, specific reason when they cannot
  be safely restored/attempted). `radio` no longer depends on
  `cat-diagnostics` at all — the engine lives entirely in `ui`
  (`ui/src/diagnostics.rs` for the `DiagOutcome`/`DiagResult`/`DiagSummary`
  data model, `terminal.rs` for `RadioSnapshot`/`snapshot_state`/
  `restore_state`/`run_diagnostics_task`), since it only ever calls typed
  `Radio`/`Ft991aExtras` methods, never `cat_framework`/`CatClient`
  directly. Because this genuinely keys the transmitter (PTT, and CW if a
  callsign is supplied), pressing `[D]` first shows a red-bordered,
  explicit-acknowledgment warning screen (`ControlState::DiagWarning`)
  before anything is sent, then prompts for an operator-supplied callsign
  (`InputAction::DiagCwCallsign`, reusing the existing text-entry widget) —
  the CW keying step only ever sends `"TEST <CALLSIGN>"` (never a bare,
  unidentified `"TEST"`), and is `Skipped` rather than sent bare if the
  prompt is left blank, without aborting the rest of the run. See
  `docs/adr/0006-hand-coded-full-parity-diagnostics.md` for the full
  per-command safety reasoning and emulator verification results.
- Packaging: `packaging/build-deb.sh` builds a `ft991a-radio-control` `.deb`
  (mirrors `ts570d`'s script), and `packaging/build-windows-package.ps1`
  produces a Windows zip package, both consumed by radio-cat-rs's shared
  `.github/workflows/release-app.yml` (that sibling repo's own ADR 0008)
  via this repo's own thin `.github/workflows/release.yml` caller.
  `.github/workflows/ci.yml` runs fmt/clippy/tests on `ubuntu-latest` plus
  a Windows cross-check job. The release caller will not actually resolve
  until a human pushes `radio-cat-rs`'s `main` branch — expected, not a
  bug here. See `docs/adr/0005-debian-and-windows-packaging.md`.

See `docs/adr/0001-second-radio-on-shared-cat-framework.md` and
`docs/adr/0002-rts-dtr-ptt-cw-keying.md` for the design record, and
`docs/adr/README.md` for a running status summary. `planning/architect/
task_plan.md` records the full multi-wave dispatch history (§1-11) if you
need the reasoning behind a specific design decision.

## Superpowers Coding Model (MANDATORY)
- Use planning-with-files skill for ALL implementation work
- Follow TDD, frequent commits, verification-before-completion
- Check for applicable skills BEFORE any action

## Planning-with-Files Requirement
- Each agent and subagent must maintain their own planning-with-files in a directory under `./planning/` with their name
- Directories: `./planning/architect/`, `./planning/app/`, `./planning/yaesu/`, `./planning/ui/`, `./planning/emulator/`, `./planning/code_review/`
- Planning files include: `task_plan.md`, `findings.md`, `progress.md` in each agent's directory
- This prevents conflicts between agents working on different aspects of the project
- Planning files must be created and maintained before any implementation work

## Planning Directory Ownership and Boundaries
- Each agent owns ONLY their planning directory under `./planning/{agent_name}/`
- Agents must NEVER edit planning files in other agents' directories
- All planning work MUST use planning-with-files skill
- Planning files must be created BEFORE any implementation work
- Each agent is responsible for: `task_plan.md`, `findings.md`, `progress.md` in their own directory only
- Any violation of these boundaries is a critical issue

## Architect Review Workflow (MANDATORY)
- ALL subagents must write their implementation plan to their `task_plan.md` BEFORE writing any code
- Plans are reviewed by the architect and user before work proceeds
- Subagents execute ONE task at a time, reporting results before moving to the next
- The architect coordinates parallelization across subagents
- No subagent proceeds past planning without architect approval

## Core Technologies
- monoio: io_uring async runtime on Linux — tokio is NEVER used
- On Windows, `#[monoio::main]` doesn't exist: `src/main.rs` splits into a
  shared `run_app()` plus platform-gated entry points, using a hand-rolled
  thread-parking `block_on`/`Waker` on Windows (see `src/main.rs`)
- ratatui + crossterm: Terminal UI
- Shared CAT engine and transport abstractions consumed from `radio-cat-rs`
  as git dependencies (`branch = "main"`): `cat-framework`, `cat-client`,
  `cat-transport-core`, `cat-transport-serial`
- FT-991A protocol emulator with virtual TTY (Linux/Unix only — no Windows
  emulator exists; Windows builds are validated against real hardware)

## Essential Commands
- Build: `cargo build --workspace` / `cargo build --workspace --release`
- Test: `cargo test --workspace` / `cargo test -p <crate> test_name`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt`
- Windows cross-compile check: `cargo check --target x86_64-pc-windows-gnu -p ft991a`
  (requires `rustup target add x86_64-pc-windows-gnu`; type-checks only)
- Emulator: `cargo run -p emulator -- --background` (prints `PTY_SLAVE=<path>`)
- App against the emulator: `cargo run --bin ft991a -- --port <path> --baud 9600`
- App against real hardware: `cargo run --bin ft991a -- --port /dev/ttyUSB0 --baud 9600`
- Headless server mode: `cargo run --bin ft991a -- server --port <path> --raw-tcp-port <n>` (Linux only)
- Remote TCP client mode: `cargo run --bin ft991a -- --server <host:port>` (Linux and Windows)
- Debian package: `./packaging/build-deb.sh` (produces `*.deb` in repo root)
- Windows package: `pwsh ./packaging/build-windows-package.ps1` (produces `*.zip` in repo root; not runnable in a Linux sandbox)
- `pin-test` (shared RS-232 pin-test tool, from `radio-cat-rs`'s `cat-transport-serial`): `cargo build --release -p cat-transport-serial --bin pin-test`

## Crate Dependency Model (MANDATORY — ALL AGENTS MUST FOLLOW)

This project depends on a **shared, radio-independent generic CAT engine**
published by the `radio-cat-rs` repository, rather than defining its own
local copy. All FT-991A-specific knowledge lives in this repo's `radio`
crate. This mirrors `ts570d`'s dependency-inversion model exactly, with one
difference: `ts570d` also once had a local `framework` crate (since removed
in its own migration onto `radio-cat-rs`); this repo's generic engine has
always been an **external** dependency (`cat-framework` et al.).

```
cat-framework  (external crate, from radio-cat-rs — NOT part of this repo)
  └── defines: generic CAT engine — CommandTable<C>, CommandDefinition<C>, CommandForm,
               CommandOperation, CommandRequest, ParameterValues, ResponseBuilder,
               CommandOutcome, CatCommandCatalog / CatRadio traits, CatFramework<R>
  └── defines: CatSession, Transport, ModemControlLines traits, generic errors
  └── contains NO radio-specific command ids, modes, frequencies, state, or handlers
  └── this repo NEVER forks, vendors, or duplicates this crate locally

cat-transport-serial  (external crate, from radio-cat-rs — NOT part of this repo)
  └── SerialCatSession/SerialPort/SerialConfig — real io_uring transport on
      Linux, native Win32 COM-port transport on Windows, same public API on
      both platforms. This repo has NO local `serial` crate.

radio  (depends on: cat-framework, cat-client, cat-transport-core)
  └── defines: Ft991aCommandId, FT991A_COMMAND_TABLE (91 commands),
      EX_MENU_TABLE (151 of 153 settings items)
  └── defines: Ft991aRadio (CatRadio impl + emulator state machine), Ft991aState, Ft991aEvent
  └── defines: Radio trait (generic radio concepts), Ft991aExtras trait
      (~50 FT-991A-specific methods, NotImplemented-defaulted, mirrors
      Radio's idiom), CwKeying trait (RTS/DTR CW keying, bounded on
      + ModemControlLines) + FT-991A domain types
  └── implements: Radio + Ft991aExtras (unconditional) + CwKeying
      (bounded) for Ft991a<S: CatSession> (controller client)
  └── Ft991a is generic over S — never imports a transport crate directly
  └── NO diagnostics engine or cat-diagnostics dependency of any kind —
      the `[D]` screen (ui/src/terminal.rs, ui/src/diagnostics.rs) calls
      typed Radio/Ft991aExtras methods directly instead (docs/adr/0006)

ui  (depends on: radio only)
  └── uses: radio::{Radio, Ft991aExtras, CwKeying} trait bounds
      (ui::run<R: Radio + Ft991aExtras + CwKeying + 'static>(radio: R)) —
      this is a disclosed, real widening from "any Radio implementation":
      ui is contractually FT-991A-shaped, not radio-generic, per the Rust
      coherence constraint recorded in planning/architect/task_plan.md §11.3
  └── uses: radio domain types (Frequency, Mode, ...) for display
  └── defines: DiagOutcome/DiagResult/DiagSummary (ui/src/diagnostics.rs)
      and the full hand-coded diagnostics engine
      (RadioSnapshot/snapshot_state/restore_state/run_diagnostics_task in
      terminal.rs) for the `[D]` screen — calls typed Radio/Ft991aExtras
      methods only, never cat_framework/CatClient (docs/adr/0006,
      superseding docs/adr/0004's cat-diagnostics-wrapped design)
  └── NEVER imports cat-transport-serial or any other transport crate

emulator  (depends on: cat-framework + radio)
  └── runs CatFramework<Ft991aRadio>; owns PTY hosting, logging, TUI display

src/main.rs  (depends on: all crates — the wiring layer only)
  └── creates Ft991a<SerialCatSession<SerialPort>> and passes it to ui::run()
  └── platform-gated main(): #[monoio::main] on Linux, hand-rolled block_on
      on Windows — see src/main.rs's windows_block_on module
```

### Rules (violation is a blocking issue)
1. This repo has NO local generic-CAT-engine crate. The generic engine is
   consumed as an external dependency (`cat-framework`, from `radio-cat-rs`).
   Do not create a local `framework`-equivalent crate, even temporarily.
2. **`radio`** NEVER imports a transport crate directly. Transport is
   injected by the app via generics (`S: CatSession`).
3. **`radio`** owns the single source of truth for the command table
   (`FT991A_COMMAND_TABLE`) and the settings table (`EX_MENU_TABLE`). There
   must be exactly ONE of each, and both must be derived from the official
   Yaesu FT-991A manual — never assumed from `ts570d`'s TS-570D table.
4. **`ui`** may depend on `radio` (for the `Radio`/`Ft991aExtras`/`CwKeying`
   traits and domain types) but NEVER on `cat-transport-serial` or any
   transport crate. It uses trait abstractions, not concrete transports or
   sessions.
5. **`src/main.rs`** is the ONLY place concrete types are wired together.
6. Unit tests use **mock/fake implementations** of the relevant trait —
   never the real impl from another crate.
   - `radio` tests use an in-crate fake `CatSession` (not a real transport)
   - `ui` tests use an in-crate `MockRadio` implementing `Radio` +
     `Ft991aExtras` + `CwKeying`
7. Never depend on `ts570d` directly. It is a sibling application, not a
   dependency — both repos depend on the shared library, not on each other.
8. New `Ft991a<S>` methods that are generic radio concepts (present in
   `ts570d::Radio` or clearly universal) go on `Radio`; FT-991A-specific
   methods go on `Ft991aExtras`; anything needing `ModemControlLines` goes
   on `CwKeying`. Don't add a fourth trait without checking whether one of
   these three already fits.

### Generic framework vs. FT-991A responsibilities
The generic `cat-framework` (shared, external) knows how to **process** a
command: framing, command lookup, syntactic parsing, structural parameter
validation, generic dispatch lifecycle, and response construction — all
generic over a radio-defined `CommandId`.

This repo's `radio` crate knows what a command **means**: command
identifiers, command definitions, radio state and transitions,
state-dependent validation, command semantics, response values, and
protocol-specific errors. It implements `cat_framework::CatRadio` to receive
parsed commands.

### Radio trait scope
The `Radio` trait (defined in this repo's `radio` crate, controller/UI-facing)
contains abstract radio concepts shared with `ts570d`'s `Radio` trait —
frequency control, mode, PTT, meters, gain controls, power, scan, RIT/XIT,
noise blanker, memory channels, squelch, preamplifier, attenuator, VOX,
etc. FT-991A-specific features (keyer memory, QMB, antenna tuner, DVS,
parametric EQ, the `EX` settings menu, and anything else without a
`ts570d::Radio` precedent) live on `Ft991aExtras` as inherent-shaped
trait methods, not on `Radio`.

## Architecture
- `radio/`: FT-991A command table + settings table, `CatRadio` impl,
  controller client, `Radio`/`Ft991aExtras`/`CwKeying` traits + domain types
- `ui/`: Ratatui terminal interface (depends on `radio` only) — 12-group
  menu + `EX` menu number-entry/themed-browsing access paths
- No local `serial/` crate — transport is `cat-transport-serial` (external)
- `emulator/`: Virtual TTY + radio emulator, runs `CatFramework<Ft991aRadio>`
- `src/main.rs`: application wiring, platform-gated entry point

## Code Style
- Imports: std → external → local
- Error handling: thiserror + Result<T, E>
- Naming: snake_case/PascalCase conventions
- Async: monoio runtime on Linux — tokio is NEVER used; Windows uses a
  hand-rolled thread-parking executor, not a third async-runtime crate

## Testing Strategy
- Unit tests for individual components (e.g. `radio` crate tests drive
  `cat_framework::CatFramework<Ft991aRadio>` directly, in-process — no PTY
  needed for command-table/state-machine coverage, mirroring `ts570d`)
- Integration tests with the PTY-hosted `emulator` crate (Linux/Unix only)
- Windows: `cargo check --target x86_64-pc-windows-gnu` type-checks the
  build; there is no Windows-compatible emulator, so runtime behavior is
  validated against real hardware, not in this repo's own test suite

## Linux-Specific
- io_uring kernel requirements (5.1+) — provided by `cat-transport-serial`,
  not reimplemented locally
- `emulator`'s PTY hosting is Unix-specific (`nix`/`serialport`'s
  `TTYPort::pair()`) — it has no Windows equivalent and isn't gated for one
