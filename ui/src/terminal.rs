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

//! Terminal setup/teardown and the main event loop.
//!
//! Structurally mirrors `ts570d/ui/src/terminal.rs`'s raw-mode/alternate-
//! screen setup, panic-safe restore, and 200ms-poll/10ms-event-poll/5ms-
//! idle-sleep timing (§6.6) — that part is command-count-independent.
//!
//! **Architecture note (judgment call, see `planning/ui/task_plan.md`
//! decision 3):** ts570d splits polling and key-handling into two
//! `monoio::spawn`-ed tasks linked by channels, so key events stay
//! responsive during a slow poll cycle. This slice's poll set is much
//! smaller (10 calls vs. ts570d's ~21) and §6.6 cites ts570d's line numbers
//! only for signature/cadence, not the channel plumbing itself — so this
//! implementation uses a single sequential loop instead. Every explicitly
//! specified requirement (signature, 200ms poll cadence, 10ms/5ms event
//! timing, 3-consecutive-failed-cycles disconnect logic) is still met.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use radio::{Frequency, Radio, RadioResult};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    control::{handle_key, ControlState, ExecuteAction, KeyResult},
    layout::{
        draw_control_panel, draw_disconnected, draw_errors, draw_header, draw_status, split_areas,
    },
    Ft991aDisplay, UiError, UiResult,
};

/// How often `poll_radio_state` re-queries the radio.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// How long a single `event::poll` call blocks waiting for a key event.
const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(10);

/// How long to sleep between loop iterations after handling (or not
/// finding) a key event.
const IDLE_SLEEP: Duration = Duration::from_millis(5);

/// A poll cycle counts as "failed" once at least half of the 10 getters
/// error in that cycle. Scaled down proportionally from ts570d's
/// `FAIL_THRESHOLD = 10` (out of ~21 poll calls) for this slice's smaller
/// 10-call poll set — not given an exact number in
/// `planning/architect/task_plan.md` §6.6, so this is a judgment call.
const FAIL_THRESHOLD: usize = 5;

/// Number of consecutive failed poll cycles before `connected` flips to
/// `false`. Matches ts570d's threshold exactly (command-count-independent).
const CONSECUTIVE_FAILURE_LIMIT: u32 = 3;

/// Initialize the terminal: enable raw mode and enter the alternate screen.
fn init_terminal() -> UiResult<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its normal state.
fn cleanup_terminal() -> UiResult<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    Ok(())
}

/// Draw a single frame using the given radio state and control state.
fn draw_frame(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &Ft991aDisplay,
    control: &ControlState,
) -> UiResult<()> {
    terminal.draw(|f| {
        let area = f.size();
        let (header_area, status_area, errors_area, ctrl_area) = split_areas(area);
        draw_header(f, header_area);
        draw_status(f, status_area, state);
        draw_errors(f, errors_area, state);
        if state.initializing || !state.connected {
            draw_disconnected(f, ctrl_area, &state.poll_errors, state.initializing);
        } else {
            draw_control_panel(f, ctrl_area, control);
        }
    })?;
    Ok(())
}

/// Poll all radio state getters and update `state` in place.
///
/// `state.poll_errors` is cleared at the start of each call and
/// re-populated with any errors from this cycle. Previous values are
/// preserved when a getter fails. 10 poll calls (vs. ts570d's ~21),
/// proportional to this slice's smaller `Radio` trait surface (§6.6) — does
/// NOT include `get_id`, which is fetched once at startup, not per-tick
/// (`ID` is a fixed protocol constant per `ft991a.rs`'s doc comment).
async fn poll_radio_state<R: Radio>(radio: &mut R, state: &mut Ft991aDisplay) {
    radio.flush_rx();
    state.poll_errors.clear();

    macro_rules! poll {
        ($label:expr, $expr:expr, $ok:expr) => {
            match $expr.await {
                Ok(v) => $ok(v),
                Err(e) => {
                    state.poll_errors.push(format!("{}: {}", $label, e));
                }
            }
        };
    }

    poll!("FA", radio.get_vfo_a(), |f: Frequency| {
        state.vfo_a_hz = f.hz();
    });
    poll!("FB", radio.get_vfo_b(), |f: Frequency| {
        state.vfo_b_hz = f.hz();
    });
    poll!("MD", radio.get_mode(), |m| {
        state.mode = m;
    });
    poll!("TX", radio.get_tx_state(), |t| {
        state.tx_state = t;
    });
    poll!("SM", radio.get_smeter(), |v: u8| {
        state.smeter = v;
    });
    poll!("PS", radio.get_power_on(), |v: bool| {
        state.power_on = v;
    });
    poll!("AG", radio.get_af_gain(), |v: u8| {
        state.af_gain = v;
    });
    poll!("RG", radio.get_rf_gain(), |v: u8| {
        state.rf_gain = v;
    });
    poll!("SQ", radio.get_squelch(), |v: u8| {
        state.squelch = v;
    });
    poll!("PC", radio.get_power(), |v: u8| {
        state.power_watts = v;
    });
}

/// Execute a validated [`ExecuteAction`] against the radio, returning a
/// human-readable description and the result.
///
/// The `O` key ([`ExecuteAction::TogglePowerOn`]) calls
/// [`Radio::set_power_on`] directly, which is a faithful 1:1 `PS<0/1>;`
/// mapping (`radio/src/ft991a.rs` lines ~298-309) that deliberately does
/// **not** implement the manual's "dummy data, then wait 1-2s, then
/// `PS1;`" wake-from-standby sequence — that's an explicitly deferred
/// `wake_and_power_on()` helper, not yet on the `Radio` trait (§6.5). If
/// the radio is in deep standby, this may not wake it; that is an inherited
/// limitation from the `radio` crate, not something this UI works around.
async fn execute_action<R: Radio>(
    radio: &mut R,
    action: ExecuteAction,
) -> (&'static str, RadioResult<()>) {
    match action {
        ExecuteAction::SetVfoA(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_a(f).await,
                Err(e) => Err(e),
            };
            ("VFO A set", r)
        }
        ExecuteAction::SetVfoB(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_b(f).await,
                Err(e) => Err(e),
            };
            ("VFO B set", r)
        }
        ExecuteAction::SetMode(mode) => ("Mode set", radio.set_mode(mode).await),
        // §6.5: `T` always sends transmit()/receive() based on the *last
        // polled* TxState — CatKeyed means this session already asserted
        // PTT, so toggle it off; Off or RadioKeyedNonCat both mean this
        // session has not asserted PTT via CAT, so assert it.
        ExecuteAction::ToggleTx(last_state) => {
            if last_state == radio::TxState::CatKeyed {
                ("RX (CAT)", radio.receive().await)
            } else {
                ("TX (CAT)", radio.transmit().await)
            }
        }
        ExecuteAction::SetAfGain(v) => ("AF gain set", radio.set_af_gain(v).await),
        ExecuteAction::SetRfGain(v) => ("RF gain set", radio.set_rf_gain(v).await),
        ExecuteAction::SetSquelch(v) => ("Squelch set", radio.set_squelch(v).await),
        ExecuteAction::SetPower(v) => ("TX power set", radio.set_power(v).await),
        ExecuteAction::TogglePowerOn(currently_on) => {
            ("Power toggled", radio.set_power_on(!currently_on).await)
        }
    }
}

/// Run the terminal UI against a live [`radio::Radio`] implementation.
///
/// Single sequential loop (see module docs for why this departs from
/// ts570d's two-task/channel architecture): every iteration, poll the
/// radio if `POLL_INTERVAL` has elapsed, redraw, then wait up to
/// `EVENT_POLL_TIMEOUT` for a key event and handle it inline.
pub async fn run<R: Radio + 'static>(mut radio: R) -> UiResult<()> {
    let mut terminal = init_terminal()?;
    let result = run_loop(&mut terminal, &mut radio).await;
    cleanup_terminal()?;
    result
}

async fn run_loop<R: Radio>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    radio: &mut R,
) -> UiResult<()> {
    let mut display = Ft991aDisplay::default();
    let mut control = ControlState::default();
    let mut fail_cycles: u32 = 0;

    // Draw initial "connecting" frame before the first poll.
    draw_frame(terminal, &display, &control)?;

    // One-time ID fetch (§6.6) — ID is a fixed protocol constant, not
    // polled per-tick.
    match radio.get_id().await {
        Ok(id) => display.id = id,
        Err(e) => display.poll_errors.push(format!("ID: {}", e)),
    }

    // Force an immediate first poll cycle.
    let mut last_poll = Instant::now()
        .checked_sub(POLL_INTERVAL)
        .unwrap_or_else(Instant::now);

    loop {
        if last_poll.elapsed() >= POLL_INTERVAL {
            poll_radio_state(radio, &mut display).await;

            if display.poll_errors.len() >= FAIL_THRESHOLD {
                fail_cycles = fail_cycles.saturating_add(1);
            } else {
                fail_cycles = 0;
            }
            display.connected = fail_cycles < CONSECUTIVE_FAILURE_LIMIT;
            display.initializing = false;

            last_poll = Instant::now();
        }

        draw_frame(terminal, &display, &control)?;

        if event::poll(EVENT_POLL_TIMEOUT).map_err(UiError::Io)? {
            if let Event::Key(key) = event::read().map_err(UiError::Io)? {
                match handle_key(key, &mut control, &display) {
                    KeyResult::Quit => break,
                    KeyResult::Continue => {}
                    KeyResult::Execute(action) => {
                        let (desc, result) = execute_action(radio, action).await;
                        control = match result {
                            Ok(()) => ControlState::Feedback {
                                message: format!("OK: {}", desc),
                                is_error: false,
                            },
                            Err(e) => ControlState::Feedback {
                                message: format!("Error: {}", e),
                                is_error: true,
                            },
                        };
                    }
                }
            }
        }

        // `monoio::time::sleep` doesn't exist on Windows (`monoio` cannot
        // compile there at all — io_uring is Linux-only). A plain blocking
        // `std::thread::sleep` is behaviorally identical here: per ADR
        // 0004 §1, `ft991a` is a confirmed single-sequential-loop
        // architecture with no concurrent task (unlike `ts570d`'s two-task
        // design) that a blocking sleep could ever starve.
        #[cfg(target_os = "linux")]
        monoio::time::sleep(IDLE_SLEEP).await;
        #[cfg(target_os = "windows")]
        std::thread::sleep(IDLE_SLEEP);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Gated `target_os = "linux"` in addition to `test`: every async test below
// uses `#[monoio::test(driver = "legacy")]`, and `monoio` is a Linux-only
// *target-gated* dependency (`[target.'cfg(target_os = "linux")'.
// dependencies]` in `ui/Cargo.toml`, mirroring `cat-transport-serial`'s own
// gating in `radio-cat-rs`, per ADR 0004). Without this, `cargo check
// --target x86_64-pc-windows-gnu` for a `--tests`/`--all-targets` build
// would fail to resolve the `monoio::test` attribute macro even though the
// non-test module above is otherwise portable. This mirrors the same fix
// `radio-cat-rs`'s `cat-transport-serial/src/session.rs` applied to its own
// `#[cfg(all(test, target_os = "linux"))]`-gated test module for the
// identical reason.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use radio::{Mode, RadioError, TxState};

    /// In-crate mock `Radio` for pure-logic tests of the polling/execute
    /// helpers, per `CLAUDE.md`'s testing rule — never the real
    /// `radio::Ft991a` client, no live terminal or serial port needed.
    struct MockRadio {
        vfo_a: RadioResult<Frequency>,
        fail_all: bool,
    }

    impl MockRadio {
        fn ok() -> Self {
            Self {
                vfo_a: Frequency::new(14_250_000),
                fail_all: false,
            }
        }

        fn failing() -> Self {
            Self {
                vfo_a: Err(radio::RadioError::NotImplemented),
                fail_all: true,
            }
        }
    }

    #[async_trait(?Send)]
    impl Radio for MockRadio {
        async fn get_vfo_a(&mut self) -> RadioResult<Frequency> {
            self.vfo_a
                .as_ref()
                .copied()
                .map_err(|_| RadioError::NotImplemented)
        }
        async fn get_vfo_b(&mut self) -> RadioResult<Frequency> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Frequency::new(14_100_000)
            }
        }
        async fn get_mode(&mut self) -> RadioResult<Mode> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(Mode::Usb)
            }
        }
        async fn get_tx_state(&mut self) -> RadioResult<TxState> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(TxState::Off)
            }
        }
        async fn get_smeter(&mut self) -> RadioResult<u8> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(42)
            }
        }
        async fn get_power_on(&mut self) -> RadioResult<bool> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(true)
            }
        }
        async fn get_af_gain(&mut self) -> RadioResult<u8> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(200)
            }
        }
        async fn get_rf_gain(&mut self) -> RadioResult<u8> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(255)
            }
        }
        async fn get_squelch(&mut self) -> RadioResult<u8> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(10)
            }
        }
        async fn get_power(&mut self) -> RadioResult<u8> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(50)
            }
        }
        async fn set_vfo_a(&mut self, freq: Frequency) -> RadioResult<()> {
            self.vfo_a = Ok(freq);
            Ok(())
        }
        async fn transmit(&mut self) -> RadioResult<()> {
            Ok(())
        }
        async fn receive(&mut self) -> RadioResult<()> {
            Ok(())
        }
        async fn set_power_on(&mut self, _on: bool) -> RadioResult<()> {
            Ok(())
        }
    }

    #[monoio::test(driver = "legacy")]
    async fn test_poll_radio_state_success_populates_all_fields() {
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        poll_radio_state(&mut radio, &mut display).await;
        assert!(display.poll_errors.is_empty());
        assert_eq!(display.vfo_a_hz, 14_250_000);
        assert_eq!(display.vfo_b_hz, 14_100_000);
        assert_eq!(display.mode, Mode::Usb);
        assert_eq!(display.tx_state, TxState::Off);
        assert_eq!(display.smeter, 42);
        assert!(display.power_on);
        assert_eq!(display.af_gain, 200);
        assert_eq!(display.rf_gain, 255);
        assert_eq!(display.squelch, 10);
        assert_eq!(display.power_watts, 50);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_poll_radio_state_records_ten_errors_when_all_fail() {
        let mut radio = MockRadio::failing();
        let mut display = Ft991aDisplay::default();
        poll_radio_state(&mut radio, &mut display).await;
        assert_eq!(display.poll_errors.len(), 10);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_set_vfo_a() {
        let mut radio = MockRadio::ok();
        let (desc, result) = execute_action(&mut radio, ExecuteAction::SetVfoA(14_300_000)).await;
        assert_eq!(desc, "VFO A set");
        assert!(result.is_ok());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_set_vfo_a_rejects_out_of_range() {
        let mut radio = MockRadio::ok();
        let (_desc, result) =
            execute_action(&mut radio, ExecuteAction::SetVfoA(Frequency::MAX_HZ + 1)).await;
        assert!(result.is_err());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_tx_from_cat_keyed_sends_receive() {
        let mut radio = MockRadio::ok();
        let (desc, result) =
            execute_action(&mut radio, ExecuteAction::ToggleTx(TxState::CatKeyed)).await;
        assert_eq!(desc, "RX (CAT)");
        assert!(result.is_ok());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_tx_from_off_sends_transmit() {
        let mut radio = MockRadio::ok();
        let (desc, result) =
            execute_action(&mut radio, ExecuteAction::ToggleTx(TxState::Off)).await;
        assert_eq!(desc, "TX (CAT)");
        assert!(result.is_ok());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_tx_from_radio_keyed_non_cat_sends_transmit() {
        // §6.5: pressing T while RadioKeyedNonCat still sends transmit()
        // (asserting CAT PTT alongside the existing non-CAT key source) —
        // it does not attempt to clear the non-CAT source, since the
        // manual documents no command that would.
        let mut radio = MockRadio::ok();
        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ToggleTx(TxState::RadioKeyedNonCat),
        )
        .await;
        assert_eq!(desc, "TX (CAT)");
        assert!(result.is_ok());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_power_on_flips_state() {
        let mut radio = MockRadio::ok();
        let (desc, result) = execute_action(&mut radio, ExecuteAction::TogglePowerOn(true)).await;
        assert_eq!(desc, "Power toggled");
        assert!(result.is_ok());
    }
}
