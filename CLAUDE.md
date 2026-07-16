# Yaesu FT-991A Radio Control - Agent Guidelines

## Current Status: SCAFFOLDING ONLY — no implementation yet

There is no Rust code, no `Cargo.toml`, and no crate in this repository. Do
**not** run `cargo init`, do **not** write `.rs` files, and do **not**
implement any FT-991A CAT command until:

1. the shared-library repo `radio-cat-rs` has published `cat-framework`
   (and ideally `cat-client`/`CatSession`) in a state this repo can depend
   on;
2. the official Yaesu FT-991A CAT operation reference manual has been added
   to this repository for the `yaesu` agent to work from; and
3. the architect/user has given an explicit go-ahead.

See `docs/adr/0001-second-radio-on-shared-cat-framework.md` for the full
decision record and blocked status, and `docs/adr/README.md` for a running
status summary. Every rule below describes the architecture this repo will
have once unblocked — it is binding on future implementation work, not a
description of anything that exists today.

## Superpowers Coding Model (MANDATORY, once implementation starts)
- Use planning-with-files skill for ALL implementation work
- Follow TDD, frequent commits, verification-before-completion
- Check for applicable skills BEFORE any action

## Planning-with-Files Requirement
- Each agent and subagent must maintain their own planning-with-files in a directory under `./planning/` with their name
- Directories: `./planning/architect/`, `./planning/app/`, `./planning/yaesu/`, `./planning/serial/`, `./planning/ui/`, `./planning/emulator/`, `./planning/code_review/`
- Planning files include: `task_plan.md`, `findings.md`, `progress.md` in each agent's directory
- This prevents conflicts between agents working on different aspects of the project
- Planning files must be created and maintained before any implementation work
- These directories already exist with starter `task_plan.md` files recording the blocked status above — update them in place, do not replace the convention

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

## Core Technologies (once implementation starts)
- monoio: io_uring async runtime — same as `ts570d`; tokio is NEVER used
- ratatui + crossterm: Terminal UI
- Shared CAT engine and transport abstractions consumed from `radio-cat-rs`
  (crate names expected: `cat-framework`, `cat-client`, `cat-transport-serial`,
  `cat-transport-tcp`, `cat-transport-udp` — exact names are set by that
  repo, not this one)
- FT-991A protocol emulator with virtual TTY, for testing without hardware

## Essential Commands
No `Cargo.toml` exists yet — none of these are runnable today. Recorded for
when a workspace is created:
- Build: `cargo build` / `cargo build --release`
- Test: `cargo test` / `cargo test test_name`
- Lint: `cargo clippy` / `cargo fmt`
- Emulator: `cargo run --bin emulator`

## Crate Dependency Model (MANDATORY — ALL AGENTS MUST FOLLOW, once crates exist)

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

serial  (depends on: cat-framework / cat-transport-serial — see .claude/agents/serial.md;
          whether this repo reimplements a transport or wraps cat-transport-serial
          is an OPEN DECISION, not yet made)

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
  └── NEVER imports from serial or any transport crate

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
   but NEVER on `serial` or any transport crate. It uses the `Radio` trait,
   not concrete transports or sessions.
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

## Architecture (once implementation starts)
- `radio/`: FT-991A command table, `CatRadio` impl, controller client, `Radio` trait + domain types
- `ui/`: Ratatui terminal interface (depends on `cat-framework` + `radio`)
- `serial/`: transport layer — shape not yet decided, see `.claude/agents/serial.md`
- `emulator/`: Virtual TTY + radio emulator, runs `CatFramework<Ft991aRadio>`
- No `docs/architecture/network-readiness.md`-equivalent exists yet in this
  repo; `ts570d`'s copy documents the pattern this repo will inherit once its
  `CatSession` usage is implemented.

## Code Style (once implementation starts)
- Imports: std → external → local
- Error handling: thiserror + Result<T, E>
- Naming: snake_case/PascalCase conventions
- Async: monoio runtime throughout — tokio is NEVER used

## Testing Strategy (once implementation starts)
- Unit tests for individual components
- Integration tests with virtual TTY
- Linux-only testing with emulator (matching `ts570d`'s io_uring constraint)

## Linux-Specific (once implementation starts)
- io_uring kernel requirements (5.1+), if this repo implements its own serial
  transport rather than consuming `cat-transport-serial`
- Serial port permissions and udev rules
- Virtual TTY via pseudo-terminals
