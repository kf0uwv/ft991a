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
use radio::Ft991a;

/// Parsed command-line arguments.
struct Args {
    port: String,
    baud: u32,
    stop_bits: u8,
}

/// Print usage and exit with code 1.
fn usage_exit() -> ! {
    eprintln!(
        "Usage: ft991a --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]\n\
         \n\
           --port      Serial port path (required)\n\
                       Examples: /dev/pts/5  /dev/ttyUSB0\n\
           --baud      Baud rate: 4800, 9600, 19200, 38400  (default: 9600)\n\
           --stop-bits Stop bits: 1 or 2                    (default: 2)"
    );
    std::process::exit(1);
}

/// Parse `--port <path>`, `--baud <rate>`, and `--stop-bits <n>` from
/// `std::env::args()`.  Unknown flags are silently ignored.  Exits with an
/// error message and code 1 for missing or invalid values.
fn parse_args() -> Args {
    let mut args_iter = std::env::args().skip(1);
    let mut port: Option<String> = None;
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
            Some(_) => {}
            None => break,
        }
    }

    match port {
        Some(p) => Args {
            port: p,
            baud,
            stop_bits,
        },
        None => usage_exit(),
    }
}

/// Entry point.  Uses monoio's io_uring runtime (single-threaded, !Send).
#[monoio::main(timer_enabled = true)]
async fn main() {
    // 1. Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Starting FT-991A Radio Control Application");

    // 2. Parse CLI arguments.
    let args = parse_args();

    // 3. Open the port via the io_uring serial driver.
    //    SerialPort::open must be called inside an active monoio runtime
    //    because it registers the fd with io_uring.
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
    let radio = Ft991a::new(SerialCatSession::new(port));

    // 5. Run the radio + UI event loop.
    if let Err(e) = ui::run(radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
}
