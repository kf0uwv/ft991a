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
//! **Wave 4 Task 4 (§11.3 point 6):** `execute_action`'s bound widens to
//! `Radio + Ft991aExtras + CwKeying` and it gains a `display: &mut
//! Ft991aDisplay` parameter, needed by group 5's real-time RTS CW-keying
//! toggle — the one action that mutates the live display state directly
//! (optimistic set, rolled back on `CwKeying::assert_rts` error) rather
//! than waiting for the next poll cycle. See `execute_action`'s own doc
//! comment for the full rationale.
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
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use radio::{
    ctcss_tone_hz, dcs_code_number, AgcMode, Band, CwKeying, EncoderSelector, Frequency,
    Ft991aExtras, KeyerPlaybackMode, MemoryChannelEntry, MemoryTag, Meter, Mode, PreampMode, Radio,
    RadioError, RadioIndicator, RadioResult, RepeaterShift, ScanState, TaggedMemoryChannel,
    ToneSquelchMode,
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    control::{handle_key, ControlState, ExecuteAction, KeyResult},
    diagnostics::{DiagOutcome, DiagResult, DiagSummary},
    layout::{
        draw_control_panel, draw_diagnostics_live, draw_disconnected, draw_errors, draw_header,
        draw_status, split_areas,
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
/// Returns `(&'static str, RadioResult<String>)` where the `String` is
/// extra feedback text — non-empty for the read-type actions added by Wave
/// 4 Task 3 ([`ExecuteAction::GetMemoryChannel`],
/// [`ExecuteAction::ReadMemoryChannel`],
/// [`ExecuteAction::ReadMemoryChannelTag`]), empty for every plain "set"
/// action (mirroring `ts570d::ui::terminal::execute_action`'s own return
/// shape and its `run_loop`'s "use the extra text if present, else `OK:
/// {desc}`" convention, adopted verbatim below).
///
/// The `O` key ([`ExecuteAction::TogglePowerOn`]) calls
/// [`Radio::set_power_on`] directly, which is a faithful 1:1 `PS<0/1>;`
/// mapping (`radio/src/ft991a.rs` lines ~298-309) that deliberately does
/// **not** implement the manual's "dummy data, then wait 1-2s, then
/// `PS1;`" wake-from-standby sequence — that's an explicitly deferred
/// `wake_and_power_on()` helper, not yet on the `Radio` trait (§6.5). If
/// the radio is in deep standby, this may not wake it; that is an inherited
/// limitation from the `radio` crate, not something this UI works around.
///
/// **`display: &mut Ft991aDisplay` (Wave 4 Task 4, §11.3 point 6):** needed
/// solely for [`ExecuteAction::ToggleRts`] — `rts_asserted` is tracked
/// **locally**, never polled (see the field's own doc comment in
/// `lib.rs`), so this is the one action whose executor must mutate the
/// live display state directly rather than just reading it: it sets
/// `rts_asserted` to the new value *optimistically*, before the
/// [`radio::CwKeying::assert_rts`] call, then rolls it back to the prior
/// value if that call returns an `Err`. Every other action leaves
/// `display` untouched (its fields are refreshed by the next
/// [`poll_radio_state`] cycle instead).
async fn execute_action<R: Radio + Ft991aExtras + CwKeying>(
    radio: &mut R,
    action: ExecuteAction,
    display: &mut Ft991aDisplay,
) -> (&'static str, RadioResult<String>) {
    /// Convert a unit result to a `String` result with no extra feedback
    /// text — see this function's own doc comment.
    fn ok_unit(r: RadioResult<()>) -> RadioResult<String> {
        r.map(|()| String::new())
    }

    match action {
        ExecuteAction::SetVfoA(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_a(f).await,
                Err(e) => Err(e),
            };
            ("VFO A set", ok_unit(r))
        }
        ExecuteAction::SetVfoB(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_b(f).await,
                Err(e) => Err(e),
            };
            ("VFO B set", ok_unit(r))
        }
        ExecuteAction::SetMode(mode) => ("Mode set", ok_unit(radio.set_mode(mode).await)),
        // §6.5: `T` always sends transmit()/receive() based on the *last
        // polled* TxState — CatKeyed means this session already asserted
        // PTT, so toggle it off; Off or RadioKeyedNonCat both mean this
        // session has not asserted PTT via CAT, so assert it.
        ExecuteAction::ToggleTx(last_state) => {
            if last_state == radio::TxState::CatKeyed {
                ("RX (CAT)", ok_unit(radio.receive().await))
            } else {
                ("TX (CAT)", ok_unit(radio.transmit().await))
            }
        }
        ExecuteAction::SetAfGain(v) => ("AF gain set", ok_unit(radio.set_af_gain(v).await)),
        ExecuteAction::SetRfGain(v) => ("RF gain set", ok_unit(radio.set_rf_gain(v).await)),
        ExecuteAction::SetSquelch(v) => ("Squelch set", ok_unit(radio.set_squelch(v).await)),
        ExecuteAction::SetPower(v) => ("TX power set", ok_unit(radio.set_power(v).await)),
        ExecuteAction::TogglePowerOn(currently_on) => (
            "Power toggled",
            ok_unit(radio.set_power_on(!currently_on).await),
        ),

        // --- Group 3 (MemoryChannels) ---
        ExecuteAction::SelectMemoryChannel(ch) => (
            "Memory channel selected",
            ok_unit(radio.set_memory_channel(ch).await),
        ),
        ExecuteAction::GetMemoryChannel => match radio.get_memory_channel().await {
            Ok(ch) => (
                "Memory channel",
                Ok(format!("Selected memory channel: {ch}")),
            ),
            Err(e) => ("Get memory channel", Err(e)),
        },
        ExecuteAction::ReadMemoryChannel(ch) => match radio.read_memory_channel(ch).await {
            Ok(entry) => (
                "Memory channel read",
                Ok(format!(
                    "CH{:03}: {} Hz {}",
                    entry.channel,
                    entry.frequency_hz,
                    entry.mode.name()
                )),
            ),
            Err(e) => ("Read memory channel", Err(e)),
        },
        ExecuteAction::WriteMemoryChannelFromVfoA(ch) => {
            // Query VFO A/mode fresh at execution time rather than trusting
            // a `Ft991aDisplay` snapshot captured when the key was pressed
            // — mirrors `ts570d::ui`'s own `WriteMemoryChannelFromVfoA`.
            match (radio.get_vfo_a().await, radio.get_mode().await) {
                (Ok(freq), Ok(mode)) => {
                    let entry = MemoryChannelEntry {
                        channel: ch,
                        frequency_hz: freq.hz(),
                        clarifier_offset_hz: 0,
                        rx_clarifier_on: false,
                        tx_clarifier_on: false,
                        mode,
                        tone_status: 0,
                        offset_type: 0,
                    };
                    (
                        "Memory channel written from VFO A",
                        ok_unit(radio.write_memory_channel(entry).await),
                    )
                }
                (Err(e), _) | (_, Err(e)) => ("Memory channel written from VFO A", Err(e)),
            }
        }
        ExecuteAction::ReadMemoryChannelTag(ch) => match radio.read_memory_channel_tag(ch).await {
            Ok(tagged) => (
                "Memory channel + tag read",
                Ok(format!(
                    "CH{:03}: {} Hz {} \"{}\"",
                    tagged.entry.channel,
                    tagged.entry.frequency_hz,
                    tagged.entry.mode.name(),
                    tagged.tag.as_str()
                )),
            ),
            Err(e) => ("Read memory channel + tag", Err(e)),
        },
        ExecuteAction::WriteMemoryChannelTagFromVfoA(ch, tag_str) => {
            match (radio.get_vfo_a().await, radio.get_mode().await) {
                (Ok(freq), Ok(mode)) => {
                    let entry = MemoryChannelEntry {
                        channel: ch,
                        frequency_hz: freq.hz(),
                        clarifier_offset_hz: 0,
                        rx_clarifier_on: false,
                        tx_clarifier_on: false,
                        mode,
                        tone_status: 0,
                        offset_type: 0,
                    };
                    match MemoryTag::new(&tag_str) {
                        Ok(tag) => (
                            "Memory channel + tag written from VFO A",
                            ok_unit(
                                radio
                                    .write_memory_channel_tag(TaggedMemoryChannel { entry, tag })
                                    .await,
                            ),
                        ),
                        Err(e) => ("Memory channel + tag written from VFO A", Err(e)),
                    }
                }
                (Err(e), _) | (_, Err(e)) => ("Memory channel + tag written from VFO A", Err(e)),
            }
        }

        // --- Group 4 (ClarifierToneIfShift) ---
        ExecuteAction::SetRxClarifierOn(on) => (
            "RX clarifier set",
            ok_unit(radio.set_rx_clarifier_on(on).await),
        ),
        ExecuteAction::SetTxClarifierOn(on) => (
            "TX clarifier set",
            ok_unit(radio.set_tx_clarifier_on(on).await),
        ),
        ExecuteAction::ClarifierClear => {
            ("Clarifier cleared", ok_unit(radio.clarifier_clear().await))
        }
        ExecuteAction::ClarifierDown(hz) => (
            "Clarifier down set",
            ok_unit(radio.clarifier_down(hz).await),
        ),
        ExecuteAction::ClarifierUp(hz) => {
            ("Clarifier up set", ok_unit(radio.clarifier_up(hz).await))
        }
        ExecuteAction::SetIfShift(hz) => ("IF shift set", ok_unit(radio.set_if_shift_hz(hz).await)),
        ExecuteAction::SetToneSquelchMode(mode) => (
            "Tone squelch mode set",
            ok_unit(radio.set_tone_squelch_mode(mode).await),
        ),
        ExecuteAction::SetCtcssTone(hz) => {
            ("CTCSS tone set", ok_unit(radio.set_ctcss_tone_hz(hz).await))
        }
        ExecuteAction::SetDcsCode(code) => {
            ("DCS code set", ok_unit(radio.set_dcs_code(code).await))
        }

        // --- Group 6 (ScanVoxBusy) ---
        ExecuteAction::SetScanState(state) => {
            ("Scan state set", ok_unit(radio.set_scan_state(state).await))
        }
        ExecuteAction::SetVoxOn(on) => ("VOX set", ok_unit(radio.set_vox_on(on).await)),
        ExecuteAction::SetVoxGain(v) => ("VOX gain set", ok_unit(radio.set_vox_gain(v).await)),
        ExecuteAction::SetVoxDelay(ms) => ("VOX delay set", ok_unit(radio.set_vox_delay(ms).await)),

        // --- Group 5 (KeyerCwBreakIn) ---
        //
        // `ToggleRts` is sync (`CwKeying::assert_rts` is a plain `&self`
        // fn, no `.await` — §10.3/§11.3), and is the only arm in this match
        // that mutates `display` — see this function's own doc comment.
        ExecuteAction::ToggleRts(prev_asserted) => {
            let new_asserted = !prev_asserted;
            display.rts_asserted = new_asserted; // optimistic set
            match radio.assert_rts(new_asserted) {
                Ok(()) => ("RTS CW key toggled", Ok(String::new())),
                Err(e) => {
                    display.rts_asserted = prev_asserted; // roll back
                    ("RTS CW key toggled", Err(e))
                }
            }
        }
        ExecuteAction::SetBreakInOn(on) => {
            ("Break-in set", ok_unit(radio.set_break_in_on(on).await))
        }
        ExecuteAction::SetSemiBreakInDelay(ms) => (
            "Semi break-in delay set",
            ok_unit(radio.set_semi_break_in_delay(ms).await),
        ),
        ExecuteAction::SetCwSpotOn(on) => ("CW spot set", ok_unit(radio.set_cw_spot_on(on).await)),
        ExecuteAction::SetKeyerEnabled(on) => (
            "Electronic keyer set",
            ok_unit(radio.set_keyer_enabled(on).await),
        ),
        ExecuteAction::SetKeyerSpeed(wpm) => {
            ("Keyer speed set", ok_unit(radio.set_keyer_speed(wpm).await))
        }
        ExecuteAction::SetKeyerPitchHz(hz) => (
            "Keyer pitch set",
            ok_unit(radio.set_keyer_pitch_hz(hz).await),
        ),
        ExecuteAction::ZeroIn => ("CW zero-in triggered", ok_unit(radio.zero_in().await)),
        ExecuteAction::ReadKeyerMemory(ch) => match radio.read_keyer_memory(ch).await {
            Ok(msg) => ("Keyer memory read", Ok(format!("CH{ch}: \"{msg}\""))),
            Err(e) => ("Read keyer memory", Err(e)),
        },
        ExecuteAction::WriteKeyerMemory(ch, msg) => (
            "Keyer memory written",
            ok_unit(radio.write_keyer_memory(ch, &msg).await),
        ),
        ExecuteAction::PlayKeyerMemory(ch) => (
            "Keyer memory playback triggered",
            ok_unit(
                radio
                    .play_keyer_memory(ch, radio::KeyerPlaybackMode::KeyerMemory)
                    .await,
            ),
        ),
        ExecuteAction::PlayMessageKeyer(ch) => (
            "Message keyer playback triggered",
            ok_unit(
                radio
                    .play_keyer_memory(ch, radio::KeyerPlaybackMode::MessageKeyer)
                    .await,
            ),
        ),

        // --- Group 2 (VfoMemoryQuickOps) — all 11 are zero-argument
        // write-only wire triggers, same `ok_unit` shape as ClarifierClear/
        // ZeroIn above. ---
        ExecuteAction::CopyVfoAToB => ("VFO A copied to B", ok_unit(radio.copy_vfo_a_to_b().await)),
        ExecuteAction::CopyVfoBToA => ("VFO B copied to A", ok_unit(radio.copy_vfo_b_to_a().await)),
        ExecuteAction::SwapVfos => ("VFOs swapped", ok_unit(radio.swap_vfos().await)),
        ExecuteAction::StoreVfoToMemory => (
            "VFO A stored to memory",
            ok_unit(radio.store_vfo_to_memory().await),
        ),
        ExecuteAction::RecallMemoryToVfo => (
            "Memory recalled to VFO A",
            ok_unit(radio.recall_memory_to_vfo().await),
        ),
        ExecuteAction::MemoryChannelUp => (
            "Memory channel stepped up",
            ok_unit(radio.memory_channel_up().await),
        ),
        ExecuteAction::MemoryChannelDown => (
            "Memory channel stepped down",
            ok_unit(radio.memory_channel_down().await),
        ),
        ExecuteAction::ToggleVfoMemoryMode => (
            "VFO/Memory mode toggled",
            ok_unit(radio.toggle_vfo_memory_mode().await),
        ),
        ExecuteAction::QmbStore => ("QMB stored", ok_unit(radio.qmb_store().await)),
        ExecuteAction::QmbRecall => ("QMB recalled", ok_unit(radio.qmb_recall().await)),
        ExecuteAction::QuickSplit => ("Quick split toggled", ok_unit(radio.quick_split().await)),

        // --- Group 9 (BandStepEncoder) ---
        ExecuteAction::SetBand(band) => ("Band selected", ok_unit(radio.set_band(band).await)),
        ExecuteAction::BandUp => ("Band stepped up", ok_unit(radio.band_up().await)),
        ExecuteAction::BandDown => ("Band stepped down", ok_unit(radio.band_down().await)),
        ExecuteAction::SetFineStep(on) => ("Fine step set", ok_unit(radio.set_fine_step(on).await)),
        ExecuteAction::MicUp => ("Mic UP pressed", ok_unit(radio.mic_up().await)),
        ExecuteAction::MicDown => ("Mic DOWN pressed", ok_unit(radio.mic_down().await)),
        // `ED`/`EU`/`EK` — real wire triggers, honestly disclosed as
        // context-dependent/no-persisted-state in
        // `control.rs::band_step_encoder_commands`'s doc comment.
        ExecuteAction::EncoderDown(encoder, steps) => (
            "Encoder nudged down",
            ok_unit(radio.encoder_down(encoder, steps).await),
        ),
        ExecuteAction::EncoderUp(encoder, steps) => (
            "Encoder nudged up",
            ok_unit(radio.encoder_up(encoder, steps).await),
        ),
        ExecuteAction::EntKey => ("ENT key pressed", ok_unit(radio.ent_key().await)),

        // --- Group 7 (AttenuatorNoiseAgcNotchFilter) — plain 1:1
        // `Radio`/`Ft991aExtras` passthroughs, same shape as every other
        // arm in this match; no executor-side branching logic, so (per
        // Wave 4 Task 3/5's established division of test coverage) no new
        // `terminal.rs` tests are needed for these — see
        // `control.rs::attenuator_noise_agc_notch_filter_commands`'s doc
        // comment for the full trait-mix citation. ---
        ExecuteAction::SetAttenuatorOn(on) => {
            ("Attenuator set", ok_unit(radio.set_attenuator_on(on).await))
        }
        ExecuteAction::SetPreampMode(mode) => (
            "Pre-amp mode set",
            ok_unit(radio.set_preamp_mode(mode).await),
        ),
        ExecuteAction::SetNoiseBlankerOn(on) => (
            "Noise blanker set",
            ok_unit(radio.set_noise_blanker_on(on).await),
        ),
        ExecuteAction::SetNoiseBlankerLevel(level) => (
            "Noise blanker level set",
            ok_unit(radio.set_noise_blanker_level(level).await),
        ),
        ExecuteAction::SetNoiseReductionOn(on) => (
            "Noise reduction set",
            ok_unit(radio.set_noise_reduction_on(on).await),
        ),
        ExecuteAction::SetNoiseReductionLevel(level) => (
            "Noise reduction level set",
            ok_unit(radio.set_noise_reduction_level(level).await),
        ),
        ExecuteAction::SetAgcMode(mode) => {
            ("AGC mode set", ok_unit(radio.set_agc_mode(mode).await))
        }
        ExecuteAction::SetAutoNotchOn(on) => {
            ("Auto notch set", ok_unit(radio.set_auto_notch_on(on).await))
        }
        ExecuteAction::SetNarrowOn(on) => {
            ("Narrow filter set", ok_unit(radio.set_narrow_on(on).await))
        }
        ExecuteAction::SetFilterWidthIndex(index) => (
            "Filter width set",
            ok_unit(radio.set_filter_width_index(index).await),
        ),
        ExecuteAction::SetContourOn(on) => ("Contour set", ok_unit(radio.set_contour_on(on).await)),
        ExecuteAction::SetContourFrequencyHz(hz) => (
            "Contour frequency set",
            ok_unit(radio.set_contour_frequency_hz(hz).await),
        ),
        ExecuteAction::SetApfOn(on) => ("APF set", ok_unit(radio.set_apf_on(on).await)),
        ExecuteAction::SetApfFrequencyHz(hz) => (
            "APF frequency set",
            ok_unit(radio.set_apf_frequency_hz(hz).await),
        ),
        ExecuteAction::SetManualNotchOn(on) => (
            "Manual notch set",
            ok_unit(radio.set_manual_notch_on(on).await),
        ),
        ExecuteAction::SetManualNotchFrequencyHz(hz) => (
            "Manual notch frequency set",
            ok_unit(radio.set_manual_notch_frequency_hz(hz).await),
        ),

        // --- Group 8 (SpeechMicMonitor) — same plain-passthrough shape. ---
        ExecuteAction::SetMicGain(level) => {
            ("Mic gain set", ok_unit(radio.set_mic_gain(level).await))
        }
        ExecuteAction::SetSpeechProcessorLevel(level) => (
            "Speech processor level set",
            ok_unit(radio.set_speech_processor_level(level).await),
        ),
        ExecuteAction::SetSpeechProcessorOn(on) => (
            "Speech processor set",
            ok_unit(radio.set_speech_processor_on(on).await),
        ),
        ExecuteAction::SetMonitorOn(on) => ("Monitor set", ok_unit(radio.set_monitor_on(on).await)),
        ExecuteAction::SetMonitorLevel(level) => (
            "Monitor level set",
            ok_unit(radio.set_monitor_level(level).await),
        ),
        ExecuteAction::SetParametricMicEqOn(on) => (
            "Parametric mic EQ set",
            ok_unit(radio.set_parametric_mic_eq_on(on).await),
        ),

        // --- Group 10 (MetersStatus) — plain 1:1 `Radio`/`Ft991aExtras`
        // passthroughs, same shape as every other arm in this match; no
        // executor-side branching logic, so (per Wave 4 Task 3/5/6's
        // established division of test coverage) no new `terminal.rs`
        // tests are needed for these — see
        // `control.rs::meters_status_commands`'s doc comment for the full
        // trait-mix citation. ---
        ExecuteAction::SelectMeter(meter) => {
            ("Meter selected", ok_unit(radio.select_meter(meter).await))
        }
        ExecuteAction::GetSelectedMeter => match radio.get_selected_meter().await {
            Ok(m) => (
                "Selected meter read",
                Ok(format!("Selected meter: {}", m.name())),
            ),
            Err(e) => ("Get selected meter", Err(e)),
        },
        ExecuteAction::ReadMeterDirect(meter) => match radio.get_meter(meter).await {
            Ok(v) => ("Meter read", Ok(format!("{}: {}", meter.name(), v))),
            Err(e) => ("Read meter", Err(e)),
        },
        ExecuteAction::GetActiveMeterReading => match radio.get_active_meter_reading().await {
            Ok(v) => ("Active meter read", Ok(format!("Active meter: {v}"))),
            Err(e) => ("Read active meter", Err(e)),
        },
        ExecuteAction::GetInformation => {
            match radio.get_information().await {
                Ok(info) => (
                    "Status (IF) read",
                    Ok(format!(
                    "CH{:03} {} Hz mode {:X}h clar {}{} Hz RXclr:{} TXclr:{} sel:{} tone:{} off:{}",
                    info.channel,
                    info.frequency_hz,
                    info.mode,
                    if info.clarifier_offset_hz < 0 { "-" } else { "+" },
                    info.clarifier_offset_hz.unsigned_abs(),
                    info.rx_clarifier_on,
                    info.tx_clarifier_on,
                    info.select,
                    info.tone_status,
                    info.offset_type
                )),
                ),
                Err(e) => ("Read status (IF)", Err(e)),
            }
        }
        ExecuteAction::GetRadioIndicator(indicator) => {
            match radio.get_radio_indicator(indicator).await {
                Ok(b) => (
                    "Radio indicator read",
                    Ok(format!(
                        "{}: {}",
                        indicator.name(),
                        if b { "ON" } else { "OFF" }
                    )),
                ),
                Err(e) => ("Read radio indicator", Err(e)),
            }
        }
        ExecuteAction::GetMenuModeActive => match radio.get_menu_mode_active().await {
            Ok(b) => (
                "Menu mode status read",
                Ok(format!(
                    "Menu mode: {}",
                    if b { "ACTIVE" } else { "NORMAL" }
                )),
            ),
            Err(e) => ("Read menu mode status", Err(e)),
        },
        ExecuteAction::GetPllUnlocked => match radio.get_pll_unlocked().await {
            Ok(b) => (
                "PLL status read",
                Ok(format!("PLL: {}", if b { "UNLOCKED" } else { "LOCKED" })),
            ),
            Err(e) => ("Read PLL status", Err(e)),
        },

        // --- Group 11 (SystemTunerDvs) — same plain-passthrough shape. ---
        ExecuteAction::SetAutoInfoOn(on) => {
            ("Auto-info set", ok_unit(radio.set_auto_info_on(on).await))
        }
        ExecuteAction::SetFrequencyLock(on) => (
            "Frequency lock set",
            ok_unit(radio.set_frequency_lock(on).await),
        ),
        ExecuteAction::SetRepeaterShift(shift) => (
            "Repeater shift set",
            ok_unit(radio.set_repeater_shift(shift).await),
        ),
        ExecuteAction::SetTxVfo(vfo) => ("TX VFO set", ok_unit(radio.set_tx_vfo(vfo).await)),
        ExecuteAction::SetMoxOn(on) => ("MOX set", ok_unit(radio.set_mox_on(on).await)),
        ExecuteAction::SetAntennaTunerState(state) => (
            "Antenna tuner state set",
            ok_unit(radio.set_antenna_tuner_state(state).await),
        ),
        ExecuteAction::SetDimmer(led, tft) => {
            ("Dimmer set", ok_unit(radio.set_dimmer(led, tft).await))
        }
        ExecuteAction::GetDimmer => match radio.get_dimmer().await {
            Ok((led, tft)) => ("Dimmer read", Ok(format!("LED {led} TFT {tft}"))),
            Err(e) => ("Read dimmer", Err(e)),
        },
        ExecuteAction::SetDate(year, month, day) => (
            "Date set",
            ok_unit(radio.write_date(year, month, day).await),
        ),
        ExecuteAction::ReadDate => match radio.read_date().await {
            Ok((y, m, d)) => ("Date read", Ok(format!("{y:04}-{m:02}-{d:02}"))),
            Err(e) => ("Read date", Err(e)),
        },
        ExecuteAction::SetTime(hour, minute, second) => (
            "Time set",
            ok_unit(radio.write_time(hour, minute, second).await),
        ),
        ExecuteAction::ReadTime => match radio.read_time().await {
            Ok((h, mi, se)) => ("Time read", Ok(format!("{h:02}:{mi:02}:{se:02}"))),
            Err(e) => ("Read time", Err(e)),
        },
        ExecuteAction::SetTimeZoneOffset(minutes) => (
            "Time zone offset set",
            ok_unit(radio.write_time_zone_offset(minutes).await),
        ),
        ExecuteAction::ReadTimeZoneOffset => match radio.read_time_zone_offset().await {
            Ok(minutes) => {
                let sign = if minutes < 0 { '-' } else { '+' };
                let mag = minutes.unsigned_abs();
                (
                    "Time zone offset read",
                    Ok(format!("{sign}{:02}{:02}", mag / 60, mag % 60)),
                )
            }
            Err(e) => ("Read time zone offset", Err(e)),
        },
        ExecuteAction::GetOppositeBandInformation => {
            match radio.get_opposite_band_information().await {
                Ok(info) => (
                    "Opposite-band status read",
                    Ok(format!(
                        "VFO-B CH{:03} {} Hz mode {:X}h",
                        info.channel, info.frequency_hz, info.mode
                    )),
                ),
                Err(e) => ("Read opposite-band status", Err(e)),
            }
        }
        ExecuteAction::SetTxwOn(on) => ("TXW set", ok_unit(radio.set_txw_on(on).await)),
        ExecuteAction::StartDvsRecording(ch) => (
            "DVS recording started",
            ok_unit(radio.start_dvs_recording(ch).await),
        ),
        ExecuteAction::StopDvsRecording => (
            "DVS recording stopped",
            ok_unit(radio.stop_dvs_recording().await),
        ),
        ExecuteAction::GetDvsRecordingChannel => match radio.get_dvs_recording_channel().await {
            Ok(Some(ch)) => ("DVS recording status read", Ok(format!("Recording CH{ch}"))),
            Ok(None) => ("DVS recording status read", Ok("Not recording".to_string())),
            Err(e) => ("Read DVS recording status", Err(e)),
        },
        ExecuteAction::StartDvsPlayback(ch) => (
            "DVS playback started",
            ok_unit(radio.start_dvs_playback(ch).await),
        ),
        ExecuteAction::StopDvsPlayback => (
            "DVS playback stopped",
            ok_unit(radio.stop_dvs_playback().await),
        ),
        ExecuteAction::GetDvsPlaybackChannel => match radio.get_dvs_playback_channel().await {
            Ok(Some(ch)) => ("DVS playback status read", Ok(format!("Playing CH{ch}"))),
            Ok(None) => ("DVS playback status read", Ok("Not playing".to_string())),
            Err(e) => ("Read DVS playback status", Err(e)),
        },

        // --- Group 12 (`ExMenu`), path (b): number-entry escape hatch
        // (§11.4). New `Ft991aExtras` method (Wave 4 Task 1) — no existing
        // inherent method being re-exposed, unlike almost every other arm
        // above.
        ExecuteAction::SetExMenuItem(p1, value) => (
            "EX menu item set",
            ok_unit(radio.set_ex_menu_item(p1, value).await),
        ),

        // --- Profiles (§12.3) ---
        ExecuteAction::ApplyProfile(name, profile) => {
            let r = profile.apply(radio).await;
            (
                "Profile applied",
                match r {
                    Ok(()) => Ok(format!("Applied profile '{name}'")),
                    // `Profile::apply` only ever produces
                    // `ProfileError::Radio` — every other variant is a
                    // parse/load-time failure, already ruled out by the
                    // time `ProfileList` holds an already-parsed `Profile`.
                    // Kept as a fallback (not `unreachable!()`) rather than
                    // asserted away, since nothing enforces it structurally.
                    Err(radio::ProfileError::Radio(e)) => Err(e),
                    Err(other) => Err(RadioError::InvalidProtocolString(other.to_string())),
                },
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Hand-coded, full-parity diagnostics engine
// (`docs/adr/0006-hand-coded-full-parity-diagnostics.md`)
//
// Replaces the old `cat_diagnostics`-wrapped, read-only-liveness-only
// engine entirely. Mirrors `ts570d::ui::terminal`'s own
// `RadioSnapshot`/`snapshot_state`/`restore_state`/`run_diagnostics_task`
// pattern (same standard of care: every step calls a real typed
// `Radio`/`Ft991aExtras` method, verifies it, and the whole run is followed
// by an unconditional best-effort restore) — adapted to this crate's own
// single-sequential-loop architecture (no separate radio/UI tasks) and to
// FT-991A's own protocol quirks (see the ADR for the full per-command
// safety reasoning, especially the tricky ones: clarifier absolute-set
// semantics, QMB's dedicated slot, `VM`'s select-collapsing toggle, `KY`'s
// keyer-memory-playback nature).
// ---------------------------------------------------------------------------

/// Total number of diagnostic steps this engine runs (one row per method
/// call or set+verify pair — see the ADR for the full per-command mapping).
/// Verified against `run_diagnostics_task`'s actual output by
/// `test_diag_step_count_matches_actual_output` below.
pub(crate) const DIAG_STEP_COUNT: usize = 114;

/// A snapshot of every readable+settable piece of state this engine's
/// "plain parameter" steps touch, taken via typed `Radio`/`Ft991aExtras`
/// getters before anything runs. Every field is `Option<T>` so that an
/// individual getter failure is non-fatal (simply not restored later) —
/// mirrors `ts570d::ui::terminal::RadioSnapshot` exactly.
///
/// Steps whose effects are undone **inline**, within their own step (e.g.
/// `transmit`/`receive`, `swap_vfos`, memory-channel up/down, QMB
/// store/recall, `VM`, the `KY`/`KM` keyer-memory tests) deliberately do
/// **not** have a field here — this snapshot only covers state that is
/// simply set once and left for the final unconditional restore pass.
struct RadioSnapshot {
    vfo_a: Option<Frequency>,
    vfo_b: Option<Frequency>,
    mode: Option<Mode>,
    power_on: Option<bool>,
    memory_channel: Option<u8>,
    selected_meter: Option<Meter>,
    af_gain: Option<u8>,
    rf_gain: Option<u8>,
    squelch: Option<u8>,
    power: Option<u8>,
    rx_clarifier_on: Option<bool>,
    tx_clarifier_on: Option<bool>,
    /// Via `get_information()`'s `clarifier_offset_hz` (P3) — `RD`/`RU` are
    /// **absolute sets**, not relative steps (confirmed against
    /// `ft991a_radio.rs`'s own emulator implementation and
    /// `radio_trait.rs`'s doc comments — see the ADR), so this one field is
    /// enough to restore the exact original offset via one computed
    /// `clarifier_clear`/`clarifier_down`/`clarifier_up` call.
    clarifier_offset_hz: Option<i16>,
    ctcss_tone_hz: Option<f32>,
    dcs_code: Option<u16>,
    tone_squelch_mode: Option<ToneSquelchMode>,
    if_shift_hz: Option<i16>,
    keyer_pitch_hz: Option<u16>,
    keyer_enabled: Option<bool>,
    keyer_speed: Option<u8>,
    cw_spot_on: Option<bool>,
    break_in_on: Option<bool>,
    semi_break_in_delay: Option<u16>,
    scan_state: Option<ScanState>,
    vox_on: Option<bool>,
    vox_delay: Option<u16>,
    vox_gain: Option<u8>,
    attenuator_on: Option<bool>,
    preamp_mode: Option<PreampMode>,
    noise_blanker_on: Option<bool>,
    noise_blanker_level: Option<u8>,
    noise_reduction_on: Option<bool>,
    noise_reduction_level: Option<u8>,
    agc_mode: Option<AgcMode>,
    contour_on: Option<bool>,
    contour_frequency_hz: Option<u16>,
    apf_on: Option<bool>,
    apf_frequency_hz: Option<i16>,
    manual_notch_on: Option<bool>,
    manual_notch_frequency_hz: Option<u16>,
    auto_notch_on: Option<bool>,
    narrow_on: Option<bool>,
    filter_width_index: Option<u8>,
    mic_gain: Option<u8>,
    speech_processor_level: Option<u8>,
    speech_processor_on: Option<bool>,
    parametric_mic_eq_on: Option<bool>,
    monitor_on: Option<bool>,
    monitor_level: Option<u8>,
    fine_step: Option<bool>,
    auto_info_on: Option<bool>,
    dimmer: Option<(u8, u8)>,
    frequency_lock: Option<bool>,
    tx_vfo: Option<u8>,
    txw_on: Option<bool>,
    repeater_shift: Option<RepeaterShift>,
}

/// Snapshot all readable radio state this engine's plain-parameter steps
/// touch. Failures on individual fields are silently stored as `None` — the
/// snapshot itself always succeeds, mirroring `ts570d`'s own
/// `snapshot_state`.
async fn snapshot_state<R: Radio + Ft991aExtras>(radio: &mut R) -> RadioSnapshot {
    RadioSnapshot {
        vfo_a: radio.get_vfo_a().await.ok(),
        vfo_b: radio.get_vfo_b().await.ok(),
        mode: radio.get_mode().await.ok(),
        power_on: radio.get_power_on().await.ok(),
        memory_channel: radio.get_memory_channel().await.ok(),
        selected_meter: radio.get_selected_meter().await.ok(),
        af_gain: radio.get_af_gain().await.ok(),
        rf_gain: radio.get_rf_gain().await.ok(),
        squelch: radio.get_squelch().await.ok(),
        power: radio.get_power().await.ok(),
        rx_clarifier_on: radio.get_rx_clarifier_on().await.ok(),
        tx_clarifier_on: radio.get_tx_clarifier_on().await.ok(),
        clarifier_offset_hz: radio
            .get_information()
            .await
            .ok()
            .map(|info| info.clarifier_offset_hz),
        ctcss_tone_hz: radio.get_ctcss_tone_hz().await.ok(),
        dcs_code: radio.get_dcs_code().await.ok(),
        tone_squelch_mode: radio.get_tone_squelch_mode().await.ok(),
        if_shift_hz: radio.get_if_shift_hz().await.ok(),
        keyer_pitch_hz: radio.get_keyer_pitch_hz().await.ok(),
        keyer_enabled: radio.get_keyer_enabled().await.ok(),
        keyer_speed: radio.get_keyer_speed().await.ok(),
        cw_spot_on: radio.get_cw_spot_on().await.ok(),
        break_in_on: radio.get_break_in_on().await.ok(),
        semi_break_in_delay: radio.get_semi_break_in_delay().await.ok(),
        scan_state: radio.get_scan_state().await.ok(),
        vox_on: radio.get_vox_on().await.ok(),
        vox_delay: radio.get_vox_delay().await.ok(),
        vox_gain: radio.get_vox_gain().await.ok(),
        attenuator_on: radio.get_attenuator_on().await.ok(),
        preamp_mode: radio.get_preamp_mode().await.ok(),
        noise_blanker_on: radio.get_noise_blanker_on().await.ok(),
        noise_blanker_level: radio.get_noise_blanker_level().await.ok(),
        noise_reduction_on: radio.get_noise_reduction_on().await.ok(),
        noise_reduction_level: radio.get_noise_reduction_level().await.ok(),
        agc_mode: radio.get_agc_mode().await.ok(),
        contour_on: radio.get_contour_on().await.ok(),
        contour_frequency_hz: radio.get_contour_frequency_hz().await.ok(),
        apf_on: radio.get_apf_on().await.ok(),
        apf_frequency_hz: radio.get_apf_frequency_hz().await.ok(),
        manual_notch_on: radio.get_manual_notch_on().await.ok(),
        manual_notch_frequency_hz: radio.get_manual_notch_frequency_hz().await.ok(),
        auto_notch_on: radio.get_auto_notch_on().await.ok(),
        narrow_on: radio.get_narrow_on().await.ok(),
        filter_width_index: radio.get_filter_width_index().await.ok(),
        mic_gain: radio.get_mic_gain().await.ok(),
        speech_processor_level: radio.get_speech_processor_level().await.ok(),
        speech_processor_on: radio.get_speech_processor_on().await.ok(),
        parametric_mic_eq_on: radio.get_parametric_mic_eq_on().await.ok(),
        monitor_on: radio.get_monitor_on().await.ok(),
        monitor_level: radio.get_monitor_level().await.ok(),
        fine_step: radio.get_fine_step().await.ok(),
        auto_info_on: radio.get_auto_info_on().await.ok(),
        dimmer: radio.get_dimmer().await.ok(),
        frequency_lock: radio.get_frequency_lock().await.ok(),
        tx_vfo: radio.get_tx_vfo().await.ok(),
        txw_on: radio.get_txw_on().await.ok(),
        repeater_shift: radio.get_repeater_shift().await.ok(),
    }
}

/// Restore radio state from a snapshot. Best-effort: individual setter
/// failures are silently ignored (mirrors `ts570d`'s own `restore_state`).
/// PTT is cleared first via `receive()`; `power_on` is restored last so
/// every other setter has time to complete first.
async fn restore_state<R: Radio + Ft991aExtras>(radio: &mut R, snap: RadioSnapshot) {
    let _ = radio.receive().await;

    if let Some(v) = snap.vfo_a {
        let _ = radio.set_vfo_a(v).await;
    }
    if let Some(v) = snap.vfo_b {
        let _ = radio.set_vfo_b(v).await;
    }
    if let Some(v) = snap.mode {
        let _ = radio.set_mode(v).await;
    }
    if let Some(v) = snap.memory_channel {
        let _ = radio.set_memory_channel(v).await;
    }
    if let Some(v) = snap.selected_meter {
        let _ = radio.select_meter(v).await;
    }
    if let Some(v) = snap.af_gain {
        let _ = radio.set_af_gain(v).await;
    }
    if let Some(v) = snap.rf_gain {
        let _ = radio.set_rf_gain(v).await;
    }
    if let Some(v) = snap.squelch {
        let _ = radio.set_squelch(v).await;
    }
    if let Some(v) = snap.power {
        let _ = radio.set_power(v).await;
    }
    if let Some(v) = snap.rx_clarifier_on {
        let _ = radio.set_rx_clarifier_on(v).await;
    }
    if let Some(v) = snap.tx_clarifier_on {
        let _ = radio.set_tx_clarifier_on(v).await;
    }
    if let Some(offset) = snap.clarifier_offset_hz {
        let _ = match offset {
            0 => radio.clarifier_clear().await,
            o if o < 0 => radio.clarifier_down((-o) as u16).await,
            o => radio.clarifier_up(o as u16).await,
        };
    }
    if let Some(v) = snap.ctcss_tone_hz {
        let _ = radio.set_ctcss_tone_hz(v).await;
    }
    if let Some(v) = snap.dcs_code {
        let _ = radio.set_dcs_code(v).await;
    }
    if let Some(v) = snap.tone_squelch_mode {
        let _ = radio.set_tone_squelch_mode(v).await;
    }
    if let Some(v) = snap.if_shift_hz {
        let _ = radio.set_if_shift_hz(v).await;
    }
    if let Some(v) = snap.keyer_pitch_hz {
        let _ = radio.set_keyer_pitch_hz(v).await;
    }
    if let Some(v) = snap.keyer_enabled {
        let _ = radio.set_keyer_enabled(v).await;
    }
    if let Some(v) = snap.keyer_speed {
        let _ = radio.set_keyer_speed(v).await;
    }
    if let Some(v) = snap.cw_spot_on {
        let _ = radio.set_cw_spot_on(v).await;
    }
    if let Some(v) = snap.break_in_on {
        let _ = radio.set_break_in_on(v).await;
    }
    if let Some(v) = snap.semi_break_in_delay {
        let _ = radio.set_semi_break_in_delay(v).await;
    }
    if let Some(v) = snap.scan_state {
        let _ = radio.set_scan_state(v).await;
    }
    if let Some(v) = snap.vox_on {
        let _ = radio.set_vox_on(v).await;
    }
    if let Some(v) = snap.vox_delay {
        let _ = radio.set_vox_delay(v).await;
    }
    if let Some(v) = snap.vox_gain {
        let _ = radio.set_vox_gain(v).await;
    }
    if let Some(v) = snap.attenuator_on {
        let _ = radio.set_attenuator_on(v).await;
    }
    if let Some(v) = snap.preamp_mode {
        let _ = radio.set_preamp_mode(v).await;
    }
    if let Some(v) = snap.noise_blanker_on {
        let _ = radio.set_noise_blanker_on(v).await;
    }
    if let Some(v) = snap.noise_blanker_level {
        let _ = radio.set_noise_blanker_level(v).await;
    }
    if let Some(v) = snap.noise_reduction_on {
        let _ = radio.set_noise_reduction_on(v).await;
    }
    if let Some(v) = snap.noise_reduction_level {
        let _ = radio.set_noise_reduction_level(v).await;
    }
    if let Some(v) = snap.agc_mode {
        let _ = radio.set_agc_mode(v).await;
    }
    if let Some(v) = snap.contour_on {
        let _ = radio.set_contour_on(v).await;
    }
    if let Some(v) = snap.contour_frequency_hz {
        let _ = radio.set_contour_frequency_hz(v).await;
    }
    if let Some(v) = snap.apf_on {
        let _ = radio.set_apf_on(v).await;
    }
    if let Some(v) = snap.apf_frequency_hz {
        let _ = radio.set_apf_frequency_hz(v).await;
    }
    if let Some(v) = snap.manual_notch_on {
        let _ = radio.set_manual_notch_on(v).await;
    }
    if let Some(v) = snap.manual_notch_frequency_hz {
        let _ = radio.set_manual_notch_frequency_hz(v).await;
    }
    if let Some(v) = snap.auto_notch_on {
        let _ = radio.set_auto_notch_on(v).await;
    }
    if let Some(v) = snap.narrow_on {
        let _ = radio.set_narrow_on(v).await;
    }
    if let Some(v) = snap.filter_width_index {
        let _ = radio.set_filter_width_index(v).await;
    }
    if let Some(v) = snap.mic_gain {
        let _ = radio.set_mic_gain(v).await;
    }
    if let Some(v) = snap.speech_processor_level {
        let _ = radio.set_speech_processor_level(v).await;
    }
    if let Some(v) = snap.speech_processor_on {
        let _ = radio.set_speech_processor_on(v).await;
    }
    if let Some(v) = snap.parametric_mic_eq_on {
        let _ = radio.set_parametric_mic_eq_on(v).await;
    }
    if let Some(v) = snap.monitor_on {
        let _ = radio.set_monitor_on(v).await;
    }
    if let Some(v) = snap.monitor_level {
        let _ = radio.set_monitor_level(v).await;
    }
    if let Some(v) = snap.fine_step {
        let _ = radio.set_fine_step(v).await;
    }
    if let Some(v) = snap.auto_info_on {
        let _ = radio.set_auto_info_on(v).await;
    }
    if let Some((led, tft)) = snap.dimmer {
        let _ = radio.set_dimmer(led, tft).await;
    }
    if let Some(v) = snap.frequency_lock {
        let _ = radio.set_frequency_lock(v).await;
    }
    if let Some(v) = snap.tx_vfo {
        let _ = radio.set_tx_vfo(v).await;
    }
    if let Some(v) = snap.txw_on {
        let _ = radio.set_txw_on(v).await;
    }
    if let Some(v) = snap.repeater_shift {
        let _ = radio.set_repeater_shift(v).await;
    }

    // Restored last, mirrors `ts570d` exactly.
    if let Some(v) = snap.power_on {
        let _ = radio.set_power_on(v).await;
    }
}

/// Push one step's outcome, notifying the live-progress callback.
fn record_diag_outcome(
    outcomes: &mut Vec<DiagOutcome>,
    on_progress: &mut dyn FnMut(&DiagOutcome),
    code: &'static str,
    name: &'static str,
    result: DiagResult,
    start: Instant,
) {
    let outcome = DiagOutcome {
        code,
        name,
        result,
        duration: start.elapsed(),
    };
    on_progress(&outcome);
    outcomes.push(outcome);
}

/// Run the hand-coded, full-parity diagnostics engine
/// (`docs/adr/0006-hand-coded-full-parity-diagnostics.md`). Every step calls
/// a real typed `Radio`/`Ft991aExtras` method (never a raw CAT string) and
/// verifies it; the whole run is followed by an unconditional, best-effort
/// [`restore_state`] — mirrors `ts570d::ui::terminal::run_diagnostics_task`'s
/// own standard of care.
///
/// `cw_callsign` is collected **before** this function is ever called (the
/// `DiagWarning` -> `TextInput{action: DiagCwCallsign}` gate in
/// `control.rs`, front-loaded exactly like `ts570d`'s own gate) — `None`
/// means the operator left the prompt blank, so the `KY` (CW keying) step
/// is recorded `Skipped`, not attempted bare (an unidentified test
/// transmission would be a real regulatory problem, not just cosmetic).
///
/// Single pass only (no repeated rounds, unlike `ts570d`'s 3× — a
/// deliberate reduction in RF-safety exposure specific to this engine: a
/// run already keys PTT and, if a callsign was supplied, sends real CW
/// once; repeating that 3× per run buys robustness this repo's diagnostics
/// screen deliberately doesn't spend the extra transmit time on by
/// default).
async fn run_diagnostics_task<R: Radio + Ft991aExtras>(
    radio: &mut R,
    cw_callsign: Option<String>,
    mut on_progress: impl FnMut(&DiagOutcome),
) -> DiagSummary {
    let mut outcomes: Vec<DiagOutcome> = Vec::with_capacity(DIAG_STEP_COUNT);

    // Snapshot every readable+settable piece of state this run's plain
    // parameter steps touch, before anything runs.
    let snapshot = snapshot_state(radio).await;

    macro_rules! diag_get {
        ($code:expr, $name:expr, $expr:expr) => {{
            let start = Instant::now();
            let result = match $expr.await {
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
                Err(e) => DiagResult::Failure {
                    message: e.to_string(),
                },
            };
            record_diag_outcome(&mut outcomes, &mut on_progress, $code, $name, result, start);
        }};
    }

    macro_rules! diag_action {
        ($code:expr, $name:expr, $expr:expr) => {{
            let start = Instant::now();
            let result = match $expr.await {
                Ok(()) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
                Err(e) => DiagResult::Failure {
                    message: e.to_string(),
                },
            };
            record_diag_outcome(&mut outcomes, &mut on_progress, $code, $name, result, start);
        }};
    }

    macro_rules! diag_set_get {
        ($code:expr, $name:expr, $set:expr, $get:expr, $target:expr) => {{
            let start = Instant::now();
            let result = match $set.await {
                Err(e) => DiagResult::Failure {
                    message: format!("set failed: {e}"),
                },
                Ok(()) => match $get.await {
                    Err(e) => DiagResult::Failure {
                        message: format!("verify get failed: {e}"),
                    },
                    Ok(v) if v != $target => DiagResult::Failure {
                        message: format!("verify mismatch: got {:?} expected {:?}", v, $target),
                    },
                    Ok(_) => DiagResult::Success {
                        detail: "ok".to_string(),
                    },
                },
            };
            record_diag_outcome(&mut outcomes, &mut on_progress, $code, $name, result, start);
        }};
    }

    // -----------------------------------------------------------------
    // Batch: FA/FB/MD/TX
    // -----------------------------------------------------------------
    {
        let target = Frequency::new(14_195_000).expect("valid ham frequency");
        diag_set_get!(
            "FA",
            "set_vfo_a",
            radio.set_vfo_a(target),
            radio.get_vfo_a(),
            target
        );
    }
    diag_get!("FA", "get_vfo_a", radio.get_vfo_a());
    {
        let target = Frequency::new(7_100_000).expect("valid ham frequency");
        diag_set_get!(
            "FB",
            "set_vfo_b",
            radio.set_vfo_b(target),
            radio.get_vfo_b(),
            target
        );
    }
    diag_get!("FB", "get_vfo_b", radio.get_vfo_b());
    diag_set_get!(
        "MD",
        "set_mode(USB)",
        radio.set_mode(Mode::Usb),
        radio.get_mode(),
        Mode::Usb
    );
    diag_set_get!(
        "MD",
        "set_mode(LSB)",
        radio.set_mode(Mode::Lsb),
        radio.get_mode(),
        Mode::Lsb
    );
    diag_set_get!(
        "MD",
        "set_mode(CW)",
        radio.set_mode(Mode::CwU),
        radio.get_mode(),
        Mode::CwU
    );
    diag_get!("MD", "get_mode", radio.get_mode());

    // TX: transmit() immediately followed by receive() inline — self-undoing,
    // mirrors `ts570d`'s own step 56 exactly. This is the one step (besides
    // the callsign-gated `KY`) that genuinely keys PTT on real hardware.
    {
        let start = Instant::now();
        let result = match radio.transmit().await {
            Err(e) => DiagResult::Failure {
                message: format!("transmit failed: {e}"),
            },
            Ok(()) => {
                let _ = radio.receive().await;
                DiagResult::Success {
                    detail: "ok".to_string(),
                }
            }
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "TX",
            "transmit (immediately followed by receive)",
            result,
            start,
        );
    }
    diag_action!("TX", "receive", radio.receive());

    // -----------------------------------------------------------------
    // Batch: SM/RM/RI (pure reads, no restore needed) and MS
    // -----------------------------------------------------------------
    diag_get!("SM", "get_smeter", radio.get_smeter());
    diag_get!("RM", "get_meter(PO)", radio.get_meter(Meter::Po));
    diag_get!(
        "RI",
        "get_radio_indicator(TxLed)",
        radio.get_radio_indicator(RadioIndicator::TxLed)
    );
    diag_set_get!(
        "MS",
        "select_meter(PO)",
        radio.select_meter(Meter::Po),
        radio.get_selected_meter(),
        Meter::Po
    );

    // -----------------------------------------------------------------
    // Batch: PS/AG/RG/SQ/PC/ID
    // -----------------------------------------------------------------
    diag_set_get!(
        "PS",
        "set_power_on(true)",
        radio.set_power_on(true),
        radio.get_power_on(),
        true
    );
    diag_get!("PS", "get_power_on", radio.get_power_on());
    diag_set_get!(
        "AG",
        "set_af_gain",
        radio.set_af_gain(128),
        radio.get_af_gain(),
        128u8
    );
    diag_get!("AG", "get_af_gain", radio.get_af_gain());
    diag_set_get!(
        "RG",
        "set_rf_gain",
        radio.set_rf_gain(200),
        radio.get_rf_gain(),
        200u8
    );
    diag_get!("RG", "get_rf_gain", radio.get_rf_gain());
    diag_set_get!(
        "SQ",
        "set_squelch",
        radio.set_squelch(30),
        radio.get_squelch(),
        30u8
    );
    diag_get!("SQ", "get_squelch", radio.get_squelch());
    diag_set_get!(
        "PC",
        "set_power",
        radio.set_power(50),
        radio.get_power(),
        50u8
    );
    diag_get!("PC", "get_power", radio.get_power());
    diag_get!("ID", "get_id", radio.get_id());

    // -----------------------------------------------------------------
    // Batch: IF/RS/UL, EX (get-only)
    // -----------------------------------------------------------------
    diag_get!("IF", "get_information", radio.get_information());
    diag_get!("RS", "get_menu_mode_active", radio.get_menu_mode_active());
    diag_get!("UL", "get_pll_unlocked", radio.get_pll_unlocked());
    diag_get!("EX", "get_ex_menu_item(001)", radio.get_ex_menu_item(1));

    // -----------------------------------------------------------------
    // Batch: MC/MR/MW/MT
    // -----------------------------------------------------------------
    diag_set_get!(
        "MC",
        "set_memory_channel",
        radio.set_memory_channel(5),
        radio.get_memory_channel(),
        5u8
    );
    diag_get!("MC", "get_memory_channel", radio.get_memory_channel());
    diag_get!("MR", "read_memory_channel(1)", radio.read_memory_channel(1));
    // MW: write_memory_channel — self-contained snapshot/write/verify/
    // restore on channel 3 (a pure `read+write` round trip; `MemoryChannelEntry`
    // has no "vacant" concept in this trait, unlike `ts570d`, so the
    // original contents can always be written back verbatim).
    {
        let start = Instant::now();
        let test_ch: u8 = 3;
        let orig = radio.read_memory_channel(test_ch).await.ok();
        let test_entry = MemoryChannelEntry {
            channel: test_ch,
            frequency_hz: 14_205_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: Mode::Usb,
            tone_status: 0,
            offset_type: 0,
        };
        let result = match radio.write_memory_channel(test_entry).await {
            Err(e) => DiagResult::Failure {
                message: format!("write failed: {e}"),
            },
            Ok(()) => match radio.read_memory_channel(test_ch).await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify read failed: {e}"),
                },
                Ok(entry)
                    if entry.frequency_hz != test_entry.frequency_hz
                        || entry.mode != test_entry.mode =>
                {
                    DiagResult::Failure {
                        message: format!("verify mismatch: got {entry:?}"),
                    }
                }
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            },
        };
        if let Some(entry) = orig {
            let _ = radio.write_memory_channel(entry).await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "MW",
            "write_memory_channel",
            result,
            start,
        );
    }
    diag_get!(
        "MT",
        "read_memory_channel_tag(1)",
        radio.read_memory_channel_tag(1)
    );

    // -----------------------------------------------------------------
    // Batch: AB/BA/AM/VM/MA/CH/QI/QR/QS/SV
    // -----------------------------------------------------------------
    diag_action!("AB", "copy_vfo_a_to_b", radio.copy_vfo_a_to_b());
    diag_action!("BA", "copy_vfo_b_to_a", radio.copy_vfo_b_to_a());

    // AM: store_vfo_to_memory operates on the *currently selected* memory
    // channel (no channel argument) — self-contained: pick channel 2,
    // snapshot it and the current selection, then set/store/verify/restore.
    {
        let start = Instant::now();
        let test_ch: u8 = 2;
        let orig_selected = radio.get_memory_channel().await.ok();
        let orig_entry = radio.read_memory_channel(test_ch).await.ok();
        let result: DiagResult = 'step: {
            if let Err(e) = radio.set_memory_channel(test_ch).await {
                break 'step DiagResult::Failure {
                    message: format!("select channel failed: {e}"),
                };
            }
            let target = match Frequency::new(14_215_000) {
                Ok(f) => f,
                Err(e) => {
                    break 'step DiagResult::Failure {
                        message: format!("freq invalid: {e}"),
                    }
                }
            };
            if let Err(e) = radio.set_vfo_a(target).await {
                break 'step DiagResult::Failure {
                    message: format!("set_vfo_a failed: {e}"),
                };
            }
            if let Err(e) = radio.set_mode(Mode::Usb).await {
                break 'step DiagResult::Failure {
                    message: format!("set_mode failed: {e}"),
                };
            }
            if let Err(e) = radio.store_vfo_to_memory().await {
                break 'step DiagResult::Failure {
                    message: format!("store_vfo_to_memory failed: {e}"),
                };
            }
            match radio.read_memory_channel(test_ch).await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify read failed: {e}"),
                },
                Ok(entry) if entry.frequency_hz != target.hz() => DiagResult::Failure {
                    message: format!(
                        "verify mismatch: got {} expected {}",
                        entry.frequency_hz,
                        target.hz()
                    ),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            }
        };
        if let Some(entry) = orig_entry {
            let _ = radio.write_memory_channel(entry).await;
        }
        if let Some(ch) = orig_selected {
            let _ = radio.set_memory_channel(ch).await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "AM",
            "store_vfo_to_memory",
            result,
            start,
        );
    }

    // VM: toggle_vfo_memory_mode collapses any select mode other than
    // VFO(0)/Memory(1) to VFO(0) on the first toggle (confirmed via
    // `ft991a_radio.rs`'s emulator implementation), so a double-toggle only
    // restores exactly when the starting `select` (via `get_information`)
    // was 0 or 1 — otherwise this step is honestly `Skipped`, not guessed.
    {
        let start = Instant::now();
        let before = radio.get_information().await;
        let result = match before {
            Err(e) => DiagResult::Failure {
                message: format!("get_information failed: {e}"),
            },
            Ok(info) if info.select > 1 => DiagResult::Skipped {
                reason: format!(
                    "current VFO/memory select mode ({}) is not VFO(0)/Memory(1); \
                     toggle_vfo_memory_mode collapses any other mode to VFO on the \
                     first call and cannot be double-toggled back exactly",
                    info.select
                ),
            },
            Ok(info) => 'step: {
                let sel = info.select;
                if let Err(e) = radio.toggle_vfo_memory_mode().await {
                    break 'step DiagResult::Failure {
                        message: format!("toggle failed: {e}"),
                    };
                }
                let expected_after = u8::from(sel == 0);
                let after = match radio.get_information().await {
                    Err(e) => {
                        break 'step DiagResult::Failure {
                            message: format!("verify get failed: {e}"),
                        }
                    }
                    Ok(i) => i.select,
                };
                if after != expected_after {
                    break 'step DiagResult::Failure {
                        message: format!(
                            "verify mismatch: got select={after} expected {expected_after}"
                        ),
                    };
                }
                // Restore: a second toggle is exact since `sel` was 0 or 1.
                if let Err(e) = radio.toggle_vfo_memory_mode().await {
                    break 'step DiagResult::Failure {
                        message: format!("restore toggle failed: {e}"),
                    };
                }
                DiagResult::Success {
                    detail: "ok".to_string(),
                }
            }
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "VM",
            "toggle_vfo_memory_mode",
            result,
            start,
        );
    }

    diag_action!("MA", "recall_memory_to_vfo", radio.recall_memory_to_vfo());

    // CH: memory_channel_up / memory_channel_down, each self-contained —
    // verify the index actually changed, then restore via an *absolute*
    // `set_memory_channel` rather than stepping the opposite direction
    // (which might not land on the same index across a vacant-channel
    // wraparound boundary — manual gives no boundary behavior at all, see
    // the ADR / planning/yaesu/findings.md).
    {
        let start = Instant::now();
        let before = radio.get_memory_channel().await.ok();
        let result = match before {
            None => DiagResult::Failure {
                message: "get_memory_channel failed before test".to_string(),
            },
            Some(before) => 'step: {
                if let Err(e) = radio.memory_channel_up().await {
                    break 'step DiagResult::Failure {
                        message: format!("memory_channel_up failed: {e}"),
                    };
                }
                match radio.get_memory_channel().await {
                    Err(e) => {
                        break 'step DiagResult::Failure {
                            message: format!("verify get failed: {e}"),
                        }
                    }
                    Ok(after) if after == before => {
                        break 'step DiagResult::Failure {
                            message: "channel did not change".to_string(),
                        }
                    }
                    Ok(_) => {}
                }
                DiagResult::Success {
                    detail: "ok".to_string(),
                }
            }
        };
        if let Some(before) = before {
            let _ = radio.set_memory_channel(before).await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "CH",
            "memory_channel_up",
            result,
            start,
        );
    }
    {
        let start = Instant::now();
        let before = radio.get_memory_channel().await.ok();
        let result = match before {
            None => DiagResult::Failure {
                message: "get_memory_channel failed before test".to_string(),
            },
            Some(before) => 'step: {
                if let Err(e) = radio.memory_channel_down().await {
                    break 'step DiagResult::Failure {
                        message: format!("memory_channel_down failed: {e}"),
                    };
                }
                match radio.get_memory_channel().await {
                    Err(e) => {
                        break 'step DiagResult::Failure {
                            message: format!("verify get failed: {e}"),
                        }
                    }
                    Ok(after) if after == before => {
                        break 'step DiagResult::Failure {
                            message: "channel did not change".to_string(),
                        }
                    }
                    Ok(_) => {}
                }
                DiagResult::Success {
                    detail: "ok".to_string(),
                }
            }
        };
        if let Some(before) = before {
            let _ = radio.set_memory_channel(before).await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "CH",
            "memory_channel_down",
            result,
            start,
        );
    }

    // QI/QR: QMB is a dedicated single slot, not one of the 117 numbered
    // memory channels (confirmed via `IF`'s own P7 legend — 3=QMB, 4=QMB-MT
    // vs. 1=Memory — planning/yaesu/findings.md). There is no direct "read
    // QMB" method; `qmb_recall()` is the only way to observe its contents
    // (by copying them into VFO-A), so that's used to capture the original
    // contents before overwriting them with test data via `qmb_store()`.
    {
        let start = Instant::now();
        let recall_ok = radio.qmb_recall().await.is_ok();
        let orig_qmb = if recall_ok {
            let freq = radio.get_vfo_a().await.ok();
            let mode = radio.get_mode().await.ok();
            freq.map(|f| (f, mode))
        } else {
            None
        };
        let result: DiagResult = 'step: {
            let target = match Frequency::new(14_222_000) {
                Ok(f) => f,
                Err(e) => {
                    break 'step DiagResult::Failure {
                        message: format!("freq invalid: {e}"),
                    }
                }
            };
            if let Err(e) = radio.set_vfo_a(target).await {
                break 'step DiagResult::Failure {
                    message: format!("set_vfo_a failed: {e}"),
                };
            }
            if let Err(e) = radio.qmb_store().await {
                break 'step DiagResult::Failure {
                    message: format!("qmb_store failed: {e}"),
                };
            }
            if let Err(e) = radio.qmb_recall().await {
                break 'step DiagResult::Failure {
                    message: format!("qmb_recall failed: {e}"),
                };
            }
            match radio.get_vfo_a().await {
                Err(e) => {
                    break 'step DiagResult::Failure {
                        message: format!("verify get failed: {e}"),
                    }
                }
                Ok(v) if v != target => {
                    break 'step DiagResult::Failure {
                        message: format!(
                            "verify mismatch: got {} expected {}",
                            v.hz(),
                            target.hz()
                        ),
                    }
                }
                Ok(_) => {}
            }
            if orig_qmb.is_none() {
                break 'step DiagResult::Success {
                    detail: "ok (QMB was unreadable/empty before this test, so its \
                              original contents could not be captured or restored)"
                        .to_string(),
                };
            }
            DiagResult::Success {
                detail: "ok".to_string(),
            }
        };
        if let Some((freq, mode)) = orig_qmb {
            let _ = radio.set_vfo_a(freq).await;
            if let Some(m) = mode {
                let _ = radio.set_mode(m).await;
            }
            let _ = radio.qmb_store().await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "QI/QR",
            "qmb_store + qmb_recall round trip",
            result,
            start,
        );
    }

    // QS: no dedicated on/off pair exists anywhere in the master command
    // table for Quick Split (findings.md), and no getter exists either —
    // implemented (both here and by the radio itself) as a plain boolean
    // toggle, so two calls always net identity regardless of the starting
    // state.
    {
        let start = Instant::now();
        let result = match radio.quick_split().await {
            Err(e) => DiagResult::Failure {
                message: format!("first toggle failed: {e}"),
            },
            Ok(()) => match radio.quick_split().await {
                Err(e) => DiagResult::Failure {
                    message: format!("second toggle (restore) failed: {e}"),
                },
                Ok(()) => DiagResult::Success {
                    detail: "ok (toggled twice, net identity — no on/off pair or \
                              getter exists for QS)"
                        .to_string(),
                },
            },
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "QS",
            "quick_split (toggle twice)",
            result,
            start,
        );
    }

    // SV: reversible by construction — swap, verify, swap back.
    {
        let start = Instant::now();
        let before_a = radio.get_vfo_a().await.ok();
        let before_b = radio.get_vfo_b().await.ok();
        let result = 'step: {
            if let Err(e) = radio.swap_vfos().await {
                break 'step DiagResult::Failure {
                    message: format!("swap failed: {e}"),
                };
            }
            if let (Some(a), Some(b)) = (before_a, before_b) {
                let after_a = radio.get_vfo_a().await.ok();
                let after_b = radio.get_vfo_b().await.ok();
                if after_a != Some(b) || after_b != Some(a) {
                    let _ = radio.swap_vfos().await;
                    break 'step DiagResult::Failure {
                        message: "verify mismatch: VFO-A/B did not swap as expected".to_string(),
                    };
                }
            }
            if let Err(e) = radio.swap_vfos().await {
                break 'step DiagResult::Failure {
                    message: format!("restore swap failed: {e}"),
                };
            }
            DiagResult::Success {
                detail: "ok".to_string(),
            }
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "SV",
            "swap_vfos (swap, verify, swap back)",
            result,
            start,
        );
    }

    // -----------------------------------------------------------------
    // Batch: RT/RC/RD/RU/XT/CN/CT/IS
    // -----------------------------------------------------------------
    diag_set_get!(
        "RT",
        "set_rx_clarifier_on(true)",
        radio.set_rx_clarifier_on(true),
        radio.get_rx_clarifier_on(),
        true
    );
    diag_get!("RT", "get_rx_clarifier_on", radio.get_rx_clarifier_on());

    // RC/RD/RU exercise the shared `clarifier_offset_hz` field (via
    // `get_information()`). Confirmed `RD`/`RU` are *absolute sets*, not
    // relative steps (see `RadioSnapshot::clarifier_offset_hz`'s doc
    // comment / the ADR) — full exact restoration of whatever the offset
    // was before this run happens via the final `restore_state` pass.
    {
        let start = Instant::now();
        let result = 'step: {
            if let Err(e) = radio.clarifier_up(250).await {
                break 'step DiagResult::Failure {
                    message: format!("setup set failed: {e}"),
                };
            }
            match radio.get_information().await {
                Err(e) => {
                    break 'step DiagResult::Failure {
                        message: format!("setup verify failed: {e}"),
                    }
                }
                Ok(i) if i.clarifier_offset_hz != 250 => {
                    break 'step DiagResult::Failure {
                        message: format!(
                            "setup mismatch: got {} expected 250",
                            i.clarifier_offset_hz
                        ),
                    }
                }
                Ok(_) => {}
            }
            if let Err(e) = radio.clarifier_clear().await {
                break 'step DiagResult::Failure {
                    message: format!("clear failed: {e}"),
                };
            }
            match radio.get_information().await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify get failed: {e}"),
                },
                Ok(i) if i.clarifier_offset_hz != 0 => DiagResult::Failure {
                    message: format!("verify mismatch: got {} expected 0", i.clarifier_offset_hz),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            }
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "RC",
            "clarifier_clear (after clarifier_up(250) setup)",
            result,
            start,
        );
    }
    {
        let start = Instant::now();
        let result = match radio.clarifier_down(300).await {
            Err(e) => DiagResult::Failure {
                message: format!("set failed: {e}"),
            },
            Ok(()) => match radio.get_information().await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify get failed: {e}"),
                },
                Ok(i) if i.clarifier_offset_hz != -300 => DiagResult::Failure {
                    message: format!(
                        "verify mismatch: got {} expected -300",
                        i.clarifier_offset_hz
                    ),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            },
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "RD",
            "clarifier_down(300)",
            result,
            start,
        );
    }
    {
        let start = Instant::now();
        let result = match radio.clarifier_up(150).await {
            Err(e) => DiagResult::Failure {
                message: format!("set failed: {e}"),
            },
            Ok(()) => match radio.get_information().await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify get failed: {e}"),
                },
                Ok(i) if i.clarifier_offset_hz != 150 => DiagResult::Failure {
                    message: format!(
                        "verify mismatch: got {} expected 150",
                        i.clarifier_offset_hz
                    ),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            },
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "RU",
            "clarifier_up(150)",
            result,
            start,
        );
    }
    diag_set_get!(
        "XT",
        "set_tx_clarifier_on(true)",
        radio.set_tx_clarifier_on(true),
        radio.get_tx_clarifier_on(),
        true
    );
    diag_get!("XT", "get_tx_clarifier_on", radio.get_tx_clarifier_on());
    {
        let target = ctcss_tone_hz(0).expect("index 0 is a valid standard CTCSS tone");
        diag_set_get!(
            "CN",
            "set_ctcss_tone_hz",
            radio.set_ctcss_tone_hz(target),
            radio.get_ctcss_tone_hz(),
            target
        );
    }
    {
        let target = dcs_code_number(0).expect("index 0 is a valid standard DCS code");
        diag_set_get!(
            "CN",
            "set_dcs_code",
            radio.set_dcs_code(target),
            radio.get_dcs_code(),
            target
        );
    }
    diag_set_get!(
        "CT",
        "set_tone_squelch_mode(Off)",
        radio.set_tone_squelch_mode(ToneSquelchMode::Off),
        radio.get_tone_squelch_mode(),
        ToneSquelchMode::Off
    );
    diag_set_get!(
        "IS",
        "set_if_shift_hz(0)",
        radio.set_if_shift_hz(0),
        radio.get_if_shift_hz(),
        0i16
    );

    // -----------------------------------------------------------------
    // Batch: KM/KP/KR/KS/KY/CS/ZI/BI/SD
    // -----------------------------------------------------------------

    // KM: write_keyer_memory/read_keyer_memory — self-contained on channel
    // 2 (the `KY` step below uses channel 1). `write_keyer_memory`
    // validates 1-50 printable ASCII chars client-side, before any wire
    // I/O; if the channel was vacant (factory default: empty string) before
    // this test, that same validation means it cannot be restored back to
    // *exactly* empty — a genuine, documented protocol limitation, not an
    // oversight.
    {
        let start = Instant::now();
        let test_ch: u8 = 2;
        let orig = radio.read_keyer_memory(test_ch).await.ok();
        let result: DiagResult = 'step: {
            if let Err(e) = radio.write_keyer_memory(test_ch, "DIAG TEST").await {
                break 'step DiagResult::Failure {
                    message: format!("write failed: {e}"),
                };
            }
            match radio.read_keyer_memory(test_ch).await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify read failed: {e}"),
                },
                Ok(msg) if msg != "DIAG TEST" => DiagResult::Failure {
                    message: format!("verify mismatch: got {msg:?}"),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            }
        };
        match orig.as_deref() {
            Some(msg) if !msg.is_empty() => {
                let _ = radio.write_keyer_memory(test_ch, msg).await;
            }
            _ => {}
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "KM",
            "write_keyer_memory/read_keyer_memory (channel 2)",
            result,
            start,
        );
    }

    {
        let target: u16 = 700;
        diag_set_get!(
            "KP",
            "set_keyer_pitch_hz",
            radio.set_keyer_pitch_hz(target),
            radio.get_keyer_pitch_hz(),
            target
        );
    }
    diag_set_get!(
        "KR",
        "set_keyer_enabled(true)",
        radio.set_keyer_enabled(true),
        radio.get_keyer_enabled(),
        true
    );
    {
        let target: u8 = 20;
        diag_set_get!(
            "KS",
            "set_keyer_speed",
            radio.set_keyer_speed(target),
            radio.get_keyer_speed(),
            target
        );
    }

    // KY: play_keyer_memory — a real over-the-air CW transmission of a
    // *pre-stored* keyer-memory message (NOT arbitrary text like ts570d's
    // `send_cw` — see `ft991a.rs`'s `play_keyer_memory` doc comment), so
    // it is gated behind `control.rs`'s `DiagWarning` -> `DiagCwCallsign`
    // prompt (`docs/adr/0006-hand-coded-full-parity-diagnostics.md`). Only
    // ever sent with station identification ("TEST <CALLSIGN>"); a blank
    // or cancelled prompt means this one step is `Skipped`, not attempted
    // bare and not a hard failure — every other step still runs normally.
    {
        let start = Instant::now();
        let result = match &cw_callsign {
            None => DiagResult::Skipped {
                reason: "no callsign supplied — CW keying test requires station ID".to_string(),
            },
            Some(callsign) => {
                let test_ch: u8 = 1;
                let message = format!("TEST {callsign}");
                let orig = radio.read_keyer_memory(test_ch).await.ok();
                let step_result: DiagResult = 'step: {
                    if let Err(e) = radio.write_keyer_memory(test_ch, &message).await {
                        break 'step DiagResult::Failure {
                            message: format!("write failed: {e}"),
                        };
                    }
                    if let Err(e) = radio
                        .play_keyer_memory(test_ch, KeyerPlaybackMode::KeyerMemory)
                        .await
                    {
                        break 'step DiagResult::Failure {
                            message: format!("play failed: {e}"),
                        };
                    }
                    DiagResult::Success {
                        detail: format!("ok (sent \"{message}\")"),
                    }
                };
                match orig.as_deref() {
                    Some(msg) if !msg.is_empty() => {
                        let _ = radio.write_keyer_memory(test_ch, msg).await;
                    }
                    _ => {}
                }
                step_result
            }
        };
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "KY",
            "play_keyer_memory (CW test)",
            result,
            start,
        );
    }

    diag_set_get!(
        "CS",
        "set_cw_spot_on(false)",
        radio.set_cw_spot_on(false),
        radio.get_cw_spot_on(),
        false
    );
    diag_action!("ZI", "zero_in", radio.zero_in());
    diag_set_get!(
        "BI",
        "set_break_in_on(true)",
        radio.set_break_in_on(true),
        radio.get_break_in_on(),
        true
    );
    {
        let target: u16 = 50;
        diag_set_get!(
            "SD",
            "set_semi_break_in_delay",
            radio.set_semi_break_in_delay(target),
            radio.get_semi_break_in_delay(),
            target
        );
    }

    // -----------------------------------------------------------------
    // Batch: SC/VX/VD/VG/BY
    // -----------------------------------------------------------------
    diag_set_get!(
        "SC",
        "set_scan_state(Off)",
        radio.set_scan_state(ScanState::Off),
        radio.get_scan_state(),
        ScanState::Off
    );
    diag_set_get!(
        "VX",
        "set_vox_on(false)",
        radio.set_vox_on(false),
        radio.get_vox_on(),
        false
    );
    {
        let target: u16 = 300;
        diag_set_get!(
            "VD",
            "set_vox_delay",
            radio.set_vox_delay(target),
            radio.get_vox_delay(),
            target
        );
    }
    {
        let target: u8 = 5;
        diag_set_get!(
            "VG",
            "set_vox_gain",
            radio.set_vox_gain(target),
            radio.get_vox_gain(),
            target
        );
    }
    diag_get!("BY", "get_rx_busy", radio.get_rx_busy());

    // -----------------------------------------------------------------
    // Batch: RA/PA/NB/NL/NR/RL/GT/CO/BP/BC/NA/SH
    // -----------------------------------------------------------------
    diag_set_get!(
        "RA",
        "set_attenuator_on(false)",
        radio.set_attenuator_on(false),
        radio.get_attenuator_on(),
        false
    );
    diag_set_get!(
        "PA",
        "set_preamp_mode(IPO)",
        radio.set_preamp_mode(PreampMode::Ipo),
        radio.get_preamp_mode(),
        PreampMode::Ipo
    );
    diag_set_get!(
        "NB",
        "set_noise_blanker_on(true)",
        radio.set_noise_blanker_on(true),
        radio.get_noise_blanker_on(),
        true
    );
    {
        let target: u8 = 1;
        diag_set_get!(
            "NL",
            "set_noise_blanker_level",
            radio.set_noise_blanker_level(target),
            radio.get_noise_blanker_level(),
            target
        );
    }
    diag_set_get!(
        "NR",
        "set_noise_reduction_on(true)",
        radio.set_noise_reduction_on(true),
        radio.get_noise_reduction_on(),
        true
    );
    {
        let target: u8 = 1;
        diag_set_get!(
            "RL",
            "set_noise_reduction_level",
            radio.set_noise_reduction_level(target),
            radio.get_noise_reduction_level(),
            target
        );
    }
    diag_set_get!(
        "GT",
        "set_agc_mode(Fast)",
        radio.set_agc_mode(AgcMode::Fast),
        radio.get_agc_mode(),
        AgcMode::Fast
    );
    diag_set_get!(
        "CO",
        "set_contour_on(false)",
        radio.set_contour_on(false),
        radio.get_contour_on(),
        false
    );
    {
        let target: u16 = 1000;
        diag_set_get!(
            "CO",
            "set_contour_frequency_hz",
            radio.set_contour_frequency_hz(target),
            radio.get_contour_frequency_hz(),
            target
        );
    }
    diag_set_get!(
        "CO",
        "set_apf_on(false)",
        radio.set_apf_on(false),
        radio.get_apf_on(),
        false
    );
    diag_set_get!(
        "CO",
        "set_apf_frequency_hz(0)",
        radio.set_apf_frequency_hz(0),
        radio.get_apf_frequency_hz(),
        0i16
    );
    diag_set_get!(
        "BP",
        "set_manual_notch_on(false)",
        radio.set_manual_notch_on(false),
        radio.get_manual_notch_on(),
        false
    );
    {
        let target: u16 = 1000;
        diag_set_get!(
            "BP",
            "set_manual_notch_frequency_hz",
            radio.set_manual_notch_frequency_hz(target),
            radio.get_manual_notch_frequency_hz(),
            target
        );
    }
    diag_set_get!(
        "BC",
        "set_auto_notch_on(false)",
        radio.set_auto_notch_on(false),
        radio.get_auto_notch_on(),
        false
    );
    diag_set_get!(
        "NA",
        "set_narrow_on(false)",
        radio.set_narrow_on(false),
        radio.get_narrow_on(),
        false
    );
    {
        let target: u8 = 10;
        diag_set_get!(
            "SH",
            "set_filter_width_index",
            radio.set_filter_width_index(target),
            radio.get_filter_width_index(),
            target
        );
    }

    // -----------------------------------------------------------------
    // Batch: MG/PL/PR/ML
    // -----------------------------------------------------------------
    {
        let target: u8 = 50;
        diag_set_get!(
            "MG",
            "set_mic_gain",
            radio.set_mic_gain(target),
            radio.get_mic_gain(),
            target
        );
    }
    {
        let target: u8 = 50;
        diag_set_get!(
            "PL",
            "set_speech_processor_level",
            radio.set_speech_processor_level(target),
            radio.get_speech_processor_level(),
            target
        );
    }
    diag_set_get!(
        "PR",
        "set_speech_processor_on(false)",
        radio.set_speech_processor_on(false),
        radio.get_speech_processor_on(),
        false
    );
    diag_set_get!(
        "PR",
        "set_parametric_mic_eq_on(false)",
        radio.set_parametric_mic_eq_on(false),
        radio.get_parametric_mic_eq_on(),
        false
    );
    diag_set_get!(
        "ML",
        "set_monitor_on(false)",
        radio.set_monitor_on(false),
        radio.get_monitor_on(),
        false
    );
    {
        let target: u8 = 50;
        diag_set_get!(
            "ML",
            "set_monitor_level",
            radio.set_monitor_level(target),
            radio.get_monitor_level(),
            target
        );
    }

    // -----------------------------------------------------------------
    // Batch: BS/BU/BD/FS/ED/EU/EK/DN/UP
    // -----------------------------------------------------------------
    // No `get_band` getter exists anywhere on this trait (checked in
    // full) — `BS`/`BU`/`BD` can only be verified `Ok`-only, an honest
    // trait-surface gap rather than a guessed frequency-range check
    // (selecting a band doesn't necessarily retune VFO-A into that band's
    // edges — it's a band-stacking-register concept). VFO-A/mode are
    // restored by the final `restore_state` regardless of what these do.
    diag_action!(
        "BS",
        "set_band(FourteenMHz)",
        radio.set_band(Band::FourteenMHz)
    );
    diag_action!("BU", "band_up", radio.band_up());
    diag_action!("BD", "band_down", radio.band_down());
    diag_set_get!(
        "FS",
        "set_fine_step(false)",
        radio.set_fine_step(false),
        radio.get_fine_step(),
        false
    );
    // ED/EU/EK/ZI mutate no persisted state at all in this radio's own
    // implementation (confirmed via `ft991a_radio.rs`'s emulator source —
    // "structurally and semantically validated... but mutate no persisted
    // Ft991aState field"), so `Ok`-only verification is the most this
    // engine (or real hardware, per the manual's own silence here) can ever
    // check.
    diag_action!(
        "ED",
        "encoder_down(Main,1)",
        radio.encoder_down(EncoderSelector::Main, 1)
    );
    diag_action!(
        "EU",
        "encoder_up(Main,1)",
        radio.encoder_up(EncoderSelector::Main, 1)
    );
    diag_action!("EK", "ent_key", radio.ent_key());
    // DN/UP (mic_down/mic_up) step `vfo_a_hz` by a fixed amount — already
    // `Radio`-trait methods, just previously unreachable by the read-only
    // engine (zero-width Action commands, no query form). Treated exactly
    // like `ts570d`'s own `mic_up`/`mic_down` steps: `Ok`-only, relying on
    // the final VFO-A restore (safe under normal test conditions, nowhere
    // near the saturating band edges).
    diag_action!("DN", "mic_down", radio.mic_down());
    diag_action!("UP", "mic_up", radio.mic_up());

    // -----------------------------------------------------------------
    // Batch: AC/AI/DA/DT/LK/OI/FT/TS/MX, LM/PB (DVS, get-only)
    // -----------------------------------------------------------------
    // AC (antenna tuner state) deliberately stays get-only: `state=2`
    // ("start tuning") is plausibly RF-relevant on real hardware (a tuning
    // cycle typically keys a low-power test carrier) and antenna-tuner
    // testing was never part of the 28 commands this round's full-parity
    // work targeted — same conservative-scope reasoning as `MX`/DVS below.
    diag_get!(
        "AC",
        "get_antenna_tuner_state",
        radio.get_antenna_tuner_state()
    );
    diag_set_get!(
        "AI",
        "set_auto_info_on(false)",
        radio.set_auto_info_on(false),
        radio.get_auto_info_on(),
        false
    );
    {
        let led: u8 = 1;
        let tft: u8 = 8;
        diag_set_get!(
            "DA",
            "set_dimmer",
            radio.set_dimmer(led, tft),
            radio.get_dimmer(),
            (led, tft)
        );
    }

    // DT: date/time/time-zone — three independent self-contained
    // snapshot/write/verify/restore steps.
    {
        let start = Instant::now();
        let orig = radio.read_date().await.ok();
        let result: DiagResult = 'step: {
            if let Err(e) = radio.write_date(2026, 7, 26).await {
                break 'step DiagResult::Failure {
                    message: format!("write failed: {e}"),
                };
            }
            match radio.read_date().await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify read failed: {e}"),
                },
                Ok(d) if d != (2026, 7, 26) => DiagResult::Failure {
                    message: format!("verify mismatch: got {d:?}"),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            }
        };
        if let Some((y, m, d)) = orig {
            let _ = radio.write_date(y, m, d).await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "DT",
            "write_date/read_date",
            result,
            start,
        );
    }
    {
        let start = Instant::now();
        let orig = radio.read_time().await.ok();
        let result: DiagResult = 'step: {
            if let Err(e) = radio.write_time(12, 34, 56).await {
                break 'step DiagResult::Failure {
                    message: format!("write failed: {e}"),
                };
            }
            match radio.read_time().await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify read failed: {e}"),
                },
                Ok(t) if t != (12, 34, 56) => DiagResult::Failure {
                    message: format!("verify mismatch: got {t:?}"),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            }
        };
        if let Some((h, m, s)) = orig {
            let _ = radio.write_time(h, m, s).await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "DT",
            "write_time/read_time",
            result,
            start,
        );
    }
    {
        let start = Instant::now();
        let orig = radio.read_time_zone_offset().await.ok();
        let result: DiagResult = 'step: {
            if let Err(e) = radio.write_time_zone_offset(0).await {
                break 'step DiagResult::Failure {
                    message: format!("write failed: {e}"),
                };
            }
            match radio.read_time_zone_offset().await {
                Err(e) => DiagResult::Failure {
                    message: format!("verify read failed: {e}"),
                },
                Ok(v) if v != 0 => DiagResult::Failure {
                    message: format!("verify mismatch: got {v}"),
                },
                Ok(_) => DiagResult::Success {
                    detail: "ok".to_string(),
                },
            }
        };
        if let Some(v) = orig {
            let _ = radio.write_time_zone_offset(v).await;
        }
        record_diag_outcome(
            &mut outcomes,
            &mut on_progress,
            "DT",
            "write_time_zone_offset/read_time_zone_offset",
            result,
            start,
        );
    }

    diag_set_get!(
        "LK",
        "set_frequency_lock(false)",
        radio.set_frequency_lock(false),
        radio.get_frequency_lock(),
        false
    );
    diag_get!(
        "OI",
        "get_opposite_band_information",
        radio.get_opposite_band_information()
    );
    diag_set_get!(
        "FT",
        "set_tx_vfo",
        radio.set_tx_vfo(0),
        radio.get_tx_vfo(),
        0u8
    );
    diag_set_get!(
        "TS",
        "set_txw_on(false)",
        radio.set_txw_on(false),
        radio.get_txw_on(),
        false
    );
    // MX (MOX — manual transmitter key) deliberately stays get-only: setting
    // it ON keys the transmitter directly, and MOX testing was never part
    // of the 28 commands this round's full-parity work targeted.
    diag_get!("MX", "get_mox_on", radio.get_mox_on());
    // DVS (LM/PB) deliberately stays get-only: start/stop recording or
    // playback has real physical side effects (overwrites a voice memory
    // slot) and, like AC/MX above, was never part of the 28 commands this
    // round's full-parity work targeted.
    diag_get!(
        "LM",
        "get_dvs_recording_channel",
        radio.get_dvs_recording_channel()
    );
    diag_get!(
        "PB",
        "get_dvs_playback_channel",
        radio.get_dvs_playback_channel()
    );

    // Bonus: `get_repeater_shift`/`set_repeater_shift` have no dedicated
    // top-level command-table row of their own (derived from the same
    // `offset_type` field embedded in `MW`/`MR`/`MT`/`IF`), so they're not
    // one of the 91 — included anyway for full trait-surface coverage.
    diag_set_get!(
        "OS*",
        "set_repeater_shift(Simplex)",
        radio.set_repeater_shift(RepeaterShift::Simplex),
        radio.get_repeater_shift(),
        RepeaterShift::Simplex
    );

    // Restore all snapshotted radio state unconditionally (best-effort).
    restore_state(radio, snapshot).await;

    DiagSummary { outcomes }
}

/// Draw one live-progress diagnostics frame (header/status/errors plus the
/// diagnostics panel itself) — factored out so it can be called both once
/// up front (0 outcomes yet) and from inside [`run_diagnostics_task`]'s
/// progress callback below, without duplicating [`draw_frame`]'s own
/// header/status/errors setup.
fn draw_diagnostics_frame(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    display: &Ft991aDisplay,
    outcomes: &[DiagOutcome],
    total: usize,
) -> UiResult<()> {
    terminal.draw(|f| {
        let area = f.size();
        let (header_area, status_area, errors_area, ctrl_area) = split_areas(area);
        draw_header(f, header_area);
        draw_status(f, status_area, display);
        draw_errors(f, errors_area, display);
        draw_diagnostics_live(f, ctrl_area, outcomes, total);
    })?;
    Ok(())
}

/// Run [`run_diagnostics_task`], redrawing the screen after every step's
/// outcome for live progress, and return the [`ControlState`] to transition
/// to once it completes.
///
/// This is the one place in this crate that calls [`Terminal::draw`]
/// directly from inside a synchronous callback (`on_progress: FnMut(&
/// DiagOutcome)` is deliberately **not** `async`) rather than through the
/// normal once-per-loop-iteration [`draw_frame`] call. This works because
/// [`Terminal::draw`] is itself a plain synchronous function (ratatui does
/// no I/O awaiting of its own), so calling it from inside a sync closure
/// that a single `.await`ed diagnostics run invokes repeatedly is exactly
/// as safe as calling it from `run_loop`'s own synchronous match arms.
/// Blocks the whole event loop for the duration of the run (this crate's
/// existing single-sequential-loop architecture, per `terminal.rs`'s own
/// module docs, has no separate task to keep servicing key events
/// meanwhile) — acceptable here since the run itself is bounded
/// (`DIAG_STEP_COUNT` steps, single pass, each a bounded typed method
/// call).
async fn run_diagnostics_screen<R: Radio + Ft991aExtras + CwKeying>(
    radio: &mut R,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    display: &Ft991aDisplay,
    cw_callsign: Option<String>,
) -> ControlState {
    let mut outcomes: Vec<DiagOutcome> = Vec::with_capacity(DIAG_STEP_COUNT);

    // Draw the initial "0/total" frame before the first probe goes out —
    // otherwise the screen would appear frozen on the prior `ControlState`
    // until the first outcome arrives.
    let _ = draw_diagnostics_frame(terminal, display, &outcomes, DIAG_STEP_COUNT);

    let summary = run_diagnostics_task(radio, cw_callsign, |outcome| {
        outcomes.push(outcome.clone());
        let _ = draw_diagnostics_frame(terminal, display, &outcomes, DIAG_STEP_COUNT);
    })
    .await;

    ControlState::Diagnostics { summary, cursor: 0 }
}

/// Run the terminal UI against a live [`radio::Radio`] implementation.
///
/// Single sequential loop (see module docs for why this departs from
/// ts570d's two-task/channel architecture): every iteration, poll the
/// radio if `POLL_INTERVAL` has elapsed, redraw, then wait up to
/// `EVENT_POLL_TIMEOUT` for a key event and handle it inline.
///
/// **Bound widened in Wave 4 Task 2** (`planning/architect/task_plan.md`
/// §11.3 point 3) from `R: Radio + 'static` to `R: Radio + Ft991aExtras +
/// CwKeying + 'static` — a disclosed, real narrowing of this crate's scope:
/// `ui` is no longer usable against "any `Radio` implementation" in the
/// abstract, only against types that also implement the FT-991A-specific
/// `Ft991aExtras`/`CwKeying` traits (see `crate` module docs). Costs nothing
/// against this repo's only concrete wiring today
/// (`Ft991a<SerialCatSession<SerialPort>>`, `src/main.rs`), which already
/// satisfies all three bounds unconditionally.
pub async fn run<R: Radio + Ft991aExtras + CwKeying + 'static>(mut radio: R) -> UiResult<()> {
    let mut terminal = init_terminal()?;
    let result = run_loop(&mut terminal, &mut radio).await;
    cleanup_terminal()?;
    result
}

async fn run_loop<R: Radio + Ft991aExtras + CwKeying>(
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
            // Unix terminals only ever report key-down as `Event::Key`, so
            // this loop was never exercised against anything else — but
            // Windows' native console backend reports key-up too (crossterm
            // surfaces it as the same `Event::Key` variant with `kind ==
            // KeyEventKind::Release`), which without this filter fired
            // `handle_key` twice per keystroke (once on press, once on
            // release), duplicating every input. `KeyEventKind::Press` is
            // the only kind that should ever trigger an action; `Repeat`
            // (held-key autorepeat) is deliberately excluded too, since
            // this UI already advances via its own event-poll loop rather
            // than relying on OS key-repeat.
            if let Event::Key(key) = event::read().map_err(UiError::Io)? {
                if key.kind == KeyEventKind::Press {
                    match handle_key(key, &mut control, &display) {
                        KeyResult::Quit => break,
                        KeyResult::Continue => {}
                        KeyResult::RunDiagnostics(cw_callsign) => {
                            control =
                                run_diagnostics_screen(radio, terminal, &display, cw_callsign)
                                    .await;
                        }
                        KeyResult::Execute(action) => {
                            let (desc, result) = execute_action(radio, action, &mut display).await;
                            control = match result {
                                // Empty extra text -> plain "OK: {desc}"; the
                                // read-type actions (§ execute_action doc
                                // comment) return their fetched value here
                                // instead, which takes priority when present —
                                // mirrors `ts570d::ui`'s own convention.
                                Ok(msg) if msg.is_empty() => ControlState::Feedback {
                                    message: format!("OK: {}", desc),
                                    is_error: false,
                                },
                                Ok(msg) => ControlState::Feedback {
                                    message: msg,
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
        /// Records every `set_ex_menu_item(p1, value)` call, in order — Wave
        /// 4 Task 8's round-trip test verifies against this, the same
        /// "record what was called" shape a `fail_all`-gated stub can't
        /// express on its own (unlike the plain-boolean-result methods
        /// above, the test needs the *arguments*, not just success/failure).
        ex_menu_calls: Vec<(u16, i32)>,
    }

    impl MockRadio {
        fn ok() -> Self {
            Self {
                vfo_a: Frequency::new(14_250_000),
                fail_all: false,
                ex_menu_calls: Vec::new(),
            }
        }

        fn failing() -> Self {
            Self {
                vfo_a: Err(radio::RadioError::NotImplemented),
                fail_all: true,
                ex_menu_calls: Vec::new(),
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

    // Wave 4 Task 2 (§11.3 point 3's last bullet): `run`'s bound widened to
    // require `Ft991aExtras`/`CwKeying` too, so every in-crate `MockRadio`
    // test double needs these — inheriting the traits' own `NotImplemented`
    // default bodies, exactly like `radio::NopRadio` does.
    #[async_trait(?Send)]
    impl Ft991aExtras for MockRadio {
        // Wave 4 Task 8 (§11.4, path (b)): overridden (not left at its
        // `NotImplemented` default) so the round-trip test can verify both
        // that `execute_action` reaches this call at all, and with exactly
        // the arguments the state machine produced — mirrors `CwKeying`'s
        // `assert_rts` override below (Wave 4 Task 4), the established
        // precedent for testing a *new* (not re-exposed) `Ft991aExtras`
        // capability this way.
        async fn set_ex_menu_item(&mut self, p1: u16, value: i32) -> RadioResult<()> {
            if self.fail_all {
                return Err(RadioError::NotImplemented);
            }
            self.ex_menu_calls.push((p1, value));
            Ok(())
        }

        // Overridden so the diagnostics-engine tests below can exercise the
        // `VM`/QMB/clarifier steps (which all read `get_information()`)
        // without every one of them falling back to `NotImplemented`.
        async fn get_information(&mut self) -> RadioResult<radio::ChannelStatusFields> {
            if self.fail_all {
                return Err(RadioError::NotImplemented);
            }
            Ok(radio::ChannelStatusFields {
                channel: 0,
                frequency_hz: self.vfo_a.as_ref().map(|f| f.hz()).unwrap_or(14_250_000),
                clarifier_offset_hz: 0,
                rx_clarifier_on: false,
                tx_clarifier_on: false,
                mode: Mode::Usb.as_u8(),
                select: 0,
                tone_status: 0,
                offset_type: 0,
            })
        }

        // Overridden so `test_run_diagnostics_task_ky_step_sends_when_callsign_supplied`
        // can verify the `KY` step actually attempts playback (not just that
        // it's skipped) — mirrors `set_ex_menu_item`'s own "override for a
        // specific test's sake" precedent above.
        async fn read_keyer_memory(&mut self, _channel: u8) -> RadioResult<String> {
            if self.fail_all {
                return Err(RadioError::NotImplemented);
            }
            Ok(String::new())
        }
        async fn write_keyer_memory(&mut self, _channel: u8, _message: &str) -> RadioResult<()> {
            if self.fail_all {
                return Err(RadioError::NotImplemented);
            }
            Ok(())
        }
        async fn play_keyer_memory(
            &mut self,
            _channel: u8,
            _mode: radio::KeyerPlaybackMode,
        ) -> RadioResult<()> {
            if self.fail_all {
                return Err(RadioError::NotImplemented);
            }
            Ok(())
        }
    }

    // Wave 4 Task 4 (§11.3 point 6): `assert_rts` is overridden (not left
    // at its `NotImplemented` default) so both the optimistic-set and
    // rollback-on-error paths of `execute_action`'s `ToggleRts` arm are
    // exercisable — `MockRadio::ok()`'s `fail_all: false` succeeds,
    // `MockRadio::failing()`'s `fail_all: true` fails, mirroring how every
    // other overridden `Radio` method above already branches on the same
    // flag.
    impl CwKeying for MockRadio {
        fn assert_rts(&self, _asserted: bool) -> RadioResult<()> {
            if self.fail_all {
                Err(RadioError::NotImplemented)
            } else {
                Ok(())
            }
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
        let mut display = Ft991aDisplay::default();
        let (desc, result) =
            execute_action(&mut radio, ExecuteAction::SetVfoA(14_300_000), &mut display).await;
        assert_eq!(desc, "VFO A set");
        assert!(result.is_ok());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_set_vfo_a_rejects_out_of_range() {
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let (_desc, result) = execute_action(
            &mut radio,
            ExecuteAction::SetVfoA(Frequency::MAX_HZ + 1),
            &mut display,
        )
        .await;
        assert!(result.is_err());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_tx_from_cat_keyed_sends_receive() {
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ToggleTx(TxState::CatKeyed),
            &mut display,
        )
        .await;
        assert_eq!(desc, "RX (CAT)");
        assert!(result.is_ok());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_tx_from_off_sends_transmit() {
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ToggleTx(TxState::Off),
            &mut display,
        )
        .await;
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
        let mut display = Ft991aDisplay::default();
        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ToggleTx(TxState::RadioKeyedNonCat),
            &mut display,
        )
        .await;
        assert_eq!(desc, "TX (CAT)");
        assert!(result.is_ok());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_power_on_flips_state() {
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let (desc, result) =
            execute_action(&mut radio, ExecuteAction::TogglePowerOn(true), &mut display).await;
        assert_eq!(desc, "Power toggled");
        assert!(result.is_ok());
    }

    // =========================================================================
    // Group 5 (KeyerCwBreakIn) — RTS CW-keying toggle (§11.3 point 6): the
    // optimistic-set and rollback-on-error paths.
    // =========================================================================

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_rts_success_sets_display_optimistically() {
        let mut radio = MockRadio::ok(); // fail_all: false -> assert_rts succeeds
        let mut display = Ft991aDisplay::default();
        assert!(!display.rts_asserted);

        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ToggleRts(false), // carried "currently not asserted"
            &mut display,
        )
        .await;

        assert_eq!(desc, "RTS CW key toggled");
        assert!(result.is_ok());
        assert!(
            display.rts_asserted,
            "successful assert_rts must leave the optimistic set in place"
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_rts_rollback_on_error() {
        let mut radio = MockRadio::failing(); // fail_all: true -> assert_rts errors
        let mut display = Ft991aDisplay::default();
        assert!(!display.rts_asserted);

        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ToggleRts(false), // carried "currently not asserted"
            &mut display,
        )
        .await;

        assert_eq!(desc, "RTS CW key toggled");
        assert!(result.is_err());
        assert!(
            !display.rts_asserted,
            "a failed assert_rts must roll the optimistic set back to its prior value"
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_toggle_rts_from_asserted_true_rolls_back_to_true_on_error() {
        // Same rollback path, but starting from `rts_asserted == true` (the
        // "turn it back off" direction) — confirms rollback restores the
        // *carried* prior value, not just `false`.
        let mut radio = MockRadio::failing();
        let mut display = Ft991aDisplay {
            rts_asserted: true,
            ..Ft991aDisplay::default()
        };

        let (_desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ToggleRts(true), // carried "currently asserted"
            &mut display,
        )
        .await;

        assert!(result.is_err());
        assert!(
            display.rts_asserted,
            "rollback must restore the carried prior value (true), not default to false"
        );
    }

    // --- Group 12 (`ExMenu`), path (b): number-entry escape hatch (§11.4,
    // Wave 4 Task 8) ---

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_set_ex_menu_item_dispatches_to_radio() {
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::SetExMenuItem(60, 2),
            &mut display,
        )
        .await;
        assert_eq!(desc, "EX menu item set");
        assert!(result.is_ok());
        assert_eq!(radio.ex_menu_calls, vec![(60, 2)]);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_set_ex_menu_item_propagates_error() {
        let mut radio = MockRadio::failing();
        let mut display = Ft991aDisplay::default();
        let (_desc, result) = execute_action(
            &mut radio,
            ExecuteAction::SetExMenuItem(60, 2),
            &mut display,
        )
        .await;
        assert!(result.is_err());
        assert!(radio.ex_menu_calls.is_empty());
    }

    /// Full round trip through the state machine (`control::handle_key`)
    /// and into the radio (`execute_action`) for an
    /// [`radio::ft991a_radio::ExMenuValueKind::Enumerated`] item: `[N]` ->
    /// type "060" -> `Enter` (looks up item 060 "PC KEYING", forks to
    /// `ListSelect` with labels `OFF`/`DAKY`/`RTS`/`DTR`) -> move the cursor
    /// to `RTS` -> `Enter` (confirm) -> `execute_action` -> verify
    /// `MockRadio::set_ex_menu_item` was called with `(60, 2)` (wire `"2"` =
    /// `RTS`, per `radio/src/ft991a_radio.rs`'s `EX_MENU_TABLE` row for
    /// P1=60).
    #[monoio::test(driver = "legacy")]
    async fn test_ex_menu_number_entry_round_trip_enumerated() {
        use crate::control::SelectAction;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        fn key(code: KeyCode) -> KeyEvent {
            KeyEvent::new(code, KeyModifiers::NONE)
        }

        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let mut control = ControlState::Menu;

        assert_eq!(
            handle_key(key(KeyCode::Char('N')), &mut control, &display),
            KeyResult::Continue
        );
        assert!(matches!(control, ControlState::ExNumberEntry { .. }));

        assert_eq!(
            handle_key(key(KeyCode::Char('6')), &mut control, &display),
            KeyResult::Continue
        );
        assert_eq!(
            handle_key(key(KeyCode::Char('0')), &mut control, &display),
            KeyResult::Continue
        );

        assert_eq!(
            handle_key(key(KeyCode::Enter), &mut control, &display),
            KeyResult::Continue
        );
        match &control {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(*action, SelectAction::SetExMenuItem(60));
                assert_eq!(
                    options,
                    &vec![
                        "OFF".to_string(),
                        "DAKY".to_string(),
                        "RTS".to_string(),
                        "DTR".to_string(),
                    ]
                );
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }

        // Move cursor from "OFF" (0) to "RTS" (2).
        handle_key(key(KeyCode::Right), &mut control, &display);
        handle_key(key(KeyCode::Right), &mut control, &display);

        let result = handle_key(key(KeyCode::Enter), &mut control, &display);
        let KeyResult::Execute(action) = result else {
            panic!("expected KeyResult::Execute, got {result:?}");
        };
        assert_eq!(action, ExecuteAction::SetExMenuItem(60, 2));

        let (desc, exec_result) = execute_action(&mut radio, action, &mut display).await;
        assert_eq!(desc, "EX menu item set");
        assert!(exec_result.is_ok());
        assert_eq!(radio.ex_menu_calls, vec![(60, 2)]);
    }

    /// Same round trip, [`radio::ft991a_radio::ExMenuValueKind::Range`]
    /// fork: `[N]` -> "001" -> `Enter` (item 001 "AGC FAST DELAY",
    /// `20..=4000` step `20`, forks to `TextInput`) -> type "100" -> `Enter`
    /// -> `execute_action` -> verify `(1, 100)`.
    #[monoio::test(driver = "legacy")]
    async fn test_ex_menu_number_entry_round_trip_range() {
        use crate::control::InputAction;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        fn key(code: KeyCode) -> KeyEvent {
            KeyEvent::new(code, KeyModifiers::NONE)
        }

        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let mut control = ControlState::Menu;

        handle_key(key(KeyCode::Char('N')), &mut control, &display);
        handle_key(key(KeyCode::Char('0')), &mut control, &display);
        handle_key(key(KeyCode::Char('0')), &mut control, &display);
        handle_key(key(KeyCode::Char('1')), &mut control, &display);
        handle_key(key(KeyCode::Enter), &mut control, &display);

        match &control {
            ControlState::TextInput { action, prompt, .. } => {
                assert_eq!(*action, InputAction::SetExMenuItem(1));
                assert!(prompt.contains("20..=4000"), "prompt: {prompt}");
            }
            other => panic!("expected TextInput, got {other:?}"),
        }

        for c in "100".chars() {
            handle_key(key(KeyCode::Char(c)), &mut control, &display);
        }
        let result = handle_key(key(KeyCode::Enter), &mut control, &display);
        let KeyResult::Execute(action) = result else {
            panic!("expected KeyResult::Execute, got {result:?}");
        };
        assert_eq!(action, ExecuteAction::SetExMenuItem(1, 100));

        let (desc, exec_result) = execute_action(&mut radio, action, &mut display).await;
        assert_eq!(desc, "EX menu item set");
        assert!(exec_result.is_ok());
        assert_eq!(radio.ex_menu_calls, vec![(1, 100)]);
    }

    // --- Profiles (§12.3) ---

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_apply_profile_dispatches_ex_menu_items() {
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let profile = radio::Profile {
            ex_menu: vec![(60, 2)],
            ..Default::default()
        };
        let (desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ApplyProfile("test".to_string(), profile),
            &mut display,
        )
        .await;
        assert_eq!(desc, "Profile applied");
        assert_eq!(result.unwrap(), "Applied profile 'test'");
        assert_eq!(radio.ex_menu_calls, vec![(60, 2)]);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_execute_action_apply_profile_propagates_radio_error() {
        // `MockRadio` leaves `set_mode` at its `Radio` trait default
        // (`NotImplemented`) — exercises `ApplyProfile`'s error path
        // (`ProfileError::Radio(e) => Err(e)`) without needing a dedicated
        // failing-mode fixture.
        let mut radio = MockRadio::ok();
        let mut display = Ft991aDisplay::default();
        let profile = radio::Profile {
            mode: Some(Mode::Usb),
            ..Default::default()
        };
        let (_desc, result) = execute_action(
            &mut radio,
            ExecuteAction::ApplyProfile("test".to_string(), profile),
            &mut display,
        )
        .await;
        assert!(matches!(result, Err(RadioError::NotImplemented)));
    }

    // -----------------------------------------------------------------------
    // Diagnostics engine (`docs/adr/0006-hand-coded-full-parity-
    // diagnostics.md`) — structural/safety-property tests. Full behavioral
    // verification (real set/verify/restore round trips against real state)
    // is done against the live `emulator`, not mocked here — see the ADR's
    // "Verified" section. `MockRadio` mostly falls back to `NotImplemented`
    // for the ~130 methods this engine calls, so these tests check
    // resilience/structure (no panics, correct step count, the CW-skip
    // contract, and a real snapshot/restore round trip on the one field
    // `MockRadio` backs with real state), not per-step pass/fail correctness.
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn test_diag_step_count_matches_actual_output() {
        let mut radio = MockRadio::ok();
        let summary = run_diagnostics_task(&mut radio, None, |_| {}).await;
        assert_eq!(summary.total(), DIAG_STEP_COUNT);
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn test_diag_runs_to_completion_even_when_almost_everything_fails() {
        // `MockRadio::failing()` returns `Err(NotImplemented)` for nearly
        // every method this engine calls — confirms no step panics or
        // aborts the run early.
        let mut radio = MockRadio::failing();
        let summary = run_diagnostics_task(&mut radio, None, |_| {}).await;
        assert_eq!(summary.total(), DIAG_STEP_COUNT);
        assert!(
            crate::diagnostics::count_failed(&summary.outcomes) > 0,
            "expected at least one Failure against an all-NotImplemented radio"
        );
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn test_diag_ky_step_skipped_without_callsign_but_run_still_completes() {
        let mut radio = MockRadio::ok();
        let summary = run_diagnostics_task(&mut radio, None, |_| {}).await;
        assert_eq!(summary.total(), DIAG_STEP_COUNT);
        let ky = summary
            .outcomes
            .iter()
            .find(|o| o.code == "KY")
            .expect("KY step must be present");
        assert!(
            matches!(ky.result, DiagResult::Skipped { .. }),
            "KY step should be Skipped when no callsign is supplied, got {:?}",
            ky.result
        );
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn test_diag_ky_step_attempts_playback_when_callsign_supplied() {
        let mut radio = MockRadio::ok();
        let summary = run_diagnostics_task(&mut radio, Some("W1AW".to_string()), |_| {}).await;
        let ky = summary
            .outcomes
            .iter()
            .find(|o| o.code == "KY")
            .expect("KY step must be present");
        assert!(
            ky.result.is_success(),
            "KY step should succeed against MockRadio::ok() with a supplied callsign, got {:?}",
            ky.result
        );
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn test_diag_progress_callback_invoked_once_per_step() {
        let mut radio = MockRadio::ok();
        let mut progress_calls = 0usize;
        let summary = run_diagnostics_task(&mut radio, None, |_| progress_calls += 1).await;
        assert_eq!(progress_calls, summary.total());
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn test_snapshot_restore_round_trips_vfo_a() {
        // `MockRadio::vfo_a` is real field-backed state (unlike almost every
        // other field, which is a stateless `fail_all`-gated stub) — the one
        // field this mock can prove `restore_state` actually writes back,
        // since the engine's own `FA` steps deliberately mutate it away
        // from its starting value.
        let mut radio = MockRadio::ok();
        let original = radio.get_vfo_a().await.unwrap();
        assert_ne!(original.hz(), 14_195_000, "test fixture sanity check");

        let snapshot = snapshot_state(&mut radio).await;
        assert_eq!(snapshot.vfo_a, Some(original));

        // Simulate what a diagnostic step does: mutate away from the
        // original.
        radio
            .set_vfo_a(Frequency::new(14_195_000).unwrap())
            .await
            .unwrap();
        assert_eq!(radio.get_vfo_a().await.unwrap().hz(), 14_195_000);

        restore_state(&mut radio, snapshot).await;
        assert_eq!(
            radio.get_vfo_a().await.unwrap(),
            original,
            "restore_state must put VFO-A back to its pre-run value"
        );
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn test_snapshot_state_stores_none_on_getter_failure_not_panic() {
        let mut radio = MockRadio::failing();
        let snapshot = snapshot_state(&mut radio).await;
        assert_eq!(snapshot.vfo_a, None);
        // Restoring an all-`None` snapshot must not panic even though every
        // setter also fails.
        restore_state(&mut radio, snapshot).await;
    }
}
