# Yaesu FT-991A Radio Control - Agent Guidelines

## Current Status: IMPLEMENTATION UNDERWAY (started 2026-07-17)

All three preconditions that previously blocked implementation have
cleared:

1. the shared-library repo `radio-cat-rs` has published `cat-framework`,
   `cat-client`, `cat-transport-core`, and `cat-transport-serial` as
   consumable git dependencies (`branch = "main"`), and the sibling `ts570d`
   repo has already migrated onto them — see `ts570d/Cargo.toml` for the
   exact dependency syntax this repo mirrors;
2. the official Yaesu FT-991A CAT Operation Reference Manual is checked in
   at `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf`; and
3. the architect/user has given an explicit go-ahead.

There is still no `Cargo.toml` or crate checked in as of this update — see
`planning/architect/task_plan.md` for the workspace design (crate layout,
dependency versions, the resolved `serial`-crate decision) and the dispatch
queue that produces it. Subagents dispatched against that plan **should**
run `cargo init`/create `Cargo.toml` files/write `.rs` files within their
own owned directories (`radio/`, `ui/`, `emulator/`, `src/`) — the blanket
prohibition below is superseded for this work. Agents should still check
`docs/adr/0001-second-radio-on-shared-cat-framework.md` and
`docs/adr/README.md` at the start of a session as a sanity check, not assume
this status is permanent or applies to directories outside their own scope.

See `docs/adr/0001-second-radio-on-shared-cat-framework.md` for the full
decision record, and `docs/adr/README.md` for a running status summary.
Every rule below describes the architecture this repo now implements — it
is binding on implementation work.

## Superpowers Coding Model (MANDATORY)
- Use planning-with-files skill for ALL implementation work
- Follow TDD, frequent commits, verification-before-completion
- Check for applicable skills BEFORE any action

## Planning-with-Files Requirement
- Each agent and subagent must maintain their own planning-with-files in a directory under `./planning/` with their name
- Directories: `./planning/architect/`, `./planning/app/`, `./planning/yaesu/`, `./planning/serial/`, `./planning/ui/`, `./planning/emulator/`, `./planning/code_review/`
- Planning files include: `task_plan.md`, `findings.md`, `progress.md` in each agent's directory
- This prevents conflicts between agents working on different aspects of the project
- Planning files must be created and maintained before any implementation work
- These directories already exist with starter `task_plan.md` files; the
  `architect` directory's has been updated to reflect the unblocked status
  above as of 2026-07-17 — update the others in place as each agent is
  dispatched, do not replace the convention

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
- monoio: io_uring async runtime — same as `ts570d`; tokio is NEVER used
- ratatui + crossterm: Terminal UI
- Shared CAT engine and transport abstractions consumed from `radio-cat-rs`
  as git dependencies (`branch = "main"`): `cat-framework`, `cat-client`,
  `cat-transport-core`, `cat-transport-serial` — see
  `planning/architect/task_plan.md` for the exact `Cargo.toml` shape,
  mirroring `ts570d/Cargo.toml`
- FT-991A protocol emulator with virtual TTY, for testing without hardware
  (a later dispatch wave — see `planning/architect/task_plan.md`)

## Essential Commands
No `Cargo.toml` exists yet as of this update — none of these are runnable
today. Recorded for when the `app` agent's workspace-scaffolding task lands:
- Build: `cargo build` / `cargo build --release`
- Test: `cargo test` / `cargo test test_name`
- Lint: `cargo clippy` / `cargo fmt`
- Emulator: `cargo run --bin emulator`

## Crate Dependency Model (MANDATORY — ALL AGENTS MUST FOLLOW)

This project depends on a **shared, radio-independent generic CAT engine**
published by the `radio-cat-rs` repository, rather than defining its own
local copy. All FT-991A-specific knowledge lives in this repo's `radio`
crate. This mirrors `ts570d`'s dependency-inversion model exactly, with one
difference: `ts570d`'s generic engine is a local crate (`framework`);
this repo's generic engine is an **external** dependency (`cat-framework`
et al., from `radio-cat-rs`).

```
cat-framework  (external crate, from radio-cat-rs — NOT part of this repo)
  └── defines: generic CAT engine — CommandTable<C>, CommandDefinition<C>, CommandForm,
               CommandOperation, CommandRequest, ParameterValues, ResponseBuilder,
               CommandOutcome, CatCommandCatalog / CatRadio traits, CatFramework<R>
  └── defines: CatSession, Transport trait, generic errors
  └── contains NO radio-specific command ids, modes, frequencies, state, or handlers
  └── this repo NEVER forks, vendors, or duplicates this crate locally

cat-transport-serial  (external crate, from radio-cat-rs — NOT part of this repo)
  └── DECIDED (planning/architect/task_plan.md, 2026-07-17): this repo has
      NO local `serial` crate. `cat-transport-serial` already provides a
      real io_uring `SerialPort`/`SerialConfig`/`SerialCatSession` — the
      FT-991A's documented CAT serial behavior (4800/9600/19200/38400 baud,
      standard 8-data-bit/no-parity RS-232C framing, optional RTS/CTS
      hardware handshake, ';'-terminated frames) is fully covered by the
      existing `SerialConfig`/`FlowControl` surface with no code changes.
      Reimplementing it locally here would recreate the exact duplication
      `ts570d` eliminated by extracting it.

radio  (depends on: cat-framework only)
  └── defines: Ft991aCommandId, FT991A_COMMAND_TABLE (the single command table)
  └── defines: Ft991aRadio (CatRadio impl + emulator state machine), Ft991aState, Ft991aEvent
  └── defines: Radio trait + FT-991A domain types (Frequency, Mode, InformationResponse,
               MemoryChannelEntry, RadioError, RadioResult) — controller/UI-facing
  └── implements: Radio trait for Ft991a<S: CatSession> (controller client)
  └── Ft991a is generic over S: CatSession — never imports a transport crate directly

ui  (depends on: cat-framework + radio)
  └── uses: radio::Radio trait abstraction (ui::run<R: Radio>(radio: &mut R))
  └── uses: radio domain types (Frequency, Mode, ...) for display
  └── NEVER imports cat-transport-serial or any other transport crate

emulator  (depends on: cat-framework + radio)
  └── runs CatFramework<Ft991aRadio>; owns PTY hosting, logging, TUI display

app/src/main.rs  (depends on: all crates — the wiring layer only)
  └── creates Ft991a<SomeCatSession> and passes &mut radio to ui::run()
```

### Rules (violation is a blocking issue)
1. This repo has NO local generic-CAT-engine crate. The generic engine is
   consumed as an external dependency (`cat-framework`, from `radio-cat-rs`).
   Do not create a local `framework`-equivalent crate, even temporarily.
2. **`radio`** NEVER imports a transport crate directly. Transport is
   injected by the app via generics (`S: CatSession`).
3. **`radio`** owns the single source of truth for the command table
   (`FT991A_COMMAND_TABLE`). There must be exactly ONE command table, and it
   must be derived from the official Yaesu FT-991A manual — never assumed
   from `ts570d`'s TS-570D table.
4. **`ui`** may depend on `radio` (for the `Radio` trait and domain types)
   but NEVER on `cat-transport-serial` or any transport crate. It uses the
   `Radio` trait, not concrete transports or sessions.
5. **`app/main.rs`** is the ONLY place concrete types are wired together.
6. Unit tests use **mock/fake implementations** of the relevant trait —
   never the real impl from another crate.
   - `radio` tests use an in-crate `FakeCatSession` (not a real transport)
   - `ui` tests use an in-crate `MockRadio` impl of the `Radio` trait
7. Never depend on `ts570d` directly. It is a sibling application, not a
   dependency — both repos depend on the shared library, not on each other.

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
should contain the same abstract radio concepts `ts570d`'s `Radio` trait
does — frequency control, mode, PTT, meters, gain controls, power, scan,
RIT/XIT, noise blanker, memory channels, squelch, preamplifier, attenuator,
VOX, etc. — to the extent the FT-991A supports them; do not assume parity
without checking the manual.

FT-991A-specific features (anything analogous to TS-570D's keyer, voice
synthesizer, antenna tuner, menu access — the FT-991A's own distinguishing
features, once identified from its manual) live in the `radio` crate as
inherent methods on `Ft991a`, NOT in the `Radio` trait.

## Architecture
- `radio/`: FT-991A command table, `CatRadio` impl, controller client, `Radio` trait + domain types
- `ui/`: Ratatui terminal interface (depends on `cat-framework` + `radio`) — later dispatch wave
- No local `serial/` crate — transport is `cat-transport-serial` (external, see above)
- `emulator/`: Virtual TTY + radio emulator, runs `CatFramework<Ft991aRadio>` — later dispatch wave
- No `docs/architecture/network-readiness.md`-equivalent exists yet in this
  repo; `ts570d`'s copy documents the pattern this repo will inherit once its
  `CatSession` usage is implemented.

## Code Style
- Imports: std → external → local
- Error handling: thiserror + Result<T, E>
- Naming: snake_case/PascalCase conventions
- Async: monoio runtime throughout — tokio is NEVER used

## Testing Strategy
- Unit tests for individual components (e.g. `radio` crate tests drive
  `cat_framework::CatFramework<Ft991aRadio>` directly, in-process — no PTY
  needed for command-table/state-machine coverage, mirroring `ts570d`)
- Integration tests with virtual TTY, once the `emulator` crate lands
- Linux-only testing with emulator (matching `ts570d`'s io_uring constraint)

## Linux-Specific
- io_uring kernel requirements (5.1+) — provided by `cat-transport-serial`,
  not reimplemented locally
- Serial port permissions and udev rules
- Virtual TTY via pseudo-terminals
