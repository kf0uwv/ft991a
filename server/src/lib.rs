// Copyright 2026 Matt Franklin
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Network server mode (`planning/architect/task_plan.md` §12): one process
//! owns the physical FT-991A serial session, shared by any number of remote
//! clients over the network instead of each needing their own exclusive
//! serial connection.
//!
//! This crate now only supplies the FT-991A-specific glue over
//! `radio-cat-rs`'s generic `cat-rigctl` crate: `rigctl_radio`'s
//! `Ft991aRigctl<S>` wrapper (`impl cat_rigctl::RigctlRadio` for it — see
//! that module's doc comment for why a local wrapper is needed, not a
//! direct impl on `radio::Ft991a<S>`), plus [`run`], a thin wrapper around
//! [`cat_rigctl::run`] that supplies `radio::Ft991a::new` and
//! `radio::FT991A_COMMAND_TABLE`. Everything else — the request broker,
//! raw TCP/UDP listeners, the rigctld-compatible TCP listener for WSJT-X,
//! and listener orchestration/error propagation — lives in
//! `cat-server`/`cat-rigctl`, shared with `ts570d`.
//!
//! Cross-platform since `radio-cat-rs` docs/adr/0006-windows-network-
//! transport.md's 2026-07-26 amendment gave `cat-rigctl` a real Windows
//! backend — [`run`] is `#[cfg]`-selected per platform to match
//! `cat_rigctl::run`'s own split (`async fn` on Linux, a plain blocking
//! `fn` on Windows, since `#[monoio::main]` cannot exist there); a caller
//! (`main.rs`) sees one `run` either way and needs no platform branching
//! of its own beyond `.await`ing it only on Linux.

mod rigctl_radio;

use rigctl_radio::Ft991aRigctl;

pub use cat_rigctl::ServerConfig;

/// Bring up the broker (owning `session`, the one physical radio
/// connection) plus every listener `config` requests, and run until one of
/// them fails. `S` is generic (not hardcoded to `SerialCatSession`) so
/// `main.rs` remains the only place a concrete transport type is named, per
/// this repo's Rule 5 — but this crate is otherwise contractually
/// FT-991A-shaped (it names `radio::FT991A_COMMAND_TABLE`/`radio::Ft991a`
/// directly, exactly like `ui` does for the UI-facing traits), not
/// radio-generic.
#[cfg(target_os = "linux")]
pub async fn run<S>(session: S, config: ServerConfig) -> std::io::Result<()>
where
    S: cat_transport_core::CatSession + 'static,
    S::Error: std::error::Error + 'static,
{
    cat_rigctl::run(session, &radio::FT991A_COMMAND_TABLE, config, |s| {
        Ft991aRigctl::new(radio::Ft991a::new(s))
    })
    .await
}

/// Windows implementation of [`run`] — same behavior as the Linux one
/// (same [`ServerConfig`], same listeners, same rigctld/WSJT-X support),
/// but a plain blocking `fn`: `cat_rigctl::run` itself is a plain `fn` on
/// Windows (genuine OS threads instead of `monoio`'s cooperative tasks),
/// so there is nothing to `.await` here at all.
#[cfg(target_os = "windows")]
pub fn run<S>(session: S, config: ServerConfig) -> std::io::Result<()>
where
    S: cat_transport_core::CatSession + Send + 'static,
    S::Error: std::error::Error + 'static,
{
    cat_rigctl::run(session, &radio::FT991A_COMMAND_TABLE, config, |s| {
        Ft991aRigctl::new(radio::Ft991a::new(s))
    })
}

// Gated to Linux: exercises the `async fn run` implementation via
// `#[monoio::test]`. The Windows `run` is a thin, low-risk wrapper over
// `cat_rigctl::run`'s own Windows implementation, which has its own test
// coverage in `radio-cat-rs` (`rigctl_windows`'s tests) — see that crate's
// docs/adr/0006-windows-network-transport.md amendment.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use cat_transport_core::test_support::ScriptedCatSession;

    #[monoio::test(driver = "legacy")]
    async fn run_with_no_listeners_configured_returns_an_error() {
        let session = ScriptedCatSession::new();
        let result = run(session, ServerConfig::default()).await;
        assert!(result.is_err());
    }
}
