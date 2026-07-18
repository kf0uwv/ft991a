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
