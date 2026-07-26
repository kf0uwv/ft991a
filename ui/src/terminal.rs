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
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use radio::{
    CwKeying, Frequency, Ft991aExtras, MemoryChannelEntry, MemoryTag, Radio, RadioError,
    RadioResult, TaggedMemoryChannel,
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    control::{handle_key, ControlState, ExecuteAction, KeyResult},
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

/// Draw one live-progress diagnostics frame (header/status/errors plus the
/// diagnostics panel itself) — factored out so it can be called both once
/// up front (0 outcomes yet) and from inside the [`run_diagnostics_with`]
/// progress callback below, without duplicating [`draw_frame`]'s own
/// header/status/errors setup.
///
/// [`run_diagnostics_with`]: radio::Ft991aExtras::run_diagnostics_with
fn draw_diagnostics_frame(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    display: &Ft991aDisplay,
    outcomes: &[radio::DiagnosticOutcome],
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

/// Run the shared diagnostics engine
/// (`docs/adr/0004-shared-diagnostics-screen.md`) against
/// [`radio::FT991A_COMMAND_TABLE`], redrawing the screen after every
/// command outcome for live progress, and return the [`ControlState`] to
/// transition to once it completes.
///
/// This is the one place in this crate that calls
/// [`Terminal::draw`] directly from inside a synchronous callback
/// ([`radio::Ft991aExtras::run_diagnostics_with`]'s `on_progress:
/// FnMut(&DiagnosticOutcome)` is deliberately **not** `async` — see that
/// trait method's own doc comment) rather than through the normal
/// once-per-loop-iteration [`draw_frame`] call. This works because
/// [`Terminal::draw`] is itself a plain synchronous function (ratatui does
/// no I/O awaiting of its own), so calling it from inside a sync closure
/// that a single `.await`ed diagnostics run invokes repeatedly is exactly
/// as safe as calling it from `run_loop`'s own synchronous match arms.
/// Blocks the whole event loop for the duration of the run (this crate's
/// existing single-sequential-loop architecture, per `terminal.rs`'s own
/// module docs, has no separate task to keep servicing key events
/// meanwhile) — acceptable here since the run itself is bounded (91
/// commands × up to `DEFAULT_COMMAND_TIMEOUT` = 2s each in the worst case
/// of every command failing to answer; in the common case of a live or
/// emulated radio, every non-`Skipped` command completes in well under a
/// second and the whole run is correspondingly fast).
async fn run_diagnostics_screen<R: Radio + Ft991aExtras + CwKeying>(
    radio: &mut R,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    display: &Ft991aDisplay,
) -> ControlState {
    let total = radio::FT991A_COMMAND_TABLE.definitions().len();
    let mut outcomes: Vec<radio::DiagnosticOutcome> = Vec::with_capacity(total);

    // Draw the initial "0/total" frame before the first probe goes out —
    // otherwise the screen would appear frozen on the prior `ControlState`
    // until the first outcome arrives.
    let _ = draw_diagnostics_frame(terminal, display, &outcomes, total);

    let result = radio
        .run_diagnostics_with(|outcome| {
            outcomes.push(outcome.clone());
            let _ = draw_diagnostics_frame(terminal, display, &outcomes, total);
        })
        .await;

    match result {
        Ok(summary) => ControlState::Diagnostics { summary, cursor: 0 },
        // `Ft991a<S>`'s real implementation never returns `Err` here (only
        // the trait's own `NotImplemented` default does, which this app's
        // concrete wiring never uses) — kept as a real branch, not
        // `unwrap()`ed away, since nothing enforces that structurally.
        Err(e) => ControlState::Feedback {
            message: format!("Error: {e}"),
            is_error: true,
        },
    }
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
            if let Event::Key(key) = event::read().map_err(UiError::Io)? {
                match handle_key(key, &mut control, &display) {
                    KeyResult::Quit => break,
                    KeyResult::Continue => {}
                    KeyResult::RunDiagnostics => {
                        control = run_diagnostics_screen(radio, terminal, &display).await;
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
}
