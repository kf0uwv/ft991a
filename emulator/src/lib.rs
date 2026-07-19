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

pub mod emulator;
pub mod io;
pub mod logger;
pub mod port;
pub mod pty;
pub mod tui;

#[derive(Debug, thiserror::Error)]
pub enum EmulatorError {
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod integration_tests {
    use std::io::Write;
    use std::time::Duration;

    use crate::emulator::Emulator;

    /// Integration test: spawn the emulator in a background thread, open the
    /// slave PTY as a standard file, send "FA;" and verify the response
    /// matches `FA\d{9};` (FT-991A's 9-digit VFO A frequency wire format,
    /// see `radio::ft991a_radio` module docs — NOT ts570d's 11-digit shape).
    #[test]
    fn test_emulator_fa_round_trip() {
        // Build the emulator (creates PTY pair).
        let mut emu = Emulator::new().expect("Emulator::new failed");
        let slave_path = emu.slave_path().to_string();

        // Verify the slave path exists before spawning the thread.
        assert!(
            std::path::Path::new(&slave_path).exists(),
            "slave PTY does not exist before spawn: {}",
            slave_path
        );

        // Spawn the emulator event loop in a background thread.
        std::thread::spawn(move || {
            let _ = emu.run();
        });

        // Give the thread a moment to enter its read loop.
        std::thread::sleep(Duration::from_millis(100));

        // Verify the slave path still exists after spawn.
        assert!(
            std::path::Path::new(&slave_path).exists(),
            "slave PTY does not exist after spawn: {}",
            slave_path
        );

        // Open the slave PTY as a regular file (blocking I/O).
        let mut slave_write = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&slave_path)
            .unwrap_or_else(|e| panic!("failed to open slave PTY '{}': {}", slave_path, e));
        let slave_read = slave_write.try_clone().expect("clone slave fd");

        // Send the FA query command.
        slave_write
            .write_all(b"FA;")
            .expect("write FA; to slave PTY");
        slave_write.flush().expect("flush slave PTY");

        // Read the response (terminated by ';').
        let mut response = String::new();
        let mut buf = [0u8; 1];
        loop {
            use std::io::Read;
            (&slave_read)
                .read_exact(&mut buf)
                .expect("read byte from slave PTY");
            response.push(buf[0] as char);
            if buf[0] == b';' {
                break;
            }
            // Safety limit to avoid hanging test.
            if response.len() > 64 {
                panic!("Response too long: {:?}", response);
            }
        }

        // Validate: must match FA\d{9};
        assert_eq!(&response[..2], "FA", "response prefix: {}", response);
        assert_eq!(&response[response.len() - 1..], ";", "response suffix");
        let digits = &response[2..response.len() - 1];
        assert_eq!(digits.len(), 9, "expected 9 frequency digits: {}", response);
        assert!(
            digits.chars().all(|c| c.is_ascii_digit()),
            "non-digit in frequency: {}",
            response
        );
    }
}
