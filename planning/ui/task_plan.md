# UI Agent Task Plan

## Goal
Build the ratatui/crossterm terminal interface for FT-991A control,
depending only on the shared `cat-framework` crate (transitively) and this
repo's `radio` crate (its `Radio` trait and domain types) — never on a
transport or session crate.

## Current Status: Wave 2 — real TUI implementation (unblocked)

Preconditions cleared: `radio` crate's 11-command first slice landed and
committed (`e3698cf`); architect Wave 2 planning pass designed the
right-sized TUI in `planning/architect/task_plan.md` §6 (read in full).
`emulator` does not exist yet (a different agent is building it in
parallel) — do not touch it.

## Spec source of truth

`planning/architect/task_plan.md` §6 (lines ~434-664). Where the plan's
prose and the actual `radio` crate source (`radio/src/radio_trait.rs`,
`radio/src/ft991a.rs`) differ, the crate source wins — flag discrepancies,
don't silently improvise.

## Design decisions made while reading source (before writing code)

1. **Squelch range confirmed 0-100**, not 0-255 — `ft991a.rs`'s
   `get_squelch`/`set_squelch` doc comments say "range 000-100". AF/RF gain
   confirmed 0-255 per their own doc comments. Frequency range confirmed
   `Frequency::MIN_HZ` (30_000) / `MAX_HZ` (470_000_000) in
   `radio_trait.rs`. Power (PC) confirmed 5-100 watts per `ft991a.rs`'s
   `get_power`/`set_power` doc comments ("005-100").
2. **`run` signature**: current placeholder (`ui/src/lib.rs` line 49) is
   already `run<R: Radio + 'static>(radio: R) -> UiResult<()>` — takes by
   value, not `&mut`. Task prompt confirms this is correct and must not
   change (matches `app`'s later `main.rs` call site expectation). Note:
   root `CLAUDE.md`'s architecture diagram prose says
   `ui::run<R: Radio>(radio: &mut R)` — this is stale/aspirational prose
   describing an earlier sketch, not binding; the actual placeholder
   signature (by value) is what's real and what the task prompt directs me
   to preserve. Flagging this as a discrepancy in my final report, not
   silently resolving it either direction beyond what the task explicitly
   directs.
3. **Polling loop architecture — judgment call**: `ts570d/ui/src/terminal.rs`
   splits into two `monoio::spawn`-ed tasks (radio task + UI task) linked by
   `Rc<RefCell<VecDeque<T>>>` channels, specifically so key events stay
   responsive while a slow poll cycle is in flight. Architect §6.6 cites
   specific line numbers in that file (130-160 signature, 2101/2143
   poll-cadence, 2209/2254 event-poll/sleep cadence) but does not mandate
   reproducing the two-task/channel plumbing itself — only the timing
   shape. Given this slice's much smaller poll set (10 calls vs. ts570d's
   ~21) and the "right-sized, not a port" mandate from §6.1, I'm
   implementing a **single sequential loop** (no `monoio::spawn`, no
   channels): poll every 200ms, redraw, `event::poll` for up to 10ms,
   execute any resulting action inline, sleep 5ms. This is simpler and
   still meets every explicitly-specified requirement (signature, 200ms
   poll cadence, 10ms/5ms event timing, 3-consecutive-failed-cycles
   disconnect logic). Flagging this as a judgment call in my report.
4. **Fail-cycle threshold**: ts570d uses `FAIL_THRESHOLD = 10` errors out of
   ~21 poll calls (roughly half) to mark one cycle "failed", then 3
   consecutive failed cycles → `connected = false`. Scaling proportionally
   for this slice's 10 poll calls, I'm using `FAIL_THRESHOLD = 5` (a
   cycle counts as failed if at least half the 10 polls errored). Not
   specified explicitly in §6 — flagged as a judgment call.

## File layout to produce (per §6.7)

- `ui/src/lib.rs` — `UiError`/`UiResult` (keep), add `Ft991aDisplay` (§6.2),
  re-export `run`, declare `mod control; mod layout; mod terminal;`.
- `ui/src/layout.rs` — `split_areas`, `draw_header`, `draw_errors`,
  `draw_disconnected` (near-verbatim port from ts570d), `draw_status` (new,
  collapsed 2-row body per §6.3), `draw_control_panel` (simplified,
  `Normal`/`TextInput`/`ListSelect`/`Feedback` only).
- `ui/src/control.rs` — `ControlState`/`InputAction`/`SelectAction`/
  `ExecuteAction`/`handle_key` per §6.4, flat 9-entry keybinding table per
  §6.5.
- `ui/src/terminal.rs` — `run`/`poll_radio_state`/terminal setup-teardown
  per §6.6 (single-loop design, see decision 3 above).
- `ui/Cargo.toml` — add real `ratatui`/`crossterm` deps per §6.8.
- No `diag.rs` (§6.1 — out of scope this wave).

## Verification plan

- Unit tests in `control.rs` (state transitions, validation ranges — mirrors
  `ts570d/ui/src/control.rs`'s test module shape) and small pure rendering-
  decision helpers (e.g. TxState → label/color mapping) — no live
  terminal/serial port needed.
- `cargo build -p ui`, `cargo test -p ui`, `cargo clippy -p ui --all-targets
  -- -D warnings`, `cargo fmt --check` all clean before reporting done.

## Constraints reaffirmed

- Do not touch `radio/`, `emulator/` (doesn't exist), `src/main.rs`, root
  `Cargo.toml` `[dependencies]`/`[[bin]]` (workspace `members` already lists
  `ui`, confirmed by reading it — no edit needed there either).
- Do not touch `ts570d` or `radio-cat-rs` (read-only reference).
- Do not commit.
