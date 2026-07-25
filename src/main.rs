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
    port: String,
    baud: u32,
    stop_bits: u8,
    /// `--profile <name>` (`planning/architect/task_plan.md` §12.3) —
    /// looked up by name in [`radio::default_profile_dir`] and applied once,
    /// right after connecting, before the UI event loop starts.
    profile: Option<String>,
}

/// Print usage and exit with code 1.
fn usage_exit() -> ! {
    eprintln!(
        "Usage: ft991a --port <serial-port-path> [--baud <rate>] [--stop-bits <n>] [--profile <name>]\n\
         \n\
           --port      Serial port path (required)\n\
                       Examples: /dev/pts/5  /dev/ttyUSB0\n\
           --baud      Baud rate: 4800, 9600, 19200, 38400  (default: 9600)\n\
           --stop-bits Stop bits: 1 or 2                    (default: 2)\n\
           --profile   Name of a settings profile to apply on startup\n\
                       (looked up in the default profile directory, e.g.\n\
                       ~/.config/ft991a/profiles/<name>.toml on Linux)"
    );
    std::process::exit(1);
}

/// Parse `--port <path>`, `--baud <rate>`, `--stop-bits <n>`, and
/// `--profile <name>` from `std::env::args()`.  Unknown flags are silently
/// ignored.  Exits with an error message and code 1 for missing or invalid
/// values.
fn parse_args() -> Args {
    let mut args_iter = std::env::args().skip(1);
    let mut port: Option<String> = None;
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
            Some(_) => {}
            None => break,
        }
    }

    match port {
        Some(p) => Args {
            port: p,
            baud,
            stop_bits,
            profile,
        },
        None => usage_exit(),
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
/// (`planning/architect/task_plan.md` §12.2). Linux-only — see
/// [`run_server_mode`]'s doc comment.
#[cfg(target_os = "linux")]
struct ServerArgs {
    port: String,
    baud: u32,
    stop_bits: u8,
    raw_tcp_port: Option<u16>,
    raw_udp_port: Option<u16>,
    rigctl_port: Option<u16>,
}

#[cfg(target_os = "linux")]
fn server_usage_exit() -> ! {
    eprintln!(
        "Usage: ft991a server --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]\n\
                     [--raw-tcp-port <port>] [--raw-udp-port <port>] [--rigctl-port <port>]\n\
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
#[cfg(target_os = "linux")]
fn parse_server_args() -> ServerArgs {
    let mut args_iter = std::env::args().skip(2);
    let mut port: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut stop_bits: u8 = 2;
    let mut raw_tcp_port: Option<u16> = None;
    let mut raw_udp_port: Option<u16> = None;
    let mut rigctl_port: Option<u16> = None;

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
            Some(_) => {}
            None => break,
        }
    }

    if raw_tcp_port.is_none() && raw_udp_port.is_none() && rigctl_port.is_none() {
        eprintln!("error: at least one of --raw-tcp-port/--raw-udp-port/--rigctl-port is required");
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
/// Linux-only — the `server` crate wraps `radio-cat-rs`'s `cat-server`,
/// which only compiles where `monoio::net`/`monoio::spawn` are available
/// (mirrors `cat-transport-serial`'s own Windows/Linux split, except no
/// Windows backend exists for the server side at all; see `server/
/// Cargo.toml`'s doc comment). The root `Cargo.toml` only depends on
/// `server` under `cfg(target_os = "linux")`, so this function's Windows
/// counterpart below never touches that crate at all.
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
    };

    if let Err(e) = server::run(session, config).await {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

/// Windows has no server-mode backend at all (see [`run_server_mode`]'s doc
/// comment) — `ft991a server ...` fails fast with a clear message rather
/// than silently behaving like plain `ft991a`.
#[cfg(not(target_os = "linux"))]
async fn run_server_mode() {
    eprintln!("error: `ft991a server` is only available on Linux");
    std::process::exit(1);
}

/// The actual application logic, shared by both platform entry points below:
/// initialize logging, parse args, open the serial port, construct the
/// typed FT-991A client, and run the UI event loop. Platform-neutral —
/// `SerialPort`/`SerialConfig`/`Ft991a`/`ui::run` all behave identically on
/// Linux and Windows (see `radio-cat-rs` ADR 0004). Only *what drives this
/// future to completion* differs per platform; see `main` below.
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

    // 3. Open the port via the platform serial backend (io_uring on Linux,
    //    a worker-thread-backed COM port on Windows — see ADR 0004). On
    //    Linux this must be called inside an active monoio runtime because
    //    it registers the fd with io_uring.
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
        "Serial port opened: {} @ {} baud {} stop bit(s)",
        args.port, args.baud, args.stop_bits
    );

    // 4. Wrap in a CatSession (serial framing), then the typed FT-991A client.
    let mut radio = Ft991a::new(SerialCatSession::new(port));

    // 5. Apply a startup profile, if requested (§12.3).
    if let Some(name) = &args.profile {
        apply_named_profile(&mut radio, name).await;
    }

    // 6. Run the radio + UI event loop.
    if let Err(e) = ui::run(radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
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
