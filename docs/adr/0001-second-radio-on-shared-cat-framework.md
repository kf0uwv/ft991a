# 1. Second radio on the shared CAT framework

Date: 2026-07-15

## Status

Accepted. **Not started** — blocked on `radio-cat-rs` extraction and/or an
explicit go-ahead (see "Consequences").

## Context

The sibling repository `ts570d` (Kenwood TS-570D/S CAT control) was refactored
into a radio-independent generic CAT engine (`framework`) plus a
TS-570D-specific `radio` crate, explicitly so that a second transceiver could
reuse the generic engine without change. Its ADR 0004 ("Extraction boundary
for a shared CAT library") records the shape a second radio takes and states
plainly: "No second radio is implemented in this work." Its ADR 0005
("Network transport and server/control mode readiness") goes further and
names this repository directly: "`ft991a` is the eventual second `CatRadio`
implementation," to be built once `framework` (and the transport-independent
`CatSession` boundary introduced there) is extracted into a shared-library
repository, `radio-cat-rs`.

This repo is that second radio: Yaesu FT-991A CAT control. `radio-cat-rs` is
being scaffolded in parallel, in another concurrent workstream, and does not
yet contain usable crates. We want the intended dependency and design
boundary recorded now, before any code exists, so that when `radio-cat-rs` is
ready, this repo's implementation is a straightforward build against a known
shape rather than a redesign — the same reasoning `ts570d` ADR 0004 applied to
its own extraction boundary.

## Decision

### Depend on the shared library, not on `ts570d`, and not on a duplicated local engine

This repository will depend on the shared-library crates published by
`radio-cat-rs` — `cat-framework` (generic command table, parser, dispatch
lifecycle, response builder, `CatCommandCatalog`/`CatRadio` traits) and, once
available, `cat-client`/`CatSession` and one or more `cat-transport-*`
crates — as external dependencies, once that repository extracts and
publishes them.

This repo will **not**:
- depend on `ts570d` directly (it is a sibling, not a dependency — the whole
  point of extraction is that neither radio depends on the other);
- vendor or duplicate a local copy of the generic CAT engine (no local
  `framework`-equivalent crate is created here, even temporarily, to unblock
  early work — see "Consequences" for what to do instead if unblocked before
  `radio-cat-rs` is ready).

### Shape of a second radio (per `ts570d` ADR 0004)

Per `ts570d` ADR 0004's "Adding a second radio" section, this repo reuses the
shared engine unchanged and provides only its own:

- `Ft991aCommandId` enum;
- a static `FT991A_COMMAND_TABLE: cat_framework::CommandTable<Ft991aCommandId>`;
- a state machine (`Ft991aState`, transitions, state-dependent validation);
- `Ft991aEvent` / `Ft991aError` types;
- a `CatRadio` implementation (`Ft991aRadio`) supplying command definitions
  and command semantics — the shared engine handles framing, lookup, parsing,
  structural validation, and response formatting; this crate decides what
  each command *means*.

Illustrative shape (mirrors `ts570d` ADR 0004's illustrative `FakeRadio`, not
real FT-991A commands):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ft991aCommandId { Frequency /* ... */ }

struct Ft991aRadio { /* state machine fields */ }

impl cat_framework::CatCommandCatalog for Ft991aRadio {
    type CommandId = Ft991aCommandId;
    fn command_table(&self) -> &'static cat_framework::CommandTable<Self::CommandId> {
        &FT991A_COMMAND_TABLE
    }
}
```

### The FT-991A command set is not the TS-570D command set

The FT-991A is a Yaesu radio; `ts570d` is Kenwood. Command codes, parameter
encodings, field widths, and response layouts are unrelated between the two
manufacturers' CAT protocols — nothing in `TS570D_COMMAND_TABLE` transfers by
assumption, structurally or by value. When implementation begins,
`FT991A_COMMAND_TABLE` must be derived from the official Yaesu FT-991A CAT
operation reference manual, command by command, the same discipline
`ts570d`'s `kenwood` agent applied to the Kenwood TS-570D manual (see
`ts570d`'s ADR 0003 and `.claude/agents/kenwood.md`). The manual is not yet
present in this repository; see `.claude/agents/yaesu.md` for the acquisition
and read-before-implementing requirement.

### Depend on the `CatSession` boundary from day one

Per `ts570d` ADR 0005, this repo's controller client and UI must be
transport-independent from the start — generic over the shared library's
session abstraction (`CatSession`, sitting above the byte-level `Transport`
trait), never over a concrete transport type. Concretely:

- an `Ft991a<S: CatSession>` controller client, analogous to `ts570d`'s
  `Ts570d<S: CatSession>`;
- the UI depends on this repo's `Radio` trait and FT-991A domain types only,
  never on `serial`, `tcp`, `udp`, or any other transport crate;
- only the application wiring layer (this repo's `src/main.rs` equivalent)
  names a concrete `CatSession` implementation.

This repo therefore inherits the same serial/TCP/UDP/mock readiness `ts570d`
built for itself, without having to redesign it — because it is consuming
the same shared abstraction, not reinventing one.

### Status: blocked, not started

No Rust code, `Cargo.toml`, or command implementation exists in this
repository. This ADR records intent and boundary only. Work does not begin
until:

1. `radio-cat-rs` has extracted and published at least `cat-framework` (and
   ideally `cat-client`/`CatSession`) in a state this repo can depend on; and
2. an explicit architect/user go-ahead to start implementation; and
3. the official Yaesu FT-991A CAT manual is available in this repository for
   the `yaesu` agent to work from.

## Consequences

- Extraction readiness in `ts570d` and scaffolding readiness in
  `radio-cat-rs` are both prerequisites outside this repo's control; this
  repo cannot get ahead of them by design (no vendored/duplicated engine).
- If implementation pressure arrives before `radio-cat-rs` is ready, the
  correct response is to unblock `radio-cat-rs` (or explicitly re-scope this
  ADR), not to build a local generic-engine substitute here — doing so would
  recreate the exact coupling `ts570d` ADR 0001 eliminated and would need to
  be un-done later.
- Because this repo depends on the shared library rather than on `ts570d`,
  the two radio repos remain independent siblings: neither can break the
  other, and a bug fix or feature in the shared engine benefits both once
  each repo bumps its dependency.
- The FT-991A command table, state machine, and domain types are new work
  requiring the Yaesu manual — none of `ts570d`'s TS-570D-specific code
  (`ts570d_radio.rs`, `protocol/*`, domain types) is reusable here beyond
  serving as a structural example of how a `CatRadio` implementation is
  organized.
- This ADR does not select a crate layout or workspace structure beyond
  naming the expected pieces (`radio`, `ui`, `serial`, `emulator`,
  application wiring) — a future ADR (or the architect's task plan) settles
  the concrete `Cargo.toml`/workspace shape once `radio-cat-rs` publishes
  something to depend on.
