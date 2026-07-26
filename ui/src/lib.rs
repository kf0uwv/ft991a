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

//! FT-991A Terminal UI.
//!
//! Wave 2 shipped a flat single-screen TUI, right-sized for this repo's
//! 11-command first slice. Wave 4 Task 2 (`planning/architect/task_plan.md`
//! §11.2/§11.6 item 2) reintroduces `ts570d/ui`'s `Menu` -> `GroupMenu`
//! grouped structure "in spirit," now that the command surface has grown to
//! 91 CAT commands + 151 `EX` items and no longer fits comfortably on one
//! flat screen — see `control.rs`'s module docs for the 12-group layout.
//!
//! Per this repo's `CLAUDE.md` dependency model, `ui` depends on `radio`
//! only (for the [`radio::Radio`] trait and domain types) and never imports
//! a transport crate directly.
//!
//! **Scope narrowing, disclosed (§11.3 point 5):** [`run`]'s generic bound
//! widened in Wave 4 Task 2 from `R: Radio + 'static` to `R: Radio +
//! Ft991aExtras + CwKeying + 'static`. `ui` is therefore no longer usable
//! against "any [`radio::Radio`] implementation" in the abstract — it is
//! (and, per this repo's history, always effectively has been, since no
//! second radio type has ever been wired to it) contractually an FT-991A-
//! shaped UI, requiring the FT-991A-specific [`radio::Ft991aExtras`] and
//! [`radio::CwKeying`] traits too. This costs nothing against this repo's
//! only concrete wiring (`Ft991a<SerialCatSession<SerialPort>>` in
//! `src/main.rs`), which satisfies all three bounds unconditionally today.

pub(crate) mod control;
pub(crate) mod diagnostics;
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
    /// Real-time RTS CW-keying line state, group 5's `K` key (§11.3 point
    /// 6). Tracked **locally**, not polled — there is no "read back what I
    /// asserted" ioctl (`CwKeying::read_cts` reads the *status* line CTS,
    /// not RTS's own asserted state). Set optimistically by `terminal.rs`'s
    /// `execute_action` when `K` is pressed, then rolled back to its prior
    /// value if `CwKeying::assert_rts` returns an `Err`.
    pub rts_asserted: bool,
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
            rts_asserted: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use radio::{CwKeying, Ft991aExtras, Radio, RadioError, RadioResult};

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

    // Wave 4 Task 2 (§11.3 point 3's last bullet): `ui::run`'s bound widened
    // to require `Ft991aExtras`/`CwKeying` too — this in-crate `MockRadio`
    // needs both impls (inheriting their `NotImplemented` default bodies) to
    // keep satisfying the new bound, exactly like `radio::NopRadio` does.
    #[async_trait(?Send)]
    impl Ft991aExtras for MockRadio {}
    impl CwKeying for MockRadio {}

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

    #[test]
    fn test_display_default_rts_not_asserted() {
        let d = Ft991aDisplay::default();
        assert!(!d.rts_asserted);
    }

    // Compile-time check only: MockRadio must satisfy the widened
    // `Radio + Ft991aExtras + CwKeying + 'static` bound `run` now requires
    // (§11.3 point 3), without needing every method implemented (all three
    // traits' default bodies return `RadioError::NotImplemented`).
    #[allow(dead_code)]
    fn _mock_radio_satisfies_run_bound() {
        fn assert_bound<R: Radio + Ft991aExtras + CwKeying + 'static>() {}
        assert_bound::<MockRadio>();
    }

    // Runtime check: exercise `handle_key`'s state machine (the part of
    // `run`'s event loop this crate can test without a live terminal) using
    // a value of the widened-bound type, to confirm nothing about the bound
    // widening broke `ControlState` transitions.
    #[test]
    fn test_state_machine_runs_against_widened_bound_type() {
        fn assert_bound<R: Radio + Ft991aExtras + CwKeying + 'static>(_r: &R) {}
        let radio = MockRadio;
        assert_bound(&radio);

        let mut state = crate::control::ControlState::default();
        let display = Ft991aDisplay::default();
        let result = crate::control::handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut state,
            &display,
        );
        assert!(matches!(result, crate::control::KeyResult::Quit));
    }

    #[allow(dead_code)]
    fn _radio_error_type_is_reachable(_e: RadioError) {}
}
