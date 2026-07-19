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

use std::io::{Read, Write};

use serialport::SerialPort;

use crate::EmulatorError;

/// Buffers bytes from the serial port and emits complete `;`-terminated command strings.
#[derive(Default)]
pub struct CommandFramer {
    buf: Vec<u8>,
}

impl CommandFramer {
    pub fn new() -> Self {
        CommandFramer::default()
    }

    /// Push raw bytes into the framer.
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Drain and return any complete `;`-terminated commands from the buffer.
    /// Each returned string does NOT include the trailing `;`.
    pub fn drain_commands(&mut self) -> Vec<String> {
        let mut commands = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b';') {
            let raw = self.buf.drain(..=pos).collect::<Vec<u8>>();
            // raw includes the ';' at the end; strip it and trim whitespace
            let s = String::from_utf8_lossy(&raw[..raw.len().saturating_sub(1)])
                .trim()
                .to_string();
            if !s.is_empty() {
                commands.push(s);
            }
        }
        commands
    }
}

/// Wraps the master side of a PTY (`Box<dyn SerialPort>`) for blocking read/write I/O.
///
/// `SerialPort` requires `std::io::Read + std::io::Write`, so the port is used
/// directly without any fd duplication.
pub struct EmulatorIo {
    port: Box<dyn SerialPort>,
    framer: CommandFramer,
}

impl EmulatorIo {
    /// Create an `EmulatorIo` from the master serial port.
    pub fn from_port(port: Box<dyn SerialPort>) -> Self {
        EmulatorIo {
            port,
            framer: CommandFramer::new(),
        }
    }

    /// Perform a blocking read from the master port, push data into the framer,
    /// and return any complete commands that have accumulated.
    pub fn read_commands(&mut self) -> Result<Vec<String>, EmulatorError> {
        let mut tmp = [0u8; 256];
        let n = self.port.read(&mut tmp).map_err(EmulatorError::Io)?;
        if n == 0 {
            return Ok(Vec::new());
        }
        self.framer.push(&tmp[..n]);
        Ok(self.framer.drain_commands())
    }

    /// Write a response string to the master port (no semicolon appended —
    /// the caller is responsible for including the full response).
    pub fn write_response(&mut self, response: &str) -> Result<(), EmulatorError> {
        self.port
            .write_all(response.as_bytes())
            .map_err(EmulatorError::Io)?;
        self.port.flush().map_err(EmulatorError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framer_single_command() {
        let mut f = CommandFramer::new();
        f.push(b"FA;");
        let cmds = f.drain_commands();
        assert_eq!(cmds, vec!["FA"]);
    }

    #[test]
    fn test_framer_multiple_commands() {
        let mut f = CommandFramer::new();
        f.push(b"FA;FB;");
        let cmds = f.drain_commands();
        assert_eq!(cmds, vec!["FA", "FB"]);
    }

    #[test]
    fn test_framer_partial_then_complete() {
        let mut f = CommandFramer::new();
        f.push(b"FA0001400");
        assert!(f.drain_commands().is_empty());
        f.push(b"0000;");
        let cmds = f.drain_commands();
        assert_eq!(cmds, vec!["FA00014000000"]);
    }

    #[test]
    fn test_framer_strips_whitespace() {
        let mut f = CommandFramer::new();
        f.push(b"FA;\n");
        let cmds = f.drain_commands();
        // The \n is after the ; so it stays in the buffer for the next call,
        // which is fine — it'll be emitted as empty and skipped.
        assert_eq!(cmds, vec!["FA"]);
    }
}
