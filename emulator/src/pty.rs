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

use std::os::unix::io::AsRawFd;

use serialport::{SerialPort, TTYPort};

use crate::EmulatorError;

/// A Linux pseudo-terminal pair backed by `serialport::TTYPort`.
///
/// `TTYPort::pair()` opens the next free PTY via `posix_openpt`, unlocks it,
/// configures the slave in raw mode, and returns both ends as `TTYPort` values.
///
/// The master is the emulator's I/O handle (`Box<dyn SerialPort>`).
/// The slave is kept alive as a field so the master does not see EIO when no
/// external process has connected yet — closing the slave would cause reads on
/// the master to return EIO immediately.
/// The slave path (`/dev/pts/N`) is stored at construction time for callers
/// that need to open the slave side independently.
pub struct PtyPair {
    /// Master side — the emulator reads commands from and writes responses to this port.
    /// Wrapped in `Option` so it can be consumed by `into_parts()`.
    master: Option<Box<dyn SerialPort>>,
    /// Raw file descriptor of the master, captured before boxing.
    /// Remains valid as long as `master` (or the moved-out port) is alive.
    master_fd: std::os::unix::io::RawFd,
    /// Kept open to ensure the slave side stays alive.
    _slave: TTYPort,
    /// Cached slave device path (e.g. `/dev/pts/5`).
    slave_path: String,
}

impl PtyPair {
    /// Create a new PTY pair.
    ///
    /// `TTYPort::pair()` already configures the slave in raw mode (via `cfmakeraw`),
    /// so no additional termios setup is required here.
    pub fn new() -> Result<Self, EmulatorError> {
        let (master, slave) = TTYPort::pair().map_err(|e| EmulatorError::Pty(e.to_string()))?;

        // Capture the raw fd before boxing, while the concrete type is still known.
        let master_fd = master.as_raw_fd();

        // The slave's name is always Some(...) after TTYPort::pair().
        let slave_path = slave
            .name()
            .ok_or_else(|| EmulatorError::Pty("slave TTYPort has no name".to_string()))?;

        Ok(PtyPair {
            master: Some(Box::new(master)),
            master_fd,
            _slave: slave,
            slave_path,
        })
    }

    /// Get the slave device path (e.g., "/dev/pts/5").
    pub fn slave_path(&self) -> &str {
        &self.slave_path
    }

    /// Returns the raw file descriptor of the master side for use in tests.
    ///
    /// The fd is valid as long as the master port has not been dropped.
    ///
    /// # Panics
    /// Panics if the master has already been taken.
    pub fn master_raw_fd(&self) -> std::os::unix::io::RawFd {
        assert!(
            self.master.is_some(),
            "master port already taken from PtyPair"
        );
        self.master_fd
    }

    /// Consume the master port, leaving `None` in its place.
    ///
    /// # Panics
    /// Panics if the master has already been taken.
    pub fn take_master(&mut self) -> Box<dyn SerialPort> {
        self.master
            .take()
            .expect("master port already taken from PtyPair")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_creation() {
        let pair = PtyPair::new().expect("PTY creation failed");
        assert!(
            pair.slave_path().starts_with("/dev/pts/"),
            "Expected /dev/pts/N path, got: {}",
            pair.slave_path()
        );
        assert!(
            std::path::Path::new(pair.slave_path()).exists(),
            "Slave path does not exist: {}",
            pair.slave_path()
        );
    }

    #[test]
    fn test_slave_path_exists_after_pty_creation() {
        let pair = PtyPair::new().expect("PTY creation failed");
        let path = pair.slave_path().to_string();
        assert!(
            std::path::Path::new(&path).exists(),
            "Slave path {} should exist while master is held",
            path
        );
    }

    #[test]
    fn test_master_raw_fd_is_valid() {
        let pair = PtyPair::new().expect("PTY creation failed");
        let fd = pair.master_raw_fd();
        // A valid open fd must be non-negative.
        assert!(fd >= 0, "master_raw_fd() returned a negative fd: {fd}");
        // Verify the fd is actually open by calling fcntl F_GETFD.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(
            flags >= 0,
            "master_raw_fd() {fd} is not a valid open fd (fcntl returned {flags})"
        );
    }
}
