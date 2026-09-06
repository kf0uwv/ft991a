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

//! FT-991A Radio Control Application
//!
//! Main entry point: parse --port <path> from argv, open the io_uring serial
//! driver on that path, create a typed Ft991a client, and run the UI.
//!
//! The emulator (once its own dispatch wave lands) will run as a
//! **separate process**:
//!   cargo run --bin emulator
//! It will print the PTY slave path to stdout; pass that path here via
//! --port.

use tracing::info;

use cat_transport_serial::{SerialCatSession, SerialConfig, SerialPort};
use radio::{Ft991a, Profile};

/// Parsed command-line arguments.
struct Args {
    /// `--port <path>` — open a local serial port. Mutually exclusive with
    /// `server`; exactly one of the two is required.
    port: Option<String>,
    baud: u32,
    stop_bits: u8,
    /// `--profile <name>` (`planning/architect/task_plan.md` §12.3) —
    /// looked up by name in [`radio::default_profile_dir`] and applied once,
    /// right after connecting, before the UI event loop starts.
    profile: Option<String>,
    /// `--server <host:port>` — connect over TCP to a remote `ft991a
    /// server --raw-tcp-port <n>` instance's `cat-server` raw listener
    /// instead of opening a local serial port (`planning/app/task_plan.md`'s
    /// Wave 4 task). Mutually exclusive with `port`. Available on both
    /// Linux and Windows (see [`run_over_tcp`]'s doc comment) since
    /// `radio-cat-rs` ADR 0006 gave `cat-transport-tcp`/`cat-transport-core`
    /// a Windows backend.
    server: Option<String>,
}

/// Print usage and exit with code 1.
fn usage_exit() -> ! {
    eprintln!(
        "Usage: ft991a --port <serial-port-path> [--baud <rate>] [--stop-bits <n>] [--profile <name>]\n\
         \n\
           --port      Serial port path — connect directly to a\n\
                       physically-attached radio (required unless --server\n\
                       is given; mutually exclusive with it)\n\
                       Examples: /dev/pts/5  /dev/ttyUSB0\n\
           --baud      Baud rate: 4800, 9600, 19200, 38400  (default: 9600)\n\
                       (only applies with --port; ignored with --server —\n\
                       the remote `ft991a server` process owns that)\n\
           --stop-bits Stop bits: 1 or 2                    (default: 2)\n\
                       (only applies with --port; ignored with --server)\n\
           --server    <host:port> of a remote `ft991a server\n\
                       --raw-tcp-port <n>` instance — connect over TCP\n\
                       instead of opening a local serial port (required\n\
                       unless --port is given; mutually exclusive with it)\n\
           --profile   Name of a settings profile to apply on startup\n\
                       (looked up in the default profile directory, e.g.\n\
                       ~/.config/ft991a/profiles/<name>.toml on Linux)"
    );
    std::process::exit(1);
}

/// Parse `--port <path>`, `--baud <rate>`, `--stop-bits <n>`, `--server
/// <host:port>`, and `--profile <name>` from `std::env::args()`.  Unknown
/// flags are silently ignored.  Exits with an error message and code 1 for
/// missing or invalid values, or if `--port`/`--server` are both given or
/// both omitted.
fn parse_args() -> Args {
    let mut args_iter = std::env::args().skip(1);
    let mut port: Option<String> = None;
    let mut server: Option<String> = None;
    let mut profile: Option<String> = None;
    // FT-991A CAT baud rate default (9600) and choice set (4800/9600/
    // 19200/38400) per the manual's Menu items 029 "232C RATE" / 031
    // "CAT RATE" — see planning/architect/task_plan.md §1. Not the same
    // set as ts570d's (1200/2400/4800/9600).
    let mut baud: u32 = 9600;
    // FT-991A default serial framing is 8N2 (SerialConfig::default()'s
    // documented Yaesu default — see planning/architect/task_plan.md §1),
    // unlike ts570d's 8N1 default.
    let mut stop_bits: u8 = 2;

    loop {
        match args_iter.next().as_deref() {
            Some("--port") => match args_iter.next() {
                Some(path) => port = Some(path),
                None => usage_exit(),
            },
            Some("--baud") => match args_iter.next() {
                Some(val) => {
                    let rate: u32 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --baud value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match rate {
                        4800 | 9600 | 19200 | 38400 => baud = rate,
                        _ => {
                            eprintln!(
                                "error: invalid baud rate {}; valid values: 4800, 9600, 19200, 38400",
                                rate
                            );
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --baud requires a value");
                    std::process::exit(1);
                }
            },
            Some("--stop-bits") => match args_iter.next() {
                Some(val) => {
                    let n: u8 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --stop-bits value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match n {
                        1 | 2 => stop_bits = n,
                        _ => {
                            eprintln!("error: invalid stop bits {}; valid values: 1 or 2", n);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --stop-bits requires a value");
                    std::process::exit(1);
                }
            },
            Some("--profile") => match args_iter.next() {
                Some(name) => profile = Some(name),
                None => {
                    eprintln!("error: --profile requires a value");
                    std::process::exit(1);
                }
            },
            Some("--server") => match args_iter.next() {
                Some(addr) => server = Some(addr),
                None => {
                    eprintln!("error: --server requires a value");
                    std::process::exit(1);
                }
            },
            Some(_) => {}
            None => break,
        }
    }

    match (port, server) {
        (Some(_), Some(_)) => {
            eprintln!("error: --port and --server are mutually exclusive");
            std::process::exit(1);
        }
        (None, None) => usage_exit(),
        (port, server) => Args {
            port,
            baud,
            stop_bits,
            profile,
            server,
        },
    }
}

/// Look up `name` in [`radio::default_profile_dir`] and apply it against
/// `radio`. Exits with an error message and code 1 on any failure (missing
/// profile directory, no such file, parse error, or a radio error while
/// applying it) — an explicitly requested `--profile` that silently didn't
/// take effect would be more confusing than a hard failure at startup.
async fn apply_named_profile<R: radio::Radio + radio::Ft991aExtras>(radio: &mut R, name: &str) {
    let Some(dir) = radio::default_profile_dir() else {
        eprintln!("error: could not determine the default profile directory");
        std::process::exit(1);
    };
    let path = dir.join(format!("{name}.toml"));
    let profile = Profile::load_from_file(&path).unwrap_or_else(|e| {
        eprintln!("error: failed to load profile {:?}: {e}", path.display());
        std::process::exit(1);
    });
    if let Err(e) = profile.apply(radio).await {
        eprintln!("error: failed to apply profile {name:?}: {e}");
        std::process::exit(1);
    }
    info!("Applied profile {name:?} from {}", path.display());
}

/// Command-line arguments specific to `ft991a server ...`
/// (`planning/architect/task_plan.md` §12.2). Cross-platform since
/// `radio-cat-rs` docs/adr/0006-windows-network-transport.md's 2026-07-26
/// amendment gave `cat-rigctl` a real Windows backend — see
/// [`run_server_mode`]'s doc comment.
struct ServerArgs {
    port: String,
    baud: u32,
    stop_bits: u8,
    raw_tcp_port: Option<u16>,
    raw_udp_port: Option<u16>,
    rigctl_port: Option<u16>,
    /// The typed console protocol, for `ft991a-gui` and for
    /// `ft991a --server`.
    console_port: Option<u16>,
}

fn server_usage_exit() -> ! {
    eprintln!(
        "Usage: ft991a server --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]\n\
                     [--raw-tcp-port <port>] [--raw-udp-port <port>] [--rigctl-port <port>]\n\
                     [--console-port <port>]\n\
         \n\
           --port          Serial port path (required)\n\
           --baud          Baud rate: 4800, 9600, 19200, 38400  (default: 9600)\n\
           --stop-bits     Stop bits: 1 or 2                    (default: 2)\n\
           --raw-tcp-port  Bind cat-server's raw length-prefixed TCP protocol\n\
           --raw-udp-port  Bind cat-server's raw enveloped UDP protocol\n\
           --rigctl-port   Bind a Hamlib rigctld-compatible TCP listener\n\
                           (for WSJT-X's \"Hamlib NET rigctl\" rig type)\n\
           At least one of --raw-tcp-port/--raw-udp-port/--rigctl-port is required."
    );
    std::process::exit(1);
}

/// Parse `ft991a server`'s own flags from `std::env::args()`, skipping both
/// the program name and the `server` subcommand word itself (positions 0
/// and 1).
fn parse_server_args() -> ServerArgs {
    let mut args_iter = std::env::args().skip(2);
    let mut port: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut stop_bits: u8 = 2;
    let mut raw_tcp_port: Option<u16> = None;
    let mut raw_udp_port: Option<u16> = None;
    let mut rigctl_port: Option<u16> = None;
    let mut console_port: Option<u16> = None;

    fn parse_port_number(val: Option<String>, flag: &str) -> u16 {
        match val.and_then(|v| v.parse::<u16>().ok()) {
            Some(p) => p,
            None => {
                eprintln!("error: {flag} requires a valid port number (0-65535)");
                std::process::exit(1);
            }
        }
    }

    loop {
        match args_iter.next().as_deref() {
            Some("--port") => match args_iter.next() {
                Some(path) => port = Some(path),
                None => server_usage_exit(),
            },
            Some("--baud") => match args_iter.next() {
                Some(val) => {
                    let rate: u32 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --baud value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match rate {
                        4800 | 9600 | 19200 | 38400 => baud = rate,
                        _ => {
                            eprintln!(
                                "error: invalid baud rate {}; valid values: 4800, 9600, 19200, 38400",
                                rate
                            );
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --baud requires a value");
                    std::process::exit(1);
                }
            },
            Some("--stop-bits") => match args_iter.next() {
                Some(val) => {
                    let n: u8 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --stop-bits value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match n {
                        1 | 2 => stop_bits = n,
                        _ => {
                            eprintln!("error: invalid stop bits {}; valid values: 1 or 2", n);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --stop-bits requires a value");
                    std::process::exit(1);
                }
            },
            Some("--raw-tcp-port") => {
                raw_tcp_port = Some(parse_port_number(args_iter.next(), "--raw-tcp-port"))
            }
            Some("--raw-udp-port") => {
                raw_udp_port = Some(parse_port_number(args_iter.next(), "--raw-udp-port"))
            }
            Some("--rigctl-port") => {
                rigctl_port = Some(parse_port_number(args_iter.next(), "--rigctl-port"))
            }
            Some("--console-port") => {
                console_port = Some(parse_port_number(args_iter.next(), "--console-port"))
            }
            Some(_) => {}
            None => break,
        }
    }

    if raw_tcp_port.is_none()
        && raw_udp_port.is_none()
        && rigctl_port.is_none()
        && console_port.is_none()
    {
        eprintln!(
            "error: at least one of --raw-tcp-port/--raw-udp-port/--rigctl-port/--console-port \
             is required"
        );
        std::process::exit(1);
    }

    match port {
        Some(p) => ServerArgs {
            port: p,
            baud,
            stop_bits,
            raw_tcp_port,
            raw_udp_port,
            rigctl_port,
            console_port,
        },
        None => server_usage_exit(),
    }
}

/// `ft991a server ...` — headless network server mode
/// (`planning/architect/task_plan.md` §12.2): one process owns the serial
/// port, exposed over the network to WSJT-X (via the new rigctld-compatible
/// listener) and/or other `radio-cat-rs`-aware clients (via the existing
/// raw `cat-server` TCP/UDP listeners), instead of running the local TUI.
///
/// Cross-platform since `radio-cat-rs` docs/adr/0006-windows-network-
/// transport.md's 2026-07-26 amendment gave `cat-rigctl` a real Windows
/// backend, closing the gap that previously kept this Linux-only even
/// after `cat-transport-tcp`/`cat-transport-udp`/`cat-server` themselves
/// became cross-platform (see that amendment for the full history).
#[cfg(target_os = "linux")]
async fn run_server_mode() {
    let args = parse_server_args();

    let port = SerialPort::open(
        &args.port,
        SerialConfig {
            baud_rate: args.baud,
            stop_bits: args.stop_bits,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!(
        "Serial port opened (server mode): {} @ {} baud {} stop bit(s)",
        args.port, args.baud, args.stop_bits
    );

    let session = SerialCatSession::new(port);
    let config = server::ServerConfig {
        raw_tcp_port: args.raw_tcp_port,
        raw_udp_port: args.raw_udp_port,
        rigctl_port: args.rigctl_port,
        native_port: args.console_port,
    };

    if let Err(e) = server::run(session, config).await {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

/// Windows counterpart of the Linux [`run_server_mode`] above — same
/// behavior (same `ServerArgs`, same listeners, same rigctld/WSJT-X
/// support), identical up to `server::run` itself being a plain blocking
/// `fn` on Windows rather than `async fn` (`cat_rigctl::run` is
/// `#[cfg]`-selected the same way, since `#[monoio::main]` cannot exist on
/// Windows). Kept as an `async fn` purely so the call site in [`run_app`]
/// (`run_server_mode().await`) needs no platform branching of its own —
/// this function has no real `.await` point, and calling `server::run`
/// synchronously here simply blocks this thread until a listener fails,
/// which is this entry point's entire job (nothing else runs concurrently
/// in `ft991a server` mode).
#[cfg(target_os = "windows")]
async fn run_server_mode() {
    let args = parse_server_args();

    let port = SerialPort::open(
        &args.port,
        SerialConfig {
            baud_rate: args.baud,
            stop_bits: args.stop_bits,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!(
        "Serial port opened (server mode): {} @ {} baud {} stop bit(s)",
        args.port, args.baud, args.stop_bits
    );

    let session = SerialCatSession::new(port);
    let config = server::ServerConfig {
        raw_tcp_port: args.raw_tcp_port,
        raw_udp_port: args.raw_udp_port,
        rigctl_port: args.rigctl_port,
        native_port: args.console_port,
    };

    if let Err(e) = server::run(session, config) {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

/// The actual application logic, shared by both platform entry points below:
/// initialize logging, parse args, connect (serial or TCP), construct the
/// typed FT-991A client, and run the UI event loop.
async fn run_app() {
    // 1. Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Starting FT-991A Radio Control Application");

    // `ft991a server ...` (§12.2) branches off entirely before the direct/
    // TUI argument parsing below — it has its own flag set and never opens
    // a `ui::run` session.
    if std::env::args().nth(1).as_deref() == Some("server") {
        run_server_mode().await;
        return;
    }

    // 2. Parse CLI arguments.
    let args = parse_args();

    // 3. Connect, either to a local serial port or a remote `ft991a server`
    //    over TCP (`parse_args` guarantees exactly one of `args.port`/
    //    `args.server` is set), then run the radio + UI event loop.
    match &args.server {
        Some(addr) => run_over_tcp(&args, addr).await,
        None => run_over_serial(&args).await,
    }

    info!("Application stopped");
}

/// `--port <path>` (default): open a local serial port via the platform
/// backend (io_uring on Linux, a worker-thread-backed COM port on Windows —
/// see ADR 0004) and connect directly to a physically-attached radio.
/// `SerialPort`/`SerialConfig`/`Ft991a`/`ui::run` all behave identically on
/// Linux and Windows here. On Linux this must be called inside an active
/// monoio runtime because it registers the fd with io_uring.
async fn run_over_serial(args: &Args) {
    let path = args
        .port
        .as_deref()
        .expect("parse_args guarantees args.port is set when args.server is None");

    let port = SerialPort::open(
        path,
        SerialConfig {
            baud_rate: args.baud,
            stop_bits: args.stop_bits,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!(
        "Serial port opened: {} @ {} baud {} stop bit(s)",
        path, args.baud, args.stop_bits
    );

    let mut radio = Ft991a::new(SerialCatSession::new(port));

    if let Some(name) = &args.profile {
        apply_named_profile(&mut radio, name).await;
    }

    if let Err(e) = ui::run(radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }
}

/// `--server <host:port>` — connect over TCP to a remote `ft991a server
/// --raw-tcp-port <n>` instance's `cat-server` raw listener instead of
/// opening a local serial port (`planning/app/task_plan.md`'s Wave 4 task).
///
/// Available on both Linux and Windows: `radio-cat-rs` ADR 0006 gave
/// `cat-transport-tcp::TcpCatSession`/`cat-transport-core` real Windows
/// backends (a dedicated worker thread + the shared `completion` primitive,
/// the same shape ADR 0004 already used for `cat-transport-serial`), with
/// the identical public `TcpCatSession::connect`/`CatSession` API on both
/// platforms — so this function needs no platform branching of its own, and
/// no longer has a stub Windows counterpart. See [`TcpClientSession`]'s doc
/// comment for the `Ft991a<S>` trait-bound adapter this needs, and
/// [`cat_transport_core::NoModemControlLines`]'s doc comment for how the
/// (unrelated) "no RTS/DTR over TCP" gap is closed below.
async fn run_over_tcp(args: &Args, addr: &str) {
    let session = cat_transport_tcp::TcpCatSession::connect(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("error: failed to connect to {addr}: {e}");
            std::process::exit(1);
        });

    info!("Connected to remote CAT server at {addr}");

    let mut radio = Ft991a::new(cat_transport_core::NoModemControlLines::new(
        TcpClientSession::new(session),
    ));

    if let Some(name) = &args.profile {
        apply_named_profile(&mut radio, name).await;
    }

    if let Err(e) = ui::run(radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }
}

/// A [`cat_transport_core::CatSession`] adapter wrapping
/// [`cat_transport_tcp::TcpCatSession`], so it can be used as the `S` in
/// `radio::Ft991a<S>` — mirrors `server/src/broker_session.rs`'s
/// `BrokerCatSession` exactly, and for the same reason: every `Ft991a<S>`
/// trait impl in `radio` is bounded on `S: CatSession<Error =
/// TransportError>` specifically, not `TcpCatSession`'s own `Error =
/// TcpSessionError`, so a thin error-mapping wrapper is required. Lives
/// here (the app wiring layer), never in `radio`, per this repo's Rule 2
/// (`radio` never imports a transport crate directly).
///
/// Deliberately does **not** also implement
/// [`cat_transport_core::ModemControlLines`] itself anymore — per
/// `radio-cat-rs` ADR 0006 §7, that (unrelated) concern is now handled by
/// wrapping this adapter in [`cat_transport_core::NoModemControlLines`]
/// instead (see [`run_over_tcp`]), which composes the same honest-error
/// behavior this struct used to hand-write five near-identical `Err(...)`
/// bodies for. `From`/`Into`'s orphan rules mean `NoModemControlLines`
/// itself can't also do this struct's `TcpSessionError` → `TransportError`
/// mapping (it would need to see both concrete error types), so the two
/// concerns stay factored into two small, separately-composable layers
/// rather than one — see `NoModemControlLines`'s own doc comment for why.
struct TcpClientSession {
    session: cat_transport_tcp::TcpCatSession,
}

impl TcpClientSession {
    fn new(session: cat_transport_tcp::TcpCatSession) -> Self {
        Self { session }
    }
}

#[async_trait::async_trait(?Send)]
impl cat_transport_core::CatSession for TcpClientSession {
    type Error = cat_transport_core::TransportError;

    async fn execute(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8>,
    ) -> Result<cat_transport_core::ResponseDisposition, Self::Error> {
        self.session
            .execute(request, response)
            .await
            .map_err(|e| match e {
                cat_transport_tcp::TcpSessionError::Io(io_err) => {
                    cat_transport_core::TransportError::Io(io_err)
                }
                cat_transport_tcp::TcpSessionError::FrameTooLarge { len, max } => {
                    cat_transport_core::TransportError::Other(format!(
                        "frame length {len} exceeds max frame size {max} bytes"
                    ))
                }
            })
    }
}

/// Linux entry point. Uses monoio's io_uring runtime (single-threaded,
/// !Send). Unchanged from before the Windows port — zero behavior change.
#[cfg(target_os = "linux")]
#[monoio::main(timer_enabled = true)]
async fn main() {
    run_app().await
}

/// Windows entry point. `monoio` cannot compile on Windows at all
/// (io_uring is a Linux kernel interface), so there is no `#[monoio::main]`
/// equivalent available. Per `radio-cat-rs` ADR 0004 §1, this repo's
/// architecture is a single sequential loop with no concurrent task
/// (confirmed: no `monoio::spawn` anywhere in this repo), so a minimal
/// hand-rolled `block_on` is sufficient — no new async-runtime crate
/// dependency.
#[cfg(target_os = "windows")]
fn main() {
    windows_block_on::block_on(run_app())
}

/// A minimal, single-threaded, thread-parking `block_on` executor for the
/// Windows entry point, per ADR 0004 §1's exact specification.
///
/// This is intentionally tiny and narrowly scoped: it drives exactly one
/// top-level future (`run_app()`) to completion on the calling thread, with
/// no support for spawning additional tasks. That is sufficient here
/// because `ft991a` never spawns concurrent tasks (unlike `ts570d`, whose
/// two-task `ui`/`radio` design would need a different, heavier Windows
/// executor — see ADR 0004 §1's `ts570d` discussion, not applicable here).
///
/// # How it works
///
/// `block_on` repeatedly polls the future. Every `Future::poll` call is
/// handed a [`Context`] wrapping a [`Waker`]. If the future returns
/// `Poll::Pending`, it means some other party (here, a background worker
/// thread inside `cat-transport-serial`'s Windows `SerialPort`, per ADR
/// 0004 §1's completion-primitive design) has been given a *clone* of that
/// `Waker` and has promised to call `.wake()` on it once progress is
/// possible again. Until then, this thread has nothing productive to do, so
/// it calls [`std::thread::park`] to yield the CPU. When the future's
/// waker is invoked from another thread, `wake()` calls
/// [`std::thread::Thread::unpark`] on *this* thread, which causes the
/// parked `park()` call to return, and the loop polls again.
///
/// `std::thread::park`'s documented contract permits spurious wakeups (a
/// `park()` call may return without a matching `unpark()`), which is why
/// this is a loop that always re-polls rather than a one-shot wait: a
/// spurious wakeup here just causes one extra `poll()` call that returns
/// `Pending` again, which is harmless, not a correctness bug.
#[cfg(target_os = "windows")]
mod windows_block_on {
    use std::future::Future;
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake};
    use std::thread::{self, Thread};

    /// A [`Wake`] implementation that unparks the thread which created it.
    /// This is the classic minimal thread-parking waker pattern: `wake()`
    /// (and `wake_by_ref()`, via the default trait method that clones
    /// `Arc<Self>` and calls `wake()`) is safe to call from any thread —
    /// exactly the guarantee `std::task::Waker` requires — because
    /// `Thread::unpark()` itself is documented as safe to call from any
    /// thread, any number of times, at any point in that thread's
    /// lifetime, including before it parks (in which case the *next*
    /// `park()` call returns immediately rather than blocking).
    struct ThreadWaker(Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Block the calling thread until `future` resolves, returning its
    /// output. Drives exactly one future to completion; does not support
    /// spawning additional concurrent tasks.
    pub fn block_on<F: Future>(future: F) -> F::Output {
        // `pin!` gives us a stack-pinned, `Pin<&mut F>` without requiring
        // `F: Unpin` or a heap allocation (`Box::pin`) — the future may be
        // a large, self-referential compiler-generated async-fn state
        // machine, exactly the shape `run_app()` is.
        let mut future = pin!(future);

        let waker = Arc::new(ThreadWaker(thread::current())).into();
        let mut cx = Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                // Some pending operation elsewhere (e.g. the Windows serial
                // worker thread) holds a clone of `waker` and will call
                // `.wake()` on it once this future can make progress
                // again. Park until then; spurious wakeups just cause one
                // harmless extra `poll()` (see module docs).
                Poll::Pending => thread::park(),
            }
        }
    }
}
