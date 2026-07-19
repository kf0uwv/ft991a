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

use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// A single field mutation that occurred while handling a CAT command.
#[derive(Debug, Clone)]
pub struct StateChange {
    /// The `RadioState` field name (snake_case).
    pub field: &'static str,
    /// New value encoded as a string.
    pub value: String,
}

/// One JSON event emitted by the background logger.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LogEvent<'a> {
    Startup {
        ts: u64,
        port: &'a str,
        mode: &'a str,
    },
    Command {
        ts: u64,
        raw: &'a str,
        response: &'a str,
    },
    StateChange {
        ts: u64,
        field: &'a str,
        value: &'a str,
    },
}

/// Returns the current time as milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Wraps a `Write` sink and serializes `LogEvent` values as newline-delimited JSON.
pub struct BackgroundLogger {
    sink: Box<dyn Write>,
}

impl BackgroundLogger {
    /// Create a logger that writes to stdout.
    pub fn stdout() -> Self {
        Self {
            sink: Box::new(io::stdout()),
        }
    }

    /// Create a logger that writes to the file at `path`, creating or truncating it.
    pub fn file(path: &str) -> io::Result<Self> {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        Ok(Self { sink: Box::new(f) })
    }

    /// Serialize `event` as a single JSON line and write it to the sink.
    ///
    /// Errors are intentionally ignored so that a logging failure never
    /// crashes the emulator.
    pub fn log_event(&mut self, event: &LogEvent<'_>) {
        let _ = self.log_event_inner(event);
    }

    fn log_event_inner(&mut self, event: &LogEvent<'_>) -> io::Result<()> {
        let json = serde_json::to_string(event).map_err(io::Error::other)?;
        self.sink.write_all(json.as_bytes())?;
        self.sink.write_all(b"\n")?;
        self.sink.flush()?;
        Ok(())
    }
}
