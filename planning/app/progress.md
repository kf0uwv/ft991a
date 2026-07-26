# App Agent Progress

## Wave 1 Task 2 — complete (2026-07-17)

Deliverables:
- Root `Cargo.toml` superseded (workspace `members = ["radio", "ui"]`,
  full `[workspace.dependencies]` set mirroring `ts570d`, `[package]`/
  `[[bin]]` for the `ft991a` binary, `[dev-dependencies]` with `radio` +
  `monoio` + `cat-transport-serial` — no `emulator` entry, deferred).
- `src/main.rs`: CLI `--port`/`--baud`/`--stop-bits` parsing (baud choices
  4800/9600/19200/38400, default 9600; stop-bits default 2 — an FT-991A-
  specific judgment call, see `findings.md`), `SerialPort::open`,
  `Ft991a::new(SerialCatSession::new(port))`, `ui::run(radio).await`.
- `ui/Cargo.toml` + `ui/src/lib.rs`: minimal placeholder crate, depends on
  `radio` + `monoio` + `thiserror` only. `run<R: radio::Radio +
  'static>(_radio: R) -> UiResult<()>` returns `Ok(())` immediately. Has
  its own unit tests (in-crate `MockRadio`, per `CLAUDE.md`'s testing
  rule) — not just an empty file.

Verification (all run from repo root, all green):
- `cargo build --workspace` — clean.
- `cargo test --workspace --all-targets` — 51 `radio` tests + 2 new `ui`
  tests + 0 `ft991a` bin tests, all pass. (Note: `--all-targets` excludes
  doctests by cargo's own design.)
- `cargo test --workspace --doc` — `radio`'s 1 doctest passes (ui has 0).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean after
  one fix (`ui/src/lib.rs` test: `std::io::Error::new(ErrorKind::Other,
  ..)` → `std::io::Error::other(..)` per `clippy::io_other_error`).
- `cargo fmt --all -- --check` — clean.
- Manual smoke check: `cargo run --bin ft991a --` (no args) prints the
  expected usage banner with the FT-991A-specific baud/stop-bits text and
  exits 1.

No obstacles hit; `radio`'s real API matched the architect's plan closely
enough that no scope conflicts arose. Did not touch `radio/`, `ts570d/`,
or `radio-cat-rs/`. Did not create or wire an `emulator` crate. Did not
build the real ratatui TUI. No commits made.

## Wave 2 Task 5 — complete (2026-07-17)

Deliverables:
- Root `Cargo.toml`: added `emulator = { path = "emulator" }` as the first
  entry in `[dev-dependencies]` (mirrors `ts570d/Cargo.toml`'s ordering).
  `[workspace] members = ["radio", "ui", "emulator"]` was already correct
  (landed by the `emulator` task) — untouched. `[dependencies]`, `[[bin]]`,
  `[workspace]` untouched.
- `src/main.rs`: confirmed, not assumed, that no change was needed. Built
  clean against the real `ui` crate on the first try. `ui::terminal::run`
  (re-exported as `ui::run`) is `pub async fn run<R: Radio + 'static>(mut
  radio: R) -> UiResult<()>` — identical signature shape to the Wave 1
  placeholder; only the parameter binding changed internally (`_radio` →
  `mut radio`), which doesn't affect the call site. §6.6's
  signature-stability note held exactly as planned.

Verification (all from repo root, all green):
- `cargo build --workspace` — clean.
- `cargo build --workspace --all-targets` — clean; confirms `ft991a`'s test
  target picks up the new `emulator` dev-dependency without error.
- `cargo test --workspace --all-targets` — 125 tests pass: `emulator` 15,
  `radio` 51, `ui` 59, `ft991a`/`emulator` bin targets 0 (no unit tests in
  main.rs, expected).
- `cargo test --workspace --doc` — `radio`'s 1 doctest passes; `ui` and
  `emulator` have 0 doctests.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean, no
  fixes needed.
- `cargo fmt --all -- --check` — clean.

End-to-end smoke test (first time `ft991a` and `emulator` binaries have run
against each other in this project's history):
1. `cargo run -p emulator -- --background` → printed `PTY_SLAVE=/dev/pts/1`
   plus a JSON `startup` log line, then ran headless.
   (Note: `cargo run --bin emulator` alone errors with "no bin target
   named `emulator` in default-run packages" in this 3-member workspace —
   needed `-p emulator` or `--bin emulator -p emulator` to disambiguate;
   recorded as a findings note, not a bug — `--background` mode's own
   scripting docs should probably mention `-p emulator` explicitly for
   future smoke tests.)
2. Ran `cargo run --bin ft991a -- --port /dev/pts/1 --baud 9600` inside a
   detached `tmux` session (needed because this agent's Bash tool has no
   controlling TTY at all — direct invocation failed with `UI error: IO
   error: No such device or address (os error 6)` from
   `crossterm::terminal::enable_raw_mode()`, which is an artifact of the
   sandboxed shell having no tty, not a bug in `ui` or `emulator`).
3. Captured the tmux pane shortly after startup: a fully rendered ratatui
   frame — box-drawn header "FT-991A RADIO CONTROL", VFO A/B frequencies
   (14.000.000 / 14.100.000 MHz), mode USB, S-meter bar, AF/RF gain bars,
   squelch, power 100W, "Connecting to radio..." status transitioning to
   populated fields (`ID:0670`, `PS:ON`) as the first poll cycle completed.
4. Cross-checked against the emulator's structured JSON command log
   (`--background` mode): confirmed real FT-991A CAT wire traffic —
   `FA;`→`FA014000000;`, `FB;`→`FB014100000;`, `MD0;`→`MD02;`, `TX;`→`TX0;`,
   `SM0;`→`SM0000;`, `PS;`→`PS1;`, `AG;`→`AG0128;`, `RG;`→`RG0255;`,
   `SQ;`→`SQ0000;`, `PC;`→`PC100;` — repeating every ~200ms, matching
   `ui/src/terminal.rs`'s documented `POLL_INTERVAL`. All 10 commands got
   valid, correctly-formatted responses every cycle; no malformed frames,
   no timeouts.
   `object of study`: this is genuine two-process, real-CAT-protocol,
   over-a-live-PTY interoperation — not just independently passing unit
   test suites.
5. Scanned the app's full log output for `ERROR`/`WARN`/`panic` — none
   found across ~160KB of captured output covering multiple seconds of
   live polling.
6. Sent `q` to the tmux pane — `ft991a` exited cleanly (process gone from
   `ps aux` within 1s), tmux session auto-closed. Killed the background
   `emulator` process afterward. No leftover processes.

Result: **the two Wave 2 deliverables interoperate correctly end-to-end.**
No wire-format mismatch, no hang, no crash. No bugs found in `ui/src/*.rs`
or `emulator/src/*.rs` — out-of-scope files were not touched.

`docs/adr/README.md`'s "Repository status" note updated: Wave 2 now marked
landed-and-integrated (not just "dispatch queue designed"), with a summary
of Task 5's wiring confirmation and smoke-test result.

No commits made.

## Wave 3 Task (Windows buildability) — code complete, verification partially blocked (2026-07-19)

Deliverables (all landed, per `radio-cat-rs` ADR 0004 §1):
- `src/main.rs`: split into `async fn run_app()` (unchanged logic) +
  `#[cfg(target_os = "linux")] #[monoio::main(timer_enabled = true)] async
  fn main() { run_app().await }` (byte-identical behavior to before) +
  `#[cfg(target_os = "windows")] fn main() {
  windows_block_on::block_on(run_app()) }`, plus a new
  `mod windows_block_on` (Windows-only): a ~35-line hand-rolled `block_on`
  using `std::pin::pin!`, `std::task::{Context, Wake}`, and
  `std::thread::park`/`unpark`, per ADR 0004 §1's exact specification.
- `ui/src/terminal.rs`: the single `monoio::time::sleep(IDLE_SLEEP).await`
  replaced with `#[cfg(target_os = "linux")]
  monoio::time::sleep(IDLE_SLEEP).await;` / `#[cfg(target_os = "windows")]
  std::thread::sleep(IDLE_SLEEP);`. The `#[cfg(test)] mod tests` block
  (8 `#[monoio::test(driver = "legacy")]` functions) re-gated
  `#[cfg(all(test, target_os = "linux"))]`, mirroring
  `cat-transport-serial/src/session.rs`'s identical pattern in
  `radio-cat-rs`.
- Root `Cargo.toml`: `monoio` moved out of unconditional `[dependencies]`/
  `[dev-dependencies]` into new
  `[target.'cfg(target_os = "linux")'.dependencies]` /
  `[target.'cfg(target_os = "linux")'.dev-dependencies]` sections.
- `ui/Cargo.toml`: `monoio` moved from unconditional `[dependencies]` into
  a new `[target.'cfg(target_os = "linux")'.dependencies]` section.
- `emulator/`: left untouched (Cargo.toml and src/*.rs both) — see
  Findings/task_plan for reasoning (PTY-based, Unix-specific dev tool,
  explicitly out of this task's scope, no `monoio` dependency to begin
  with).
- `radio/`: left untouched, per constraint (read-only reference). See
  Findings for the empirical finding that its unconditional `monoio`
  dependency is, in practice, not itself a Windows-compile blocker.

Verification:
- **Linux — zero regressions, exact baseline match.** Before and after this
  task's edits: `cargo build --workspace` clean;
  `cargo test --workspace --all-targets` → 501 `radio` + 59 `ui` + 15
  `emulator` = 575 tests, all passing, identical counts before/after;
  `cargo test --workspace --doc` → `radio`'s 1 doctest passing (ui/emulator
  0), identical before/after; `cargo clippy --workspace --all-targets --
  -D warnings` clean, zero fixes needed; `cargo fmt --all -- --check`
  clean.
- **Windows cross-compile — real dependency state fails, root-caused to
  `radio-cat-rs`, not `ft991a`.** `rustup target list --installed` already
  had `x86_64-pc-windows-gnu` (no install needed).
  `cargo check --target x86_64-pc-windows-gnu -p ft991a` against the real,
  currently-committed `cat-transport-serial` (git dep, `branch = "main"`,
  resolved commit `d1de083`) fails with 19 errors, all inside
  `cat-transport-serial`'s `io_uring.rs` (unconditional `monoio` crate
  reference, `std::os::fd`/`std::os::unix`, and several Unix-only `libc`
  termios/ioctl items) — because that module isn't
  `#[cfg(target_os = "linux")]`-gated at the currently-pinned commit; ADR
  0004's implementation of that gating exists only as **uncommitted**
  changes in the local `radio-cat-rs` working tree (confirmed via `git
  status`/`git log` there), not yet pushed to the `main` branch this repo's
  git dependency tracks. **`ft991a`'s own code contributes zero errors to
  this failure** — verified by temporarily patching
  (`[patch."https://github.com/kf0uwv/radio-cat-rs"]`, root `Cargo.toml`,
  pointing at the local `radio-cat-rs` working tree) and re-running: both
  `cargo check --target x86_64-pc-windows-gnu -p ft991a` and `-p ui`
  succeeded cleanly, zero errors or warnings, once `cat-transport-serial`'s
  own (uncommitted) Windows gating was in the picture. The patch was then
  removed and `Cargo.lock` regenerated back to the real git-pinned
  dependency (confirmed via `cargo build --workspace` running clean again)
  before finishing — it was diagnostic-only, never part of the committed
  deliverable, and `radio-cat-rs` is out of this agent's authority to
  modify or push to.

Judgment calls:
- `Waker` construction via `std::task::Wake` + `Arc` (blanket `impl From<Arc<W>>
  for Waker`) rather than a hand-rolled `RawWaker`/`RawWakerVTable` — ADR
  0004 §1 explicitly left this choice open ("your call on which is cleaner
  Rust"); the `Wake` trait route is the standard-library-blessed safe
  wrapper for exactly this pattern and avoids the unsafe vtable
  boilerplate entirely.
- `emulator/`'s Cargo.toml/src left fully untouched — no `monoio` dependency
  exists there at all, and its PTY-based design is Unix-specific by
  construction; gating it for Windows would be new scope not requested and
  not achievable without a redesign of its transport, which the task
  explicitly frames as optional/out-of-scope for this pass.

## Wave 4 Task — complete (TCP client mode, `--server` flag, 2026-07-25)

Deliverables:
- Root `Cargo.toml`: `cat-transport-core`/`cat-transport-tcp`/`async-trait`
  added to the existing `[target.'cfg(target_os = "linux")'.dependencies]`
  table (merged with `server`'s own entry there — do not create a second
  `[target...]` table of the same key, TOML rejects the duplicate).
- `src/main.rs`: `Args.port` → `Option<String>`, new `Args.server:
  Option<String>`; `parse_args()` gained `--server <host:port>` plus a
  mutual-exclusivity/at-least-one-required check on `(port, server)`.
  Split the old inline `run_app()` tail into `run_over_serial(&Args)`
  (unchanged serial path) and `run_over_tcp(&Args, addr)` (new —
  `#[cfg(target_os = "linux")]` real impl using `TcpCatSession::connect` +
  a new `TcpClientSession` adapter, `#[cfg(not(target_os = "linux"))]`
  stub that errors and exits, mirroring `run_server_mode`'s existing
  dual-definition pattern exactly). New `TcpClientSession` (Linux-only)
  implements `CatSession<Error = TransportError>` (mapping
  `TcpSessionError` → `TransportError`, same shape as `server/src/
  broker_session.rs::BrokerCatSession`) and `ModemControlLines` (every
  method returns an honest "not available over TCP" error — required so
  `Ft991a<TcpClientSession>` satisfies `ui::run`'s unconditional
  `CwKeying` bound; see `task_plan.md`'s blocker writeup for why).

Verification (all green):
- `cargo build --workspace`, `cargo test --workspace` (434 `ui` unit
  tests unaffected + full workspace), `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo fmt --check`.
- `cargo check --target x86_64-pc-windows-gnu -p ft991a` — confirmed the
  Linux-only gating actually keeps Windows green (this was the whole
  point of gating rather than adding these deps unconditionally).
- Real end-to-end run, not just compile-checked: `ft991a server --port
  <emulator-pty> --raw-tcp-port 4533` in one process, `ft991a --server
  127.0.0.1:4533` in a tmux-wrapped second process (via the `run` skill's
  TUI pattern) — confirmed live radio state displayed (VFO A/B, mode,
  S-meter, PWR, "No errors" panel) pulled over the TCP hop, then drove an
  actual VFO A frequency change (`F` → `14250000` → Enter) through the
  full stack (TUI → `TcpClientSession` → `cat-server` raw TCP listener →
  broker → emulator) and watched it land, before quitting cleanly (`q`
  `q`) and confirming the process exited (no lingering `--server`
  process).

No commits made.

## `server` crate migration onto `cat_server::BrokerCatSession`/`cat-rigctl` (2026-07-26)

Files changed:
- `.cargo/config.toml` (new, untracked, never committed) — local `[patch]`
  block pointing every `radio-cat-rs`-sourced crate at the local checkout
  (commit `36f783d`, not yet pushed).
- `Cargo.toml` — added `cat-rigctl` to `[workspace.dependencies]`.
- `server/Cargo.toml` — added `cat-rigctl`; removed now-unused
  `cat-framework`/`cat-client`/`cat-server` (direct)/`thiserror`/
  `tracing`/`futures`.
- `server/src/broker_session.rs` — deleted (moved verbatim into
  `cat-server` by the sibling agent).
- `server/src/rigctl.rs` — deleted (generalized into `cat-rigctl` by the
  sibling agent).
- `server/src/rigctl_radio.rs` — new: `Ft991aRigctl<S>` wrapper +
  `impl RigctlRadio` (see `findings.md` for why a wrapper, not a direct
  impl on `radio::Ft991a<S>`, is required), with the Hamlib mode tables
  and freq range ported verbatim.
- `server/src/lib.rs` — shrunk to a `ServerConfig` re-export + thin
  `run()` wrapper around `cat_rigctl::run`.
- `planning/app/{task_plan,findings,progress}.md` — this task's record.
- `src/main.rs`/`CLAUDE.md` — NOT touched by this task; both already had
  unrelated, pre-existing uncommitted changes from an earlier task this
  session (`--server <host:port>` TCP client mode) when this task began.
  Confirmed `server::run`/`server::ServerConfig`'s public shape needed
  zero changes for `main.rs` to keep compiling (verified via build, not
  assumed).

Verification (all green):
- `cargo build --workspace`, `cargo test --workspace` (server: 8 tests,
  full workspace otherwise unaffected), `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo fmt --check`, `cargo check
  --target x86_64-pc-windows-gnu -p ft991a`.
- Real end-to-end run against the live `emulator` + real `/usr/bin/rigctl`
  (Hamlib 4.6.5) CLI: `f`/`F <hz>.000000`/`f` round-trips the decimal form
  Hamlib actually sends; `t`/`T 1`/`t`/`T 0`/`t` round-trips PTT — both
  clean, fast, no `RPRT -1`/timeout, every time. Mode (`m`/`M`) verified
  correct via the raw wire subset this bridge's own docs say `netrigctl.c`
  actually sends (`nc`, bypassing Hamlib's C client) — see `findings.md`
  for a real, pre-existing (not a migration regression) compatibility gap
  found where the literal `rigctl -m 2 ... M <mode> <width>` CLI
  subcommand specifically silently no-ops on this Hamlib version, due to
  an unimplemented `\get_lock_mode` probe Hamlib's `rig_set_mode()` now
  sends first.
- Killed all emulator/server background processes started for this
  verification; confirmed `.cargo/config.toml` shows as untracked (not
  staged) in `git status` before committing.

Committed locally (not pushed) — see final report for the commit hash.
Staged only the files listed above; left the pre-existing unrelated
`src/main.rs`/`CLAUDE.md` changes (from an earlier task this session)
uncommitted, exactly as found.
