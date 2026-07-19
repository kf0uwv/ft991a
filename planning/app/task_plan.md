# App Agent Task Plan

## Goal
Wave 1 Task 2 (per `planning/architect/task_plan.md` §5): supersede the
`yaesu` agent's minimal placeholder root `Cargo.toml` (`members = ["radio"]`
only) with the real workspace, add `src/main.rs` wiring layer, and add a
genuinely minimal placeholder `ui` crate. NOT the ratatui TUI — that is
Wave 2. NOT the `emulator` crate — deferred to its own wave.

## Ground truth read this session
- `radio/Cargo.toml`, `radio/src/lib.rs`, `radio/src/radio_trait.rs`,
  `radio/src/ft991a.rs` (full) — the real, reviewed `radio` crate.
  Confirmed: `Ft991a::new(session: S)` where
  `S: CatSession<Error = TransportError>` (NOT a `SerialCatSession`-typed
  param — generic over any conforming session, matching `ts570d::Ts570d::new`
  shape). `Radio` trait is `#[async_trait(?Send)]`, first-slice subset
  (vfo a/b, mode, ptt/tx-state, smeter, power on/off, af/rf gain, squelch,
  power, id, flush_rx).
- `ts570d/Cargo.toml`, `ts570d/src/main.rs`, `ts570d/ui/Cargo.toml`,
  `ts570d/ui/src/lib.rs` (head), `ts570d/emulator/Cargo.toml` — structural
  templates.
- `radio-cat-rs/cat-transport-serial/src/io_uring.rs` — confirmed
  `SerialConfig { baud_rate, data_bits, stop_bits, parity, flow_control }`,
  `SerialPort::open(path: &str, config: SerialConfig) -> SerialResult<Self>`.
- `radio-cat-rs/cat-transport-serial/src/session.rs` — confirmed
  `impl<T: Transport> CatSession for SerialCatSession<T> { type Error =
  TransportError; }`, satisfying `Ft991a::new`'s bound.
- Existing root `Cargo.toml` (yaesu's placeholder) — explicitly flagged as
  superseded by this task, not extended in place; will be replaced with the
  full workspace shape while preserving its `[workspace.package]` metadata
  and the `cat-*`/monoio/async-trait/thiserror/tempfile/mockall versions it
  already pinned (so `radio`'s existing dependency resolution doesn't
  change).

## Plan

### 1. Root `Cargo.toml`
- `[workspace] members = ["radio", "ui"]` (NOT `emulator`).
- `[workspace.package]`: carry over unchanged from the placeholder (version
  0.1.0, edition 2021, authors, license Apache-2.0, homepage/repository,
  description, rust-version 1.75).
- `[workspace.dependencies]`: keep the placeholder's existing entries
  (monoio 0.2.3, async-trait 0.1, thiserror 1.0.61, tempfile 3.10.1,
  mockall 0.12.1, the four `cat-*` git deps on `branch = "main"` — exact
  same git URL/branch syntax as `radio/Cargo.toml` already resolves via the
  placeholder) and add the remaining entries `ts570d/Cargo.toml` centralizes
  that this repo doesn't have yet, for Wave 2+ to consume without another
  Cargo.toml edit: `local-sync`, `ratatui`, `crossterm`, `serde`+`serde_json`,
  `tracing`+`tracing-subscriber`, `bytes`, `futures`, `libc`, `nix` — same
  versions `ts570d` pins (no reason found to diverge).
- `[package]` name `ft991a`, workspace-inherited metadata; `[[bin]] name =
  "ft991a" path = "src/main.rs"`.
- `[dependencies]`: monoio, thiserror, tracing, tracing-subscriber, libc
  (workspace), `cat-transport-serial` (workspace — only concrete transport
  `main.rs` names), `radio = { path = "radio" }`, `ui = { path = "ui" }`.
- `[dev-dependencies]`: `radio = { path = "radio" }`, monoio,
  `cat-transport-serial` (workspace). NO `emulator` entry — crate doesn't
  exist yet, deferred per architect's explicit Wave-1 scoping.
- `[profile.release]` lto/codegen-units/panic=abort, `[profile.dev]`
  debug=true — mirror `ts570d`.

### 2. `src/main.rs`
Mirror `ts570d/src/main.rs` shape exactly, two deltas:
- Baud choices `4800 | 9600 | 19200 | 38400` (FT-991A, per
  `planning/architect/task_plan.md` §1), default 9600.
- Stop-bits CLI default: `2` (not ts570d's `1`) — judgment call, see
  Findings: the architect's task_plan.md §1 explicitly cites
  `SerialConfig::default()`'s 8N2 framing as the FT-991A's documented
  default (manual doesn't spell out data/parity/stop bits beyond "standard
  serial cable", so `SerialConfig::default()` is the cited fallback);
  ts570d's own CLI default of 1 stop bit is a TS-570D-specific choice that
  doesn't transfer.
- `Ft991a::new(SerialCatSession::new(port))` — construct directly, no
  extra wrapping needed since `Ft991a::new` takes any `S: CatSession<Error
  = TransportError>` directly (confirmed above), matching `Ts570d::new`'s
  shape.
- `ui::run(radio).await` — same call shape as ts570d.

### 3. `ui` crate (minimal placeholder)
- `ui/Cargo.toml`: depends on `radio` (path) + `monoio` + `thiserror`
  only — no ratatui/crossterm yet (that's Wave 2, when the real TUI is
  built against this dependency).
- `ui/src/lib.rs`: `UiError` (thiserror, one variant for now),
  `UiResult<T>`, `pub async fn run<R: radio::Radio + 'static>(_radio: R) ->
  UiResult<()> { Ok(()) }` — genuinely empty stub, mirrors ts570d's
  `run<R: Radio + 'static>(radio: R) -> UiResult<()>` signature shape
  without any of its implementation.

### 4. Verification
Run, in order, from the ft991a repo root:
- `cargo build --workspace`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
All must be clean. Confirm `radio`'s 51 tests + 1 doctest still pass
unaffected in the full-workspace context.

## Constraints reaffirmed
- Do NOT touch `radio/` (read-only reference).
- Do NOT create/wire an `emulator` crate.
- Do NOT build the real ratatui TUI.
- Do NOT touch `ts570d` or `radio-cat-rs`.
- Do NOT commit.

---

## Wave 2 Task 5 (sequential follow-up, per `planning/architect/task_plan.md`
§8's "Task 5" subsection)

### Goal
Confirm the two independent Wave 2 deliverables — the real `ui` crate
(ratatui TUI) and the `emulator` crate (PTY-hosted
`CatFramework<Ft991aRadio>`) — are correctly wired together end-to-end.
This is the first point both land together.

### Ground truth read this session
- `planning/architect/task_plan.md` §8 Task 5: scope is root `Cargo.toml`
  (`emulator` in `[dev-dependencies]`, mirroring ts570d's shape) and
  confirming `src/main.rs`'s `ui::run(radio).await` call site needs no
  change (§6.6's signature-stability note).
- Root `Cargo.toml`: `[workspace] members = ["radio", "ui", "emulator"]`
  already correct (added by the `emulator` task). `[dev-dependencies]` had
  `radio`/`monoio`/`cat-transport-serial` but was missing `emulator` —
  confirmed by diffing against `ts570d/Cargo.toml`'s
  `[dev-dependencies]` block (`emulator = { path = "emulator" }` first
  entry there).
- `src/main.rs`: calls `ui::run(radio).await` where `radio: Ft991a<SerialCatSession<SerialPort>>`.
- `ui/src/lib.rs`: `pub use terminal::run;`.
- `ui/src/terminal.rs:223`: `pub async fn run<R: Radio + 'static>(mut radio: R) -> UiResult<()>` —
  identical signature shape to the Wave 1 placeholder
  (`run<R: radio::Radio + 'static>(_radio: R) -> UiResult<()>`), just
  `radio` renamed from `_radio` to `radio` (no longer unused) and `mut`
  added (needed internally) — neither changes the call site. Confirmed no
  `src/main.rs` change needed.
- `emulator/src/main.rs`: binary `emulator`, flags `--tui` / `--background`
  (mutually exclusive) / `--port <path|virtual>` / `--log-file <path>`.
  Default (`--port` absent) is `PortMode::Virtual` (a PTY pair), printing
  `PTY_SLAVE=<path>` as the first stdout line. `--background` runs
  headless with a `BackgroundLogger` (stdout or file) — the right mode for
  a scripted smoke test (no TUI to fight with).

### Plan
1. Root `Cargo.toml`: add `emulator = { path = "emulator" }` to
   `[dev-dependencies]`, first line of that block (mirrors ts570d's
   ordering). Do not touch `[dependencies]`, `[[bin]]`, `[workspace]`.
2. Confirm (not assume) `src/main.rs` needs no change by building.
3. Full workspace verification: build (default + all-targets), test
   (all-targets + doc), clippy (`-D warnings`), fmt --check.
4. End-to-end smoke test: `cargo run --bin emulator -- --background` in
   background, capture `PTY_SLAVE=<path>`, then run
   `cargo run --bin ft991a -- --port <path> --baud 9600` briefly against
   it, observe connection behavior, then kill both.
5. Update `docs/adr/README.md` repository-status note if warranted.

### Constraints reaffirmed
- Do NOT touch `radio/`, `ui/src/*.rs`, `emulator/src/*.rs` (reviewed,
  out of scope). Report, don't fix, any genuine bug found there.
- Do NOT touch `ts570d` or `radio-cat-rs`.
- Do NOT commit.

---

## Wave 3 Task (Windows buildability, 2026-07-19)

### Goal
Per `radio-cat-rs` ADR 0004 §1 (`cat-transport-serial` now has a real
Windows COM backend; `SerialPort`/`SerialConfig`/`Transport`/
`ModemControlLines` are platform-identical) and its Consequences section
("`ft991a` and `ts570d` each still need their own follow-on work ... to
replace `#[monoio::main]` with a Windows-compatible entry point"): make
`ft991a` itself `cargo check --target x86_64-pc-windows-gnu` clean, with
zero Linux behavior change.

### Ground truth read this session
- ADR 0004 §1 in full: `ft991a` (confirmed single-sequential-loop, no
  `monoio::spawn` anywhere) needs only a hand-rolled ~30-line
  thread-parking `block_on` (poll in a loop; on `Poll::Pending`,
  `std::thread::park()`; `Waker` calls `Thread::current().unpark()`) — no
  new crate dependency. `monoio::time::sleep(5ms).await` →
  `std::thread::sleep(5ms)`.
- `src/main.rs`: single `#[monoio::main(timer_enabled = true)] async fn
  main()` — parses args, `SerialPort::open`, `Ft991a::new`, `ui::run(radio)
  .await`.
- `ui/src/terminal.rs`: `pub async fn run<R: Radio + 'static>(mut radio: R)
  -> UiResult<()>`, one `monoio::time::sleep(IDLE_SLEEP).await` call at the
  bottom of `run_loop`'s `loop {}`; a `#[cfg(test)] mod tests` with 8
  `#[monoio::test(driver = "legacy")]` functions.
- Root `Cargo.toml`: `monoio` unconditional in `[workspace.dependencies]`
  (fine — workspace deps aren't compiled unless referenced), `[dependencies]`,
  and `[dev-dependencies]` — needs Linux target-gating in the latter two.
- `ui/Cargo.toml`: `monoio = { workspace = true }` unconditional in
  `[dependencies]` — needs Linux target-gating.
- **Discrepancy found, not assumed away**: `radio/Cargo.toml` still lists
  `monoio = { workspace = true }` unconditionally in `[dependencies]` (NOT
  target-gated), unlike `cat-transport-serial/Cargo.toml`'s already-gated
  `[target.'cfg(target_os = "linux")'.dependencies] monoio` entry. Grepping
  `radio/src/*.rs` shows `monoio` is used ONLY inside `#[monoio::test(...)]`
  attributes in `#[cfg(test)]` modules (`radio_trait.rs:2161`,
  `ft991a.rs` ~48 occurrences) — no non-test code path calls a `monoio` API.
  Despite that, an *unconditional* (non-target-gated) `[dependencies]`
  entry means the `monoio` crate itself must still compile as part of
  building `radio`'s lib target on any platform, including Windows — and
  ADR 0004 states plainly that `monoio` "cannot compile on Windows at all."
  This task's instructions say "Do NOT touch `radio/`" and call it
  "read-only reference," so no edit was made there; this is flagged as a
  found discrepancy against the task's assumption ("likely already has
  this gating from earlier work") for the architect, not worked around.
  Confirmed empirically in Verification below whether it actually breaks
  the Windows cross-compile check for `-p ft991a`/`-p ui` (both depend on
  `radio`) or whether cargo's per-target dependency resolution avoids
  building `radio`'s `[dev-dependencies]`-only monoio test path and radio
  itself somehow still fails/succeeds.
- `emulator/Cargo.toml`: no `monoio` dependency at all (uses `serialport`,
  a PTY-based Unix-specific crate, plus `ctrlc`/`ratatui`/`crossterm`). Not
  in `ft991a`'s `members` build path for the binary crate itself except as
  a `[dev-dependencies]` entry of the root `Cargo.toml`. Decision: leave
  `emulator` untouched — it's dev/test infra, its PTY hosting is
  Unix-specific by design (per its own doc comments and earlier session
  notes), out of this task's explicit scope (`ft991a` binary +
  `ui`), and the task prompt explicitly frames it as optional
  ("may be out of scope for a first Windows pass").

### Plan
1. `src/main.rs`: split into `async fn run_app() -> Result<(), ...>` (the
   existing body) + `#[cfg(target_os = "linux")] #[monoio::main(timer_enabled
   = true)] async fn main()` (unchanged behavior) +
   `#[cfg(target_os = "windows")] fn main()` calling a hand-rolled
   `windows_block_on` per ADR 0004 §1's exact description (thread-parking
   `block_on`, `Waker` via `std::task::Wake`/`Arc`).
2. `ui/src/terminal.rs`: `#[cfg(target_os = "linux")]
   monoio::time::sleep(IDLE_SLEEP).await;` /
   `#[cfg(target_os = "windows")] std::thread::sleep(IDLE_SLEEP);` in place
   of the single unconditional call. Gate the `#[cfg(test)] mod tests` block
   `#[cfg(target_os = "linux")]` (matching `cat-transport-serial/src/
   session.rs`'s `#[cfg(all(test, target_os = "linux"))]` pattern for its
   `#[monoio::test]`-based module).
3. Root `Cargo.toml`: move `monoio` out of unconditional `[dependencies]`/
   `[dev-dependencies]` into
   `[target.'cfg(target_os = "linux")'.dependencies]` /
   `[target.'cfg(target_os = "linux")'.dev-dependencies]`.
4. `ui/Cargo.toml`: same treatment, `[target.'cfg(target_os =
   "linux")'.dependencies] monoio = { workspace = true }`.
5. Leave `emulator/` untouched (see reasoning above).
6. Verification: Windows cross-compile check (`-p ft991a`, `-p ui`) +ull
   Linux `build`/`test --all-targets`/`test --doc`/`clippy -D warnings`/
   `fmt --check`, comparing exact test counts against the 501+1/59/15
   baseline captured before any edit.

### Constraints reaffirmed
- Do NOT touch `radio/` or `radio-cat-rs` (read-only reference) — report,
  don't fix, the monoio-gating discrepancy found there.
- Do NOT touch `emulator/src/*.rs`; Cargo.toml treatment decided above
  (leave as-is, Linux-only dev tool, reasoning stated).
- Do NOT commit.
- If the hand-rolled `block_on`/`Waker` has any subtlety not fully
  confident is correct, say so explicitly rather than presenting uncertain
  code as solid.
