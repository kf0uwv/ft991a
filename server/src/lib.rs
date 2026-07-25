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
//! Wires up `radio-cat-rs`'s `cat-server` request broker (which owns the
//! physical [`cat_transport_core::CatSession`] behind a single ordered
//! worker — see [`cat_server::broker`]'s own docs) plus two kinds of
//! listener:
//! - the existing `cat-server` raw TCP/UDP listeners (custom
//!   length-prefixed/enveloped framing carrying raw CAT bytes — any
//!   `radio-cat-rs`-aware client, not WSJT-X);
//! - [`rigctl`], a new Hamlib rigctld-compatible TCP listener, for WSJT-X's
//!   "Hamlib NET rigctl" rig type.
//!
//! Linux-only, mirroring `cat-server`'s own scope (see this crate's
//! `Cargo.toml` doc comment) — the `ft991a` binary only depends on this
//! crate under `cfg(target_os = "linux")`.

mod broker_session;
pub mod rigctl;

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use cat_server::ClientRegistry;
use cat_transport_core::CatSession;
use monoio::net::{udp::UdpSocket, TcpListener};
use tracing::info;

/// Which network listeners to bring up. Every field is optional — a `None`
/// port simply skips binding that listener, so a deployment can run with
/// only the pieces it needs (typically just `rigctl_port`, for WSJT-X).
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    /// `cat-server`'s raw length-prefixed TCP protocol.
    pub raw_tcp_port: Option<u16>,
    /// `cat-server`'s raw enveloped UDP protocol.
    pub raw_udp_port: Option<u16>,
    /// The new Hamlib rigctld-compatible TCP listener, for WSJT-X.
    pub rigctl_port: Option<u16>,
}

/// Bring up the broker (owning `session`, the one physical radio
/// connection) plus every listener `config` requests, and run until one of
/// them fails. `S` is generic (not hardcoded to `SerialCatSession`) so
/// `main.rs` remains the only place a concrete transport type is named, per
/// this repo's Rule 5 — but this crate is otherwise contractually
/// FT-991A-shaped (it names `radio::FT991A_COMMAND_TABLE`/`Ft991aCommandId`
/// directly, exactly like `ui` does for the UI-facing traits), not
/// radio-generic.
pub async fn run<S>(session: S, config: ServerConfig) -> io::Result<()>
where
    S: CatSession + 'static,
    S::Error: std::error::Error + 'static,
{
    let (worker, handle) = cat_server::build(session, &radio::FT991A_COMMAND_TABLE);
    monoio::spawn(worker.run());

    let mut tasks = Vec::new();

    if let Some(port) = config.raw_tcp_port {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        info!("Raw CAT TCP listener bound on 0.0.0.0:{port}");
        let handle = handle.clone();
        let registry = Rc::new(RefCell::new(ClientRegistry::new()));
        tasks.push(monoio::spawn(async move {
            let _ = cat_server::tcp::serve(listener, handle, registry).await;
        }));
    }

    if let Some(port) = config.raw_udp_port {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        info!("Raw CAT UDP listener bound on 0.0.0.0:{port}");
        let handle = handle.clone();
        let registry = Rc::new(RefCell::new(ClientRegistry::new()));
        tasks.push(monoio::spawn(async move {
            let _ = cat_server::udp::serve(socket, handle, registry).await;
        }));
    }

    if let Some(port) = config.rigctl_port {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        info!("Rigctld-compatible TCP listener bound on 0.0.0.0:{port} (for WSJT-X)");
        let handle = handle.clone();
        tasks.push(monoio::spawn(async move {
            let _ = rigctl::serve(listener, handle).await;
        }));
    }

    if tasks.is_empty() {
        return Err(io::Error::other(
            "server mode requires at least one of --raw-tcp-port/--raw-udp-port/--rigctl-port",
        ));
    }

    // Every listener loop above only returns on a fatal accept()/bind-time
    // error (or never, on the happy path) — wait for the first one to end.
    futures::future::select_all(tasks).await;
    Ok(())
}

// `BrokerHandle` re-exported for `main.rs`'s benefit, if it ever needs to
// hold one directly (not required by the current `--profile`-at-startup
// wiring, but harmless surface — mirrors `cat_server`'s own re-export
// shape).
pub use cat_server::BrokerHandle as ServerBrokerHandle;

#[cfg(test)]
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
