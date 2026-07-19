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

//! FT-991A Terminal UI — Wave 2 real ratatui interface.
//!
//! A flat single-screen TUI, right-sized for this repo's 11-command first
//! slice — NOT a port of `ts570d/ui`'s ~60-command three-level menu tree.
//! See `planning/architect/task_plan.md` §6 for the full design rationale.
//!
//! Per this repo's `CLAUDE.md` dependency model, `ui` depends on `radio`
//! only (for the [`radio::Radio`] trait and domain types) and never imports
//! a transport crate directly.

pub(crate) mod control;
pub(crate) mod layout;
mod terminal;

pub use terminal::run;

use radio::{Mode, TxState};

/// Errors that can occur while running the UI.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience [`Result`] alias for UI operations.
pub type UiResult<T> = Result<T, UiError>;

/// Live radio state for UI rendering.
///
/// Mirrors `ts570d::ui::RadioDisplay`'s *role* (a plain snapshot struct
/// polled from the `Radio` trait), not its field list — RIT/XIT/split/
/// memory/antenna/AGC/etc. don't exist in this first slice. See
/// `planning/architect/task_plan.md` §6.2.
#[derive(Debug, Clone, PartialEq)]
pub struct Ft991aDisplay {
    pub vfo_a_hz: u64,
    pub vfo_b_hz: u64,
    pub mode: Mode,
    pub tx_state: TxState,
    /// 0-255, read-only.
    pub smeter: u8,
    pub power_on: bool,
    /// 0-255.
    pub af_gain: u8,
    /// 0-255.
    pub rf_gain: u8,
    /// 0-100 (NOT 255 — squelch has its own, narrower range; see
    /// `radio/src/ft991a.rs`'s `get_squelch`/`set_squelch` doc comments).
    pub squelch: u8,
    /// TX power, watts, 5-100 (`PC`).
    pub power_watts: u8,
    /// Fetched once at startup, not polled per-tick — `ID` is a fixed
    /// protocol constant (`"0670"`).
    pub id: String,
    /// Errors from the most recent poll cycle.
    pub poll_errors: Vec<String>,
    /// `false` when the radio has been unresponsive for 3 consecutive poll
    /// cycles.
    pub connected: bool,
    /// `true` from startup until the first poll cycle completes.
    pub initializing: bool,
}

impl Default for Ft991aDisplay {
    fn default() -> Self {
        Self {
            vfo_a_hz: 14_000_000,
            vfo_b_hz: 14_100_000,
            // `Mode` has no `Default` impl in `radio_trait.rs` — `Usb` is
            // picked explicitly here, matching ts570d's own default-mode
            // choice and the FT-991A's most common general-coverage mode.
            mode: Mode::Usb,
            tx_state: TxState::Off,
            smeter: 0,
            power_on: false,
            af_gain: 200,
            rf_gain: 255,
            squelch: 0,
            power_watts: 100,
            id: String::new(),
            poll_errors: Vec::new(),
            connected: true,
            initializing: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use radio::{Radio, RadioError, RadioResult};

    /// Minimal mock `Radio` implementation, per `CLAUDE.md`'s testing rule
    /// ("ui tests use an in-crate MockRadio impl of the Radio trait" — not
    /// the real `radio::Ft991a` client).
    struct MockRadio;

    #[async_trait(?Send)]
    impl Radio for MockRadio {
        async fn get_id(&mut self) -> RadioResult<String> {
            Ok("0670".to_string())
        }
    }

    #[test]
    fn test_ui_error_wraps_io_error() {
        let io_err = std::io::Error::other("boom");
        let ui_err: UiError = io_err.into();
        assert!(matches!(ui_err, UiError::Io(_)));
    }

    #[test]
    fn test_display_default_uses_usb_mode() {
        let d = Ft991aDisplay::default();
        assert_eq!(d.mode, Mode::Usb);
    }

    #[test]
    fn test_display_default_starts_initializing_and_connected() {
        let d = Ft991aDisplay::default();
        assert!(d.initializing);
        assert!(d.connected);
        assert!(d.poll_errors.is_empty());
    }

    // Compile-time check only: MockRadio must satisfy `Radio + 'static` for
    // `run`'s bound, without needing every method implemented (trait's
    // default bodies return `RadioError::NotImplemented`).
    #[allow(dead_code)]
    fn _mock_radio_satisfies_run_bound() {
        fn assert_bound<R: Radio + 'static>() {}
        assert_bound::<MockRadio>();
    }

    #[allow(dead_code)]
    fn _radio_error_type_is_reachable(_e: RadioError) {}
}
