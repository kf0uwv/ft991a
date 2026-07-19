# Architect Progress

## 2026-07-17 — Unblocking review + first-implementation design

- Confirmed all three blockers in ADR 0001 have cleared: `radio-cat-rs` has
  published `cat-framework`/`cat-client`/`cat-transport-core`/
  `cat-transport-serial` (commit `0c13844`, consumed already by `ts570d`);
  `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` is present; user gave an
  explicit go-ahead.
- Updated status in `docs/adr/0001-second-radio-on-shared-cat-framework.md`,
  `docs/adr/README.md`, and `CLAUDE.md` (status header/body, resolved the
  `serial`-crate "OPEN DECISION" to "no local `serial` crate — depend on
  `cat-transport-serial` directly", removed stale "(once implementation
  starts)" qualifiers).
- Read `ts570d`'s post-remap reference implementation
  (`Cargo.toml`/`radio/Cargo.toml`/`src/main.rs`/`radio/src/ts570d.rs`/
  `radio/src/ts570d_radio.rs`) and the actual `radio-cat-rs` crates
  (`cat-framework/src/cat.rs`, `cat-client/src/client.rs`,
  `cat-transport-serial/src/lib.rs` + `io_uring.rs`) as ground truth.
- Read the full 20-page FT-991A CAT manual and derived an 11-command
  first-slice `FT991A_COMMAND_TABLE` scope (`FA`, `FB`, `MD`, `TX`, `SM`,
  `PS`, `AG`, `RG`, `SQ`, `PC`, `ID`), each cited to a manual page, plus an
  explicit non-goal list for follow-on waves (`IF`, `RM`/`RI`, memory
  channels, the 153-entry `EX` menu table, etc.).
- Found and documented a structural gotcha: several FT-991A read commands
  (`MD`, `SM`, `AG`, `RG`, `SQ`) take a selector parameter even on read,
  which `cat-framework`'s parser only classifies via `set_forms` (not a
  true zero-width `Query`) — the same pattern `ts570d`'s own `SM`/`MR`
  commands already use; documented the reusable fix shape.
- Designed `Ft991aState`/`Ft991aRadio`/`CatRadio` impl shape and the
  `Ft991a<S: CatSession>` controller client (reusing `ts570d`'s
  `SharedSession<S>` adapter near-verbatim) — see
  `planning/architect/task_plan.md` §3–4.
- Wrote the Wave 1 dispatch queue into `planning/architect/task_plan.md`:
  a `yaesu` task (radio crate, first slice) and an `app` task (workspace
  scaffold + minimal wiring binary against a placeholder `ui` stub).
  Explicitly deferred `ui` (needs a stable `Radio` trait first) and
  `emulator` (not needed to test Wave 1 — `CatFramework::process_frame` unit
  tests suffice, mirroring how `ts570d_radio.rs` tests itself) to later
  waves, with reasoning recorded.
- Did not write any `.rs`/`.toml` files or run `cargo init`, per the
  architect's code-editing prohibition. Did not touch `ts570d` or
  `radio-cat-rs`.
- Next: present this plan to the user for review; on approval, dispatch the
  `yaesu` and `app` tasks (one at a time per the Architect Review Workflow,
  or in parallel since their file scopes don't overlap — user's call).

## 2026-07-17 — Wave 2 design: `ui`/`emulator` dispatch queue

- Confirmed Wave 1 landed and was committed (`e3698cf`): `radio` crate (11
  commands), real `Ft991a<S: CatSession>` client, first-slice `Radio`
  trait, `ui` crate reduced to a genuine placeholder stub (`run()` returns
  `Ok(())` immediately).
- Read the committed `radio/src/{radio_trait.rs,ft991a.rs,lib.rs}` in full
  (not the §4 sketch) to scope Wave 2 against the real trait surface.
- Read `ts570d/ui/src/{layout.rs,control.rs}` in full as structural
  reference (not for menu-item porting) and grepped `terminal.rs` for
  `run`/`poll_radio_state` signatures and poll cadence.
- Read `ts570d/emulator/src/{emulator.rs,main.rs,lib.rs,pty.rs,io.rs,
  port.rs,logger.rs,tui.rs}` and `Cargo.toml` in full — confirmed which
  files are genuinely radio-generic (7 of 9: `Cargo.toml`, `lib.rs`,
  `pty.rs`, `io.rs`, `port.rs`, `logger.rs`, `main.rs`) vs. which need real
  FT-991A-specific rewriting (`emulator.rs`'s 2 type substitutions,
  `tui.rs`'s field set) — see task_plan.md §7.1's delta table.
- Designed a **right-sized, flat** `ui` control-state machine (`Normal` →
  `{TextInput, ListSelect}`, no `GroupMenu` layer) for the 9 write-capable
  commands + 2 read-only display values — explicitly not a scaled-down
  port of ts570d's 8-group/60-item menu tree, with reasoning recorded
  (task_plan.md §6.1) for why that tree exists in ts570d (command count)
  and why it isn't warranted here yet. Reused ts570d's `CommandKind`
  descriptor pattern and connection-health/`draw_disconnected` handling
  unchanged, since those are genuinely command-count-independent.
  Explicitly deferred ts570d's `Diagnostic` mode (no `diag.rs` this wave).
  Worked out the one place the FT-991A's trait genuinely needs bespoke UI
  logic not present in ts570d: the 3-valued `TxState` (`Off`/`CatKeyed`/
  `RadioKeyedNonCat`) needs a 3-way render and an explicit "T doesn't clear
  external TX" note, since ts570d's TX/RX is a simple write-only bool.
- Designed the `emulator` crate as a near-verbatim mirror of
  `ts570d/emulator`, per this dispatch's framing that emulator
  infrastructure doesn't scale with command-table size — confirmed that
  framing file-by-file rather than assuming it. Flagged one real
  uncertainty for the `emulator` agent to resolve against the manual, not
  copy blindly: `port.rs`'s hardcoded physical-mode baud (ts570d uses
  4800; FT-991A's own default is 9600 per menu 029/031).
- Wrote the Wave 2 dispatch queue into `planning/architect/task_plan.md`
  §8: Task 3 (`ui` agent) and Task 4 (`emulator` agent), called to run in
  **parallel** (checked, not assumed, that neither's deliverable depends on
  the other's output — both depend only on the already-landed `radio`
  crate, and their file scopes don't overlap), plus a sequential Task 5
  (`app` agent) to wire them together afterward, since that task's entire
  point is combining both Wave 2 deliverables and cannot start before they
  exist.
- Decided **no new ADR** is warranted for the UI/emulator design — checked
  `ts570d/docs/adr/README.md`'s 5 ADRs first and found none of them cover
  ts570d's own UI design (menu shape, LCD convention, keybindings), only
  cross-cutting architectural boundaries (domain-type placement, command
  table cardinality, extraction boundary, transport readiness). Recorded
  the decision and reasoning in task_plan.md §9 instead. Updated
  `docs/adr/README.md`'s repository-status paragraph to reflect Wave 1
  landed / Wave 2 dispatched, without adding an ADR table row.
- Did not write any `.rs`/`.toml` files, run `cargo init`, or touch
  `ts570d`/`radio-cat-rs`, per the architect's code-editing prohibition.
- Next: present this plan to the user for review; on approval, dispatch
  Tasks 3 and 4 in parallel (per the parallel-vs-sequential call above),
  then Task 5 once both are reviewed and approved.

## 2026-07-18 — Wave 3 design: full CAT coverage + RTS/DTR PTT/CW-keying

- Scoped a large new ask: (A) full CAT command-table coverage (11 of 91
  top-level commands implemented today) and (B) a genuinely new
  capability — PTT/CW keying via RS-232/USB hardware modem control lines
  (RTS/DTR), as an alternative to the `TX;` CAT-command path.
- Read the current landed Wave-1 code in full
  (`radio/src/{ft991a_radio.rs,ft991a.rs,radio_trait.rs}`, `src/main.rs`)
  to ground the RTS/DTR design against the real `Ft991a<S: CatSession>` /
  `SharedSession<S>` shape, not the Wave-1 sketch.
- Read `radio-cat-rs`'s `cat-transport-core`/`cat-transport-serial` source
  and `Cargo.toml`s in full — confirmed neither `Transport` nor
  `SerialPort` exposes runtime RTS/DTR control today (only a one-time
  open-time assert), confirmed `SerialCatSession.transport` is public, and
  confirmed `cat-transport-core` has no `libc`/`nix` dependency (kept that
  way — trait signatures only).
- Re-read the full 20-page FT-991A CAT manual, this time transcribing
  every command in the p.3 master table (91 total) and the full 153-row
  `EX` menu table (p.7-9), plus the p.1 RS-232C pinout table.
- **Found and recorded two corrections to the user's brief**, from direct
  manual reading rather than trusting the summary: (1) the RS-232C 9-pin
  CAT connector has no DTR pin at all (only RTS/CTS, per the p.1 pinout
  table) — DTR-based keying is USB-only, unverified in this manual; (2)
  there is no single "048 PTT SELECT" item — 048 is "AM PORT SELECT," and
  the actual PTT-select items are four *mode-specific* entries (047/071/
  076/108, each paired with a DATA-vs-USB port-select sibling) tied to a
  different physical connector (rear DATA jack / USB audio) than the CAT
  port, used for soundcard-driven PTT this app has no other reason to
  touch. Narrowed this wave's RTS/DTR feature to Menu item **060 "PC
  KEYING"** — CW keying over the *existing* CAT connection — and recorded
  047/071/076/108 as an explicit non-goal pending a future soundcard wave.
- Designed the RTS/DTR capability as a new `ModemControlLines` trait in
  `cat-transport-core` (sync fns, matching the `flush_rx` precedent — not
  `#[async_trait]`), implemented concretely for `SerialPort` in
  `cat-transport-serial` via `TIOCMBIS`/`TIOCMBIC`/`TIOCMGET` (generalizing
  the ioctl mechanism `SerialPort::open` already prototypes once), plus a
  blanket delegating impl for `SerialCatSession<T: Transport +
  ModemControlLines>`. Deliberately **not** added to the base `Transport`/
  `CatSession` traits (per the brief's explicit instruction) — this is a
  cross-repo `radio-cat-rs` task, flagged to dispatch first via that
  repo's own agents, since this repo's architect/agents don't touch
  `radio-cat-rs`.
- Designed the `ft991a`-side consumption as an **additive** second impl
  block on `Ft991a<S>` (bounded on `S: CatSession<Error=TransportError> +
  ModemControlLines`), plus a `SharedSession<S: ModemControlLines>`
  delegating impl mirroring the existing `CatSession` delegation exactly.
  Confirmed this requires **zero `main.rs` changes** for the RS-232C
  single-port case — `SerialPort`/`SerialCatSession` already flow through
  unchanged, so the new methods become callable automatically. Explicitly
  deferred the USB dual-port case (not manual-cited, no concrete need yet,
  would require a second independent modem-control handle) rather than
  design it speculatively.
- Grouped the remaining 79 non-`EX` top-level commands into 10 batches
  (§10.5 of `task_plan.md`), and found a cross-batch dependency worth
  reordering around: `IF`/`MR`/`MT`/`OI` share an identical composite
  payload shape, so the Meters/Status batch (containing `IF`, Wave-1's own
  deferred item) should land before the Memory-channel-records batch and
  before the tail of the Misc batch that needs `OI`.
- Confirmed `EX` is genuinely one composite command (`EX<3-digit menu
  number><item-specific-width value>;`), not 153 separate codes — traced
  through a deliberately-malformed-parameter-width example to confirm
  `cat-framework`'s existing "structural match, then per-item semantic
  validation" pattern (already used by `FA`'s range check) needs no new
  framework capability here, just ~6 `CommandForm` widths and a per-item
  lookup table — confirming the brief's suspicion that this is much less
  framework work than 91-more-commands' worth, though still substantial
  domain-modeling work (153 items, one field's encoding — item 087 "RADIO
  ID" — not resolvable from this manual at all, flagged not guessed).
  Carved out a small priority sub-batch (plumbing + the 9 PTT/keying-
  relevant `EX` items, including 060) to front-load ahead of the other
  ~144 items, directly serving this wave's marquee feature.
- Flagged (not designed) that `ui`'s Wave-2 flat 9-key design has now hit
  its own stated growth trigger (Wave 2 §6.1) at ~90 commands + a 153-item
  settings menu, and sketched a rough future shape (grouped menu reusing
  the CAT-batch groupings, plus a numeric-entry escape hatch for `EX`) —
  explicitly deferred to its own future wave, not designed in detail now,
  per the brief's instruction.
- Wrote the full Wave 3 dispatch queue into `planning/architect/
  task_plan.md` §10.8: the cross-repo `radio-cat-rs` task first (parallel
  with CAT-batch work, not strictly serial before it, since it only blocks
  the `ft991a`-side consumption task); then Meters/Status, then `EX`
  plumbing+PTT-subset, then Memory records, then the remaining CAT batches
  in flexible order, then the `ft991a`-side RTS/DTR consumption task
  (blocked on the cross-repo task), then the remaining `EX` sub-batches,
  then a future `ui`/`app` follow-up wave.
- Did not write any `.rs`/`.toml` files, dispatch any subagent, or touch
  `ts570d`/`radio-cat-rs`, per the architect's standing constraints (used
  `Read`/`Bash` read-only against both sibling repos for research only).
- Next: present this plan to the user for review. On approval: route task
  1 (§10.8) to `radio-cat-rs`'s own agents via the coordinating session;
  dispatch task 2 (`yaesu`, Meters/Status batch) as the first in-repo CAT
  coverage task, one `yaesu` dispatch at a time per the Architect Review
  Workflow.

## 2026-07-19 — Wave 4 design: full `ui`/`emulator` redesign against landed command coverage

- Confirmed the trigger §10.7 flagged has been pulled: `radio` now
  implements all 91 top-level commands and 151/153 `EX` items; `ui` is
  unchanged since Wave 2 (verified by reading the actual committed
  `ui/src/{control.rs,layout.rs,terminal.rs,lib.rs}` in full, not assumed
  from planning prose) — still the flat 9-key `Normal` screen; `emulator/
  src/tui.rs` is likewise unchanged since Wave 2 (verified via grep of its
  function/struct list).
- Read `radio/src/ft991a_radio.rs` (`Ft991aCommandId`'s 91 variants + batch
  doc-comments, `ExMenuValueKind`/`ExMenuItem`/`EX_MENU_TABLE`,
  `Ft991aState`'s grown field set), `radio/src/radio_trait.rs` (full ~90-
  method `Radio` trait via grep), and `radio/src/ft991a.rs` (full method
  list via grep) to ground the design against the real, current command/
  trait surface rather than the abstract batch list.
- **Found a real, load-bearing gap**: ~20 client-side methods (keyer memory
  store/play, QMB, quick split, VFO/memory toggle, raw encoder nudges, ENT
  key, mic up/down, antenna tuner state, dimmer, date/time/tz, opposite-band
  info, TXW, DVS record/playback, contour, APF, manual notch, parametric mic
  EQ, the `IF` composite read) exist only as `Ft991a<S>` inherent methods,
  never added to the `Radio` trait — confirmed this follows `CLAUDE.md`'s
  "Radio trait scope" rule rather than being an oversight, and cross-checked
  against `ts570d`'s actual practice (which reads that same policy more
  generously in places, e.g. keyer speed and antenna-tuner-thru *are* on
  `ts570d`'s `Radio` trait) to calibrate where `ft991a`'s own trait already
  drew a reasonable, already-generous line.
- **Found a second real gap**: no `get`/`set_ex_menu_item` exists anywhere
  in `radio` today — the `EX` menu's two access paths (§10.7) are both
  blocked on a new client-side capability, not just a UI-side design
  exercise. Also flagged that `ExMenuValueKind::Enumerated` carries no
  human-readable labels (raw wire digits only), a real UX gap for a
  `ListSelect`-driven `EX` value picker across 90+ enumerated items.
- **Resolved the RTS/DTR keybinding placement question concretely** (the
  brief's explicit ask, not deferred again): traced why `main.rs` can't host
  it (owns no event loop once `ui::run(radio)` is called — `radio` is moved
  by value, single sequential loop, no interleaving point), then traced why
  `ui::run<R: Radio>` can't reach `Ft991a::assert_rts` as-is (inherent
  method, and confirmed via Rust's coherence rules that it can *never*
  become a `Radio` trait method with the rest of the trait's
  default-`NotImplemented`-body idiom, since that would require two
  overlapping `impl Radio for Ft991a<S>` blocks — E0119, needs unstable
  specialization). Resolution: two new `radio`-crate traits (`Ft991aExtras`,
  unconditional; `CwKeying`, bounded on `+ ModemControlLines` same as the
  existing §10.4 impl), `ui::run`'s bound widens to include both — costs
  `main.rs` nothing today since its one concrete wiring
  (`Ft991a<SerialCatSession<SerialPort>>`) already satisfies both bounds
  unconditionally. Recorded the explicit, disclosed consequence: `ui`
  becomes contractually FT-991A-only from here (already true in practice).
- Confirmed the 12-group UI structure directly against the real `Radio`
  vs. inherent-only split (not the abstract batch list) — reused the 10 CAT
  batches from §10.5 as 11 groups (kept theme boundaries even where a group
  straddles `Radio`/`Ft991aExtras`, since the new unconditional bound makes
  that split invisible to the UI code) plus `EX` as its own 12th, specially-
  structured group.
- Designed `EX`'s two-path access (§10.7's (a)/(b)) as a concrete state
  machine: `ExNumberEntry` (3-digit P1, validated against the real
  `EX_MENU_TABLE` at runtime) feeding into a shared `ExValueEntry` state
  that forks on the item's own `ExMenuValueKind` (`Enumerated` reuses
  `ListSelect`, `Range` reuses `TextInput`) — path (a)'s themed browsing
  groups (bucketed from `EX_MENU_TABLE`'s real `p1` values, not a
  hand-maintained duplicate list) feed into the same `ExValueEntry` state,
  so (a) depends on (b) but not vice versa, directly answering the brief's
  explicit dependency question.
- Designed `emulator/tui.rs`'s growth as **flat, wider annunciators, not a
  grouped/paginated display** — reasoned concretely that the
  discoverability pressure forcing `ui`'s grouped redesign (90+ keys can't
  each get a memorable binding) doesn't apply to a read-only display with no
  keybindings at all, and that `ts570d/emulator/tui.rs`'s own precedent
  already handles a comparably dense annunciator set as a flat list.
  Flagged full live `EX`-state display as explicitly out of scope for the
  main screen (dilutes an "operating state" screen into a "settings dump"),
  recommending an optional on-demand dump instead.
- Wrote the full Wave 4 dispatch queue into `planning/architect/
  task_plan.md` §11.6: one `yaesu` prerequisite task (the two new traits +
  `get`/`set_ex_menu_item`), then a sequential `ui` chain (skeleton, then
  seven group-content tasks ordered by shared-file sequencing rather than
  design dependency, then the `EX` escape hatch, then `EX` browsing), plus
  one `emulator` task independent of and parallelizable with the entire `ui`
  chain.
- Did not write any `.rs`/`.toml` files, dispatch any subagent, or touch
  `ts570d`/`radio-cat-rs`, per the architect's standing constraints (used
  `Read`/`Bash`/`Grep`-via-`Bash` read-only against both sibling repos and
  this repo's own committed code for research only). Noted the concurrent
  Windows-entry-point session's territory (`src/main.rs`, `Cargo.toml`s) —
  read `src/main.rs` in full and found it complete/coherent, not partially
  edited, so treated its content as ground truth for this design rather
  than flagging it as in-flight.
- Next: present this plan to the user for review. On approval, dispatch
  Wave 4 task 1 (`yaesu`, the `Ft991aExtras`/`CwKeying`/`EX`-accessor
  prerequisite) first — everything else in §11.6 is blocked on it.
