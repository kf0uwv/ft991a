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

use std::time::Duration;

use serialport::SerialPort;

use crate::pty::PtyPair;
use crate::EmulatorError;

/// Controls whether the emulator binds to a virtual PTY or a physical serial device.
pub enum PortMode {
    /// Create a PTY pair and print the slave path to stdout.
    Virtual,
    /// Open a physical serial port device at the given path.
    Physical(String),
}

/// Parse `--port <value>` from an argument iterator.
///
/// - `--port` absent → `Virtual`
/// - `--port virtual` → `Virtual`
/// - `--port <path>` (any other value) → `Physical(path)`
pub fn parse_port_arg(mut args: impl Iterator<Item = String>) -> PortMode {
    while let Some(arg) = args.next() {
        if arg == "--port" {
            match args.next() {
                None => return PortMode::Virtual,
                Some(s) if s == "virtual" => return PortMode::Virtual,
                Some(path) => return PortMode::Physical(path),
            }
        }
    }
    PortMode::Virtual
}

/// Open a port according to `mode`.
///
/// Returns `(port, slave_path_opt)` where `slave_path_opt` is `Some(path)` for
/// virtual mode and `None` for physical mode.
pub fn open_port(mode: &PortMode) -> Result<(Box<dyn SerialPort>, Option<String>), EmulatorError> {
    match mode {
        PortMode::Virtual => {
            let mut pty = PtyPair::new()?;
            let slave_path = pty.slave_path().to_string();
            let master = pty.take_master();
            // Keep the PtyPair alive by leaking it — the slave fd must stay open.
            // Safety: we intentionally leak the PtyPair so the slave fd is never closed.
            // The process exits when the emulator exits, so this is not a true leak.
            std::mem::forget(pty);
            Ok((master, Some(slave_path)))
        }
        PortMode::Physical(path) => {
            // FT-991A CAT baud rate default is 9600 (manual p.7, menu items
            // 029 "232C RATE"/031 "CAT RATE", option set 4800/9600/19200/
            // 38400 bps; matches this repo's own established default in
            // `src/main.rs`'s `--baud` default and
            // `planning/architect/task_plan.md` §1) — unlike ts570d's
            // physical-mode default of 4800, which does not apply here.
            let port = serialport::new(path, 9600)
                .timeout(Duration::from_millis(100))
                .open()
                .map_err(|e| EmulatorError::Pty(e.to_string()))?;
            Ok((port, None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> impl Iterator<Item = String> {
        v.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn test_parse_port_arg_absent() {
        let mode = parse_port_arg(args(&[]));
        assert!(matches!(mode, PortMode::Virtual));
    }

    #[test]
    fn test_parse_port_arg_virtual() {
        let mode = parse_port_arg(args(&["--port", "virtual"]));
        assert!(matches!(mode, PortMode::Virtual));
    }

    #[test]
    fn test_parse_port_arg_physical() {
        let mode = parse_port_arg(args(&["--port", "/dev/ttyUSB0"]));
        match mode {
            PortMode::Physical(path) => assert_eq!(path, "/dev/ttyUSB0"),
            PortMode::Virtual => panic!("expected Physical, got Virtual"),
        }
    }
}
