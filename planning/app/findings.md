# App Agent Findings

## Wave 1 Task 2 (2026-07-17)

- `radio/Cargo.toml` and `radio/src/*` are the real, reviewed crate (built
  by the `yaesu` agent). `Ft991a::new(session: S)` takes any
  `S: CatSession<Error = TransportError>` directly — same shape as
  `ts570d::Ts570d::new`. `SerialCatSession<T: Transport>` satisfies this
  bound (`type Error = TransportError`, confirmed in
  `radio-cat-rs/cat-transport-serial/src/session.rs`).
- The existing root `Cargo.toml` is a placeholder the `yaesu` agent added
  (its own header comment says so explicitly) purely so `radio` had a
  workspace root to build/test/lint against. It lists
  `members = ["radio"]` only and is missing `ui`, the `[package]`/`[[bin]]`
  section, and most of `ts570d`'s `[workspace.dependencies]` set
  (tracing/tracing-subscriber/libc/ratatui/crossterm/serde/bytes/futures/
  nix/local-sync). Superseding it, not extending it, per my task's explicit
  instruction.
- Judgment call: FT-991A CLI default stop-bits = 2 (not `ts570d`'s 1) —
  the architect's task_plan.md §1 cites `SerialConfig::default()`'s 8N2
  framing as the FT-991A's documented default serial framing (no explicit
  data/parity/stop-bit spec in the manual beyond "standard serial cable,
  not null-modem"); ts570d's CLI default of 1 stop bit is a TS-570D-specific
  choice recorded in ts570d's own manual, not something that transfers by
  default.
- `ui` crate: kept dependencies to `radio` + `monoio` + `thiserror` only —
  no `ratatui`/`crossterm` yet, since the stub doesn't render anything.
  Those go into `ui/Cargo.toml` when Wave 2 builds the real TUI.

## Wave 2 Task 5 (2026-07-17)

- Root `Cargo.toml`'s `[dev-dependencies]` was genuinely missing
  `emulator` before this task — confirmed by direct `grep`, not assumed.
  The `emulator` task's scope ("add to workspace members") and this task's
  scope ("add as this package's dev-dependency") really are two different
  edits, exactly as the dispatch predicted; they hadn't been conflated by
  the prior task.
- `cargo run --bin emulator` (no `-p`) fails with "no bin target named
  `emulator` in default-run packages" in this workspace, even though
  `emulator` is the only crate providing that bin name. Needed
  `cargo run -p emulator -- <args>` instead. This is a `cargo` workspace
  quirk (multi-member workspace, no `default-run` or `default-members` set
  narrowing which package's targets `--bin <name>` searches without `-p`),
  not a bug in `emulator/Cargo.toml`. Worth remembering for any future
  scripted smoke test in this repo.
- This sandboxed agent shell has no controlling TTY (`tty` → "not a tty").
  Running `ft991a` directly (even via `cargo run --bin ft991a -- --port
  ... &`) fails immediately with `UI error: IO error: No such device or
  address (os error 6)` from `crossterm::terminal::enable_raw_mode()` —
  this is an environment limitation, not an app/ui bug. Worked around by
  running inside a detached `tmux` session (`tmux new-session -d -s
  <name> "<command>"`), which provides a real pty (`/dev/pts/N`) for
  crossterm to attach to. `tmux capture-pane -p` on that session showed a
  correctly rendered first frame, but returned blank on later captures
  despite the process staying alive and CPU-active — most likely a tmux
  capture-mode/alternate-screen interaction quirk (`tmux capture-pane -a`
  itself reported "no alternate screen" even though the app sends
  `\x1b[?1049h`), not a rendering bug in `ui`: the *primary* evidence for
  correct operation was cross-checking the emulator's own structured JSON
  command log (`--background` mode), which showed real, correctly-formed
  FT-991A CAT request/response pairs for all 10 polled commands repeating
  on the expected ~200ms cadence — independent, stronger confirmation of
  correct wire-level interoperation than a terminal screenshot would be.
- No genuine bugs found in `ui/src/*.rs` or `emulator/src/*.rs` during the
  smoke test. Both out-of-scope directories were left untouched.

## Wave 3 Task (Windows buildability, 2026-07-19)

- **The `radio` crate's unconditional `monoio` dependency turned out NOT to
  be a Windows cross-compile problem, contrary to my initial concern**
  (recorded in `task_plan.md` before empirical verification). `monoio`
  0.2.4 (the version actually resolved — the workspace pins `"0.2.3"` but
  semver-floats) has its own internal `#[cfg(all(target_os = "linux",
  feature = "iouring"))]` gating around every io_uring-specific code path
  (confirmed by reading `~/.cargo/registry/.../monoio-0.2.4/src/lib.rs`);
  its default features include both `iouring` and `legacy` (`mio`-backed,
  which supports Windows/IOCP). So the `monoio` crate itself genuinely
  compiles for `x86_64-pc-windows-gnu` — confirmed empirically:
  `cargo check --target x86_64-pc-windows-gnu -p ui` shows `Checking mio
  v0.8.11` / `Checking monoio v0.2.4` succeeding cleanly. `radio`'s
  `[dependencies] monoio = { workspace = true }` being unconditional (not
  target-gated like `cat-transport-serial`'s) is *inconsistent* with this
  workspace's stated convention, but is not, in itself, a Windows
  build-breaker. Not fixed (out of scope, `radio/` is read-only reference)
  — noted here as a corrected finding, not left as an open blocker claim.
- **The real, currently-blocking issue is entirely on the `radio-cat-rs`
  side, not `ft991a`'s.** `ft991a`'s `Cargo.toml` pins
  `cat-transport-serial = { git = "https://github.com/kf0uwv/radio-cat-rs",
  branch = "main" }`. That branch's current tip (commit `d1de083`, "Add
  ModemControlLines trait...") is confirmed, via `git show
  d1de083:cat-transport-serial/src/lib.rs`, to have `pub mod io_uring;`
  completely **unconditional** — none of ADR 0004's `#[cfg(target_os =
  "linux")] pub mod io_uring;` / `#[cfg(target_os = "windows")] pub mod
  windows;` gating exists at that commit. Checking the local `radio-cat-rs`
  clone directly: `git status` there shows the entire ADR 0004
  implementation (`config.rs`, `windows.rs`, `oneshot.rs`, `baud.rs`,
  `timeouts.rs`, plus modified `lib.rs`/`io_uring.rs`/`session.rs`/
  `Cargo.toml`) exists only as **uncommitted working-tree changes** —
  `git log` there shows only 3 commits, none matching this ADR's work.
  Concretely, `cargo check --target x86_64-pc-windows-gnu -p ft991a` today
  fails with 19 errors in `cat-transport-serial` alone: 3×
  `unresolved crate 'monoio'` (io_uring.rs unconditionally references
  `monoio::net::UnixStream` even though the `monoio` *dependency* is
  correctly Linux-gated in that commit's `Cargo.toml` — it's the *module*
  that isn't gated), plus `std::os::fd`/`std::os::unix` unresolved, plus
  `libc::{TIOCMBIS,TIOCMBIC,TIOCMGET,ioctl,O_NOCTTY,O_NONBLOCK,tcflush,
  TCIFLUSH}` all missing (Unix-only `libc` items, absent on the Windows
  target). This is exactly the class of failure ADR 0004 describes fixing
  — it just hasn't reached the `main` branch this repo's git dependency
  tracks yet.
- **Verified my own `ft991a`-side changes are correct independent of that
  blocker**, via a temporary, local-only `[patch."https://github.com/
  kf0uwv/radio-cat-rs"]` section in the root `Cargo.toml` pointing
  `cat-framework`/`cat-client`/`cat-transport-core`/`cat-transport-serial`
  at the local `radio-cat-rs` checkout's *working tree* (which does have
  ADR 0004's implementation, just uncommitted). With that patch in place,
  `cargo check --target x86_64-pc-windows-gnu -p ft991a` and `-p ui` both
  succeeded cleanly with zero errors or warnings. The patch was then
  reverted (removed from `Cargo.toml`, `Cargo.lock` regenerated back to the
  real git-pinned state via `cargo build --workspace`) before finishing —
  it was a diagnostic step only, not part of the deliverable, and I have no
  authority to touch `radio-cat-rs` (separate repo, separate agent) to make
  it permanent by committing/pushing there.
- This means: **`ft991a`'s own Windows-readiness work (this task) is
  complete and correct**, but the end-to-end verification bar stated in
  the task ("`cargo check --target x86_64-pc-windows-gnu -p ft991a` ...
  must succeed") cannot be met against the *real* current state of the
  `radio-cat-rs` `main` branch — only against its local uncommitted working
  tree. This is a cross-repo sequencing gap (radio-cat-rs's ADR 0004 work
  needs to be committed and pushed to `main` before `ft991a`'s Windows
  build will actually succeed against the real dependency), not a defect
  introduced by this task, and not something `ft991a`'s own repo can fix.
