# 4. Shared diagnostics screen (`cat-diagnostics`)

Date: 2026-07-26

## Status

Superseded by [ADR 0006](0006-hand-coded-full-parity-diagnostics.md)
(2026-07-26, later the same day) — the user chose full parity with
`ts570d`'s own diagnostics screen over this design's read-only-liveness
trade-off. `radio` no longer depends on `cat-diagnostics` at all; the
engine now lives entirely in `ui`. Kept below for historical context.

## Context

`radio-cat-rs` ADR 0007 (consumed locally per [ADR 0003](0003-consume-radio-cat-rs-windows-network-transport.md))
adds a new, radio-generic `cat-diagnostics` crate: `run_diagnostics`/
`run_diagnostics_with(client: &mut CatClient<C, S>, table: &'static
CommandTable<C>, config, on_progress)` exercises every command's documented
read form and reports pass/fail/timeout/skipped + latency, mirroring the
capability behind `ts570d`'s existing `[D]` diagnostics screen but without
that screen's radio-specific set/get value-correctness checks (liveness
only — see `cat-diagnostics`'s own ADR for the full trade). This ADR
records how `ft991a` wires it in.

### The question the task explicitly asked to settle

The task brief suggested calling `cat_diagnostics::run_diagnostics(...)`
directly from `ui`, and asked to confirm whether that would violate this
repo's "`ui` never imports a transport crate" rule (it would not —
`cat-diagnostics` depends only on `cat-framework`/`cat-client`/
`cat-transport-core`, the same tier `cat-client` itself already sits at,
never a concrete transport). That framing turned out to be the wrong
question. Reading `ui/src/lib.rs` and `radio/src/ft991a.rs` found a harder,
type-level fact:

- `ui::run<R: Radio + Ft991aExtras + CwKeying + 'static>` only ever holds a
  generic `R` value. It never sees the concrete `Ft991a<S>` struct.
- `Ft991a<S>`'s `client: cat_client::CatClient<Ft991aCommandId,
  SharedSession<S>>` field is `pub(crate)` — invisible outside the `radio`
  crate.
- `cat_diagnostics::run_diagnostics_with` requires `&mut CatClient<C, S>`
  **directly** as its first parameter.

So `ui` cannot call it — not "should not" for dependency-hygiene reasons,
but genuinely cannot, for the same reason `ui` can't call any other
`CatClient`-level API today. This is not a judgment call to weigh; it
settles the design by itself. `radio` is the only crate with a `CatClient`
to hand `cat_diagnostics`, so `radio` must wrap it.

## Decision

### 1. `radio` wraps `cat_diagnostics::run_diagnostics_with`; `ui` never depends on `cat-diagnostics` at all

New `radio/src/diagnostics.rs` defines three concrete, non-generic types —
[`DiagnosticResult`], [`DiagnosticOutcome`], [`DiagnosticSummary`] —
mirroring `cat_diagnostics`'s own `CommandResult`/`CommandOutcome<C>`/
`DiagnosticReport<C>` shapes exactly, minus the `id: C` field (`ui` has no
use for a typed command id, only the human-readable `code`/`name`) and
minus the `C: cat_framework::CommandId` generic parameter entirely. A new
`Ft991aExtras::run_diagnostics_with<F>(&mut self, on_progress: F) ->
RadioResult<DiagnosticSummary> where F: FnMut(&DiagnosticOutcome)` trait
method (default body: `Err(RadioError::NotImplemented)`, this trait's
established idiom) is the only public surface; `Ft991a<S>`'s override
(`ft991a.rs`) is the only real implementation, converting
`cat_diagnostics`'s types to `radio`'s own as each outcome arrives.

**Consequence, not incidental:** since `radio::DiagnosticSummary`/
`DiagnosticOutcome`/`DiagnosticResult` carry no `cat_framework`/
`cat_diagnostics` type at all, `ui` can render the whole screen using only
what it already depends on. `ui/Cargo.toml` gains **no new dependency** —
confirmed by `cargo build -p ui` never pulling in `cat-diagnostics`
transitively for that crate's own compilation unit. This is the natural
consequence of §1's hard constraint, not a second, independently-argued
design choice — worth stating plainly since the task explicitly asked
whether `ui` importing `cat-diagnostics` would have been a rule violation
(it would not have been, per Context above; it's simply unnecessary once
the real constraint is satisfied).

### 2. Cross-cutting `[D]` escape hatch, not a 13th `CommandGroup`

Mirrors `[L]` Profiles' own precedent exactly (`control.rs`'s
`PROFILE_LIST_KEY` doc comment): diagnostics exercises the *entire*
`FT991A_COMMAND_TABLE`, not one thematic group, so it lives alongside
`PROFILE_LIST_KEY`/`EX_NUMBER_ENTRY_KEY` as its own `Menu`-level key
(`DIAGNOSTICS_KEY = 'D'`, matching `ts570d`'s own `[D]` mnemonic),
verified unique against every other top-level key by
`test_diagnostics_key_is_unique`.

### 3. A new `KeyResult::RunDiagnostics` variant, not folded into `ExecuteAction`

`handle_key` (`control.rs`) is a pure, synchronous function with no radio
access — by design (its signature takes no `&mut R`). Running diagnostics
needs live radio access `handle_key` structurally cannot have, so pressing
`[D]` from `Menu` produces a new `KeyResult::RunDiagnostics` (no
`ControlState` transition at all — `handle_key` leaves `state` at `Menu`),
distinct from `KeyResult::Execute(ExecuteAction)`. `terminal.rs`'s
`run_loop` (which already holds both `radio` and `terminal`) is the only
place that acts on it, via a new `run_diagnostics_screen` function.

This is a deliberate departure from every other action in this app, all of
which map onto one `execute_action` call producing a single `(desc,
RadioResult<String>)` pair rendered as one `ControlState::Feedback` screen.
A diagnostics run is qualitatively different: dozens of individual command
probes, each worth showing live progress for as it happens (mirroring
`ts570d`'s own diagnostics screen's "Now testing: `<label>`" progress).
Folding it into `ExecuteAction` would have meant either blocking with no
visible progress until the whole run finished, or inventing a second,
parallel plumbing path anyway — so a dedicated `KeyResult` variant is more
honest about the real shape of the feature, not more complexity for its
own sake.

### 4. Live progress via direct, repeated `Terminal::draw` calls from a synchronous callback — no channel needed

`run_diagnostics_screen` calls `radio.run_diagnostics_with(|outcome| {
outcomes.push(outcome.clone()); draw_diagnostics_frame(terminal, display,
&outcomes, total); })`. This works, and needs no new concurrency
primitive, for one specific reason worth recording: `ratatui::Terminal::
draw` is a **plain synchronous function** — it does no I/O awaiting of its
own — so calling it from inside a sync `FnMut` closure that a single
`.await`ed `run_diagnostics_with` call invokes repeatedly is exactly as
safe as calling it from `run_loop`'s own synchronous match arms. This
crate's existing single-sequential-loop architecture (`terminal.rs`'s own
module docs, chosen over `ts570d`'s two-task/channel design because this
app's poll set is much smaller) already established that no concurrent
task exists to keep servicing key events during a slow operation — a
diagnostics run blocks the whole event loop for its duration, same as
`ts570d`'s own diagnostics screen blocks *its* radio task. Bounded and
acceptable: worst case (every command errors) is roughly `command_count ×
DEFAULT_COMMAND_TIMEOUT` (91 × 2s ≈ 3 minutes); the common case (a live or
emulated radio actually answering) completes in well under a second, as
observed manually against the `emulator` (see Consequences).

### 5. Rendering: one shared `draw_diagnostics_panel`, two call sites

`layout.rs`'s new `draw_diagnostics_panel(f, area, outcomes, total,
cursor)` serves both:
- **live progress** (`cursor: None`, called directly by `terminal.rs` via
  the thin `draw_diagnostics_live` wrapper — which also draws the same
  outer " Controls " bordered block `draw_control_panel` itself draws, so
  the live screen looks identical to every other screen mid-run) — the
  view auto-scrolls to follow the tail as `outcomes` grows;
- **the completed report** (`cursor: Some(n)`, via `ControlState::
  Diagnostics { summary, cursor }` and `draw_control_panel`'s normal
  dispatch) — same scrolling-list shape as `draw_ex_sub_group_menu`/
  `draw_profile_list` (cursor-centered window), with `Up`/`Down` to scroll
  and `Esc` back to `Menu`, plus a detail line for the selected row
  showing its full request/response or error text.

## Consequences

- `radio/Cargo.toml` gains `cat-diagnostics` as a new dependency (alongside
  the temporary `[patch]` from ADR 0003); `ui/Cargo.toml` gains **nothing
  new** — still `radio` + `thiserror` + `ratatui` + `crossterm` only.
- `radio/src/ft991a.rs`'s test module gains an `AutoAckSession` fake
  `CatSession` (answers every `execute()` call as an immediate success,
  echoing the request back) specifically so `run_diagnostics_with`'s
  wiring can be exercised against the **real, full** 91-command
  `FT991A_COMMAND_TABLE` without hand-scripting 90+ exact per-command
  `ScriptedCatSession` exchanges — `cat_client::CatClient::query_with_param`
  never validates response *content* shape (a radio-layer concern,
  confirmed by reading `cat-client/src/client.rs::execute_query`), so this
  is sufficient and not a weaker test than the alternative.
- Verified end-to-end against the live `emulator` (Linux, via `tmux`):
  pressing `[D]` from a connected `ft991a --port <pty>` session showed live
  per-command progress, landed on "63 passed / 0 failed / 28 skipped / 91
  total," scrolled correctly with `Up`/`Down` (including a per-row
  request/response detail line), and returned cleanly to `Menu` on `Esc`.
- This screen's "passing" inherits `cat-diagnostics`'s own documented
  trade: liveness only (the command answered), not `ts570d`-style
  set/get value-correctness. Anyone wanting deeper FT-991A-specific
  verification would need to add that separately in this app's own `ui` or
  `radio` layer — out of this ADR's scope, matching `cat-diagnostics`'s own
  ADR's explicit non-goal.
