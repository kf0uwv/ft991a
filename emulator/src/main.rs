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

use emulator::emulator::Emulator;
use emulator::logger::BackgroundLogger;
use emulator::port::{self, PortMode};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let tui = args.iter().any(|a| a == "--tui");
    let background = args.iter().any(|a| a == "--background");

    // --tui and --background are mutually exclusive.
    if tui && background {
        eprintln!("Error: --tui and --background are mutually exclusive");
        std::process::exit(1);
    }

    // Parse optional --log-file <path> (only meaningful with --background).
    let log_file: Option<String> = {
        let mut lf = None;
        let mut it = args.iter().peekable();
        while let Some(arg) = it.next() {
            if arg == "--log-file" {
                lf = it.next().cloned();
            }
        }
        lf
    };

    // Determine port mode from --port argument.
    let mode = port::parse_port_arg(args.into_iter());

    // Open the port and print status.
    let (serial_port, slave_path_opt) = match port::open_port(&mode) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("Failed to open port: {err}");
            std::process::exit(1);
        }
    };

    // Always print the PTY slave path as the FIRST line in KEY=VALUE format
    // for virtual mode, so scripts can parse it regardless of run mode.
    let slave_path = match (&mode, slave_path_opt) {
        (PortMode::Virtual, Some(ref path)) => {
            println!("PTY_SLAVE={path}");
            path.clone()
        }
        (PortMode::Physical(ref path), None) => {
            println!("Connected to {path}");
            path.clone()
        }
        // Fallback (should not occur).
        (_, Some(path)) => path,
        (_, None) => String::new(),
    };

    let mut emu = Emulator::from_port(serial_port, slave_path);

    // Set up Ctrl-C handler for graceful shutdown.
    ctrlc::set_handler(|| {
        // Restore terminal in case --tui is active.
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        println!("\nEmulator shutting down.");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let result = if tui {
        emu.run_with_tui()
    } else if background {
        // Build the logger: file if --log-file was given, otherwise stdout.
        let logger = if let Some(ref path) = log_file {
            match BackgroundLogger::file(path) {
                Ok(l) => l,
                Err(err) => {
                    eprintln!("Failed to open log file '{path}': {err}");
                    std::process::exit(1);
                }
            }
        } else {
            BackgroundLogger::stdout()
        };
        emu.run_background(logger)
    } else {
        emu.run()
    };

    if let Err(err) = result {
        eprintln!("Emulator error: {err}");
        std::process::exit(1);
    }
}
