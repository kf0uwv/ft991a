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

//! Interactive control state machine for keyboard-driven radio commands.
//!
//! Grouped three-level design (`Menu` -> `GroupMenu` -> `{TextInput,
//! ListSelect}`), per `planning/architect/task_plan.md` §11.2/§11.6 item 2 —
//! reintroducing `ts570d/ui`'s `CommandGroup`/`GroupMenu` layer "in spirit"
//! now that this crate's command surface has grown well past the 9-command
//! flat screen Wave 2 shipped. **12 groups** (§11.2). As of Wave 4 dispatch
//! queue item 9 (§11.6, the wave's final task), all 12 groups are
//! populated, including group 12 (`ExMenu`), which is itself a two-path
//! area (§11.4): path (b) is the `[N]` number-entry escape hatch (Task 8,
//! `ControlState::ExNumberEntry`); path (a) is themed browsing (Task 9,
//! `ControlState::ExSubGroupMenu`) — 6 sub-groups reachable from `ExMenu`'s
//! own `GroupMenu` screen, each a genuinely scrollable list (`ex_theme_key`/
//! `ex_theme_items`/`ex_menu_commands`'s doc comments have the full design).
//! Both paths converge on the same value-entry fork
//! (`enter_ex_value_entry`) once an item is found, differing only in how
//! `Esc` routes back (`ExValueEntryOrigin`).

use crossterm::event::{KeyCode, KeyEvent};

use radio::ft991a_radio::ExMenuValueKind;
use radio::{
    ex_menu_item, AgcMode, Band, EncoderSelector, ExMenuItem, Frequency, MemoryTag, Meter, Mode,
    PreampMode, RadioIndicator, RepeaterShift, ScanState, ToneSquelchMode, TxState,
    CTCSS_TONES_DECIHZ, DCS_CODES, EX_MENU_TABLE, SH_BANDWIDTH_TABLE,
};

use crate::Ft991aDisplay;

// ---------------------------------------------------------------------------
// CommandGroup — the 12 top-level groups (§11.2)
// ---------------------------------------------------------------------------

/// Top-level command groups shown in the `Menu` screen.
///
/// Order and grouping mirror §11.2's table exactly (reusing the CAT batch
/// groupings' theme boundaries as UI group boundaries); only group 1 has
/// real content yet (see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGroup {
    FrequencyLevels,
    VfoMemoryQuickOps,
    MemoryChannels,
    ClarifierToneIfShift,
    KeyerCwBreakIn,
    ScanVoxBusy,
    AttenuatorNoiseAgcNotchFilter,
    SpeechMicMonitor,
    BandStepEncoder,
    MetersStatus,
    SystemTunerDvs,
    ExMenu,
}

/// All 12 groups, in §11.2's table order.
pub(crate) const ALL_GROUPS: [CommandGroup; 12] = [
    CommandGroup::FrequencyLevels,
    CommandGroup::VfoMemoryQuickOps,
    CommandGroup::MemoryChannels,
    CommandGroup::ClarifierToneIfShift,
    CommandGroup::KeyerCwBreakIn,
    CommandGroup::ScanVoxBusy,
    CommandGroup::AttenuatorNoiseAgcNotchFilter,
    CommandGroup::SpeechMicMonitor,
    CommandGroup::BandStepEncoder,
    CommandGroup::MetersStatus,
    CommandGroup::SystemTunerDvs,
    CommandGroup::ExMenu,
];

/// The `Menu`-level keybinding that opens each group.
///
/// Not specified by `planning/architect/task_plan.md` (only the group names
/// and order were) — a judgment call, chosen for mnemonic value where
/// possible and verified unique across all 12 groups plus `Q` (Quit).
/// `GroupMenu`-level command keys (e.g. group 1's `M` for "Set mode") live
/// in a different `ControlState` match arm, so reusing a letter here that a
/// group also uses internally (e.g. `M` is both `MemoryChannels`'s entry key
/// and `FrequencyLevels`'s internal "Set mode" key) is not a collision.
pub(crate) fn group_key(group: CommandGroup) -> char {
    match group {
        CommandGroup::FrequencyLevels => 'F',
        CommandGroup::VfoMemoryQuickOps => 'O',
        CommandGroup::MemoryChannels => 'M',
        CommandGroup::ClarifierToneIfShift => 'C',
        CommandGroup::KeyerCwBreakIn => 'K',
        // 'S' is reserved for SystemTunerDvs below.
        CommandGroup::ScanVoxBusy => 'X',
        CommandGroup::AttenuatorNoiseAgcNotchFilter => 'A',
        CommandGroup::SpeechMicMonitor => 'P',
        CommandGroup::BandStepEncoder => 'B',
        CommandGroup::MetersStatus => 'T',
        CommandGroup::SystemTunerDvs => 'S',
        CommandGroup::ExMenu => 'E',
    }
}

/// The human-readable group name shown in the `Menu`/`GroupMenu` screens,
/// per §11.2's table.
pub(crate) fn group_label(group: CommandGroup) -> &'static str {
    match group {
        CommandGroup::FrequencyLevels => "Frequency & Levels",
        CommandGroup::VfoMemoryQuickOps => "VFO / Memory Quick-Ops",
        CommandGroup::MemoryChannels => "Memory Channels",
        CommandGroup::ClarifierToneIfShift => "Clarifier / Tone / IF-Shift",
        CommandGroup::KeyerCwBreakIn => "Keyer / CW / Break-In",
        CommandGroup::ScanVoxBusy => "Scan / VOX / Busy",
        CommandGroup::AttenuatorNoiseAgcNotchFilter => "Attenuator / Noise / AGC / Notch / Filter",
        CommandGroup::SpeechMicMonitor => "Speech / Mic / Monitor",
        CommandGroup::BandStepEncoder => "Band / Step / Encoder",
        CommandGroup::MetersStatus => "Meters / Status",
        CommandGroup::SystemTunerDvs => "System / Tuner / DVS",
        CommandGroup::ExMenu => "EX Menu",
    }
}

/// Look up the group whose `group_key` matches `key` (case-insensitive).
fn group_for_key(key: char) -> Option<CommandGroup> {
    let key = key.to_ascii_uppercase();
    ALL_GROUPS.into_iter().find(|&g| group_key(g) == key)
}

/// The `Menu`-level keybinding for the `EX` menu number-entry escape hatch
/// (`planning/architect/task_plan.md` §11.4, path (b)) — always visible from
/// `Menu`, independent of path (a)'s themed browsing sub-groups (Wave 4
/// Task 9, landed) — same key also folded into `ExMenu`'s own `GroupMenu`
/// command list for discoverability (see `ex_menu_commands`'s doc comment).
///
/// **Deviates from §11.4's own literal `'[X]'` notation.** `'X'` is already
/// `CommandGroup::ScanVoxBusy`'s top-level [`group_key`] (assigned by this
/// crate's own Wave 4 Task 2, a judgment call the architect's plan text
/// couldn't have anticipated since single-letter key assignments were
/// explicitly left to the `ui` agent — see [`group_key`]'s own doc comment).
/// `'N'` (mnemonic: "Number entry") is the chosen replacement, verified
/// unique against every [`ALL_GROUPS`] key plus `Q` by
/// `test_ex_number_entry_key_is_unique` below. Flagged as a discrepancy in
/// this task's final report, not silently substituted.
pub(crate) const EX_NUMBER_ENTRY_KEY: char = 'N';

/// The `Menu`-level keybinding for the profile list (`planning/architect/
/// task_plan.md` §12.3) — a cross-cutting bulk-apply action, not a
/// `CommandGroup`, so it lives alongside [`EX_NUMBER_ENTRY_KEY`] as its own
/// top-level escape hatch rather than as a 13th group. `'L'` (mnemonic:
/// "Load profile") — verified unique against every [`ALL_GROUPS`] key plus
/// `Q`/[`EX_NUMBER_ENTRY_KEY`] by `test_profile_list_key_is_unique`.
pub(crate) const PROFILE_LIST_KEY: char = 'L';

/// Return the `(key, label)` pairs for rendering the `Menu` screen's group
/// list. Does not include the fixed `[Q] Quit` entry — see
/// `layout::draw_control_panel`.
pub(crate) fn menu_group_labels() -> Vec<(char, &'static str)> {
    ALL_GROUPS
        .iter()
        .map(|&g| (group_key(g), group_label(g)))
        .collect()
}

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

/// What radio action to perform when text input is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    SetVfoA,
    SetVfoB,
    SetAfGain,
    SetRfGain,
    SetSquelch,
    SetPower,
    // --- Group 3 (`MemoryChannels`): `MC`/`MR`/`MW`/`MT` ---
    SelectMemoryChannel,
    ReadMemoryChannel,
    WriteMemoryChannelFromVfoA,
    ReadMemoryChannelTag,
    WriteMemoryChannelTagFromVfoA,
    // --- Group 4 (`ClarifierToneIfShift`): `RD`/`RU`/`IS` ---
    ClarifierDown,
    ClarifierUp,
    SetIfShift,
    // --- Group 6 (`ScanVoxBusy`): `VG`/`VD` ---
    SetVoxGain,
    SetVoxDelay,
    // --- Group 5 (`KeyerCwBreakIn`): `SD`/`KS`/`KP`/`KM`/`KY` ---
    SetSemiBreakInDelay,
    SetKeyerSpeed,
    SetKeyerPitchHz,
    ReadKeyerMemory,
    WriteKeyerMemory,
    PlayKeyerMemory,
    PlayMessageKeyer,
    // --- Group 9 (`BandStepEncoder`): `ED`/`EU` ---
    EncoderDown,
    EncoderUp,
    // --- Group 7 (`AttenuatorNoiseAgcNotchFilter`): `NL`/`RL`/`CO`(freq)/`BP`(freq) ---
    SetNoiseBlankerLevel,
    SetNoiseReductionLevel,
    SetContourFrequencyHz,
    SetApfFrequencyHz,
    SetManualNotchFrequencyHz,
    // --- Group 8 (`SpeechMicMonitor`): `MG`/`PL`/`ML`(level) ---
    SetMicGain,
    SetSpeechProcessorLevel,
    SetMonitorLevel,
    // --- Group 11 (`SystemTunerDvs`): `DA`/`DT`(date/time/tz)/`LM`/`PB` ---
    SetDimmer,
    SetDate,
    SetTime,
    SetTimeZoneOffset,
    StartDvsRecording,
    StartDvsPlayback,
    // --- Group 12 (`ExMenu`), path (b): number-entry escape hatch (§11.4) ---
    /// Write an `EX` menu item whose [`ExMenuValueKind::Range`] kind was
    /// looked up by [`ControlState::ExNumberEntry`]. Carries the item's own
    /// `p1` so `validate_text_input` can re-look-up its `min`/`max`/`step`
    /// via [`ex_menu_item`] without widening [`ControlState::TextInput`]'s
    /// shared shape (used by ~30 other `InputAction`s) with a
    /// `&'static ExMenuItem` field.
    SetExMenuItem(u16),
    /// Path (a) counterpart of [`Self::SetExMenuItem`] (§11.4, Wave 4 Task
    /// 9) — identical value semantics (`validate_text_input` matches both
    /// together via `|`, sharing one arm), but also carries the originating
    /// [`ExTheme`] sub-group and cursor position, so `Esc` can restore the
    /// browsing list exactly where the user left it instead of
    /// `ExNumberEntry` (§11.4's own pseudocode gives value-entry's `Esc`
    /// two different targets, one per path — see [`ExValueEntryOrigin`]).
    SetExMenuItemFromTheme(u16, ExTheme, usize),
}

/// What radio action to perform when a list selection is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectAction {
    SetMode,
    // --- Group 4 (`ClarifierToneIfShift`): `RT`/`XT`/`CT`/`CN` ---
    // (`Toggle*` naming for the two booleans, not `Set*`, so this enum's
    // variants don't all share one prefix — `cargo clippy`'s
    // `enum_variant_names` lint flags that; matches `ts570d::ui`'s own
    // `Toggle`-vs-`Set` naming split for the same reason.)
    ToggleRxClarifier,
    ToggleTxClarifier,
    SetToneSquelchMode,
    SetCtcssTone,
    SetDcsCode,
    // --- Group 6 (`ScanVoxBusy`): `SC`/`VX` ---
    SetScanState,
    ToggleVox,
    // --- Group 5 (`KeyerCwBreakIn`): `BI`/`CS`/`KR` ---
    ToggleBreakIn,
    ToggleCwSpot,
    ToggleKeyerEnabled,
    // --- Group 9 (`BandStepEncoder`): `BS`/`FS` ---
    SetBand,
    ToggleFineStep,
    // --- Group 7 (`AttenuatorNoiseAgcNotchFilter`): `RA`/`PA`/`NB`/`NR`/
    // `GT`/`BC`/`NA`/`SH`/`CO`(on)/`CO`(APF on)/`BP`(on) ---
    ToggleAttenuator,
    SetPreampMode,
    ToggleNoiseBlanker,
    ToggleNoiseReduction,
    SetAgcMode,
    ToggleAutoNotch,
    ToggleNarrow,
    SetFilterWidthIndex,
    ToggleContour,
    ToggleApf,
    ToggleManualNotch,
    // --- Group 8 (`SpeechMicMonitor`): `PR`(on)/`ML`(on)/`PR`(P1=1, on) ---
    ToggleSpeechProcessor,
    ToggleMonitor,
    ToggleParametricMicEq,
    // --- Group 10 (`MetersStatus`): `MS`(write)/`RM`(direct)/`RI` ---
    SelectMeter,
    ReadMeterDirect,
    ReadRadioIndicator,
    // --- Group 11 (`SystemTunerDvs`): `AI`/`LK`/`OS`/`FT`/`MX`/`AC`/`TS` ---
    ToggleAutoInfo,
    ToggleFrequencyLock,
    SetRepeaterShift,
    SetTxVfo,
    ToggleMox,
    SetAntennaTunerState,
    ToggleTxw,
    // --- Group 12 (`ExMenu`), path (b): number-entry escape hatch (§11.4) ---
    /// `Enumerated`-kind counterpart to [`InputAction::SetExMenuItem`] — same
    /// "carry `p1`, re-look-up the item at confirm time" rationale.
    SetExMenuItem(u16),
    /// Path (a) counterpart of [`Self::SetExMenuItem`] — see
    /// [`InputAction::SetExMenuItemFromTheme`]'s doc comment (same
    /// rationale, `ListSelect`-side).
    SetExMenuItemFromTheme(u16, ExTheme, usize),
}

/// A validated radio command ready to execute.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecuteAction {
    SetVfoA(u64),
    SetVfoB(u64),
    SetMode(Mode),
    /// Toggle CAT TX/RX. Carries the *last polled* [`TxState`] so the
    /// executor knows whether to send `transmit()` or `receive()` — see
    /// §6.5's 3-valued `TxState` handling.
    ToggleTx(TxState),
    SetAfGain(u8),
    SetRfGain(u8),
    SetSquelch(u8),
    SetPower(u8),
    TogglePowerOn(bool),
    // --- Group 3 (`MemoryChannels`) ---
    SelectMemoryChannel(u8),
    /// Query the currently selected memory channel (`MC` read) — no input
    /// needed, so this is `Immediate` rather than `Text`-backed.
    GetMemoryChannel,
    ReadMemoryChannel(u8),
    /// Write a memory channel's frequency/mode from the currently tuned
    /// VFO A (`MW`) — the executor (`terminal.rs`) re-queries VFO A/mode
    /// fresh at execution time (mirroring `ts570d::ui`'s own
    /// `WriteMemoryChannelFromVfoA` precedent), not a display snapshot
    /// captured when the key was pressed. Clarifier offset/on and tone
    /// status/offset-type — the entry's other fields — are written as
    /// their documented defaults (`0`/`false`); this group has no
    /// dedicated UI input for them yet, a judgment call flagged in the
    /// final report.
    WriteMemoryChannelFromVfoA(u8),
    /// Read a memory channel's contents plus its tag (`MT` read) — a
    /// superset of [`Self::ReadMemoryChannel`]'s plain `MR` read.
    ReadMemoryChannelTag(u8),
    /// Write a memory channel's frequency/mode (from VFO A, see
    /// [`Self::WriteMemoryChannelFromVfoA`]'s doc comment) plus a tag
    /// (`MT` write). The `String` is the already-validated tag text (see
    /// `validate_text_input`'s `WriteMemoryChannelTagFromVfoA` arm).
    WriteMemoryChannelTagFromVfoA(u8, String),
    // --- Group 4 (`ClarifierToneIfShift`) ---
    SetRxClarifierOn(bool),
    SetTxClarifierOn(bool),
    ClarifierClear,
    ClarifierDown(u16),
    ClarifierUp(u16),
    SetIfShift(i16),
    SetToneSquelchMode(ToneSquelchMode),
    SetCtcssTone(f32),
    SetDcsCode(u16),
    // --- Group 6 (`ScanVoxBusy`) ---
    SetScanState(ScanState),
    SetVoxOn(bool),
    SetVoxGain(u8),
    SetVoxDelay(u16),
    // --- Group 5 (`KeyerCwBreakIn`) ---
    /// Toggle the real-time RTS CW-keying line (§11.3 point 6). Carries the
    /// *current* [`Ft991aDisplay::rts_asserted`] value (mirrors
    /// [`Self::ToggleTx`]/[`Self::TogglePowerOn`]'s "carry the prior known
    /// state" shape) — the executor (`terminal.rs`) flips it, asserts the
    /// new value optimistically in the display, calls
    /// [`radio::CwKeying::assert_rts`], and rolls the display field back to
    /// this carried value if that call errors.
    ToggleRts(bool),
    SetBreakInOn(bool),
    SetSemiBreakInDelay(u16),
    SetCwSpotOn(bool),
    SetKeyerEnabled(bool),
    SetKeyerSpeed(u8),
    SetKeyerPitchHz(u16),
    /// `ZI` — zero-width, write-only trigger, no persisted state.
    ZeroIn,
    /// Read one `KM` keyer memory channel's stored message
    /// ([`radio::Ft991aExtras::read_keyer_memory`]).
    ReadKeyerMemory(u8),
    /// Write one `KM` keyer memory channel's message
    /// ([`radio::Ft991aExtras::write_keyer_memory`]) — the `String` is the
    /// already-validated message text (see `validate_text_input`'s
    /// `WriteKeyerMemory` arm, which parses the same "channel:text" format
    /// [`Self::WriteMemoryChannelTagFromVfoA`] established).
    WriteKeyerMemory(u8, String),
    /// Trigger `KY` playback of a stored `KM` channel in "Keyer Memory"
    /// mode ([`radio::KeyerPlaybackMode::KeyerMemory`]) — distinct from
    /// [`Self::ToggleRts`]'s real-time keying (see
    /// [`radio::Ft991aExtras::play_keyer_memory`]'s doc comment).
    PlayKeyerMemory(u8),
    /// Trigger `KY` playback of a stored `KM` channel in "Message Keyer"
    /// mode ([`radio::KeyerPlaybackMode::MessageKeyer`]).
    PlayMessageKeyer(u8),
    // --- Group 2 (`VfoMemoryQuickOps`) ---
    //
    // All 11 of this group's commands are zero-argument, write-only wire
    // triggers (`AB BA SV AM MA CH0 CH1 VM QI QR QS`) — same "no persisted
    // state to snapshot" shape as [`Self::ClarifierClear`]/[`Self::ZeroIn`],
    // so every one of them is `CommandKind::Immediate` in
    // `vfo_memory_quick_ops_commands` below, not `Text`/`List`.
    /// Copy VFO-A's frequency into VFO-B (`AB`, [`radio::Radio::copy_vfo_a_to_b`]).
    CopyVfoAToB,
    /// Copy VFO-B's frequency into VFO-A (`BA`, [`radio::Radio::copy_vfo_b_to_a`]).
    CopyVfoBToA,
    /// Swap VFO-A's and VFO-B's frequencies (`SV`, [`radio::Radio::swap_vfos`]).
    SwapVfos,
    /// Store VFO-A into the currently selected memory channel (`AM`,
    /// [`radio::Radio::store_vfo_to_memory`]). Selection is set via group
    /// 3's `C` (Select memory channel) key.
    StoreVfoToMemory,
    /// Recall the currently selected memory channel into VFO-A (`MA`,
    /// [`radio::Radio::recall_memory_to_vfo`]).
    RecallMemoryToVfo,
    /// Step the selected memory channel up (`CH0`, [`radio::Radio::memory_channel_up`]).
    MemoryChannelUp,
    /// Step the selected memory channel down (`CH1`, [`radio::Radio::memory_channel_down`]).
    MemoryChannelDown,
    /// Emulate the front-panel `[V/M]` key (`VM`,
    /// [`radio::Ft991aExtras::toggle_vfo_memory_mode`]) — **judgment call,
    /// not manual-proven**, see that method's own doc comment.
    ToggleVfoMemoryMode,
    /// Store VFO-A into the dedicated Quick Memory Bank slot (`QI`,
    /// [`radio::Ft991aExtras::qmb_store`]) — distinct from the 117 numbered
    /// memory channels group 3 already covers.
    QmbStore,
    /// Recall the Quick Memory Bank slot into VFO-A (`QR`,
    /// [`radio::Ft991aExtras::qmb_recall`]).
    QmbRecall,
    /// Toggle Quick Split (`QS`, [`radio::Ft991aExtras::quick_split`]) —
    /// **judgment call**, see that method's own doc comment.
    QuickSplit,
    // --- Group 9 (`BandStepEncoder`) ---
    /// Select a band (`BS`, [`radio::Radio::set_band`]) — write-only on the
    /// wire, no paired "get band" exists at all (manual: no `Read`/`Answer`
    /// form for `BS`), so [`initial_list_cursor`] can never pre-select the
    /// live band the way [`Self::SetMode`]'s list does.
    SetBand(Band),
    /// Step to the next band up (`BU`, [`radio::Radio::band_up`]).
    BandUp,
    /// Step to the next band down (`BD`, [`radio::Radio::band_down`]).
    BandDown,
    /// Enable/disable the VFO-A "FAST" step key (`FS`,
    /// [`radio::Radio::set_fine_step`]).
    SetFineStep(bool),
    /// Emulate a press of the hand mic's "UP" button (`UP`,
    /// [`radio::Radio::mic_up`]) — steps VFO-A up by a fixed amount on the
    /// real radio/emulator.
    MicUp,
    /// Emulate a press of the hand mic's "DWN" button (`DN`,
    /// [`radio::Radio::mic_down`]).
    MicDown,
    /// Step the specified front-panel encoder down (`ED`,
    /// [`radio::Ft991aExtras::encoder_down`]; `steps` `1`-`99`). See
    /// `band_step_encoder_commands`'s doc comment for the honest disclosure
    /// of what this key does and does not do.
    EncoderDown(EncoderSelector, u8),
    /// Step the specified front-panel encoder up (`EU`,
    /// [`radio::Ft991aExtras::encoder_up`]).
    EncoderUp(EncoderSelector, u8),
    /// Emulate a press of the front-panel ENT key (`EK`,
    /// [`radio::Ft991aExtras::ent_key`]) — zero-width action trigger, same
    /// "no persisted state" category as [`Self::ZeroIn`]/[`Self::ClarifierClear`].
    EntKey,
    // --- Group 7 (`AttenuatorNoiseAgcNotchFilter`) ---
    //
    // 10 of these 16 are plain `radio::Radio` methods (`RA PA NB NL NR RL GT
    // BC NA SH`); the other 6 (`CO`'s contour/APF sub-fields and `BP`'s
    // manual notch) are `radio::Ft991aExtras`-only — see
    // `attenuator_noise_agc_notch_filter_commands`'s doc comment for the
    // full confirmed trait-mix citation. Every `CO`/`BP` P2-selected
    // sub-field already has its own dedicated `get`/`set` method pair on
    // `Ft991aExtras` (`get_contour_on`/`get_contour_frequency_hz`/
    // `get_apf_on`/`get_apf_frequency_hz`/`get_manual_notch_on`/
    // `get_manual_notch_frequency_hz`) — the wire-level P2 selector is
    // already resolved at the `radio`-crate client boundary, so none of
    // these needs any new two-step `ControlState` machinery; each is a
    // plain independent boolean/numeric control, the same shape
    // [`Self::SetNoiseReductionOn`]/[`Self::SetNoiseReductionLevel`]'s
    // `NR`/`RL` split already established.
    /// Enable/disable the RF attenuator (`RA`, [`radio::Radio::set_attenuator_on`]).
    SetAttenuatorOn(bool),
    /// Set the pre-amp/IPO mode (`PA`, [`radio::Radio::set_preamp_mode`]).
    SetPreampMode(PreampMode),
    /// Enable/disable the noise blanker (`NB`, [`radio::Radio::set_noise_blanker_on`]).
    SetNoiseBlankerOn(bool),
    /// Set the noise blanker level, `0`-`10` (`NL`,
    /// [`radio::Radio::set_noise_blanker_level`]).
    SetNoiseBlankerLevel(u8),
    /// Enable/disable noise reduction (`NR`, [`radio::Radio::set_noise_reduction_on`]).
    SetNoiseReductionOn(bool),
    /// Set the noise reduction level, `1`-`15` (`RL`,
    /// [`radio::Radio::set_noise_reduction_level`]).
    SetNoiseReductionLevel(u8),
    /// Set the AGC mode (`GT`, [`radio::Radio::set_agc_mode`]).
    SetAgcMode(AgcMode),
    /// Enable/disable the auto notch filter (`BC`, [`radio::Radio::set_auto_notch_on`]).
    SetAutoNotchOn(bool),
    /// Enable/disable the narrow filter (`NA`, [`radio::Radio::set_narrow_on`]).
    SetNarrowOn(bool),
    /// Set the raw filter width table index, `0`-`21` (`SH`,
    /// [`radio::Radio::set_filter_width_index`]). See
    /// `filter_width_options`'s doc comment for why the list labels are
    /// derived from the static [`radio::SH_BANDWIDTH_TABLE`] rather than the
    /// live radio state.
    SetFilterWidthIndex(u8),
    /// Enable/disable CONTOUR (`CO` `P2=0`, [`radio::Ft991aExtras::set_contour_on`]).
    SetContourOn(bool),
    /// Set the CONTOUR frequency, Hz, `10`-`3200` (`CO` `P2=1`,
    /// [`radio::Ft991aExtras::set_contour_frequency_hz`]).
    SetContourFrequencyHz(u16),
    /// Enable/disable APF (`CO` `P2=2`, [`radio::Ft991aExtras::set_apf_on`]).
    SetApfOn(bool),
    /// Set the APF frequency, Hz, `-250`..=`250` in 10 Hz steps (`CO`
    /// `P2=3`, [`radio::Ft991aExtras::set_apf_frequency_hz`]).
    SetApfFrequencyHz(i16),
    /// Enable/disable the manual notch (`BP` `P2=0`,
    /// [`radio::Ft991aExtras::set_manual_notch_on`]).
    SetManualNotchOn(bool),
    /// Set the manual notch frequency, Hz, `10`-`3200` in 10 Hz steps (`BP`
    /// `P2=1`, [`radio::Ft991aExtras::set_manual_notch_frequency_hz`]).
    SetManualNotchFrequencyHz(u16),
    // --- Group 8 (`SpeechMicMonitor`) ---
    //
    // 5 of these 6 are plain `radio::Radio` methods (`MG PL PR ML`); the
    // parametric mic EQ (`PR` `P1=1`) is `radio::Ft991aExtras`-only — see
    // `speech_mic_monitor_commands`'s doc comment.
    /// Set the microphone gain, `0`-`100` (`MG`, [`radio::Radio::set_mic_gain`]).
    SetMicGain(u8),
    /// Set the speech processor level, `0`-`100` (`PL`,
    /// [`radio::Radio::set_speech_processor_level`]).
    SetSpeechProcessorLevel(u8),
    /// Enable/disable the speech processor (`PR` `P1=0`,
    /// [`radio::Radio::set_speech_processor_on`]).
    SetSpeechProcessorOn(bool),
    /// Enable/disable the audio monitor (`ML` `P1=0`, [`radio::Radio::set_monitor_on`]).
    SetMonitorOn(bool),
    /// Set the audio monitor level, `0`-`100` (`ML` `P1=1`,
    /// [`radio::Radio::set_monitor_level`]).
    SetMonitorLevel(u8),
    /// Enable/disable the Parametric Microphone Equalizer (`PR` `P1=1`,
    /// [`radio::Ft991aExtras::set_parametric_mic_eq_on`]).
    SetParametricMicEqOn(bool),
    // --- Group 10 (`MetersStatus`) ---
    //
    // `select_meter`/`get_selected_meter`/`get_meter` are plain
    // [`radio::Radio`] methods; `get_active_meter_reading`/
    // `get_radio_indicator`/`get_menu_mode_active`/`get_pll_unlocked`/
    // `get_information` are [`radio::Ft991aExtras`]-only — see
    // `meters_status_commands`'s doc comment for the full confirmed
    // trait-mix citation.
    /// Select which physical meter `MS` subsequently reports (`MS` write,
    /// [`radio::Radio::select_meter`]).
    SelectMeter(Meter),
    /// Query which physical meter `MS` currently has selected (`MS` read,
    /// [`radio::Radio::get_selected_meter`]) — no input needed.
    GetSelectedMeter,
    /// Directly read one named meter (`RM`'s direct-select values,
    /// [`radio::Radio::get_meter`]), independent of `MS`'s current
    /// selection.
    ReadMeterDirect(Meter),
    /// Read whatever meter the front panel is currently showing (`RM`
    /// `P1=0`, [`radio::Ft991aExtras::get_active_meter_reading`]) — no
    /// input needed.
    GetActiveMeterReading,
    /// Query the composite VFO/memory-channel status payload (`IF`,
    /// [`radio::Ft991aExtras::get_information`]) — no input needed.
    GetInformation,
    /// Query one status-flag indicator (`RI`,
    /// [`radio::Ft991aExtras::get_radio_indicator`]).
    GetRadioIndicator(RadioIndicator),
    /// Query whether the radio is currently in MENU MODE (`RS`,
    /// [`radio::Ft991aExtras::get_menu_mode_active`]) — no input needed.
    GetMenuModeActive,
    /// Query whether the PLL is unlocked (`UL`,
    /// [`radio::Ft991aExtras::get_pll_unlocked`]) — no input needed.
    GetPllUnlocked,
    // --- Group 11 (`SystemTunerDvs`) ---
    //
    // `AI`/`LK`/`OS`/`FT`/`MX` are plain [`radio::Radio`] methods; `AC`/
    // `DA`/`DT`/`OI`/`TS`/`LM`/`PB` are [`radio::Ft991aExtras`]-only — see
    // `system_tuner_dvs_commands`'s doc comment for the full confirmed
    // trait-mix citation.
    /// Enable/disable auto-information broadcast (`AI`,
    /// [`radio::Radio::set_auto_info_on`]).
    SetAutoInfoOn(bool),
    /// Enable/disable the VFO-A dial lock (`LK`,
    /// [`radio::Radio::set_frequency_lock`]).
    SetFrequencyLock(bool),
    /// Set the FM repeater shift direction (`OS`,
    /// [`radio::Radio::set_repeater_shift`]).
    SetRepeaterShift(RepeaterShift),
    /// Select which VFO/band is the TX band (`FT`, `0`=VFO-A, `1`=VFO-B,
    /// [`radio::Radio::set_tx_vfo`]).
    SetTxVfo(u8),
    /// Enable/disable MOX (`MX`, [`radio::Radio::set_mox_on`]).
    SetMoxOn(bool),
    /// Set the antenna tuner state (`AC`, `0`=OFF/`1`=ON/`2`=Tuning
    /// Start-Stop, [`radio::Ft991aExtras::set_antenna_tuner_state`]).
    SetAntennaTunerState(u8),
    /// Set the dimmer levels (`DA`, LED `1`-`2`/TFT `0`-`15`,
    /// [`radio::Ft991aExtras::set_dimmer`]). `(led, tft)`.
    SetDimmer(u8, u8),
    /// Query the dimmer levels (`DA` read,
    /// [`radio::Ft991aExtras::get_dimmer`]) — no input needed.
    GetDimmer,
    /// Write the date (`DT` `P1=0`, [`radio::Ft991aExtras::write_date`]).
    /// `(year, month, day)`.
    SetDate(u16, u8, u8),
    /// Read the current date (`DT` `P1=0` read,
    /// [`radio::Ft991aExtras::read_date`]) — no input needed.
    ReadDate,
    /// Write the time (`DT` `P1=1`, [`radio::Ft991aExtras::write_time`]).
    /// `(hour, minute, second)`, 24-hour UTC.
    SetTime(u8, u8, u8),
    /// Read the current time (`DT` `P1=1` read,
    /// [`radio::Ft991aExtras::read_time`]) — no input needed.
    ReadTime,
    /// Write the time zone offset, minutes
    /// ([`radio::Ft991aExtras::write_time_zone_offset`]).
    SetTimeZoneOffset(i16),
    /// Read the current time zone offset, minutes (`DT` `P1=2` read,
    /// [`radio::Ft991aExtras::read_time_zone_offset`]) — no input needed.
    ReadTimeZoneOffset,
    /// Query the composite opposite-band (VFO-B) status payload (`OI`,
    /// read-only, [`radio::Ft991aExtras::get_opposite_band_information`]) —
    /// no input needed.
    GetOppositeBandInformation,
    /// Enable/disable "TXW" (`TS`, [`radio::Ft991aExtras::set_txw_on`]).
    SetTxwOn(bool),
    /// Start (or toggle-stop) DVS recording on the given channel (`LM`,
    /// `1`-`5`, [`radio::Ft991aExtras::start_dvs_recording`]).
    StartDvsRecording(u8),
    /// Stop DVS recording (`LM` `P2=0`,
    /// [`radio::Ft991aExtras::stop_dvs_recording`]) — no input needed.
    StopDvsRecording,
    /// Query the DVS recording state (`LM` read,
    /// [`radio::Ft991aExtras::get_dvs_recording_channel`]) — no input
    /// needed.
    GetDvsRecordingChannel,
    /// Start DVS playback on the given channel (`PB`, `1`-`5`,
    /// [`radio::Ft991aExtras::start_dvs_playback`]).
    StartDvsPlayback(u8),
    /// Stop DVS playback (`PB` `P2=0`,
    /// [`radio::Ft991aExtras::stop_dvs_playback`]) — no input needed.
    StopDvsPlayback,
    /// Query the DVS playback state (`PB` read,
    /// [`radio::Ft991aExtras::get_dvs_playback_channel`]) — no input
    /// needed.
    GetDvsPlaybackChannel,
    // --- Group 12 (`ExMenu`), path (b): number-entry escape hatch (§11.4) ---
    /// Write an arbitrary `EX` menu item's raw integer value by its `p1`
    /// menu number ([`radio::Ft991aExtras::set_ex_menu_item`]). Produced by
    /// either fork of [`ControlState::ExNumberEntry`]'s value-entry step —
    /// `TextInput`'s `Enter` (via [`InputAction::SetExMenuItem`]) for
    /// [`ExMenuValueKind::Range`] items, or `ListSelect`'s `Enter` (via
    /// [`SelectAction::SetExMenuItem`]) for
    /// [`ExMenuValueKind::Enumerated`] items.
    SetExMenuItem(u16, i32),
    // --- Profiles (`planning/architect/task_plan.md` §12.3) ---
    /// Apply a named settings profile ([`radio::Profile::apply`]) —
    /// produced by [`ControlState::ProfileList`]'s `Enter`. Carries the
    /// already-resolved name + [`radio::Profile`] value (not an index into
    /// the list) because `handle_key` transitions to `Feedback` in the same
    /// match arm that produces this action, the same shape every other
    /// group's `Immediate` command follows — the profile list itself is not
    /// reachable anymore once that transition happens.
    ApplyProfile(String, radio::Profile),
}

/// The grouped interactive control panel state machine.
///
/// `Menu` (top-level group list) -> `GroupMenu` (commands within one group)
/// -> `{TextInput, ListSelect}` -> `Feedback` (which always returns to
/// `Menu`, not back into the originating group — mirrors `ts570d`'s own
/// `Feedback`/`Esc` idiom exactly). No `Diagnostic` variant — still not
/// warranted, unchanged from Wave 2's §6.1 call (revisit only if a future
/// wave adds a self-test harness).
#[derive(Debug, Default, Clone, PartialEq)]
pub enum ControlState {
    /// Showing the top-level list of 12 command groups.
    #[default]
    Menu,
    /// Showing the commands within one group.
    ///
    /// `cursor` mirrors `ts570d`'s `GroupMenu{group,cursor}` shape but
    /// remains **unwired** to any key handling, matching the actual
    /// `ts570d` reference code today (its own `cursor` field is likewise
    /// initialized once and never read or mutated; see
    /// `planning/ui/task_plan.md`'s Wave 4 Task 2 section for the source
    /// citation) — every group's own `GroupMenu` command list, including
    /// `ExMenu`'s (7 entries: 6 themed sub-groups + the number-entry
    /// escape hatch), stays small enough for single-char keys, so no
    /// group-level list needs scrolling. [`ExSubGroupMenu`](Self::ExSubGroupMenu)
    /// is the state that *does* need and get genuine cursor-driven
    /// scrolling (§11.4 path (a), Wave 4 Task 9) — its sub-groups hold up
    /// to 45 items, well past what fits or is sensibly single-char-keyed.
    GroupMenu { group: CommandGroup, cursor: usize },
    /// User is typing a 3-digit `EX` menu item number (`P1`) — the
    /// number-entry escape hatch's first step
    /// (`planning/architect/task_plan.md` §11.4, path (b)). Reachable from
    /// `Menu` via [`EX_NUMBER_ENTRY_KEY`]. `Enter` looks `buffer` up in
    /// [`radio::EX_MENU_TABLE`] (via [`ex_menu_item`]): found -> forks
    /// directly into `TextInput`/`ListSelect` (reused verbatim, carrying a
    /// new `SetExMenuItem` action) per the item's own
    /// [`ExMenuValueKind`]; not found -> `error` is set and this state is
    /// re-entered unchanged, mirroring `TextInput`'s own error-then-retry
    /// shape. `Esc` -> `Menu`. The reverse transition (`Esc` from the
    /// forked `TextInput`/`ListSelect` back to here, buffer restored to the
    /// entered `p1`) is handled in those states' own `handle_key` arms, not
    /// here.
    ExNumberEntry {
        buffer: String,
        error: Option<String>,
    },
    /// Showing one `EX` themed sub-group's scrollable item list — the
    /// number-entry escape hatch's browsing counterpart
    /// (`planning/architect/task_plan.md` §11.4, path (a)). Reachable from
    /// `GroupMenu { group: CommandGroup::ExMenu, .. }` via
    /// [`CommandKind::ExSubGroup`]. `cursor` indexes [`ex_theme_items`]`
    /// (theme)`'s (runtime-bucketed, not hand-maintained) item list and
    /// **is** wired to `Up`/`Down` (unlike `GroupMenu`'s vestigial
    /// `cursor` — see that variant's doc comment), since a sub-group can
    /// hold up to 45 items. `Enter` forks into `TextInput`/`ListSelect` via
    /// [`enter_ex_value_entry`], same as `ExNumberEntry`'s own `Enter` arm
    /// — both paths reuse the exact same helper, differing only in how the
    /// item was found and in the [`ExValueEntryOrigin`] passed through (so
    /// the fork's own `Esc` returns to the right place). `Esc` here ->
    /// `GroupMenu { group: CommandGroup::ExMenu, .. }` (the theme picker).
    ExSubGroupMenu { theme: ExTheme, cursor: usize },
    /// Showing the list of profiles discovered in the default profile
    /// directory ([`radio::default_profile_dir`]) — reachable from `Menu`
    /// via [`PROFILE_LIST_KEY`] (`planning/architect/task_plan.md` §12.3).
    /// `profiles` is populated once, at entry (loading is a blocking
    /// filesystem read, not something to redo per keystroke). `error`
    /// surfaces a profile that failed to parse (per-file, not fatal to the
    /// whole list — see [`radio::Profile::load_all_from_dir`]'s own doc
    /// comment) or "no profiles found." `Up`/`Down` scroll like
    /// `ExSubGroupMenu`; `Enter` produces
    /// [`ExecuteAction::ApplyProfile`]; `Esc` -> `Menu`.
    ProfileList {
        profiles: Vec<(String, radio::Profile)>,
        cursor: usize,
        error: Option<String>,
    },
    /// User is typing text input.
    TextInput {
        prompt: String,
        buffer: String,
        error: Option<String>,
        action: InputAction,
    },
    /// User is selecting from a list (currently only mode selection).
    ListSelect {
        options: Vec<String>,
        cursor: usize,
        action: SelectAction,
    },
    /// Showing feedback after a command.
    Feedback { message: String, is_error: bool },
}

// ---------------------------------------------------------------------------
// KeyResult — returned by handle_key
// ---------------------------------------------------------------------------

/// The result of processing a key event.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyResult {
    /// Keep running — no radio command needed.
    Continue,
    /// Exit the UI.
    Quit,
    /// Execute a radio action with a validated value.
    Execute(ExecuteAction),
}

// ---------------------------------------------------------------------------
// Group command descriptors
// ---------------------------------------------------------------------------

/// How a group command is activated. Reused from ts570d's
/// `CommandKind::{Text, List, Immediate}` pattern — genuinely reusable at
/// any command count, per §6.1/§11.2.
enum CommandKind {
    /// Produces a `TextInput` state.
    Text {
        prompt: &'static str,
        action: InputAction,
    },
    /// Produces a `ListSelect` state.
    List {
        options: fn() -> Vec<String>,
        action: SelectAction,
    },
    /// Immediately produces an `ExecuteAction` (no input needed). Takes the
    /// current display state since `ToggleTx`/`TogglePowerOn` need it to
    /// decide which direction to toggle.
    Immediate(fn(&Ft991aDisplay) -> ExecuteAction),
    /// Navigate into a themed `EX` sub-group's scrollable item list
    /// (`ControlState::ExSubGroupMenu`, §11.4 path (a), Wave 4 Task 9) —
    /// distinct from `List` because sub-groups can hold up to 45 items
    /// (`ListSelect`'s single-row `<`/`>` rendering doesn't scale that far)
    /// and because selecting an item doesn't itself execute an action, it
    /// forks into a further value-entry state via `enter_ex_value_entry`.
    ExSubGroup(ExTheme),
    /// Navigate straight to `ControlState::ExNumberEntry` — the
    /// number-entry escape hatch (§11.4 path (b)) folded into `ExMenu`'s
    /// own command list (`ex_menu_commands`'s doc comment has the
    /// discoverability rationale), alongside its existing dedicated
    /// `Menu`-level [`EX_NUMBER_ENTRY_KEY`] binding.
    EnterExNumberEntry,
}

/// One selectable command within a [`CommandGroup`]'s `GroupMenu` screen.
struct GroupCommand {
    key: char,
    label: &'static str,
    kind: CommandKind,
}

fn mode_options() -> Vec<String> {
    vec![
        Mode::Lsb.name().to_string(),
        Mode::Usb.name().to_string(),
        Mode::CwU.name().to_string(),
        Mode::Fm.name().to_string(),
        Mode::Am.name().to_string(),
        Mode::RttyLsb.name().to_string(),
        Mode::CwL.name().to_string(),
        Mode::DataLsb.name().to_string(),
        Mode::RttyUsb.name().to_string(),
        Mode::DataFm.name().to_string(),
        Mode::FmN.name().to_string(),
        Mode::DataUsb.name().to_string(),
        Mode::AmN.name().to_string(),
        Mode::C4fm.name().to_string(),
    ]
}

/// The mode list in the same order as [`mode_options`], for cursor <->
/// `Mode` conversion.
const MODE_ORDER: [Mode; 14] = [
    Mode::Lsb,
    Mode::Usb,
    Mode::CwU,
    Mode::Fm,
    Mode::Am,
    Mode::RttyLsb,
    Mode::CwL,
    Mode::DataLsb,
    Mode::RttyUsb,
    Mode::DataFm,
    Mode::FmN,
    Mode::DataUsb,
    Mode::AmN,
    Mode::C4fm,
];

fn toggle_tx(display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ToggleTx(display.tx_state)
}

fn toggle_power_on(display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::TogglePowerOn(display.power_on)
}

// ---------------------------------------------------------------------------
// Shared list-option helpers for groups 3/4/6
// ---------------------------------------------------------------------------

/// Two-option On/Off list, cursor 0 = On (`true`), cursor 1 = Off (`false`)
/// — mirrors `ts570d::ui::control::on_off`'s own cursor convention exactly,
/// reused here (not ts570d's specific values, just the convention) for
/// every boolean toggle across groups 4/6.
fn on_off_options() -> Vec<String> {
    vec!["On".to_string(), "Off".to_string()]
}

/// The 5 [`ToneSquelchMode`] variants, `CT`'s P2 order (manual p.5) —
/// mirrors [`MODE_ORDER`]'s cursor<->value pairing pattern.
const TONE_SQUELCH_MODE_ORDER: [ToneSquelchMode; 5] = [
    ToneSquelchMode::Off,
    ToneSquelchMode::CtcssEncDec,
    ToneSquelchMode::CtcssEnc,
    ToneSquelchMode::DcsEncDec,
    ToneSquelchMode::DcsEnc,
];

fn tone_squelch_mode_options() -> Vec<String> {
    TONE_SQUELCH_MODE_ORDER
        .iter()
        .map(|m| m.name().to_string())
        .collect()
}

/// CTCSS tone options built at runtime from the crate's own
/// [`CTCSS_TONES_DECIHZ`] table (not a hand-maintained parallel list in
/// `ui`, avoiding drift — same principle §11.4 calls out for the future
/// `EX` sub-groups). Cursor index maps 1:1 onto the table's own index —
/// `select_action_to_execute`'s `SetCtcssTone` arm reads the same table by
/// the same index.
fn ctcss_tone_options() -> Vec<String> {
    CTCSS_TONES_DECIHZ
        .iter()
        .map(|&decihz| format!("{:.1} Hz", f32::from(decihz) / 10.0))
        .collect()
}

/// DCS code options built at runtime from [`DCS_CODES`] — same
/// table-driven principle as [`ctcss_tone_options`].
fn dcs_code_options() -> Vec<String> {
    DCS_CODES.iter().map(|&code| format!("{code:03}")).collect()
}

/// The 3 [`ScanState`] values, `SC`'s wire-digit order (manual p.16).
const SCAN_STATE_ORDER: [ScanState; 3] = [ScanState::Off, ScanState::Up, ScanState::Down];

fn scan_state_options() -> Vec<String> {
    vec!["Off".to_string(), "Up".to_string(), "Down".to_string()]
}

// ---------------------------------------------------------------------------
// Shared list-option helper for group 9 (`BandStepEncoder`)
// ---------------------------------------------------------------------------

/// The 16 [`Band`] variants, `BS`'s `P1` wire order (manual p.5), deliberately
/// excluding the documented gap at wire value `13` — mirrors [`MODE_ORDER`]'s
/// cursor<->value pairing pattern. `BS` has no `Read`/`Answer` form at all
/// (see [`ExecuteAction::SetBand`]'s doc comment), so unlike [`MODE_ORDER`]
/// there is no live value [`initial_list_cursor`] could ever pre-select this
/// against.
const BAND_ORDER: [Band; 16] = [
    Band::OneEightMHz,
    Band::ThreeFiveMHz,
    Band::FiveMHz,
    Band::SevenMHz,
    Band::TenMHz,
    Band::FourteenMHz,
    Band::EighteenMHz,
    Band::TwentyOneMHz,
    Band::TwentyFourPointFiveMHz,
    Band::TwentyEightMHz,
    Band::FiftyMHz,
    Band::Gen,
    Band::Mw,
    Band::Air,
    Band::OneFourFourMHz,
    Band::FourThreeZeroMHz,
];

fn band_options() -> Vec<String> {
    vec![
        "1.8 MHz".to_string(),
        "3.5 MHz".to_string(),
        "5 MHz".to_string(),
        "7 MHz".to_string(),
        "10 MHz".to_string(),
        "14 MHz".to_string(),
        "18 MHz".to_string(),
        "21 MHz".to_string(),
        "24.5 MHz".to_string(),
        "28 MHz".to_string(),
        "50 MHz".to_string(),
        "GEN".to_string(),
        "MW".to_string(),
        "AIR".to_string(),
        "144 MHz".to_string(),
        "430 MHz".to_string(),
    ]
}

// ---------------------------------------------------------------------------
// Shared list-option helpers for group 7 (`AttenuatorNoiseAgcNotchFilter`)
// ---------------------------------------------------------------------------

/// The 3 [`PreampMode`] variants, `PA`'s wire order (manual p.14) — mirrors
/// [`MODE_ORDER`]'s cursor<->value pairing pattern.
const PREAMP_ORDER: [PreampMode; 3] = [PreampMode::Ipo, PreampMode::Amp1, PreampMode::Amp2];

fn preamp_options() -> Vec<String> {
    vec!["IPO".to_string(), "AMP1".to_string(), "AMP2".to_string()]
}

/// 5 settable [`AgcMode`] options, `GT`'s **Set** (`P2`) domain (manual
/// p.10) — deliberately narrower than [`AgcMode`]'s own 7-valued *reported*
/// (`P3`) domain, since `GT`'s Set command genuinely cannot request a
/// specific AUTO sub-variant (see [`AgcMode::set_wire_value`]'s doc
/// comment). Cursor 4 ("Auto") maps to [`AgcMode::AutoFast`] — the same
/// arbitrary-but-documented sub-variant choice `AgcMode`'s own doc comment
/// already flags as this emulator's default report value, reused here
/// rather than inventing a second arbitrary choice.
const AGC_ORDER: [AgcMode; 5] = [
    AgcMode::Off,
    AgcMode::Fast,
    AgcMode::Mid,
    AgcMode::Slow,
    AgcMode::AutoFast,
];

fn agc_options() -> Vec<String> {
    vec![
        "Off".to_string(),
        "Fast".to_string(),
        "Mid".to_string(),
        "Slow".to_string(),
        "Auto".to_string(),
    ]
}

/// `SH`'s 22 raw filter-width table indices (`P2` `00`-`21`, manual p.16),
/// labeled from the static [`SH_BANDWIDTH_TABLE`] rather than the live radio
/// state.
///
/// **Why static, not display-driven**: the actual bandwidth in Hz a given
/// index represents depends on the radio's *current mode* and narrow/wide
/// (`NA`) state (`radio::Radio::get_filter_width_index`'s own doc comment)
/// — neither of which is part of `SH`'s own wire bytes, and (per this
/// crate's `CommandKind::List` shape) `options: fn() -> Vec<String>` takes
/// no `&Ft991aDisplay` parameter to read live mode from even if `NA`'s state
/// *were* polled (it isn't — this task does not extend polling, matching
/// groups 2/9's own established "no polling extension" precedent). Rather
/// than only showing a bare, uninformative index (this crate's `ED`/`EU`
/// precedent for genuinely context-dependent commands), each label instead
/// shows **all three families' narrow/wide Hz values** straight from the
/// compile-time [`SH_BANDWIDTH_TABLE`] constant — accurate and complete
/// information, just not narrowed to "what this means right now."
fn filter_width_options() -> Vec<String> {
    fn pair(narrow: Option<u16>, wide: Option<u16>) -> String {
        let n = narrow.map_or("-".to_string(), |v| v.to_string());
        let w = wide.map_or("-".to_string(), |v| v.to_string());
        format!("{n}/{w}")
    }
    SH_BANDWIDTH_TABLE
        .iter()
        .enumerate()
        .map(|(i, row)| {
            format!(
                "{i:02} SSB{} CW{} RTTY{}",
                pair(row.ssb_narrow, row.ssb_wide),
                pair(row.cw_narrow, row.cw_wide),
                pair(row.rtty_psk_narrow, row.rtty_psk_wide),
            )
        })
        .collect()
}

/// Group 3's `MC`-read command — no input needed, so `Immediate` rather
/// than `Text`-backed; ignores `display` since the currently selected
/// memory channel is not (yet) a polled [`Ft991aDisplay`] field.
fn get_memory_channel_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetMemoryChannel
}

/// Group 4's `RC` (clarifier clear) command — no input needed.
fn clarifier_clear_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ClarifierClear
}

/// Group 5's real-time RTS CW-keying toggle (§11.3 point 6). Reads the
/// *current* [`Ft991aDisplay::rts_asserted`] (locally tracked, never
/// polled — see the field's own doc comment in `lib.rs`) and carries it on
/// [`ExecuteAction::ToggleRts`] so `terminal.rs`'s executor can flip it,
/// apply it optimistically, and roll back on error — the same
/// carry-the-prior-state shape [`toggle_tx`]/[`toggle_power_on`] already
/// use.
fn toggle_rts(display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ToggleRts(display.rts_asserted)
}

/// Group 5's `ZI` (CW auto zero-in) command — a write-only, zero-width
/// trigger with no input and no persisted on/off state (see
/// [`ExecuteAction::ZeroIn`]'s doc comment).
fn zero_in_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ZeroIn
}

/// Shared 1-5 keyer memory channel parser (`KM`/`KY`'s common P1 range,
/// `radio/src/ft991a.rs` lines ~1259-1332) used by every keyer-memory
/// `InputAction` in group 5.
fn parse_keyer_channel(buffer: &str) -> Result<u8, String> {
    let ch: u16 = buffer
        .trim()
        .parse()
        .map_err(|_| "Enter a channel number 1-5".to_string())?;
    if !(1..=5).contains(&ch) {
        return Err("Channel must be 1-5".to_string());
    }
    Ok(ch as u8)
}

/// Shared 1-117 memory channel number parser (`MC`/`MR`/`MW`/`MT`'s common
/// P1 range, manual p.11-12) used by every memory-channel `InputAction`.
fn parse_memory_channel(buffer: &str) -> Result<u8, String> {
    let ch: u16 = buffer
        .trim()
        .parse()
        .map_err(|_| "Enter a channel number 1-117".to_string())?;
    if !(1..=117).contains(&ch) {
        return Err("Channel must be 1-117".to_string());
    }
    Ok(ch as u8)
}

// ---------------------------------------------------------------------------
// Group 2 (`VfoMemoryQuickOps`) — 11 zero-argument `Immediate` triggers
// ---------------------------------------------------------------------------
//
// Every one of these ignores `display`, same shape as
// `get_memory_channel_immediate`/`clarifier_clear_immediate`/
// `zero_in_immediate` above — one small named function per trigger rather
// than a single generic closure, matching this file's established
// one-function-per-command convention.

fn copy_vfo_a_to_b_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::CopyVfoAToB
}

fn copy_vfo_b_to_a_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::CopyVfoBToA
}

fn swap_vfos_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::SwapVfos
}

fn store_vfo_to_memory_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::StoreVfoToMemory
}

fn recall_memory_to_vfo_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::RecallMemoryToVfo
}

fn memory_channel_up_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::MemoryChannelUp
}

fn memory_channel_down_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::MemoryChannelDown
}

fn toggle_vfo_memory_mode_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ToggleVfoMemoryMode
}

fn qmb_store_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::QmbStore
}

fn qmb_recall_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::QmbRecall
}

fn quick_split_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::QuickSplit
}

// ---------------------------------------------------------------------------
// Group 9 (`BandStepEncoder`) helpers
// ---------------------------------------------------------------------------

fn band_up_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::BandUp
}

fn band_down_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::BandDown
}

fn mic_up_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::MicUp
}

fn mic_down_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::MicDown
}

fn ent_key_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::EntKey
}

/// Parse the encoder-selector half of `ED`/`EU`'s "encoder:steps" text
/// input (see `band_step_encoder_commands`'s `J`/`K` prompts). Accepts the
/// three [`EncoderSelector`] names case-insensitively (`main`/`sub`/`multi`)
/// rather than the raw `0`/`1`/`8` wire digits — more legible for a human
/// typing into a `TextInput`, converted to the wire digit only at
/// `terminal.rs`'s executor boundary via [`radio::Ft991aExtras::encoder_down`]/
/// [`radio::Ft991aExtras::encoder_up`] themselves.
fn parse_encoder_selector(s: &str) -> Result<EncoderSelector, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "main" => Ok(EncoderSelector::Main),
        "sub" => Ok(EncoderSelector::Sub),
        "multi" => Ok(EncoderSelector::Multi),
        _ => Err("Encoder must be 'main', 'sub', or 'multi'".to_string()),
    }
}

/// Parse the step-count half of `ED`/`EU`'s "encoder:steps" text input —
/// `1..=99`, per [`radio::Ft991aExtras::encoder_down`]/`encoder_up`'s own
/// validated range (`radio/src/ft991a.rs` lines ~2110-2135,
/// `RadioError::InvalidEncoderSteps`).
fn parse_encoder_steps(s: &str) -> Result<u8, String> {
    let steps: u16 = s
        .trim()
        .parse()
        .map_err(|_| "Enter a step count 1-99".to_string())?;
    if !(1..=99).contains(&steps) {
        return Err("Step count must be 1-99".to_string());
    }
    Ok(steps as u8)
}

// ---------------------------------------------------------------------------
// Group 10 (`MetersStatus`) helpers
// ---------------------------------------------------------------------------

/// The 6 [`Meter`] variants, `MS`'s `P1` wire order (manual p.12) — mirrors
/// [`MODE_ORDER`]'s cursor<->value pairing pattern. Shared by both `MS`
/// (meter select) and `RM`'s direct-select read (`RM`'s own selector values
/// are these plus 3 — see [`radio::Radio::get_meter`]'s doc comment — but
/// that offset is applied inside `radio::Ft991a::get_meter` itself, not
/// here).
const METER_ORDER: [Meter; 6] = [
    Meter::Comp,
    Meter::Alc,
    Meter::Po,
    Meter::Swr,
    Meter::Id,
    Meter::Vdd,
];

fn meter_options() -> Vec<String> {
    METER_ORDER.iter().map(|m| m.name().to_string()).collect()
}

/// The 7 [`RadioIndicator`] variants, `RI`'s `P1` wire order (manual p.15)
/// — deliberately excludes the manual's own documented gap (`1`, `2`, `8`,
/// `9`, `B`-`F` are not legal `RI` selectors; see [`RadioIndicator`]'s own
/// doc comment), same "structurally unrepresentable invalid value"
/// principle as [`MODE_ORDER`]/[`BAND_ORDER`].
const RADIO_INDICATOR_ORDER: [RadioIndicator; 7] = [
    RadioIndicator::HiSwr,
    RadioIndicator::Rec,
    RadioIndicator::Play,
    RadioIndicator::VfoATx,
    RadioIndicator::VfoBTx,
    RadioIndicator::VfoARx,
    RadioIndicator::TxLed,
];

fn radio_indicator_options() -> Vec<String> {
    RADIO_INDICATOR_ORDER
        .iter()
        .map(|i| i.name().to_string())
        .collect()
}

/// Group 10's `MS` read (`get_selected_meter`) — no input needed.
fn get_selected_meter_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetSelectedMeter
}

/// Group 10's `RM` `P1=0` front-panel-following read
/// (`get_active_meter_reading`) — no input needed.
fn get_active_meter_reading_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetActiveMeterReading
}

/// Group 10's `IF` composite status read (`get_information`) — no input
/// needed.
fn get_information_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetInformation
}

/// Group 10's `RS` menu-mode-active read (`get_menu_mode_active`) — no
/// input needed.
fn get_menu_mode_active_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetMenuModeActive
}

/// Group 10's `UL` PLL-unlock read (`get_pll_unlocked`) — no input needed.
fn get_pll_unlocked_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetPllUnlocked
}

// ---------------------------------------------------------------------------
// Group 11 (`SystemTunerDvs`) helpers
// ---------------------------------------------------------------------------

/// The 3 [`RepeaterShift`] variants, `OS`'s wire-digit order (manual p.13).
const REPEATER_SHIFT_ORDER: [RepeaterShift; 3] = [
    RepeaterShift::Simplex,
    RepeaterShift::Plus,
    RepeaterShift::Minus,
];

fn repeater_shift_options() -> Vec<String> {
    vec![
        "Simplex".to_string(),
        "Plus".to_string(),
        "Minus".to_string(),
    ]
}

/// `FT`'s 2-valued Answer-domain (`0`=VFO-A, `1`=VFO-B — see
/// [`ExecuteAction::SetTxVfo`]'s doc comment for the Set-domain translation
/// `radio::Ft991a::set_tx_vfo` performs internally).
fn tx_vfo_options() -> Vec<String> {
    vec!["VFO A".to_string(), "VFO B".to_string()]
}

/// `AC`'s 3-valued state (manual p.4: `0`=OFF, `1`=ON, `2`=Tuning
/// Start/Stop).
fn antenna_tuner_options() -> Vec<String> {
    vec![
        "Off".to_string(),
        "On".to_string(),
        "Tune Start/Stop".to_string(),
    ]
}

/// Group 11's `DA` read (`get_dimmer`) — no input needed.
fn get_dimmer_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetDimmer
}

/// Group 11's `DT` `P1=0` read (`read_date`) — no input needed.
fn read_date_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ReadDate
}

/// Group 11's `DT` `P1=1` read (`read_time`) — no input needed.
fn read_time_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ReadTime
}

/// Group 11's `DT` `P1=2` read (`read_time_zone_offset`) — no input needed.
fn read_time_zone_offset_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ReadTimeZoneOffset
}

/// Group 11's `OI` composite opposite-band status read
/// (`get_opposite_band_information`) — no input needed.
fn get_opposite_band_information_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetOppositeBandInformation
}

/// Group 11's `LM` `P2=0` stop-recording trigger (`stop_dvs_recording`) —
/// no input needed.
fn stop_dvs_recording_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::StopDvsRecording
}

/// Group 11's `LM` read (`get_dvs_recording_channel`) — no input needed.
fn get_dvs_recording_channel_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetDvsRecordingChannel
}

/// Group 11's `PB` `P2=0` stop-playback trigger (`stop_dvs_playback`) — no
/// input needed.
fn stop_dvs_playback_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::StopDvsPlayback
}

/// Group 11's `PB` read (`get_dvs_playback_channel`) — no input needed.
fn get_dvs_playback_channel_immediate(_display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::GetDvsPlaybackChannel
}

/// Shared "led:tft" dimmer parser (`DA` write, `radio/src/ft991a.rs` lines
/// ~2221-2231, [`radio::RadioError::InvalidDimmerLevel`]) — split on the
/// *first* `:` only, same convention as the memory-tag/keyer-memory/encoder
/// compound-field inputs elsewhere in this file. LED must be `1`-`2`, TFT
/// must be `0`-`15`.
fn parse_dimmer(buffer: &str) -> Result<(u8, u8), String> {
    if !buffer.contains(':') {
        return Err("Enter as 'led:tft', e.g. 2:8 (LED 1-2, TFT 0-15)".to_string());
    }
    let mut parts = buffer.splitn(2, ':');
    let led_str = parts.next().unwrap_or("");
    let tft_str = parts.next().unwrap_or("");
    let led: u8 = led_str
        .trim()
        .parse()
        .map_err(|_| "LED must be 1-2".to_string())?;
    let tft: u8 = tft_str
        .trim()
        .parse()
        .map_err(|_| "TFT must be 0-15".to_string())?;
    if !(1..=2).contains(&led) || tft > 15 {
        return Err("LED must be 1-2, TFT must be 0-15".to_string());
    }
    Ok((led, tft))
}

/// `DT` `P1=0` write parser (`radio/src/ft991a.rs` lines ~2248-2259,
/// [`radio::RadioError::InvalidDate`]): fixed 8-digit `YYYYMMDD` — no
/// leading/trailing separators, matching the wire field's own fixed width.
/// Month must be `1`-`12`, day must be `1`-`31` (no leap-year/month-length
/// calendar validation — same limitation the underlying `write_date` itself
/// has, not invented here).
fn parse_date(buffer: &str) -> Result<(u16, u8, u8), String> {
    let s = buffer.trim();
    if s.len() != 8 || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err("Enter as YYYYMMDD, e.g. 20260719".to_string());
    }
    let year: u16 = s[0..4]
        .parse()
        .map_err(|_| "Enter as YYYYMMDD".to_string())?;
    let month: u8 = s[4..6]
        .parse()
        .map_err(|_| "Enter as YYYYMMDD".to_string())?;
    let day: u8 = s[6..8]
        .parse()
        .map_err(|_| "Enter as YYYYMMDD".to_string())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err("Month must be 1-12, day must be 1-31".to_string());
    }
    Ok((year, month, day))
}

/// `DT` `P1=1` write parser (`radio/src/ft991a.rs` lines ~2275-2289,
/// [`radio::RadioError::InvalidTime`]): fixed 6-digit `HHMMSS`, 24-hour
/// UTC. Hour must be `0`-`23`, minute/second must be `0`-`59`.
fn parse_time(buffer: &str) -> Result<(u8, u8, u8), String> {
    let s = buffer.trim();
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err("Enter as HHMMSS, e.g. 143000".to_string());
    }
    let hour: u8 = s[0..2].parse().map_err(|_| "Enter as HHMMSS".to_string())?;
    let minute: u8 = s[2..4].parse().map_err(|_| "Enter as HHMMSS".to_string())?;
    let second: u8 = s[4..6].parse().map_err(|_| "Enter as HHMMSS".to_string())?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err("Hour must be 0-23, minute/second must be 0-59".to_string());
    }
    Ok((hour, minute, second))
}

/// `DT` `P1=2` write parser (`radio/src/ft991a.rs` lines ~2308-2320,
/// [`radio::RadioError::InvalidTimeZoneOffset`]): signed `+HHMM`/`-HHMM`,
/// combined into a single minutes value in `-720..=840`, 30-minute steps
/// (`-12:00` to `+14:00`).
fn parse_time_zone_offset(buffer: &str) -> Result<i16, String> {
    let s = buffer.trim();
    let (sign, rest): (i16, &str) = match s.as_bytes().first() {
        Some(b'+') => (1, &s[1..]),
        Some(b'-') => (-1, &s[1..]),
        _ => return Err("Enter as +HHMM or -HHMM, e.g. +0930".to_string()),
    };
    if rest.len() != 4 || !rest.chars().all(|c| c.is_ascii_digit()) {
        return Err("Enter as +HHMM or -HHMM, e.g. +0930".to_string());
    }
    let hh: i16 = rest[0..2]
        .parse()
        .map_err(|_| "Enter as +HHMM or -HHMM".to_string())?;
    let mm: i16 = rest[2..4]
        .parse()
        .map_err(|_| "Enter as +HHMM or -HHMM".to_string())?;
    let minutes = sign * (hh * 60 + mm);
    if !(-720..=840).contains(&minutes) || minutes % 30 != 0 {
        return Err(
            "Value must be -1200 to +1400 (-720 to 840 minutes), in steps of 30 min".to_string(),
        );
    }
    Ok(minutes)
}

/// Shared 1-5 DVS channel parser (`LM`/`PB`'s common channel range,
/// `radio/src/ft991a.rs` lines ~2453-2461/~2483-2491,
/// [`radio::RadioError::InvalidDvsChannel`]). Deliberately a separate
/// function from [`parse_keyer_channel`] even though both validate `1..=5`
/// — DVS channels are a distinct wire concept (`LM`/`PB`) from keyer memory
/// channels (`KM`/`KY`), and this file's established convention is one
/// named parser per domain, not reuse-by-coincidental-range.
fn parse_dvs_channel(buffer: &str) -> Result<u8, String> {
    let ch: u16 = buffer
        .trim()
        .parse()
        .map_err(|_| "Enter a DVS channel 1-5".to_string())?;
    if !(1..=5).contains(&ch) {
        return Err("DVS channel must be 1-5".to_string());
    }
    Ok(ch as u8)
}

/// Group 1 (`FrequencyLevels`) — a direct structural port of Wave 2's flat
/// 9-entry `command_table()`. Same commands, same keys, same validation:
/// this is the "no worse than today" baseline the grouped-menu redesign
/// must preserve, per §11.2's explicit note — not a redesign opportunity.
fn frequency_levels_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'F',
            label: "Set VFO A",
            kind: CommandKind::Text {
                prompt: "Enter VFO A freq Hz (30000-470000000):",
                action: InputAction::SetVfoA,
            },
        },
        GroupCommand {
            key: 'B',
            label: "Set VFO B",
            kind: CommandKind::Text {
                prompt: "Enter VFO B freq Hz (30000-470000000):",
                action: InputAction::SetVfoB,
            },
        },
        GroupCommand {
            key: 'M',
            label: "Set mode",
            kind: CommandKind::List {
                options: mode_options,
                action: SelectAction::SetMode,
            },
        },
        GroupCommand {
            key: 'T',
            label: "Toggle CAT TX/RX",
            kind: CommandKind::Immediate(toggle_tx),
        },
        GroupCommand {
            key: 'A',
            label: "Set AF gain",
            kind: CommandKind::Text {
                prompt: "Enter AF gain (0-255):",
                action: InputAction::SetAfGain,
            },
        },
        GroupCommand {
            key: 'R',
            label: "Set RF gain",
            kind: CommandKind::Text {
                prompt: "Enter RF gain (0-255):",
                action: InputAction::SetRfGain,
            },
        },
        GroupCommand {
            key: 'S',
            label: "Set squelch",
            kind: CommandKind::Text {
                prompt: "Enter squelch (0-100):",
                action: InputAction::SetSquelch,
            },
        },
        GroupCommand {
            key: 'P',
            label: "Set TX power",
            kind: CommandKind::Text {
                prompt: "Enter TX power watts (5-100):",
                action: InputAction::SetPower,
            },
        },
        GroupCommand {
            key: 'O',
            label: "Toggle power on/off",
            kind: CommandKind::Immediate(toggle_power_on),
        },
    ]
}

/// Group 2 (`VfoMemoryQuickOps`) — `AB BA AM VM MA CH QI QR QS SV` (manual
/// p.4-5/11/14-15/17-18), Wave 4 dispatch queue item 5
/// (`planning/architect/task_plan.md` §11.6). Mixed backing per §11.2's
/// table, confirmed directly against `radio_trait.rs`/`ft991a.rs` (not just
/// trusted): `AB`/`BA`/`SV` (copy/swap), `AM`/`MA` (store/recall VFO A <->
/// selected memory channel), and `CH0`/`CH1` (memory channel step up/down)
/// are all plain `Radio`-trait methods; `VM`/`QI`/`QR`/`QS` are
/// `Ft991aExtras`-only, deliberately kept off `Radio` per those methods' own
/// doc comments (`VM`'s meaning rests on a documented manual-heading
/// ambiguity, not a crisply specified concept; `QI`/`QR`/`QS` are
/// FT-991A-named "Quick" features distinct from the generic memory-channel/
/// split concepts `Radio` already covers).
///
/// All 11 commands are zero-argument, write-only wire triggers with no
/// persisted state to snapshot on the way in — every one of them is
/// `CommandKind::Immediate`, none is `Text`/`List`-backed (no numeric or
/// enumerated input exists for this group, so there is nothing here for
/// `validate_text_input`/`select_action_to_execute` to grow).
fn vfo_memory_quick_ops_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'A',
            label: "Copy VFO A -> B",
            kind: CommandKind::Immediate(copy_vfo_a_to_b_immediate),
        },
        GroupCommand {
            key: 'B',
            label: "Copy VFO B -> A",
            kind: CommandKind::Immediate(copy_vfo_b_to_a_immediate),
        },
        GroupCommand {
            key: 'W',
            label: "Swap VFO A/B",
            kind: CommandKind::Immediate(swap_vfos_immediate),
        },
        GroupCommand {
            key: 'S',
            label: "Store VFO A to memory",
            kind: CommandKind::Immediate(store_vfo_to_memory_immediate),
        },
        GroupCommand {
            key: 'R',
            label: "Recall memory to VFO A",
            kind: CommandKind::Immediate(recall_memory_to_vfo_immediate),
        },
        GroupCommand {
            key: 'U',
            label: "Memory channel up",
            kind: CommandKind::Immediate(memory_channel_up_immediate),
        },
        GroupCommand {
            key: 'D',
            label: "Memory channel down",
            kind: CommandKind::Immediate(memory_channel_down_immediate),
        },
        GroupCommand {
            key: 'M',
            label: "Toggle VFO/Memory mode",
            kind: CommandKind::Immediate(toggle_vfo_memory_mode_immediate),
        },
        GroupCommand {
            key: 'I',
            label: "QMB store",
            kind: CommandKind::Immediate(qmb_store_immediate),
        },
        GroupCommand {
            key: 'Q',
            label: "QMB recall",
            kind: CommandKind::Immediate(qmb_recall_immediate),
        },
        GroupCommand {
            key: 'P',
            label: "Quick split toggle",
            kind: CommandKind::Immediate(quick_split_immediate),
        },
    ]
}

/// Group 3 (`MemoryChannels`) — `MC`/`MR`/`MW`/`MT` (manual p.11-12), 100%
/// `Radio`-trait-backed per §11.2's table (confirmed by reading
/// `radio_trait.rs`'s trait body directly, not just trusting the table —
/// no discrepancy found for this group). `MW`/`MT`'s writes source their
/// frequency/mode from the currently tuned VFO A at execution time
/// (`terminal.rs`), mirroring `ts570d::ui`'s own `WriteMemoryChannelFromVfoA`
/// precedent, since a full [`MemoryChannelEntry`] needs more fields than a
/// single `Text` prompt can reasonably collect — see
/// [`ExecuteAction::WriteMemoryChannelFromVfoA`]'s doc comment for which
/// fields get documented defaults. Wave 4 dispatch queue item 3 (§11.6).
fn memory_channels_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'C',
            label: "Select memory channel",
            kind: CommandKind::Text {
                prompt: "Channel number (1-117):",
                action: InputAction::SelectMemoryChannel,
            },
        },
        GroupCommand {
            key: 'G',
            label: "Get selected memory channel",
            kind: CommandKind::Immediate(get_memory_channel_immediate),
        },
        GroupCommand {
            key: 'R',
            label: "Read memory channel",
            kind: CommandKind::Text {
                prompt: "Read channel (1-117):",
                action: InputAction::ReadMemoryChannel,
            },
        },
        GroupCommand {
            key: 'W',
            label: "Write channel from VFO A",
            kind: CommandKind::Text {
                prompt: "Write VFO A to channel (1-117):",
                action: InputAction::WriteMemoryChannelFromVfoA,
            },
        },
        GroupCommand {
            key: 'T',
            label: "Read channel + tag",
            kind: CommandKind::Text {
                prompt: "Read channel+tag (1-117):",
                action: InputAction::ReadMemoryChannelTag,
            },
        },
        GroupCommand {
            key: 'V',
            label: "Write channel + tag from VFO A",
            kind: CommandKind::Text {
                prompt: "Enter as 'channel:tag', e.g. 5:HOME (1-117, tag <=12 chars):",
                action: InputAction::WriteMemoryChannelTagFromVfoA,
            },
        },
    ]
}

/// Group 4 (`ClarifierToneIfShift`) — `RT RC RD RU XT CN CT IS` (manual
/// p.5-6/10/15-16/19), 100% `Radio`-trait-backed per §11.2's table
/// (confirmed by reading `radio_trait.rs`'s trait body directly — no
/// discrepancy found for this group either). CTCSS/DCS selection uses
/// `ListSelect` over the crate's own [`CTCSS_TONES_DECIHZ`]/[`DCS_CODES`]
/// tables (not free-text Hz/code entry), so an invalid tone/code is
/// structurally unrepresentable — mirrors the Mode list's own precedent and
/// avoids duplicating the 50/104-entry tables by hand. Wave 4 dispatch
/// queue item 3 (§11.6).
fn clarifier_tone_if_shift_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'X',
            label: "RX clarifier on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleRxClarifier,
            },
        },
        GroupCommand {
            key: 'Y',
            label: "TX clarifier on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleTxClarifier,
            },
        },
        GroupCommand {
            key: 'C',
            label: "Clarifier clear",
            kind: CommandKind::Immediate(clarifier_clear_immediate),
        },
        GroupCommand {
            key: 'D',
            label: "Clarifier down (offset Hz)",
            kind: CommandKind::Text {
                prompt: "Clarifier offset below tuned freq, Hz (0-9999):",
                action: InputAction::ClarifierDown,
            },
        },
        GroupCommand {
            key: 'U',
            label: "Clarifier up (offset Hz)",
            kind: CommandKind::Text {
                prompt: "Clarifier offset above tuned freq, Hz (0-9999):",
                action: InputAction::ClarifierUp,
            },
        },
        GroupCommand {
            key: 'I',
            label: "IF shift",
            kind: CommandKind::Text {
                prompt: "IF shift Hz (-1200 to 1200, step 20):",
                action: InputAction::SetIfShift,
            },
        },
        GroupCommand {
            key: 'T',
            label: "Tone squelch mode",
            kind: CommandKind::List {
                options: tone_squelch_mode_options,
                action: SelectAction::SetToneSquelchMode,
            },
        },
        GroupCommand {
            key: 'N',
            label: "CTCSS tone",
            kind: CommandKind::List {
                options: ctcss_tone_options,
                action: SelectAction::SetCtcssTone,
            },
        },
        GroupCommand {
            key: 'S',
            label: "DCS code",
            kind: CommandKind::List {
                options: dcs_code_options,
                action: SelectAction::SetDcsCode,
            },
        },
    ]
}

/// Group 5 (`KeyerCwBreakIn`) — `KM KP KR KS KY CS ZI BI SD` (manual
/// p.5-6/10-11/16/18) plus the new real-time RTS CW-keying toggle (§11.3
/// point 6). Mixed backing per §11.2's table (confirmed directly against
/// `radio_trait.rs`, not just trusted): `BI`/`SD`/`CS`/`KR`/`KS`/`KP`/`ZI`
/// are 100% `Radio`-trait-backed (lines ~1298-1361); `KM`/`KY` (keyer
/// memory message store/playback) are `Ft991aExtras`-only, deliberately
/// kept off `Radio` per that trait's own doc comment (FT-991A-inherent, not
/// a generic CW-operating concept); the RTS toggle is the crate's first
/// `CwKeying`-backed key.
///
/// Boolean toggles (`BI`/`CS`/`KR`) use `CommandKind::List` +
/// `on_off_options`, matching groups 4/6's established convention (not
/// group 1's display-state-dependent `Immediate` toggle) — same reasoning
/// as decision 4 in `planning/ui/task_plan.md`'s Wave 4 Task 3 section: no
/// dedicated `Ft991aDisplay` polling for these fields exists yet, so
/// `initial_list_cursor` defaults to cursor 0 for all three (documented
/// limitation, not silently absorbed).
///
/// `KM` write and `KY` playback both need a channel number **and** a
/// second value (a message, or a playback-family choice) that a single
/// `Text` buffer can't hold two independently-typed fields for. Following
/// group 3's `WriteMemoryChannelTagFromVfoA` precedent, `KM` write reuses
/// the "channel:text" convention (split on the *first* `:` only). `KY`
/// instead gets **two** distinct keys (`Y`/`J`) — one per
/// [`radio::KeyerPlaybackMode`] — rather than inventing a second
/// multi-field text syntax, since both variants only need the plain 1-5
/// channel number `parse_keyer_channel` already validates; this is a
/// judgment call, flagged for the final report.
///
/// The RTS toggle (`K`) has no natural collision within this group's own
/// keymap (checked against the other 11 keys below before assigning it),
/// so it uses the exact key §11.3 point 6 specifies. Wave 4 dispatch queue
/// item 4 (§11.6).
fn keyer_cw_break_in_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'K',
            label: "Toggle RTS CW key (real-time)",
            kind: CommandKind::Immediate(toggle_rts),
        },
        GroupCommand {
            key: 'B',
            label: "Break-in on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleBreakIn,
            },
        },
        GroupCommand {
            key: 'D',
            label: "Semi break-in delay",
            kind: CommandKind::Text {
                prompt: "Semi break-in delay, ms (30-3000):",
                action: InputAction::SetSemiBreakInDelay,
            },
        },
        GroupCommand {
            key: 'S',
            label: "CW spot on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleCwSpot,
            },
        },
        GroupCommand {
            key: 'E',
            label: "Electronic keyer on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleKeyerEnabled,
            },
        },
        GroupCommand {
            key: 'W',
            label: "Keyer speed (WPM)",
            kind: CommandKind::Text {
                prompt: "Keyer speed, WPM (4-60):",
                action: InputAction::SetKeyerSpeed,
            },
        },
        GroupCommand {
            key: 'P',
            label: "Keyer pitch (sidetone)",
            kind: CommandKind::Text {
                prompt: "Keyer pitch, Hz (300-1050, step 10):",
                action: InputAction::SetKeyerPitchHz,
            },
        },
        GroupCommand {
            key: 'Z',
            label: "CW auto zero-in",
            kind: CommandKind::Immediate(zero_in_immediate),
        },
        GroupCommand {
            key: 'R',
            label: "Read keyer memory",
            kind: CommandKind::Text {
                prompt: "Read keyer memory channel (1-5):",
                action: InputAction::ReadKeyerMemory,
            },
        },
        GroupCommand {
            key: 'M',
            label: "Write keyer memory message",
            kind: CommandKind::Text {
                prompt: "Enter as 'channel:message', e.g. 3:CQ CQ DE (1-5, message <=50 chars):",
                action: InputAction::WriteKeyerMemory,
            },
        },
        GroupCommand {
            key: 'Y',
            label: "Play keyer memory",
            kind: CommandKind::Text {
                prompt: "Play keyer memory channel (1-5):",
                action: InputAction::PlayKeyerMemory,
            },
        },
        GroupCommand {
            key: 'J',
            label: "Play message keyer",
            kind: CommandKind::Text {
                prompt: "Play message keyer channel (1-5):",
                action: InputAction::PlayMessageKeyer,
            },
        },
    ]
}

/// Group 6 (`ScanVoxBusy`) — `SC VX VD VG BY` (manual p.5/16-18), 100%
/// `Radio`-trait-backed per §11.2's table (confirmed directly against
/// `radio_trait.rs` — no discrepancy found). `BY` (`get_rx_busy`) is
/// deliberately **not** bound to any key — read-only, same "read-only
/// fields don't get a key" convention group 1 established for
/// `get_smeter`/`get_id`, and doubly so here since this crate's emulator
/// always reports `false` for it regardless (`ft991a.rs`'s own doc
/// comment) — nothing meaningful to poll or display yet either. Wave 4
/// dispatch queue item 3 (§11.6).
fn scan_vox_busy_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'S',
            label: "Scan state",
            kind: CommandKind::List {
                options: scan_state_options,
                action: SelectAction::SetScanState,
            },
        },
        GroupCommand {
            key: 'V',
            label: "VOX on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleVox,
            },
        },
        GroupCommand {
            key: 'G',
            label: "VOX gain",
            kind: CommandKind::Text {
                prompt: "VOX gain (0-100):",
                action: InputAction::SetVoxGain,
            },
        },
        GroupCommand {
            key: 'D',
            label: "VOX delay",
            kind: CommandKind::Text {
                prompt: "VOX delay ms (30-3000, step 10):",
                action: InputAction::SetVoxDelay,
            },
        },
    ]
}

/// Group 7 (`AttenuatorNoiseAgcNotchFilter`) — `RA PA NB NL NR RL GT CO BP
/// BC NA SH` (manual p.4/5/10/13-16), Wave 4 dispatch queue item 6 (§11.6).
///
/// **Trait mix confirmed directly against `radio_trait.rs`, not just
/// trusted from §11.2's table**: `get`/`set_attenuator_on` (`RA`),
/// `get`/`set_preamp_mode` (`PA`), `get`/`set_noise_blanker_on`/`_level`
/// (`NB`/`NL`), `get`/`set_noise_reduction_on`/`_level` (`NR`/`RL`),
/// `get`/`set_agc_mode` (`GT`), `get`/`set_auto_notch_on` (`BC`),
/// `get`/`set_narrow_on` (`NA`), and `get`/`set_filter_width_index` (`SH`)
/// are all plain [`radio::Radio`] methods (10 keys: `A P B L N R G U W F`).
/// `CO` (Contour/APF) and `BP` (Manual Notch) are [`radio::Ft991aExtras`]-
/// only, per `radio_trait.rs`'s own doc comment on the batch: "FT-991A-
/// named parametric-EQ/audio-peaking features with no generic-radio-concept
/// precedent... kept `Ft991a`-inherent-only" (6 keys: `C H X Y M Z`).
///
/// **The `CO`/`BP` shared-P3-field parametric shape does *not* need a new
/// `ControlState` variant**: `radio/src/ft991a.rs` already resolves each
/// `P2`-selected sub-field (contour on/off, contour frequency, APF on/off,
/// APF frequency, manual notch on/off, manual notch frequency) into its own
/// dedicated `get_*`/`set_*` method pair at the client-API boundary — the
/// wire-level P2 selector is not something this UI ever has to model. Each
/// pair is therefore just another independent boolean
/// ([`CommandKind::List`] plus [`on_off_options`]) or numeric
/// ([`CommandKind::Text`]) control, the same shape `NR`/`RL`'s on/level
/// split already established in this file.
///
/// **AGC (`G`) and filter width (`F`) are both `ListSelect`, not
/// `TextInput`**: `set_agc_mode` takes the enumerated [`AgcMode`] (see
/// [`AGC_ORDER`]/[`agc_options`]'s doc comment for why only 5 of its 7
/// variants are offered), and `SH` is a fixed 22-entry named table (see
/// [`filter_width_options`]'s doc comment for why its labels are built from
/// the static [`SH_BANDWIDTH_TABLE`] rather than live radio state).
///
/// None of this group's fields are polled into [`Ft991aDisplay`] yet (this
/// task only populates the command groups, matching groups 2/9's own
/// "not polled yet" precedent) — see [`initial_list_cursor`].
fn attenuator_noise_agc_notch_filter_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'A',
            label: "Attenuator on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleAttenuator,
            },
        },
        GroupCommand {
            key: 'P',
            label: "Pre-amp/IPO mode",
            kind: CommandKind::List {
                options: preamp_options,
                action: SelectAction::SetPreampMode,
            },
        },
        GroupCommand {
            key: 'B',
            label: "Noise blanker on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleNoiseBlanker,
            },
        },
        GroupCommand {
            key: 'L',
            label: "Noise blanker level",
            kind: CommandKind::Text {
                prompt: "Noise blanker level (0-10):",
                action: InputAction::SetNoiseBlankerLevel,
            },
        },
        GroupCommand {
            key: 'N',
            label: "Noise reduction on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleNoiseReduction,
            },
        },
        GroupCommand {
            key: 'R',
            label: "Noise reduction level",
            kind: CommandKind::Text {
                prompt: "Noise reduction level (1-15):",
                action: InputAction::SetNoiseReductionLevel,
            },
        },
        GroupCommand {
            key: 'G',
            label: "AGC mode",
            kind: CommandKind::List {
                options: agc_options,
                action: SelectAction::SetAgcMode,
            },
        },
        GroupCommand {
            key: 'U',
            label: "Auto notch on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleAutoNotch,
            },
        },
        GroupCommand {
            key: 'W',
            label: "Narrow filter on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleNarrow,
            },
        },
        GroupCommand {
            key: 'F',
            label: "Filter width (SH table index)",
            kind: CommandKind::List {
                options: filter_width_options,
                action: SelectAction::SetFilterWidthIndex,
            },
        },
        GroupCommand {
            key: 'C',
            label: "Contour on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleContour,
            },
        },
        GroupCommand {
            key: 'H',
            label: "Contour frequency",
            kind: CommandKind::Text {
                prompt: "Contour frequency, Hz (10-3200):",
                action: InputAction::SetContourFrequencyHz,
            },
        },
        GroupCommand {
            key: 'X',
            label: "APF on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleApf,
            },
        },
        GroupCommand {
            key: 'Y',
            label: "APF frequency",
            kind: CommandKind::Text {
                prompt: "APF frequency, Hz (-250 to 250, step 10):",
                action: InputAction::SetApfFrequencyHz,
            },
        },
        GroupCommand {
            key: 'M',
            label: "Manual notch on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleManualNotch,
            },
        },
        GroupCommand {
            key: 'Z',
            label: "Manual notch frequency",
            kind: CommandKind::Text {
                prompt: "Manual notch frequency, Hz (10-3200, step 10):",
                action: InputAction::SetManualNotchFrequencyHz,
            },
        },
    ]
}

/// Group 8 (`SpeechMicMonitor`) — `PL PR MG ML` (manual p.11-12/14), Wave 4
/// dispatch queue item 6 (§11.6).
///
/// **Trait mix confirmed directly against `radio_trait.rs`**:
/// `get`/`set_mic_gain` (`MG`), `get`/`set_speech_processor_level` (`PL`),
/// `get`/`set_speech_processor_on` (`PR` `P1=0`), and
/// `get`/`set_monitor_on`/`_level` (`ML`) are plain [`radio::Radio`] methods
/// (5 keys: `G L S M V`). The Parametric Microphone Equalizer
/// (`PR` `P1=1`) is [`radio::Ft991aExtras`]-only, per that trait's own doc
/// comment on `set_speech_processor_on`: "`PR`'s `P1=1` item... is
/// deliberately **not** on this trait — an FT-991A-named parametric-EQ
/// feature with no generic concept precedent, same treatment batch 6 gave
/// `CO`/`BP`" (1 key: `E`). Same "`P1`-selector already resolved at the
/// client-API boundary, no new `ControlState` needed" note as group 7's
/// `PR`/`ML` split applies here too.
///
/// None of this group's fields are polled into [`Ft991aDisplay`] yet (same
/// "not polled yet" precedent as group 7) — see [`initial_list_cursor`].
fn speech_mic_monitor_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'G',
            label: "Mic gain",
            kind: CommandKind::Text {
                prompt: "Mic gain (0-100):",
                action: InputAction::SetMicGain,
            },
        },
        GroupCommand {
            key: 'L',
            label: "Speech processor level",
            kind: CommandKind::Text {
                prompt: "Speech processor level (0-100):",
                action: InputAction::SetSpeechProcessorLevel,
            },
        },
        GroupCommand {
            key: 'S',
            label: "Speech processor on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleSpeechProcessor,
            },
        },
        GroupCommand {
            key: 'M',
            label: "Monitor on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleMonitor,
            },
        },
        GroupCommand {
            key: 'V',
            label: "Monitor level",
            kind: CommandKind::Text {
                prompt: "Monitor level (0-100):",
                action: InputAction::SetMonitorLevel,
            },
        },
        GroupCommand {
            key: 'E',
            label: "Parametric mic EQ on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleParametricMicEq,
            },
        },
    ]
}

/// Group 9 (`BandStepEncoder`) — `BS BU BD FS ED EU EK DN UP` (manual
/// p.4-7/9/17), Wave 4 dispatch queue item 5 (§11.6). Mixed backing per
/// §11.2's table, confirmed directly against `radio_trait.rs`/`ft991a.rs`
/// (not just trusted): `BS`/`BU`/`BD`/`FS`/`UP`/`DN` are plain
/// `Radio`-trait methods; `ED`/`EU`/`EK` are `Ft991aExtras`-only,
/// deliberately kept off `Radio` per that trait's own doc comment
/// (FT-991A-specific front-panel-encoder/ENT-key emulation, no generic
/// concept to attach to and no `ts570d::Radio` precedent).
///
/// **Band select (`B`) uses `ListSelect`, not `TextInput`**: `set_band`
/// takes a [`Band`] enum, not a raw wire code, so an invalid band is
/// structurally unrepresentable through this key — same principle as the
/// Mode/CTCSS/DCS lists elsewhere in this file. Unlike Mode, `BS` has no
/// `Read`/`Answer` form at all (manual p.3), so [`initial_list_cursor`]
/// cannot pre-select the live band the way it does for Mode — this isn't
/// "not polled yet" (group 4/5/6's documented limitation), it is
/// structurally unknowable from the wire protocol itself, and is called out
/// separately here so a future reader doesn't conflate the two.
///
/// **Fine step (`F`) uses `CommandKind::List` + [`on_off_options`]**,
/// matching groups 4/5/6's established boolean-toggle convention (not group
/// 1's display-state-dependent `Immediate` toggle) — `Ft991aDisplay` does
/// not poll `FS`, so [`initial_list_cursor`] defaults to cursor 0, the same
/// documented limitation those groups already carry.
///
/// **Mic UP/DOWN (`P`/`N`, wire `UP`/`DN`) are real, meaningful triggers**:
/// the emulator's own `handle_command` (`ft991a_radio.rs`) steps
/// `vfo_a_hz` by a fixed amount on each press, so — unlike `ED`/`EU` below —
/// pressing these keys against the emulator produces a real, observable
/// change on the next `poll_radio_state` cycle (VFO A's displayed
/// frequency moves). No dead-end concern here.
///
/// **Encoder nudges (`J`/`K`, wire `ED`/`EU`) and the ENT key (`E`, wire
/// `EK`) — the honest disclosure the task brief asked for.** Confirmed
/// still true by reading `ft991a_radio.rs`'s `handle_command` directly:
/// `ED`/`EU` are "structurally and semantically validated... but mutate no
/// persisted `Ft991aState` field... no Hz-per-step mapping is knowable from
/// this manual page alone," and `EK` is a "zero-width Action trigger, no
/// persisted-state effect." Included as real keys anyway, not omitted,
/// because:
/// - This is a modeling limitation of *this crate's own test emulator*, not
///   evidence the wire commands do nothing — on real hardware `ED`/`EU`
///   genuinely nudge whichever physical parameter the selected encoder
///   (Main/Sub/Multi) currently controls, and `EK` genuinely presses the
///   front-panel ENT key. `radio/src/ft991a.rs`'s client implementation
///   sends the real wire frame in all three cases, exactly like every other
///   command in this file.
/// - This crate already has an established, accepted category for
///   "write-only trigger, no persisted state to observe" commands —
///   [`ExecuteAction::ClarifierClear`] (`RC`) and [`ExecuteAction::ZeroIn`]
///   (`ZI`) are both already bound to real keys in groups 4/5 on exactly
///   this basis. `EK` fits that category cleanly (a well-understood,
///   single-meaning button press, like `RC`/`ZI`).
/// - `ED`/`EU` are a harder case than `RC`/`ZI`/`EK`, and this is disclosed
///   rather than glossed over: their real-world effect is
///   **context-dependent** on the physical radio's current front-panel
///   state (which function the selected encoder is bound to at the moment),
///   so this UI cannot show *what* changed — only that the trigger was
///   sent. They are included as a genuine "remote knob nudge" feature (the
///   nearest thing this protocol offers to turning a knob you can't
///   physically reach), not as something guaranteed to have an obvious
///   effect; the `Text` prompt below says as much. A reader who considers
///   this still too opaque to ship should treat this doc comment as the
///   place to revisit that call, not as evidence the decision was made
///   silently.
fn band_step_encoder_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'B',
            label: "Select band",
            kind: CommandKind::List {
                options: band_options,
                action: SelectAction::SetBand,
            },
        },
        GroupCommand {
            key: 'U',
            label: "Band up",
            kind: CommandKind::Immediate(band_up_immediate),
        },
        GroupCommand {
            key: 'D',
            label: "Band down",
            kind: CommandKind::Immediate(band_down_immediate),
        },
        GroupCommand {
            key: 'F',
            label: "Fine step on/off",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleFineStep,
            },
        },
        GroupCommand {
            key: 'P',
            label: "Mic UP button",
            kind: CommandKind::Immediate(mic_up_immediate),
        },
        GroupCommand {
            key: 'N',
            label: "Mic DOWN button",
            kind: CommandKind::Immediate(mic_down_immediate),
        },
        GroupCommand {
            key: 'J',
            label: "Encoder down (nudge)",
            kind: CommandKind::Text {
                prompt: "Enter as 'encoder:steps', e.g. main:5 (encoder=main/sub/multi, \
                         steps 1-99; effect depends on the radio's current front-panel context):",
                action: InputAction::EncoderDown,
            },
        },
        GroupCommand {
            key: 'K',
            label: "Encoder up (nudge)",
            kind: CommandKind::Text {
                prompt: "Enter as 'encoder:steps', e.g. main:5 (encoder=main/sub/multi, \
                         steps 1-99; effect depends on the radio's current front-panel context):",
                action: InputAction::EncoderUp,
            },
        },
        GroupCommand {
            key: 'E',
            label: "ENT key press",
            kind: CommandKind::Immediate(ent_key_immediate),
        },
    ]
}

/// Group 10 (`MetersStatus`) — `IF RM RI RS MS UL` (manual p.10/12/15/16/18),
/// Wave 4 dispatch queue item 7 (§11.6).
///
/// **Trait mix confirmed directly against `radio_trait.rs`, not just
/// trusted from §11.2's table**: `select_meter`/`get_selected_meter`
/// (`MS`) and `get_meter` (`RM` direct-select) are plain [`radio::Radio`]
/// methods (`radio_trait.rs` lines ~1040-1053, 3 keys: `M C D`).
/// `get_active_meter_reading` (`RM` `P1=0`), `get_radio_indicator` (`RI`),
/// `get_menu_mode_active` (`RS`), `get_pll_unlocked` (`UL`), and
/// `get_information` (`IF`) are all [`radio::Ft991aExtras`]-only
/// (`radio_trait.rs` lines ~1847-1879, 5 keys: `F I R N U`). **Discrepancy
/// noted**: §11.2's table prose parenthetically maps "`RS`→
/// `get_pll_unlocked`" and "`UL`" separately, but the actual source (both
/// `radio_trait.rs`'s doc comments and `ft991a.rs`'s implementation, read
/// directly) has this backwards — `RS` backs `get_menu_mode_active` and
/// `UL` backs `get_pll_unlocked`. The method *names* below follow the real
/// source, not the table's swapped prose.
///
/// **Read-heavy/display-focused, per the group's own framing**: none of
/// this group's 8 fields are part of the passive 200ms `poll_radio_state`
/// loop or the status bar (confirmed by reading `terminal.rs`'s
/// `poll_radio_state` directly — it only polls the original Wave 2 10-field
/// set: VFO A/B, mode, TX state, S-meter, power on, AF/RF gain, squelch, TX
/// power). So unlike some other groups' "not polled yet, defaults to
/// cursor 0" limitation, **every** field here genuinely needs its own
/// explicit key — there is nothing already passively visible to omit a key
/// for. All 8 are `List` (structurally-typed selection, for `MS`/`RM`'s
/// meter choice and `RI`'s indicator choice) or `Immediate` (the 5
/// zero-argument reads) — no `Text`-backed numeric input exists in this
/// group, matching its "read-heavy" framing.
fn meters_status_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'M',
            label: "Select meter (MS)",
            kind: CommandKind::List {
                options: meter_options,
                action: SelectAction::SelectMeter,
            },
        },
        GroupCommand {
            key: 'C',
            label: "Get currently selected meter (MS read)",
            kind: CommandKind::Immediate(get_selected_meter_immediate),
        },
        GroupCommand {
            key: 'D',
            label: "Read meter directly (RM direct-select)",
            kind: CommandKind::List {
                options: meter_options,
                action: SelectAction::ReadMeterDirect,
            },
        },
        GroupCommand {
            key: 'F',
            label: "Read front-panel-following meter (RM P1=0)",
            kind: CommandKind::Immediate(get_active_meter_reading_immediate),
        },
        GroupCommand {
            key: 'I',
            label: "Read composite status (IF)",
            kind: CommandKind::Immediate(get_information_immediate),
        },
        GroupCommand {
            key: 'R',
            label: "Read radio indicator (RI)",
            kind: CommandKind::List {
                options: radio_indicator_options,
                action: SelectAction::ReadRadioIndicator,
            },
        },
        GroupCommand {
            key: 'N',
            label: "Read menu-mode-active status (RS)",
            kind: CommandKind::Immediate(get_menu_mode_active_immediate),
        },
        GroupCommand {
            key: 'U',
            label: "Read PLL unlock status (UL)",
            kind: CommandKind::Immediate(get_pll_unlocked_immediate),
        },
    ]
}

/// Group 11 (`SystemTunerDvs`) — `AC AI DA DT LK OI OS FT TS MX LM PB`
/// (manual p.4/6/9/11/13/17-18), Wave 4 dispatch queue item 7 (§11.6). The
/// most [`radio::Ft991aExtras`]-heavy group, per the task brief's own
/// framing — confirmed by trait-mix citation below, not just asserted.
///
/// **Trait mix confirmed directly against `radio_trait.rs`**: `AI`/`LK`/
/// `OS`/`FT`/`MX` (5 get/set pairs, `radio_trait.rs` lines ~1730-1801) are
/// plain [`radio::Radio`] methods. `AC`/`DA`/`DT`(x3)/`OI`/`TS`/`LM`/`PB`
/// (`radio_trait.rs` lines ~2021-2125) are all [`radio::Ft991aExtras`]-only
/// — exactly matching §11.2's table ("auto-info/frequency-lock/repeater-
/// shift/tx-vfo/mox on Radio; antenna tuner state, dimmer, date/time/tz,
/// opposite-band info, TXW, DVS record/playback on Ft991aExtras"), no
/// discrepancy for this group.
///
/// **Date/time (`DT`) needs its own small multi-field design, per the task
/// brief's explicit flag**: `DT` is a 3-shape command where `P1` selects
/// date (`P1=0`)/time (`P1=1`)/offset (`P1=2`). `radio/src/ft991a.rs`
/// (lines ~2236-2320) already splits this into 3 fully independent
/// method pairs at the client-API boundary — `read_date`/`write_date`,
/// `read_time`/`write_time`, `read_time_zone_offset`/
/// `write_time_zone_offset` — each taking/returning its own plain tuple,
/// not a raw `P1`+wire-string pair. So the natural, cleanest fit is
/// **3 separate keybindings** (`D`/`H`/`Z` below), each with its own
/// `TextInput` and format-specific parser
/// ([`parse_date`]/[`parse_time`]/[`parse_time_zone_offset`]) — simpler
/// than inventing a new multi-step `ControlState`, and consistent with how
/// every other multi-field command in this crate (memory tag, keyer
/// memory, encoder nudge, dimmer) is already exposed as separate keys with
/// a single delimited `TextInput` buffer rather than a combined flow. No
/// new `ControlState` variant was needed — confirmed before writing any
/// code, per the task brief's explicit STOP-and-report instruction for the
/// alternative case (it does not apply here: the underlying `radio`
/// methods already split the 3-shape command into 3 independent calls,
/// so nothing here risks sending malformed wire data).
///
/// **DVS record/playback (`LM`/`PB`) each get 3 keys** (start/stop/status)
/// rather than collapsing start+stop into one toggle — `start_dvs_recording`/
/// `start_dvs_playback` take a required channel argument (unlike group 5's
/// `ToggleRts`, which has no argument to carry), and `stop_*` takes none,
/// so a single toggle key would need a `Ft991aDisplay`-tracked "currently
/// recording/playing channel" this group does not poll — kept as 3 plain,
/// independently-typed commands instead, matching this file's "full
/// method coverage over collapsing to fit an approximate table count"
/// precedent (groups 2/3/7).
///
/// None of this group's fields are polled into [`Ft991aDisplay`] yet (same
/// "not polled yet" precedent as groups 4/5/6/7/8/9) — see
/// [`initial_list_cursor`].
fn system_tuner_dvs_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            key: 'A',
            label: "Auto-info broadcast on/off (AI)",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleAutoInfo,
            },
        },
        GroupCommand {
            key: 'L',
            label: "VFO-A dial lock on/off (LK)",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleFrequencyLock,
            },
        },
        GroupCommand {
            key: 'R',
            label: "Repeater shift (OS)",
            kind: CommandKind::List {
                options: repeater_shift_options,
                action: SelectAction::SetRepeaterShift,
            },
        },
        GroupCommand {
            key: 'V',
            label: "TX VFO select (FT)",
            kind: CommandKind::List {
                options: tx_vfo_options,
                action: SelectAction::SetTxVfo,
            },
        },
        GroupCommand {
            key: 'X',
            label: "MOX on/off (MX)",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleMox,
            },
        },
        GroupCommand {
            key: 'T',
            label: "Antenna tuner state (AC)",
            kind: CommandKind::List {
                options: antenna_tuner_options,
                action: SelectAction::SetAntennaTunerState,
            },
        },
        GroupCommand {
            key: 'B',
            label: "Set dimmer levels (DA)",
            kind: CommandKind::Text {
                prompt: "Enter as 'led:tft', LED 1-2, TFT 0-15 (e.g. 2:8):",
                action: InputAction::SetDimmer,
            },
        },
        GroupCommand {
            key: 'K',
            label: "Get dimmer levels (DA read)",
            kind: CommandKind::Immediate(get_dimmer_immediate),
        },
        GroupCommand {
            key: 'D',
            label: "Set date (DT P1=0)",
            kind: CommandKind::Text {
                prompt: "Enter as YYYYMMDD, e.g. 20260719:",
                action: InputAction::SetDate,
            },
        },
        GroupCommand {
            key: 'Y',
            label: "Read date (DT P1=0)",
            kind: CommandKind::Immediate(read_date_immediate),
        },
        GroupCommand {
            key: 'H',
            label: "Set time (DT P1=1, 24h UTC)",
            kind: CommandKind::Text {
                prompt: "Enter as HHMMSS, 24h UTC, e.g. 143000:",
                action: InputAction::SetTime,
            },
        },
        GroupCommand {
            key: 'N',
            label: "Read time (DT P1=1)",
            kind: CommandKind::Immediate(read_time_immediate),
        },
        GroupCommand {
            key: 'Z',
            label: "Set time zone offset (DT P1=2)",
            kind: CommandKind::Text {
                prompt: "Enter as +HHMM or -HHMM, step 30 min, e.g. +0930:",
                action: InputAction::SetTimeZoneOffset,
            },
        },
        GroupCommand {
            key: 'F',
            label: "Read time zone offset (DT P1=2)",
            kind: CommandKind::Immediate(read_time_zone_offset_immediate),
        },
        GroupCommand {
            key: 'I',
            label: "Read opposite-band status (OI)",
            kind: CommandKind::Immediate(get_opposite_band_information_immediate),
        },
        GroupCommand {
            key: 'W',
            label: "TXW on/off (TS)",
            kind: CommandKind::List {
                options: on_off_options,
                action: SelectAction::ToggleTxw,
            },
        },
        GroupCommand {
            key: 'C',
            label: "Start DVS recording (LM)",
            kind: CommandKind::Text {
                prompt: "Enter DVS channel to record, 1-5:",
                action: InputAction::StartDvsRecording,
            },
        },
        GroupCommand {
            key: 'E',
            label: "Stop DVS recording (LM)",
            kind: CommandKind::Immediate(stop_dvs_recording_immediate),
        },
        GroupCommand {
            key: 'G',
            label: "Get DVS recording status (LM read)",
            kind: CommandKind::Immediate(get_dvs_recording_channel_immediate),
        },
        GroupCommand {
            key: 'P',
            label: "Start DVS playback (PB)",
            kind: CommandKind::Text {
                prompt: "Enter DVS channel to play, 1-5:",
                action: InputAction::StartDvsPlayback,
            },
        },
        GroupCommand {
            key: 'S',
            label: "Stop DVS playback (PB)",
            kind: CommandKind::Immediate(stop_dvs_playback_immediate),
        },
        GroupCommand {
            key: 'U',
            label: "Get DVS playback status (PB read)",
            kind: CommandKind::Immediate(get_dvs_playback_channel_immediate),
        },
    ]
}

// ---------------------------------------------------------------------------
// EX menu themed sub-groups (§11.4, path (a), Wave 4 Task 9)
// ---------------------------------------------------------------------------

/// The `EX` menu's themed browsing sub-groups, boundaries per
/// `planning/architect/task_plan.md` §11.4.
///
/// **Discrepancy flagged, not silently resolved**: §11.4's own prose calls
/// this "five sub-groups" but then lists six labeled `p1` ranges
/// (001-046/047-079/080-091/092-110/111-136/137-153) — an off-by-one
/// somewhere in the architect's own text. Resolved by counting the ranges
/// actually given rather than trusting the stated count: **six** variants
/// below.
///
/// The 79/80 and 91/92 boundaries (the one the architect's own text flagged
/// as needing verification, "080-091 (mixed — verify exact boundary against
/// the manual when building this)") were checked against
/// [`radio::EX_MENU_TABLE`]'s actual item names during this task: `079`
/// "FM PKT MODE" (last of the TX audio chain) is immediately followed by
/// `080` "RPT SHIFT 28MHz" (first of the mixed grab-bag: repeater
/// shift/ARS/DCS polarity/GM display/distance/standby beep), and `091`
/// "STANDBY BEEP" (last of the mixed group) is immediately followed by
/// `092` "RTTY LCUT FREQ" (first of the RTTY/SSB TX chain) — both land on
/// clean thematic breaks in the real table, no boundary adjustment needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExTheme {
    /// 001-046: general / AGC / CW.
    GeneralAgcCw,
    /// 047-079: TX audio chain, including the PTT/port-select family
    /// (e.g. `060` "PC KEYING", the item Wave 4 Task 8's RTS/DTR feature
    /// actually needs — §10.6's priority carve-out).
    TxAudioChain,
    /// 080-091: mixed (repeater shift, ARS, DCS polarity, GM display,
    /// distance unit, AMS TX mode, standby beep) — see this enum's own doc
    /// comment for the boundary verification.
    Mixed,
    /// 092-110: RTTY/SSB TX chain.
    RttySsbTxChain,
    /// 111-136: meter/scope.
    MeterScope,
    /// 137-153: band-limit/VOX.
    BandLimitVox,
}

/// All 6 themes, in §11.4's own listed order.
const ALL_EX_THEMES: [ExTheme; 6] = [
    ExTheme::GeneralAgcCw,
    ExTheme::TxAudioChain,
    ExTheme::Mixed,
    ExTheme::RttySsbTxChain,
    ExTheme::MeterScope,
    ExTheme::BandLimitVox,
];

/// This theme's inclusive `p1` boundaries, per §11.4.
fn ex_theme_range(theme: ExTheme) -> (u16, u16) {
    match theme {
        ExTheme::GeneralAgcCw => (1, 46),
        ExTheme::TxAudioChain => (47, 79),
        ExTheme::Mixed => (80, 91),
        ExTheme::RttySsbTxChain => (92, 110),
        ExTheme::MeterScope => (111, 136),
        ExTheme::BandLimitVox => (137, 153),
    }
}

/// The human-readable label shown for this theme in `CommandGroup::ExMenu`'s
/// own `GroupMenu` screen (the "theme picker").
pub(crate) fn ex_theme_label(theme: ExTheme) -> &'static str {
    match theme {
        ExTheme::GeneralAgcCw => "General / AGC / CW (001-046)",
        ExTheme::TxAudioChain => "TX Audio Chain (047-079)",
        ExTheme::Mixed => "Mixed (080-091)",
        ExTheme::RttySsbTxChain => "RTTY / SSB TX Chain (092-110)",
        ExTheme::MeterScope => "Meter / Scope (111-136)",
        ExTheme::BandLimitVox => "Band-Limit / VOX (137-153)",
    }
}

/// The `GroupMenu`-level key, within `CommandGroup::ExMenu`'s own command
/// list, that opens this theme's sub-group list. A separate namespace from
/// [`group_key`]'s top-level keys (see that function's own doc comment on
/// why key reuse across levels isn't a collision) — verified unique against
/// each other and against [`EX_NUMBER_ENTRY_KEY`] (also folded into this
/// same command list, see [`ex_menu_commands`]) by
/// `test_ex_menu_group_keys_unique` below.
fn ex_theme_key(theme: ExTheme) -> char {
    match theme {
        ExTheme::GeneralAgcCw => 'G',
        ExTheme::TxAudioChain => 'T',
        ExTheme::Mixed => 'X',
        ExTheme::RttySsbTxChain => 'R',
        ExTheme::MeterScope => 'S',
        ExTheme::BandLimitVox => 'B',
    }
}

/// Bucket [`radio::EX_MENU_TABLE`]'s **actual** `p1` values into `theme`'s
/// range, computed at call time from the real table — per §11.4's explicit
/// instruction ("bucketing `EX_MENU_TABLE`'s actual `p1` values at runtime
/// from the real table, not a hand-maintained parallel list in `ui`"), the
/// same table-driven principle [`ctcss_tone_options`]/[`dcs_code_options`]
/// already established for their own tables (avoids drift as more `EX`
/// sub-batches land in `radio`). Returned in the table's own ascending
/// `p1` order.
pub(crate) fn ex_theme_items(theme: ExTheme) -> Vec<&'static ExMenuItem> {
    let (lo, hi) = ex_theme_range(theme);
    EX_MENU_TABLE
        .iter()
        .filter(|item| item.p1 >= lo && item.p1 <= hi)
        .collect()
}

/// Group 12 (`ExMenu`)'s command list — the theme picker (§11.4, Wave 4
/// Task 9): one [`CommandKind::ExSubGroup`] entry per [`ExTheme`], plus the
/// number-entry escape hatch (path (b), Wave 4 Task 8) folded in as one
/// more entry (`EX_NUMBER_ENTRY_KEY`, same key as its existing dedicated
/// `Menu`-level binding — reachable both ways, per this task's own
/// discoverability judgment call: keep the fast top-level shortcut for
/// users who already know a `p1` by heart, but also surface it from inside
/// the group screen for users browsing who might not remember `[N]`
/// exists at all).
fn ex_menu_commands() -> Vec<GroupCommand> {
    let mut commands: Vec<GroupCommand> = ALL_EX_THEMES
        .iter()
        .map(|&theme| GroupCommand {
            key: ex_theme_key(theme),
            label: ex_theme_label(theme),
            kind: CommandKind::ExSubGroup(theme),
        })
        .collect();
    commands.push(GroupCommand {
        key: EX_NUMBER_ENTRY_KEY,
        label: "Enter menu number directly",
        kind: CommandKind::EnterExNumberEntry,
    });
    commands
}

fn group_commands(group: CommandGroup) -> Vec<GroupCommand> {
    match group {
        CommandGroup::FrequencyLevels => frequency_levels_commands(),
        CommandGroup::VfoMemoryQuickOps => vfo_memory_quick_ops_commands(),
        CommandGroup::MemoryChannels => memory_channels_commands(),
        CommandGroup::ClarifierToneIfShift => clarifier_tone_if_shift_commands(),
        CommandGroup::KeyerCwBreakIn => keyer_cw_break_in_commands(),
        CommandGroup::ScanVoxBusy => scan_vox_busy_commands(),
        CommandGroup::AttenuatorNoiseAgcNotchFilter => attenuator_noise_agc_notch_filter_commands(),
        CommandGroup::SpeechMicMonitor => speech_mic_monitor_commands(),
        CommandGroup::BandStepEncoder => band_step_encoder_commands(),
        CommandGroup::MetersStatus => meters_status_commands(),
        CommandGroup::SystemTunerDvs => system_tuner_dvs_commands(),
        CommandGroup::ExMenu => ex_menu_commands(),
    }
}

fn find_group_command(group: CommandGroup, key: char) -> Option<GroupCommand> {
    let key = key.to_ascii_uppercase();
    group_commands(group).into_iter().find(|c| c.key == key)
}

/// Return the `(key, label)` pairs for rendering a group's `GroupMenu`
/// screen. All 12 groups are populated as of Wave 4's dispatch queue
/// completing (this task); `layout::draw_control_panel`'s "no commands yet"
/// placeholder is now dead code in practice but left in place as a
/// defensive fallback, not removed.
pub(crate) fn group_command_labels(group: CommandGroup) -> Vec<(char, &'static str)> {
    group_commands(group)
        .into_iter()
        .map(|c| (c.key, c.label))
        .collect()
}

// ---------------------------------------------------------------------------
// EX menu value-entry fork — shared by both access paths (§11.4)
// ---------------------------------------------------------------------------

/// Where `Esc` from [`enter_ex_value_entry`]'s output state should return
/// to — the one behavioral difference between `EX`'s two access paths.
/// §11.4's own pseudocode gives value-entry's `Esc` two distinct targets:
/// "`Esc` -> back to `ExNumberEntry` (path (b)) or the `EX` group screen
/// (path (a))". Everything else about the fork — which `ControlState`
/// variant, its `prompt`/`options`, the eventual `ExecuteAction` — is
/// identical between paths for the same `p1` (see
/// `test_path_a_and_path_b_converge_*` below); only this routing differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExValueEntryOrigin {
    /// Path (b): return to `ExNumberEntry`, `buffer` restored to the `p1`
    /// already entered.
    NumberEntry,
    /// Path (a): return to the originating sub-group list, cursor restored
    /// to the item that was selected.
    Theme(ExTheme, usize),
}

/// Build the value-entry state for an already-looked-up `EX` menu item,
/// forking on [`ExMenuItem::kind`] — the "`ExValueEntry`" step of §11.4's
/// state machine, shared verbatim by both access paths (path (b)'s
/// `ExNumberEntry` and path (a)'s `ExSubGroupMenu`, Wave 4 Tasks 8 and 9
/// respectively — neither duplicates this function, both call it). Not a
/// distinct [`ControlState`] variant: per §11.4's own wording ("reuse
/// `ControlState::ListSelect` verbatim" / "reuse `ControlState::TextInput`
/// verbatim"), the fork lands directly in one of the two existing states.
/// `origin` selects which `InputAction`/`SelectAction` variant is attached
/// (`SetExMenuItem` vs. `SetExMenuItemFromTheme`), which in turn determines
/// where the fork's own `Esc` routes back to — see
/// [`ExValueEntryOrigin`].
///
/// **Read-first-then-edit skipped** (§11.4 flags this as recommended, not
/// required): would need an async `get_ex_menu_item` call to pre-fill/
/// pre-select the current value, but `handle_key` is synchronous and has no
/// `&mut R: Radio` access — only `&Ft991aDisplay`, which has no per-`p1`
/// `EX` value cache (unlike `SetMode`'s `initial_list_cursor`, which reads
/// an already-polled `display.mode` field). Forcing it would require either
/// polling all 151 `EX` items into `Ft991aDisplay` every cycle (wasteful —
/// they're rarely-changed settings, not live operating state) or making
/// `handle_key` async (a much larger, out-of-scope change to the existing
/// synchronous `handle_key`/`execute_action` split every other group
/// relies on). Cursor/buffer default to the same "not polled yet" starting
/// point every other not-yet-polled `ListSelect` field in this module
/// already uses (see `initial_list_cursor`'s doc comments) — a documented
/// limitation, not silently absorbed.
fn enter_ex_value_entry(item: &'static ExMenuItem, origin: ExValueEntryOrigin) -> ControlState {
    match item.kind {
        ExMenuValueKind::Enumerated(values) => ControlState::ListSelect {
            options: values.iter().map(|(_, label)| label.to_string()).collect(),
            cursor: 0,
            action: match origin {
                ExValueEntryOrigin::NumberEntry => SelectAction::SetExMenuItem(item.p1),
                ExValueEntryOrigin::Theme(theme, cursor) => {
                    SelectAction::SetExMenuItemFromTheme(item.p1, theme, cursor)
                }
            },
        },
        ExMenuValueKind::Range { min, max, step, .. } => ControlState::TextInput {
            prompt: format!("{} ({min}..={max}, step {step}):", item.name),
            buffer: String::new(),
            error: None,
            action: match origin {
                ExValueEntryOrigin::NumberEntry => InputAction::SetExMenuItem(item.p1),
                ExValueEntryOrigin::Theme(theme, cursor) => {
                    InputAction::SetExMenuItemFromTheme(item.p1, theme, cursor)
                }
            },
        },
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate text input for the given [`InputAction`], returning a ready-to-
/// execute [`ExecuteAction`] or a human-readable error message.
///
/// Validation ranges per §6.5 — confirmed against `radio/src/radio_trait.rs`
/// and `radio/src/ft991a.rs` doc comments, NOT assumed from ts570d parity:
/// frequency uses `Frequency::MIN_HZ`/`MAX_HZ` (30 kHz-470 MHz, not
/// ts570d's 0.5-60 MHz), AF/RF gain 0-255, squelch 0-100 (not 255), TX
/// power 5-100 watts. Unchanged from Wave 2 — group 1's port preserves
/// these exactly (§11.2).
fn validate_text_input(action: InputAction, buffer: &str) -> Result<ExecuteAction, String> {
    match action {
        InputAction::SetVfoA | InputAction::SetVfoB => {
            let hz: u64 = buffer.trim().parse().map_err(|_| {
                format!(
                    "Enter a whole number of Hz ({}-{})",
                    Frequency::MIN_HZ,
                    Frequency::MAX_HZ
                )
            })?;
            match Frequency::new(hz) {
                Ok(freq) => Ok(if action == InputAction::SetVfoA {
                    ExecuteAction::SetVfoA(freq.hz())
                } else {
                    ExecuteAction::SetVfoB(freq.hz())
                }),
                Err(_) => Err(format!(
                    "Frequency must be {}-{} Hz",
                    Frequency::MIN_HZ,
                    Frequency::MAX_HZ
                )),
            }
        }
        InputAction::SetAfGain | InputAction::SetRfGain => {
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-255".to_string())?;
            if v > 255 {
                return Err("Value must be 0-255".to_string());
            }
            Ok(if action == InputAction::SetAfGain {
                ExecuteAction::SetAfGain(v as u8)
            } else {
                ExecuteAction::SetRfGain(v as u8)
            })
        }
        InputAction::SetSquelch => {
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-100".to_string())?;
            if v > 100 {
                return Err("Value must be 0-100".to_string());
            }
            Ok(ExecuteAction::SetSquelch(v as u8))
        }
        InputAction::SetPower => {
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 5-100".to_string())?;
            if !(5..=100).contains(&v) {
                return Err("Value must be 5-100".to_string());
            }
            Ok(ExecuteAction::SetPower(v as u8))
        }

        // --- Group 3 (`MemoryChannels`): 1-117 (`MC`/`MR`/`MW`/`MT`'s
        // common P1 range, `radio/src/ft991a.rs` lines ~910-995) ---
        InputAction::SelectMemoryChannel => {
            parse_memory_channel(buffer).map(ExecuteAction::SelectMemoryChannel)
        }
        InputAction::ReadMemoryChannel => {
            parse_memory_channel(buffer).map(ExecuteAction::ReadMemoryChannel)
        }
        InputAction::WriteMemoryChannelFromVfoA => {
            parse_memory_channel(buffer).map(ExecuteAction::WriteMemoryChannelFromVfoA)
        }
        InputAction::ReadMemoryChannelTag => {
            parse_memory_channel(buffer).map(ExecuteAction::ReadMemoryChannelTag)
        }
        InputAction::WriteMemoryChannelTagFromVfoA => {
            // "channel:tag" — split on the *first* ':' only, so a tag that
            // legitimately contains a colon (a legal `MemoryTag` character)
            // still round-trips correctly.
            if !buffer.contains(':') {
                return Err("Enter as 'channel:tag', e.g. 5:HOME".to_string());
            }
            let mut parts = buffer.splitn(2, ':');
            let ch_str = parts.next().unwrap_or("");
            let tag_str = parts.next().unwrap_or("");
            let ch = parse_memory_channel(ch_str)?;
            let tag = MemoryTag::new(tag_str).map_err(|_| {
                format!(
                    "Tag must be <= {} printable ASCII chars, no ';'",
                    MemoryTag::MAX_LEN
                )
            })?;
            Ok(ExecuteAction::WriteMemoryChannelTagFromVfoA(
                ch,
                tag.as_str().to_string(),
            ))
        }

        // --- Group 4 (`ClarifierToneIfShift`) ---
        InputAction::ClarifierDown | InputAction::ClarifierUp => {
            // 0-9999 Hz (`RD`/`RU`, `radio/src/ft991a.rs` lines ~1138-1158).
            let v: u32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-9999".to_string())?;
            if v > 9999 {
                return Err("Value must be 0-9999".to_string());
            }
            Ok(if action == InputAction::ClarifierDown {
                ExecuteAction::ClarifierDown(v as u16)
            } else {
                ExecuteAction::ClarifierUp(v as u16)
            })
        }
        InputAction::SetIfShift => {
            // -1200..=1200 Hz, 20 Hz steps (`IS`, `radio/src/ft991a.rs`
            // lines ~1176-1188).
            let v: i32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number -1200 to 1200 (step 20)".to_string())?;
            if !(-1200..=1200).contains(&v) || v % 20 != 0 {
                return Err("Value must be -1200 to 1200, in steps of 20".to_string());
            }
            Ok(ExecuteAction::SetIfShift(v as i16))
        }

        // --- Group 6 (`ScanVoxBusy`) ---
        InputAction::SetVoxGain => {
            // 0-100 (`VG`, `radio/src/ft991a.rs` lines ~1512-1522).
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-100".to_string())?;
            if v > 100 {
                return Err("Value must be 0-100".to_string());
            }
            Ok(ExecuteAction::SetVoxGain(v as u8))
        }
        InputAction::SetVoxDelay => {
            // 30-3000 ms, 10 ms steps (`VD`, `radio/src/ft991a.rs` lines
            // ~1542-1554).
            let v: u32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 30-3000 (step 10)".to_string())?;
            if !(30..=3000).contains(&v) || v % 10 != 0 {
                return Err("Value must be 30-3000, in steps of 10".to_string());
            }
            Ok(ExecuteAction::SetVoxDelay(v as u16))
        }

        // --- Group 5 (`KeyerCwBreakIn`) ---
        InputAction::SetSemiBreakInDelay => {
            // 30-3000 ms, no step constraint (`SD`, `radio/src/ft991a.rs`
            // lines ~1442-1461 — unlike `VD` above, `set_semi_break_in_delay`
            // has no modulus check, only the range check).
            let v: u32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 30-3000".to_string())?;
            if !(30..=3000).contains(&v) {
                return Err("Value must be 30-3000".to_string());
            }
            Ok(ExecuteAction::SetSemiBreakInDelay(v as u16))
        }
        InputAction::SetKeyerSpeed => {
            // 4-60 WPM (`KS`, `radio/src/ft991a.rs` lines ~1378-1396).
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 4-60".to_string())?;
            if !(4..=60).contains(&v) {
                return Err("Value must be 4-60".to_string());
            }
            Ok(ExecuteAction::SetKeyerSpeed(v as u8))
        }
        InputAction::SetKeyerPitchHz => {
            // 300-1050 Hz, 10 Hz steps above 300 (`KP`, `radio/src/ft991a.rs`
            // lines ~1334-1357).
            let v: u32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 300-1050 (step 10)".to_string())?;
            if !(300..=1050).contains(&v) || (v - 300) % 10 != 0 {
                return Err("Value must be 300-1050, in steps of 10".to_string());
            }
            Ok(ExecuteAction::SetKeyerPitchHz(v as u16))
        }
        InputAction::ReadKeyerMemory => {
            parse_keyer_channel(buffer).map(ExecuteAction::ReadKeyerMemory)
        }
        InputAction::WriteKeyerMemory => {
            // "channel:message" — split on the *first* ':' only, same
            // convention as `WriteMemoryChannelTagFromVfoA` (group 3), so a
            // message that legitimately contains a colon still round-trips.
            // Message: 1-50 printable ASCII characters, no ';' (`KM` write,
            // `radio/src/ft991a.rs` lines ~1281-1302 — this implementation
            // cannot write an empty message, so the lower bound is 1, not
            // 0).
            if !buffer.contains(':') {
                return Err("Enter as 'channel:message', e.g. 3:CQ CQ DE".to_string());
            }
            let mut parts = buffer.splitn(2, ':');
            let ch_str = parts.next().unwrap_or("");
            let msg_str = parts.next().unwrap_or("");
            let ch = parse_keyer_channel(ch_str)?;
            let len = msg_str.chars().count();
            if !(1..=50).contains(&len)
                || !msg_str
                    .chars()
                    .all(|c| (' '..='~').contains(&c) && c != ';')
            {
                return Err("Message must be 1-50 printable ASCII chars, no ';'".to_string());
            }
            Ok(ExecuteAction::WriteKeyerMemory(ch, msg_str.to_string()))
        }
        InputAction::PlayKeyerMemory => {
            parse_keyer_channel(buffer).map(ExecuteAction::PlayKeyerMemory)
        }
        InputAction::PlayMessageKeyer => {
            parse_keyer_channel(buffer).map(ExecuteAction::PlayMessageKeyer)
        }

        // --- Group 9 (`BandStepEncoder`) ---
        InputAction::EncoderDown | InputAction::EncoderUp => {
            // "encoder:steps" — split on the *first* ':' only, same
            // convention as `WriteMemoryChannelTagFromVfoA`/
            // `WriteKeyerMemory`. Encoder name 1-99 step count, see
            // `parse_encoder_selector`/`parse_encoder_steps`.
            if !buffer.contains(':') {
                return Err(
                    "Enter as 'encoder:steps', e.g. main:5 (encoder=main/sub/multi)".to_string(),
                );
            }
            let mut parts = buffer.splitn(2, ':');
            let enc_str = parts.next().unwrap_or("");
            let steps_str = parts.next().unwrap_or("");
            let encoder = parse_encoder_selector(enc_str)?;
            let steps = parse_encoder_steps(steps_str)?;
            Ok(if action == InputAction::EncoderDown {
                ExecuteAction::EncoderDown(encoder, steps)
            } else {
                ExecuteAction::EncoderUp(encoder, steps)
            })
        }

        // --- Group 7 (`AttenuatorNoiseAgcNotchFilter`) ---
        InputAction::SetNoiseBlankerLevel => {
            // 0-10 (`NL`, `radio/src/ft991a.rs` lines ~1638-1659,
            // `RadioError::InvalidNoiseBlankerLevel`).
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-10".to_string())?;
            if v > 10 {
                return Err("Value must be 0-10".to_string());
            }
            Ok(ExecuteAction::SetNoiseBlankerLevel(v as u8))
        }
        InputAction::SetNoiseReductionLevel => {
            // 1-15, no 0 (`RL`, `radio/src/ft991a.rs` lines ~1681-1702,
            // `RadioError::InvalidNoiseReductionLevel`).
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 1-15".to_string())?;
            if !(1..=15).contains(&v) {
                return Err("Value must be 1-15".to_string());
            }
            Ok(ExecuteAction::SetNoiseReductionLevel(v as u8))
        }
        InputAction::SetContourFrequencyHz => {
            // 10-3200 Hz, no step constraint (`CO` `P2=1`,
            // `radio/src/ft991a.rs` lines ~1747-1767,
            // `RadioError::InvalidContourFrequency`).
            let v: u32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 10-3200".to_string())?;
            if !(10..=3200).contains(&v) {
                return Err("Value must be 10-3200".to_string());
            }
            Ok(ExecuteAction::SetContourFrequencyHz(v as u16))
        }
        InputAction::SetApfFrequencyHz => {
            // -250..=250 Hz, 10 Hz steps (`CO` `P2=3`,
            // `radio/src/ft991a.rs` lines ~1790-1815,
            // `RadioError::InvalidApfFrequency`).
            let v: i32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number -250 to 250 (step 10)".to_string())?;
            if !(-250..=250).contains(&v) || v % 10 != 0 {
                return Err("Value must be -250 to 250, in steps of 10".to_string());
            }
            Ok(ExecuteAction::SetApfFrequencyHz(v as i16))
        }
        InputAction::SetManualNotchFrequencyHz => {
            // 10-3200 Hz, 10 Hz steps (`BP` `P2=1`,
            // `radio/src/ft991a.rs` lines ~1837-1861,
            // `RadioError::InvalidManualNotchFrequency`).
            let v: u32 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 10-3200 (step 10)".to_string())?;
            if !(10..=3200).contains(&v) || v % 10 != 0 {
                return Err("Value must be 10-3200, in steps of 10".to_string());
            }
            Ok(ExecuteAction::SetManualNotchFrequencyHz(v as u16))
        }

        // --- Group 8 (`SpeechMicMonitor`) ---
        InputAction::SetMicGain => {
            // 0-100 (`MG`, `radio/src/ft991a.rs` lines ~1931-1954,
            // `RadioError::InvalidMicGain`).
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-100".to_string())?;
            if v > 100 {
                return Err("Value must be 0-100".to_string());
            }
            Ok(ExecuteAction::SetMicGain(v as u8))
        }
        InputAction::SetSpeechProcessorLevel => {
            // 0-100 (`PL`, `radio/src/ft991a.rs` lines ~1956-1982,
            // `RadioError::InvalidSpeechProcessorLevel`).
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-100".to_string())?;
            if v > 100 {
                return Err("Value must be 0-100".to_string());
            }
            Ok(ExecuteAction::SetSpeechProcessorLevel(v as u8))
        }
        InputAction::SetMonitorLevel => {
            // 0-100 (`ML` `P1=1`, `radio/src/ft991a.rs` lines ~2044-2064,
            // `RadioError::InvalidMonitorLevel`).
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-100".to_string())?;
            if v > 100 {
                return Err("Value must be 0-100".to_string());
            }
            Ok(ExecuteAction::SetMonitorLevel(v as u8))
        }

        // --- Group 11 (`SystemTunerDvs`) ---
        InputAction::SetDimmer => {
            parse_dimmer(buffer).map(|(led, tft)| ExecuteAction::SetDimmer(led, tft))
        }
        InputAction::SetDate => parse_date(buffer).map(|(y, m, d)| ExecuteAction::SetDate(y, m, d)),
        InputAction::SetTime => {
            parse_time(buffer).map(|(h, mi, se)| ExecuteAction::SetTime(h, mi, se))
        }
        InputAction::SetTimeZoneOffset => {
            parse_time_zone_offset(buffer).map(ExecuteAction::SetTimeZoneOffset)
        }
        InputAction::StartDvsRecording => {
            parse_dvs_channel(buffer).map(ExecuteAction::StartDvsRecording)
        }
        InputAction::StartDvsPlayback => {
            parse_dvs_channel(buffer).map(ExecuteAction::StartDvsPlayback)
        }

        // --- Group 12 (`ExMenu`), paths (a) and (b) ---
        //
        // Both `InputAction` variants share this one arm (`|`-combined
        // pattern binding `p1` the same way) — same value semantics for
        // either access path, per §11.4 ("the two paths converge on the
        // same value-entry state once an item is selected, they only
        // differ in how the item is found"). The theme/cursor carried by
        // `SetExMenuItemFromTheme` are only consulted by `handle_key`'s
        // `Esc` arm, not here.
        InputAction::SetExMenuItem(p1) | InputAction::SetExMenuItemFromTheme(p1, _, _) => {
            // Generalized off `item.kind`'s own `min`/`max`/`step`, per
            // §11.4 ("validated the same way `validate_text_input` already
            // validates ranges today") — not a hardcoded per-field range
            // like every other arm above.
            let item = ex_menu_item(p1).ok_or_else(|| "No such menu item".to_string())?;
            let ExMenuValueKind::Range { min, max, step, .. } = item.kind else {
                // Unreachable in practice: `enter_ex_value_entry` only ever
                // produces these variants for `Range`-kind items. Defensive,
                // not a real path.
                return Err("This EX item is not a numeric range".to_string());
            };
            let v: i32 = buffer
                .trim()
                .parse()
                .map_err(|_| format!("Enter a number {min}..={max} (step {step})"))?;
            if v < min || v > max || (v - min) % step != 0 {
                return Err(format!("Value must be {min}..={max}, in steps of {step}"));
            }
            Ok(ExecuteAction::SetExMenuItem(p1, v))
        }
    }
}

fn select_action_to_execute(action: SelectAction, cursor: usize) -> ExecuteAction {
    match action {
        SelectAction::SetMode => {
            let mode = MODE_ORDER.get(cursor).copied().unwrap_or(Mode::Usb);
            ExecuteAction::SetMode(mode)
        }
        // cursor 0 = On, cursor 1 = Off — see `on_off_options`'s doc comment.
        SelectAction::ToggleRxClarifier => ExecuteAction::SetRxClarifierOn(cursor == 0),
        SelectAction::ToggleTxClarifier => ExecuteAction::SetTxClarifierOn(cursor == 0),
        SelectAction::SetToneSquelchMode => {
            let mode = TONE_SQUELCH_MODE_ORDER
                .get(cursor)
                .copied()
                .unwrap_or(ToneSquelchMode::Off);
            ExecuteAction::SetToneSquelchMode(mode)
        }
        SelectAction::SetCtcssTone => {
            let decihz = CTCSS_TONES_DECIHZ
                .get(cursor)
                .copied()
                .unwrap_or(CTCSS_TONES_DECIHZ[0]);
            ExecuteAction::SetCtcssTone(f32::from(decihz) / 10.0)
        }
        SelectAction::SetDcsCode => {
            let code = DCS_CODES.get(cursor).copied().unwrap_or(DCS_CODES[0]);
            ExecuteAction::SetDcsCode(code)
        }
        SelectAction::SetScanState => {
            let state = SCAN_STATE_ORDER
                .get(cursor)
                .copied()
                .unwrap_or(ScanState::Off);
            ExecuteAction::SetScanState(state)
        }
        SelectAction::ToggleVox => ExecuteAction::SetVoxOn(cursor == 0),
        // --- Group 5 (`KeyerCwBreakIn`) ---
        SelectAction::ToggleBreakIn => ExecuteAction::SetBreakInOn(cursor == 0),
        SelectAction::ToggleCwSpot => ExecuteAction::SetCwSpotOn(cursor == 0),
        SelectAction::ToggleKeyerEnabled => ExecuteAction::SetKeyerEnabled(cursor == 0),
        // --- Group 9 (`BandStepEncoder`) ---
        SelectAction::SetBand => {
            let band = BAND_ORDER.get(cursor).copied().unwrap_or(Band::OneEightMHz);
            ExecuteAction::SetBand(band)
        }
        SelectAction::ToggleFineStep => ExecuteAction::SetFineStep(cursor == 0),
        // --- Group 7 (`AttenuatorNoiseAgcNotchFilter`) ---
        SelectAction::ToggleAttenuator => ExecuteAction::SetAttenuatorOn(cursor == 0),
        SelectAction::SetPreampMode => {
            let mode = PREAMP_ORDER.get(cursor).copied().unwrap_or(PreampMode::Ipo);
            ExecuteAction::SetPreampMode(mode)
        }
        SelectAction::ToggleNoiseBlanker => ExecuteAction::SetNoiseBlankerOn(cursor == 0),
        SelectAction::ToggleNoiseReduction => ExecuteAction::SetNoiseReductionOn(cursor == 0),
        SelectAction::SetAgcMode => {
            let mode = AGC_ORDER.get(cursor).copied().unwrap_or(AgcMode::Off);
            ExecuteAction::SetAgcMode(mode)
        }
        SelectAction::ToggleAutoNotch => ExecuteAction::SetAutoNotchOn(cursor == 0),
        SelectAction::ToggleNarrow => ExecuteAction::SetNarrowOn(cursor == 0),
        SelectAction::SetFilterWidthIndex => {
            // Cursor is already bounded to `0..=21` by `ListSelect`'s own
            // navigation (`filter_width_options` returns exactly 22
            // entries) — `min(21)` is a defensive clamp, not a real
            // out-of-range path.
            ExecuteAction::SetFilterWidthIndex(cursor.min(21) as u8)
        }
        SelectAction::ToggleContour => ExecuteAction::SetContourOn(cursor == 0),
        SelectAction::ToggleApf => ExecuteAction::SetApfOn(cursor == 0),
        SelectAction::ToggleManualNotch => ExecuteAction::SetManualNotchOn(cursor == 0),
        // --- Group 8 (`SpeechMicMonitor`) ---
        SelectAction::ToggleSpeechProcessor => ExecuteAction::SetSpeechProcessorOn(cursor == 0),
        SelectAction::ToggleMonitor => ExecuteAction::SetMonitorOn(cursor == 0),
        SelectAction::ToggleParametricMicEq => ExecuteAction::SetParametricMicEqOn(cursor == 0),
        // --- Group 10 (`MetersStatus`) ---
        SelectAction::SelectMeter => {
            let meter = METER_ORDER.get(cursor).copied().unwrap_or(Meter::Comp);
            ExecuteAction::SelectMeter(meter)
        }
        SelectAction::ReadMeterDirect => {
            let meter = METER_ORDER.get(cursor).copied().unwrap_or(Meter::Comp);
            ExecuteAction::ReadMeterDirect(meter)
        }
        SelectAction::ReadRadioIndicator => {
            let indicator = RADIO_INDICATOR_ORDER
                .get(cursor)
                .copied()
                .unwrap_or(RadioIndicator::HiSwr);
            ExecuteAction::GetRadioIndicator(indicator)
        }
        // --- Group 11 (`SystemTunerDvs`) ---
        SelectAction::ToggleAutoInfo => ExecuteAction::SetAutoInfoOn(cursor == 0),
        SelectAction::ToggleFrequencyLock => ExecuteAction::SetFrequencyLock(cursor == 0),
        SelectAction::SetRepeaterShift => {
            let shift = REPEATER_SHIFT_ORDER
                .get(cursor)
                .copied()
                .unwrap_or(RepeaterShift::Simplex);
            ExecuteAction::SetRepeaterShift(shift)
        }
        // Cursor is bounded to `0..=1` by `tx_vfo_options`'s 2 entries —
        // `min(1)` is a defensive clamp, not a real out-of-range path.
        SelectAction::SetTxVfo => ExecuteAction::SetTxVfo(cursor.min(1) as u8),
        SelectAction::ToggleMox => ExecuteAction::SetMoxOn(cursor == 0),
        // Cursor is bounded to `0..=2` by `antenna_tuner_options`'s 3
        // entries — `min(2)` is a defensive clamp, not a real
        // out-of-range path.
        SelectAction::SetAntennaTunerState => {
            ExecuteAction::SetAntennaTunerState(cursor.min(2) as u8)
        }
        SelectAction::ToggleTxw => ExecuteAction::SetTxwOn(cursor == 0),
        // --- Group 12 (`ExMenu`), paths (a) and (b) ---
        //
        // Both `SelectAction` variants share this one arm (`|`-combined
        // pattern, same rationale as `validate_text_input`'s combined
        // `InputAction` arm above) — `cursor` here is `ListSelect`'s own
        // confirm-time selection index, not the theme-browsing cursor
        // `SetExMenuItemFromTheme` separately carries (that one is only
        // consulted by `handle_key`'s `Esc` arm).
        //
        // `cursor` indexes the same `values` slice `enter_ex_value_entry`
        // built `options`'s labels from, in the same order — re-look-up the
        // item by `p1` to recover it (`ListSelect`'s shared shape has no
        // room for the `&'static [(&str, &str)]` itself). Every landed
        // `Enumerated` wire value is a plain unsigned decimal digit string
        // (no sign character — that's a `Range`-only concept, see
        // `ExMenuValueKind::Range`'s `signed` field), so a plain
        // `str::parse` is sufficient; no need for `ExMenuValueKind`'s own
        // `pub(crate)` `parse`/`format` helpers (not visible outside
        // `radio`). Falls back to `0` if the lookup or index is somehow
        // stale (defensive, same "clamp to a sane default" precedent as
        // `SetFilterWidthIndex`/`SetAntennaTunerState` above) — not a real
        // path since `p1`/`cursor` are always produced together by
        // `enter_ex_value_entry`/`ListSelect`'s own bounded navigation.
        SelectAction::SetExMenuItem(p1) | SelectAction::SetExMenuItemFromTheme(p1, _, _) => {
            let value = ex_menu_item(p1)
                .and_then(|item| match item.kind {
                    ExMenuValueKind::Enumerated(values) => values
                        .get(cursor)
                        .and_then(|(wire, _)| wire.parse::<i32>().ok()),
                    ExMenuValueKind::Range { .. } => None,
                })
                .unwrap_or(0);
            ExecuteAction::SetExMenuItem(p1, value)
        }
    }
}

/// Return the cursor index that should be pre-selected when a list opens,
/// based on the current radio state, so the highlight starts on the active
/// value.
fn initial_list_cursor(action: SelectAction, display: &Ft991aDisplay) -> usize {
    match action {
        SelectAction::SetMode => MODE_ORDER
            .iter()
            .position(|m| *m == display.mode)
            .unwrap_or(0),
        // Groups 4/6's fields (clarifier on/off, tone squelch mode, CTCSS
        // tone, DCS code, scan state, VOX on/off) are not polled into
        // `Ft991aDisplay` yet — this task only populates the command
        // groups, it does not extend polling/display (out of scope per the
        // task brief). No live-value pre-select is possible for these
        // yet; defaulting to cursor 0 is a documented judgment call/known
        // limitation, not silently absorbed. Revisit if a future task adds
        // these fields to `Ft991aDisplay`.
        SelectAction::ToggleRxClarifier
        | SelectAction::ToggleTxClarifier
        | SelectAction::SetToneSquelchMode
        | SelectAction::SetCtcssTone
        | SelectAction::SetDcsCode
        | SelectAction::SetScanState
        | SelectAction::ToggleVox
        // Group 5's break-in/CW-spot/keyer-enabled fields are likewise not
        // polled into `Ft991aDisplay` yet (same documented limitation as
        // groups 4/6 above) — default cursor 0.
        | SelectAction::ToggleBreakIn
        | SelectAction::ToggleCwSpot
        | SelectAction::ToggleKeyerEnabled
        // Group 9's fine step is the same "not polled yet" limitation.
        | SelectAction::ToggleFineStep
        // Group 7's 9 List-backed fields (attenuator, preamp, noise
        // blanker/reduction on/off, AGC mode, auto notch, narrow, filter
        // width, contour/APF/manual-notch on/off) and group 8's 3
        // List-backed fields (speech processor on/off, monitor on/off,
        // parametric mic EQ on/off) are, likewise, not polled into
        // `Ft991aDisplay` yet — same documented "not polled yet" bucket as
        // groups 4/5/6/9 above, not a structural limitation like `SetBand`
        // below.
        | SelectAction::ToggleAttenuator
        | SelectAction::SetPreampMode
        | SelectAction::ToggleNoiseBlanker
        | SelectAction::ToggleNoiseReduction
        | SelectAction::SetAgcMode
        | SelectAction::ToggleAutoNotch
        | SelectAction::ToggleNarrow
        | SelectAction::SetFilterWidthIndex
        | SelectAction::ToggleContour
        | SelectAction::ToggleApf
        | SelectAction::ToggleManualNotch
        | SelectAction::ToggleSpeechProcessor
        | SelectAction::ToggleMonitor
        | SelectAction::ToggleParametricMicEq => 0,
        // Group 9's band select is a *different* limitation from the ones
        // above — `BS` structurally has no `Read`/`Answer` form at all
        // (manual p.3), so there is no live value to poll even in
        // principle, not just "not polled yet" — see
        // `band_step_encoder_commands`'s doc comment. Cursor 0 is the only
        // sensible default.
        SelectAction::SetBand => 0,
        // Group 10's `MS`/`RM`-direct meter pickers are a *different* case
        // again from the "not polled yet" bucket above: `MS`'s current
        // selection genuinely could be polled in principle
        // (`get_selected_meter` exists), just isn't yet (same documented
        // limitation) — but `RI`'s indicator picker has no "current value"
        // concept at all even in principle: each of the 7 selectors is an
        // independent, stateless read (there is no single "currently
        // selected indicator" the way `MS` has a currently selected
        // meter), so cursor 0 there is an arbitrary starting point, not a
        // stand-in for a value this crate could ever pre-select.
        SelectAction::SelectMeter | SelectAction::ReadMeterDirect | SelectAction::ReadRadioIndicator => 0,
        // Group 11's 7 List-backed fields (auto-info, frequency lock,
        // repeater shift, TX VFO, MOX, antenna tuner state, TXW) are,
        // likewise, not polled into `Ft991aDisplay` yet — same documented
        // "not polled yet" bucket as groups 4/5/6/7/8/9 above.
        SelectAction::ToggleAutoInfo
        | SelectAction::ToggleFrequencyLock
        | SelectAction::SetRepeaterShift
        | SelectAction::SetTxVfo
        | SelectAction::ToggleMox
        | SelectAction::SetAntennaTunerState
        | SelectAction::ToggleTxw => 0,
        // Group 12's `EX` escape hatch (both paths): read-first-then-edit
        // was evaluated and skipped (see `enter_ex_value_entry`'s doc
        // comment for the full reasoning) — `enter_ex_value_entry` itself
        // already sets `cursor: 0` when building the `ListSelect`, so this
        // arm is never actually consulted for either `SetExMenuItem`
        // variant (only `GroupMenu`'s `CommandKind::List` path calls
        // `initial_list_cursor`, and no `GroupCommand` produces
        // `SelectAction::SetExMenuItem`/`SetExMenuItemFromTheme` directly —
        // both are only ever reached via `enter_ex_value_entry`). Included
        // for match exhaustiveness and to document the same limitation in
        // one place.
        SelectAction::SetExMenuItem(_) | SelectAction::SetExMenuItemFromTheme(_, _, _) => 0,
    }
}

// ---------------------------------------------------------------------------
// handle_key — the main event handler
// ---------------------------------------------------------------------------

/// Process a key event and transition the control state.
///
/// Returns `KeyResult::Continue`, `KeyResult::Quit`, or `KeyResult::Execute`.
pub fn handle_key(key: KeyEvent, state: &mut ControlState, display: &Ft991aDisplay) -> KeyResult {
    match state {
        ControlState::Menu => match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => KeyResult::Quit,
            KeyCode::Char(c) if c.to_ascii_uppercase() == EX_NUMBER_ENTRY_KEY => {
                *state = ControlState::ExNumberEntry {
                    buffer: String::new(),
                    error: None,
                };
                KeyResult::Continue
            }
            KeyCode::Char(c) if c.to_ascii_uppercase() == PROFILE_LIST_KEY => {
                let dir = radio::default_profile_dir();
                let (profiles, errors) = match &dir {
                    Some(dir) => radio::Profile::load_all_from_dir(dir),
                    None => (Vec::new(), Vec::new()),
                };
                let error = if dir.is_none() {
                    Some("Could not determine profile directory".to_string())
                } else if profiles.is_empty() && errors.is_empty() {
                    Some("No profiles found".to_string())
                } else if let Some((name, err)) = errors.first() {
                    Some(format!("{name}: {err}"))
                } else {
                    None
                };
                *state = ControlState::ProfileList {
                    profiles,
                    cursor: 0,
                    error,
                };
                KeyResult::Continue
            }
            KeyCode::Char(c) => {
                if let Some(group) = group_for_key(c) {
                    *state = ControlState::GroupMenu { group, cursor: 0 };
                }
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::GroupMenu { group, .. } => match key.code {
            KeyCode::Esc => {
                *state = ControlState::Menu;
                KeyResult::Continue
            }
            KeyCode::Char(c) => {
                let group = *group;
                let Some(cmd) = find_group_command(group, c) else {
                    return KeyResult::Continue;
                };
                match cmd.kind {
                    CommandKind::Text { prompt, action } => {
                        *state = ControlState::TextInput {
                            prompt: prompt.to_string(),
                            buffer: String::new(),
                            error: None,
                            action,
                        };
                        KeyResult::Continue
                    }
                    CommandKind::List { options, action } => {
                        let cursor = initial_list_cursor(action, display);
                        *state = ControlState::ListSelect {
                            options: options(),
                            cursor,
                            action,
                        };
                        KeyResult::Continue
                    }
                    CommandKind::Immediate(f) => {
                        let exec = f(display);
                        *state = ControlState::Feedback {
                            message: String::new(),
                            is_error: false,
                        };
                        KeyResult::Execute(exec)
                    }
                    // --- Group 12 (`ExMenu`) theme picker (§11.4 path (a),
                    // Wave 4 Task 9) ---
                    CommandKind::ExSubGroup(theme) => {
                        *state = ControlState::ExSubGroupMenu { theme, cursor: 0 };
                        KeyResult::Continue
                    }
                    CommandKind::EnterExNumberEntry => {
                        *state = ControlState::ExNumberEntry {
                            buffer: String::new(),
                            error: None,
                        };
                        KeyResult::Continue
                    }
                }
            }
            _ => KeyResult::Continue,
        },

        // --- Group 12 (`ExMenu`) themed sub-group list (§11.4 path (a),
        // Wave 4 Task 9) — `cursor` genuinely drives Up/Down scrolling,
        // unlike `GroupMenu`'s own vestigial `cursor` field (see
        // `ControlState::GroupMenu`'s doc comment): sub-groups can hold up
        // to 45 items.
        ControlState::ExSubGroupMenu { theme, cursor } => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
                KeyResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = ex_theme_items(*theme).len().saturating_sub(1);
                if *cursor < max {
                    *cursor += 1;
                }
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let items = ex_theme_items(*theme);
                let Some(&item) = items.get(*cursor) else {
                    // Defensive: only reachable if `EX_MENU_TABLE` were
                    // empty for this theme, which none of the 6 themes are.
                    return KeyResult::Continue;
                };
                *state = enter_ex_value_entry(item, ExValueEntryOrigin::Theme(*theme, *cursor));
                KeyResult::Continue
            }
            KeyCode::Esc => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::ExMenu,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::ProfileList {
            profiles,
            cursor,
            error,
        } => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
                KeyResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = profiles.len().saturating_sub(1);
                if *cursor < max {
                    *cursor += 1;
                }
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let Some((name, profile)) = profiles.get(*cursor) else {
                    *error = Some("No profile selected".to_string());
                    return KeyResult::Continue;
                };
                let exec = ExecuteAction::ApplyProfile(name.clone(), profile.clone());
                *state = ControlState::Feedback {
                    message: String::new(),
                    is_error: false,
                };
                KeyResult::Execute(exec)
            }
            KeyCode::Esc => {
                *state = ControlState::Menu;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::ExNumberEntry { buffer, error } => match key.code {
            KeyCode::Char(c) if c.is_ascii_digit() && buffer.len() < 3 => {
                buffer.push(c);
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Backspace => {
                buffer.pop();
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Enter => {
                if buffer.is_empty() {
                    *error = Some("Enter a menu number (001-153)".to_string());
                    return KeyResult::Continue;
                }
                match buffer.parse::<u16>().ok().and_then(ex_menu_item) {
                    Some(item) => {
                        *state = enter_ex_value_entry(item, ExValueEntryOrigin::NumberEntry);
                    }
                    None => {
                        *error = Some("No such menu item".to_string());
                    }
                }
                KeyResult::Continue
            }
            KeyCode::Esc => {
                *state = ControlState::Menu;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::TextInput {
            buffer,
            error,
            action,
            ..
        } => match key.code {
            KeyCode::Char(c) if c.is_ascii_graphic() => {
                buffer.push(c);
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Backspace => {
                buffer.pop();
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let action = *action;
                let buf = buffer.clone();
                match validate_text_input(action, &buf) {
                    Ok(exec) => {
                        *state = ControlState::Feedback {
                            message: String::new(),
                            is_error: false,
                        };
                        KeyResult::Execute(exec)
                    }
                    Err(msg) => {
                        if let ControlState::TextInput { error, .. } = state {
                            *error = Some(msg);
                        }
                        KeyResult::Continue
                    }
                }
            }
            // §11.4: `Esc` from the EX escape hatch's value-entry fork goes
            // back to wherever the item was found — `ExNumberEntry` (buffer
            // restored to the `p1` already entered, path (b)) or the
            // originating `ExSubGroupMenu` (cursor restored, path (a)) —
            // not `Menu` like every other `TextInput` action.
            KeyCode::Esc => {
                let action = *action;
                *state = match action {
                    InputAction::SetExMenuItem(p1) => ControlState::ExNumberEntry {
                        buffer: p1.to_string(),
                        error: None,
                    },
                    InputAction::SetExMenuItemFromTheme(_, theme, cursor) => {
                        ControlState::ExSubGroupMenu { theme, cursor }
                    }
                    _ => ControlState::Menu,
                };
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::ListSelect {
            options,
            cursor,
            action,
        } => match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
                KeyResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let max = options.len().saturating_sub(1);
                if *cursor < max {
                    *cursor += 1;
                }
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let exec = select_action_to_execute(*action, *cursor);
                *state = ControlState::Feedback {
                    message: String::new(),
                    is_error: false,
                };
                KeyResult::Execute(exec)
            }
            // §11.4: same path-aware redirect as `TextInput`'s `Esc` arm
            // above, for the `Enumerated` fork.
            KeyCode::Esc => {
                *state = match *action {
                    SelectAction::SetExMenuItem(p1) => ControlState::ExNumberEntry {
                        buffer: p1.to_string(),
                        error: None,
                    },
                    SelectAction::SetExMenuItemFromTheme(_, theme, cursor) => {
                        ControlState::ExSubGroupMenu { theme, cursor }
                    }
                    _ => ControlState::Menu,
                };
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::Feedback { .. } => {
            *state = ControlState::Menu;
            KeyResult::Continue
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn display() -> Ft991aDisplay {
        Ft991aDisplay::default()
    }

    fn group_menu(group: CommandGroup) -> ControlState {
        ControlState::GroupMenu { group, cursor: 0 }
    }

    // --- Menu state dispatch (new — grouped-menu skeleton) ---

    #[test]
    fn test_menu_q_quits() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Quit));
    }

    #[test]
    fn test_menu_uppercase_q_quits() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('Q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Quit));
    }

    #[test]
    fn test_menu_unbound_key_stays_in_menu() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('9')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_all_twelve_groups_reachable_from_menu_and_esc_returns() {
        for &group in ALL_GROUPS.iter() {
            let mut state = ControlState::Menu;
            let k = group_key(group);
            let result = handle_key(key(KeyCode::Char(k)), &mut state, &display());
            assert!(matches!(result, KeyResult::Continue));
            match state {
                ControlState::GroupMenu { group: g, cursor } => {
                    assert_eq!(g, group, "key {k:?} entered wrong group");
                    assert_eq!(cursor, 0);
                }
                other => panic!("key {k:?}: expected GroupMenu, got {other:?}"),
            }

            let result = handle_key(key(KeyCode::Esc), &mut state, &display());
            assert!(matches!(result, KeyResult::Continue));
            assert!(matches!(state, ControlState::Menu));
        }
    }

    #[test]
    fn test_menu_group_keys_and_quit_all_unique() {
        let mut keys: Vec<char> = ALL_GROUPS.iter().map(|&g| group_key(g)).collect();
        keys.push('Q');
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            before,
            "group entry keys (+ Quit) must be unique"
        );
        assert_eq!(keys.len(), 13); // 12 groups + Quit
    }

    #[test]
    fn test_group_menu_lowercase_key_also_enters_group() {
        // group_for_key uppercases before matching, same as the old
        // find_command did.
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('f')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::FrequencyLevels,
                ..
            }
        ));
    }

    // --- All 12 groups are now populated (Wave 4's dispatch queue is
    // complete as of this task: group 12 `ExMenu` landed both access paths,
    // Tasks 8-9). ---

    #[test]
    fn test_all_twelve_groups_are_populated() {
        for &group in ALL_GROUPS.iter() {
            assert!(
                !group_command_labels(group).is_empty(),
                "{group:?} should have commands by the end of Wave 4"
            );
        }
    }

    #[test]
    fn test_ex_menu_group_unbound_char_key_is_a_no_op() {
        // Pressing an arbitrary letter not among `ExMenu`'s own 7 command
        // keys (G/T/X/R/S/B/N) must not panic and must not transition
        // state — 'z' is unbound in every group, including this one.
        let mut state = group_menu(CommandGroup::ExMenu);
        let result = handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::ExMenu,
                ..
            }
        ));
    }

    // --- Group 1 (FrequencyLevels): regression-proof port from the old
    // flat Normal screen — same commands, same keys, same validation. ---

    #[test]
    fn test_frequency_levels_group_has_nine_commands() {
        let labels = group_command_labels(CommandGroup::FrequencyLevels);
        assert_eq!(labels.len(), 9);
    }

    #[test]
    fn test_frequency_levels_group_keys_unique() {
        let mut keys: Vec<char> = group_command_labels(CommandGroup::FrequencyLevels)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    #[test]
    fn test_group_menu_f_transitions_to_text_input_vfo_a() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let result = handle_key(key(KeyCode::Char('f')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetVfoA,
                ..
            }
        ));
    }

    #[test]
    fn test_group_menu_b_transitions_to_text_input_vfo_b() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        handle_key(key(KeyCode::Char('B')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetVfoB,
                ..
            }
        ));
    }

    #[test]
    fn test_group_menu_m_transitions_to_list_select_with_14_modes() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        handle_key(key(KeyCode::Char('m')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 14);
                assert_eq!(action, SelectAction::SetMode);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_group_menu_m_preselects_current_mode() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let mut d = display();
        d.mode = Mode::CwU;
        handle_key(key(KeyCode::Char('m')), &mut state, &d);
        match state {
            ControlState::ListSelect { cursor, .. } => assert_eq!(cursor, 2),
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_group_menu_t_is_immediate_toggle_tx() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::Off))
        ));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_group_menu_o_is_immediate_toggle_power() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let mut d = display();
        d.power_on = true;
        let result = handle_key(key(KeyCode::Char('o')), &mut state, &d);
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::TogglePowerOn(true))
        ));
    }

    #[test]
    fn test_group_menu_unbound_key_continues() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let result = handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::FrequencyLevels,
                ..
            }
        ));
    }

    #[test]
    fn test_group_menu_esc_returns_to_menu() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // =========================================================================
    // Group 3 (MemoryChannels), Group 4 (ClarifierToneIfShift), Group 6
    // (ScanVoxBusy) — this task's content.
    // =========================================================================

    // --- Reachability from Menu (all 3 groups) ---

    #[test]
    fn test_memory_channels_reachable_via_m_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('M')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::MemoryChannels,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_clarifier_tone_if_shift_reachable_via_c_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('C')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::ClarifierToneIfShift,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_keyer_cw_break_in_reachable_via_k_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('K')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::KeyerCwBreakIn,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_scan_vox_busy_reachable_via_x_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('X')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::ScanVoxBusy,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- Group 3 (MemoryChannels): key counts and uniqueness ---

    #[test]
    fn test_memory_channels_group_has_six_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::MemoryChannels);
        assert_eq!(labels.len(), 6);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- Group 3: per-key state transitions ---

    #[test]
    fn test_memory_c_transitions_to_text_input_select_channel() {
        let mut state = group_menu(CommandGroup::MemoryChannels);
        handle_key(key(KeyCode::Char('c')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SelectMemoryChannel,
                ..
            }
        ));
    }

    #[test]
    fn test_memory_g_is_immediate_get_memory_channel() {
        let mut state = group_menu(CommandGroup::MemoryChannels);
        let result = handle_key(key(KeyCode::Char('g')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::GetMemoryChannel));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_memory_r_transitions_to_text_input_read_channel() {
        let mut state = group_menu(CommandGroup::MemoryChannels);
        handle_key(key(KeyCode::Char('r')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::ReadMemoryChannel,
                ..
            }
        ));
    }

    #[test]
    fn test_memory_w_transitions_to_text_input_write_from_vfo_a() {
        let mut state = group_menu(CommandGroup::MemoryChannels);
        handle_key(key(KeyCode::Char('w')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::WriteMemoryChannelFromVfoA,
                ..
            }
        ));
    }

    #[test]
    fn test_memory_t_transitions_to_text_input_read_channel_tag() {
        let mut state = group_menu(CommandGroup::MemoryChannels);
        handle_key(key(KeyCode::Char('t')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::ReadMemoryChannelTag,
                ..
            }
        ));
    }

    #[test]
    fn test_memory_v_transitions_to_text_input_write_tag_from_vfo_a() {
        let mut state = group_menu(CommandGroup::MemoryChannels);
        handle_key(key(KeyCode::Char('v')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::WriteMemoryChannelTagFromVfoA,
                ..
            }
        ));
    }

    // --- Group 3: validation ranges (channel 1-117) ---

    #[test]
    fn test_select_memory_channel_min_boundary_1_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "1".to_string(),
            error: None,
            action: InputAction::SelectMemoryChannel,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SelectMemoryChannel(1))
        );
    }

    #[test]
    fn test_select_memory_channel_max_boundary_117_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "117".to_string(),
            error: None,
            action: InputAction::SelectMemoryChannel,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SelectMemoryChannel(117))
        );
    }

    #[test]
    fn test_select_memory_channel_0_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "0".to_string(),
            error: None,
            action: InputAction::SelectMemoryChannel,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_read_memory_channel_118_rejected_above_max() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "118".to_string(),
            error: None,
            action: InputAction::ReadMemoryChannel,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_write_memory_channel_from_vfo_a_valid_channel_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "42".to_string(),
            error: None,
            action: InputAction::WriteMemoryChannelFromVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::WriteMemoryChannelFromVfoA(42))
        );
    }

    // --- Group 3: "channel:tag" parsing for WriteMemoryChannelTagFromVfoA ---

    #[test]
    fn test_write_memory_tag_valid_channel_and_tag_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "5:HOME".to_string(),
            error: None,
            action: InputAction::WriteMemoryChannelTagFromVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::WriteMemoryChannelTagFromVfoA(
                5,
                "HOME".to_string()
            ))
        );
    }

    #[test]
    fn test_write_memory_tag_missing_colon_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "5HOME".to_string(),
            error: None,
            action: InputAction::WriteMemoryChannelTagFromVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_write_memory_tag_too_long_rejected() {
        // MemoryTag::MAX_LEN is 12 — "THIRTEENCHARS" is 13 chars, must be
        // rejected.
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "5:THIRTEENCHARS".to_string(),
            error: None,
            action: InputAction::WriteMemoryChannelTagFromVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_write_memory_tag_bad_channel_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "200:HOME".to_string(),
            error: None,
            action: InputAction::WriteMemoryChannelTagFromVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_write_memory_tag_colon_inside_tag_preserved() {
        // splitn(2, ':') means only the *first* colon is a delimiter — a
        // tag that itself contains a colon (a legal MemoryTag character)
        // round-trips intact.
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "5:A:B".to_string(),
            error: None,
            action: InputAction::WriteMemoryChannelTagFromVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::WriteMemoryChannelTagFromVfoA(
                5,
                "A:B".to_string()
            ))
        );
    }

    // --- Group 4 (ClarifierToneIfShift): key counts and uniqueness ---

    #[test]
    fn test_clarifier_tone_if_shift_group_has_nine_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::ClarifierToneIfShift);
        assert_eq!(labels.len(), 9);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- Group 4: per-key state transitions ---

    #[test]
    fn test_clarifier_x_transitions_to_list_select_rx_clarifier() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('x')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["On".to_string(), "Off".to_string()]);
                assert_eq!(action, SelectAction::ToggleRxClarifier);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_clarifier_y_transitions_to_list_select_tx_clarifier() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('y')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleTxClarifier,
                ..
            }
        ));
    }

    #[test]
    fn test_clarifier_c_is_immediate_clear() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        let result = handle_key(key(KeyCode::Char('c')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::ClarifierClear));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_clarifier_d_transitions_to_text_input_clarifier_down() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('d')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::ClarifierDown,
                ..
            }
        ));
    }

    #[test]
    fn test_clarifier_u_transitions_to_text_input_clarifier_up() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('u')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::ClarifierUp,
                ..
            }
        ));
    }

    #[test]
    fn test_clarifier_i_transitions_to_text_input_if_shift() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('i')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetIfShift,
                ..
            }
        ));
    }

    #[test]
    fn test_clarifier_t_transitions_to_list_select_tone_squelch_mode_with_five_options() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('t')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 5);
                assert_eq!(action, SelectAction::SetToneSquelchMode);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_clarifier_n_transitions_to_list_select_ctcss_with_fifty_options() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('n')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 50);
                assert_eq!(action, SelectAction::SetCtcssTone);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_clarifier_s_transitions_to_list_select_dcs_with_104_options() {
        let mut state = group_menu(CommandGroup::ClarifierToneIfShift);
        handle_key(key(KeyCode::Char('s')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 104);
                assert_eq!(action, SelectAction::SetDcsCode);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    // --- Group 4: validation ranges ---

    #[test]
    fn test_clarifier_down_max_9999_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "9999".to_string(),
            error: None,
            action: InputAction::ClarifierDown,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ClarifierDown(9999))
        );
    }

    #[test]
    fn test_clarifier_down_10000_rejected_above_max() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "10000".to_string(),
            error: None,
            action: InputAction::ClarifierDown,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_clarifier_up_0_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "0".to_string(),
            error: None,
            action: InputAction::ClarifierUp,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::ClarifierUp(0)));
    }

    #[test]
    fn test_if_shift_min_boundary_neg_1200_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "-1200".to_string(),
            error: None,
            action: InputAction::SetIfShift,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetIfShift(-1200)));
    }

    #[test]
    fn test_if_shift_max_boundary_1200_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "1200".to_string(),
            error: None,
            action: InputAction::SetIfShift,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetIfShift(1200)));
    }

    #[test]
    fn test_if_shift_1201_rejected_above_max() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "1201".to_string(),
            error: None,
            action: InputAction::SetIfShift,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_if_shift_non_multiple_of_20_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "10".to_string(),
            error: None,
            action: InputAction::SetIfShift,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_ctcss_tone_cursor_maps_to_table_value() {
        // cursor 0 must map to CTCSS_TONES_DECIHZ[0] == 67.0 Hz (6.7 Hz *
        // 10 stored as deci-Hz -> 67 -> 6.7 Hz). Confirms the ListSelect
        // cursor and the crate's own table stay in lockstep.
        assert_eq!(CTCSS_TONES_DECIHZ[0], 670);
        let exec = select_action_to_execute(SelectAction::SetCtcssTone, 0);
        assert_eq!(exec, ExecuteAction::SetCtcssTone(67.0));
    }

    #[test]
    fn test_dcs_code_cursor_maps_to_table_value() {
        assert_eq!(DCS_CODES[0], 23);
        let exec = select_action_to_execute(SelectAction::SetDcsCode, 0);
        assert_eq!(exec, ExecuteAction::SetDcsCode(23));
    }

    // --- Group 6 (ScanVoxBusy): key counts and uniqueness; BY gets no key ---

    #[test]
    fn test_scan_vox_busy_group_has_four_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::ScanVoxBusy);
        assert_eq!(labels.len(), 4);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- Group 6: per-key state transitions ---

    #[test]
    fn test_scan_s_transitions_to_list_select_scan_state_with_three_options() {
        let mut state = group_menu(CommandGroup::ScanVoxBusy);
        handle_key(key(KeyCode::Char('s')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["Off", "Up", "Down"]);
                assert_eq!(action, SelectAction::SetScanState);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_scan_v_transitions_to_list_select_vox_on() {
        let mut state = group_menu(CommandGroup::ScanVoxBusy);
        handle_key(key(KeyCode::Char('v')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleVox,
                ..
            }
        ));
    }

    #[test]
    fn test_scan_g_transitions_to_text_input_vox_gain() {
        let mut state = group_menu(CommandGroup::ScanVoxBusy);
        handle_key(key(KeyCode::Char('g')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetVoxGain,
                ..
            }
        ));
    }

    #[test]
    fn test_scan_d_transitions_to_text_input_vox_delay() {
        let mut state = group_menu(CommandGroup::ScanVoxBusy);
        handle_key(key(KeyCode::Char('d')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetVoxDelay,
                ..
            }
        ));
    }

    #[test]
    fn test_scan_state_cursor_maps_off_up_down_in_order() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetScanState, 0),
            ExecuteAction::SetScanState(ScanState::Off)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetScanState, 1),
            ExecuteAction::SetScanState(ScanState::Up)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetScanState, 2),
            ExecuteAction::SetScanState(ScanState::Down)
        );
    }

    // --- Group 6: validation ranges ---

    #[test]
    fn test_vox_gain_max_100_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "100".to_string(),
            error: None,
            action: InputAction::SetVoxGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetVoxGain(100)));
    }

    #[test]
    fn test_vox_gain_101_rejected_above_max() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "101".to_string(),
            error: None,
            action: InputAction::SetVoxGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_vox_delay_min_boundary_30_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "30".to_string(),
            error: None,
            action: InputAction::SetVoxDelay,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetVoxDelay(30)));
    }

    #[test]
    fn test_vox_delay_max_boundary_3000_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "3000".to_string(),
            error: None,
            action: InputAction::SetVoxDelay,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetVoxDelay(3000)));
    }

    #[test]
    fn test_vox_delay_below_min_29_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "29".to_string(),
            error: None,
            action: InputAction::SetVoxDelay,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_vox_delay_non_multiple_of_10_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "35".to_string(),
            error: None,
            action: InputAction::SetVoxDelay,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    // --- Group 6: BY (get_rx_busy) deliberately has no key ---

    #[test]
    fn test_scan_vox_busy_group_has_no_busy_key() {
        // `BY` is read-only and always reports `false` in this emulator —
        // same "read-only fields don't get a key" convention as group 1's
        // smeter/id. Guards against a future edit accidentally adding one
        // without updating this test (and the "four commands" count above).
        let labels = group_command_labels(CommandGroup::ScanVoxBusy);
        assert_eq!(labels.len(), 4);
    }

    // --- TextInput: typing / backspace / escape (unchanged from Wave 2,
    // except Esc now returns to Menu instead of Normal) ---

    #[test]
    fn test_text_input_typing_appends_to_buffer() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetVfoA,
        };
        handle_key(key(KeyCode::Char('1')), &mut state, &display());
        handle_key(key(KeyCode::Char('4')), &mut state, &display());
        if let ControlState::TextInput { buffer, .. } = &state {
            assert_eq!(buffer, "14");
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_text_input_backspace_removes_last_char() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "14".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        handle_key(key(KeyCode::Backspace), &mut state, &display());
        if let ControlState::TextInput { buffer, .. } = &state {
            assert_eq!(buffer, "1");
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_text_input_esc_returns_to_menu() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "14".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- TextInput validation: VFO A/B frequency range (unchanged) ---

    #[test]
    fn test_vfo_a_valid_frequency_min_boundary() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: Frequency::MIN_HZ.to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetVfoA(hz)) if hz == Frequency::MIN_HZ
        ));
    }

    #[test]
    fn test_vfo_a_valid_frequency_max_boundary() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: Frequency::MAX_HZ.to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetVfoA(hz)) if hz == Frequency::MAX_HZ
        ));
    }

    #[test]
    fn test_vfo_a_below_min_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: (Frequency::MIN_HZ - 1).to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_vfo_a_above_max_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: (Frequency::MAX_HZ + 1).to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_vfo_a_non_numeric_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "not_a_number".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_vfo_b_uses_same_range_as_vfo_a() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: Frequency::MIN_HZ.to_string(),
            error: None,
            action: InputAction::SetVfoB,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetVfoB(hz)) if hz == Frequency::MIN_HZ
        ));
    }

    // --- TextInput validation: AF/RF gain 0-255 (unchanged) ---

    #[test]
    fn test_af_gain_max_255_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "255".to_string(),
            error: None,
            action: InputAction::SetAfGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetAfGain(255))
        ));
    }

    #[test]
    fn test_af_gain_256_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "256".to_string(),
            error: None,
            action: InputAction::SetAfGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_rf_gain_0_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "0".to_string(),
            error: None,
            action: InputAction::SetRfGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetRfGain(0))
        ));
    }

    // --- TextInput validation: squelch 0-100 (NOT 255) (unchanged) ---

    #[test]
    fn test_squelch_100_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "100".to_string(),
            error: None,
            action: InputAction::SetSquelch,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetSquelch(100))
        ));
    }

    #[test]
    fn test_squelch_101_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "101".to_string(),
            error: None,
            action: InputAction::SetSquelch,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_squelch_255_rejected_not_255_range() {
        // Regression guard: squelch is 0-100, NOT 0-255 like AF/RF gain.
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "255".to_string(),
            error: None,
            action: InputAction::SetSquelch,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    // --- TextInput validation: TX power 5-100 (unchanged) ---

    #[test]
    fn test_power_5_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "5".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetPower(5))
        ));
    }

    #[test]
    fn test_power_100_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "100".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetPower(100))
        ));
    }

    #[test]
    fn test_power_4_rejected_below_min() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "4".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_power_101_rejected_above_max() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "101".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- ListSelect: navigation and confirm (unchanged) ---

    #[test]
    fn test_list_select_right_advances_cursor() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 0,
            action: SelectAction::SetMode,
        };
        handle_key(key(KeyCode::Right), &mut state, &display());
        if let ControlState::ListSelect { cursor, .. } = &state {
            assert_eq!(*cursor, 1);
        } else {
            panic!("expected ListSelect");
        }
    }

    #[test]
    fn test_list_select_left_at_zero_stays_zero() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 0,
            action: SelectAction::SetMode,
        };
        handle_key(key(KeyCode::Left), &mut state, &display());
        if let ControlState::ListSelect { cursor, .. } = &state {
            assert_eq!(*cursor, 0);
        } else {
            panic!("expected ListSelect");
        }
    }

    #[test]
    fn test_list_select_right_at_max_stays_max() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 13,
            action: SelectAction::SetMode,
        };
        handle_key(key(KeyCode::Right), &mut state, &display());
        if let ControlState::ListSelect { cursor, .. } = &state {
            assert_eq!(*cursor, 13);
        } else {
            panic!("expected ListSelect");
        }
    }

    #[test]
    fn test_list_select_enter_produces_execute_set_mode() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 13, // C4fm
            action: SelectAction::SetMode,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetMode(Mode::C4fm))
        ));
    }

    #[test]
    fn test_list_select_esc_returns_to_menu() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 0,
            action: SelectAction::SetMode,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- Feedback: any key returns to Menu (was Normal) ---

    #[test]
    fn test_feedback_any_key_returns_to_menu() {
        let mut state = ControlState::Feedback {
            message: "OK".to_string(),
            is_error: false,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- 3-valued TxState toggle behavior (§6.5, unchanged) ---

    #[test]
    fn test_toggle_tx_from_off_sends_transmit_direction() {
        // ToggleTx carries the last-polled state; the executor (terminal.rs)
        // decides transmit() vs receive() from it. Off/RadioKeyedNonCat ->
        // transmit.
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let mut d = display();
        d.tx_state = TxState::Off;
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &d);
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::Off))
        );
    }

    #[test]
    fn test_toggle_tx_from_cat_keyed_carries_cat_keyed() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let mut d = display();
        d.tx_state = TxState::CatKeyed;
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &d);
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::CatKeyed))
        );
    }

    #[test]
    fn test_toggle_tx_from_radio_keyed_non_cat_carries_that_state() {
        let mut state = group_menu(CommandGroup::FrequencyLevels);
        let mut d = display();
        d.tx_state = TxState::RadioKeyedNonCat;
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &d);
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::RadioKeyedNonCat))
        );
    }

    // =========================================================================
    // Group 5 (KeyerCwBreakIn) — this task's content, including the RTS
    // real-time CW-keying toggle (§11.3 point 6).
    // =========================================================================

    // --- Key count + uniqueness ---

    #[test]
    fn test_keyer_cw_break_in_group_has_twelve_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::KeyerCwBreakIn);
        assert_eq!(labels.len(), 12);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- 'q' is a genuine no-op within group 5 (not one of its 12 keys —
    // note 'z' is NOT a no-op here, unlike the other still-stub groups,
    // since 'Z'/zero-in is one of this group's real 12 keys) ---

    #[test]
    fn test_keyer_cw_break_in_q_key_is_a_no_op() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        let result = handle_key(key(KeyCode::Char('q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::KeyerCwBreakIn,
                ..
            }
        ));
    }

    // --- Per-key state transitions ---

    #[test]
    fn test_keyer_k_is_immediate_toggle_rts_carries_false() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        let d = display(); // rts_asserted defaults to false
        let result = handle_key(key(KeyCode::Char('k')), &mut state, &d);
        assert_eq!(result, KeyResult::Execute(ExecuteAction::ToggleRts(false)));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_keyer_k_is_immediate_toggle_rts_carries_true() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        let mut d = display();
        d.rts_asserted = true;
        let result = handle_key(key(KeyCode::Char('k')), &mut state, &d);
        assert_eq!(result, KeyResult::Execute(ExecuteAction::ToggleRts(true)));
    }

    #[test]
    fn test_keyer_b_transitions_to_list_select_break_in() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('b')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 2);
                assert_eq!(action, SelectAction::ToggleBreakIn);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_keyer_d_transitions_to_text_input_semi_break_in_delay() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('d')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetSemiBreakInDelay,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_s_transitions_to_list_select_cw_spot() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('s')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleCwSpot,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_e_transitions_to_list_select_keyer_enabled() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('e')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleKeyerEnabled,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_w_transitions_to_text_input_keyer_speed() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('w')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetKeyerSpeed,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_p_transitions_to_text_input_keyer_pitch() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('p')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetKeyerPitchHz,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_z_is_immediate_zero_in() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        let result = handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::ZeroIn));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_keyer_r_transitions_to_text_input_read_keyer_memory() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('r')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::ReadKeyerMemory,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_m_transitions_to_text_input_write_keyer_memory() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('m')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::WriteKeyerMemory,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_y_transitions_to_text_input_play_keyer_memory() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('y')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::PlayKeyerMemory,
                ..
            }
        ));
    }

    #[test]
    fn test_keyer_j_transitions_to_text_input_play_message_keyer() {
        let mut state = group_menu(CommandGroup::KeyerCwBreakIn);
        handle_key(key(KeyCode::Char('j')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::PlayMessageKeyer,
                ..
            }
        ));
    }

    // --- Validation: semi break-in delay (30-3000, no step) ---

    fn text_input(action: InputAction, buffer: &str) -> ControlState {
        ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: buffer.to_string(),
            error: None,
            action,
        }
    }

    #[test]
    fn test_semi_break_in_delay_min_boundary_30_accepted() {
        let mut state = text_input(InputAction::SetSemiBreakInDelay, "30");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetSemiBreakInDelay(30))
        );
    }

    #[test]
    fn test_semi_break_in_delay_max_boundary_3000_accepted() {
        let mut state = text_input(InputAction::SetSemiBreakInDelay, "3000");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetSemiBreakInDelay(3000))
        );
    }

    #[test]
    fn test_semi_break_in_delay_29_rejected_below_min() {
        let mut state = text_input(InputAction::SetSemiBreakInDelay, "29");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_semi_break_in_delay_3001_rejected_above_max() {
        let mut state = text_input(InputAction::SetSemiBreakInDelay, "3001");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_semi_break_in_delay_odd_value_accepted_no_step_constraint() {
        // Unlike `VD` (VOX delay), `SD` has no modulus check per
        // `radio/src/ft991a.rs`'s `set_semi_break_in_delay` — 31 is a
        // legal value.
        let mut state = text_input(InputAction::SetSemiBreakInDelay, "31");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetSemiBreakInDelay(31))
        );
    }

    // --- Validation: keyer speed (4-60 WPM) ---

    #[test]
    fn test_keyer_speed_min_boundary_4_accepted() {
        let mut state = text_input(InputAction::SetKeyerSpeed, "4");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetKeyerSpeed(4)));
    }

    #[test]
    fn test_keyer_speed_max_boundary_60_accepted() {
        let mut state = text_input(InputAction::SetKeyerSpeed, "60");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetKeyerSpeed(60)));
    }

    #[test]
    fn test_keyer_speed_3_rejected_below_min() {
        let mut state = text_input(InputAction::SetKeyerSpeed, "3");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_keyer_speed_61_rejected_above_max() {
        let mut state = text_input(InputAction::SetKeyerSpeed, "61");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- Validation: keyer pitch (300-1050 Hz, step 10) ---

    #[test]
    fn test_keyer_pitch_min_boundary_300_accepted() {
        let mut state = text_input(InputAction::SetKeyerPitchHz, "300");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetKeyerPitchHz(300))
        );
    }

    #[test]
    fn test_keyer_pitch_max_boundary_1050_accepted() {
        let mut state = text_input(InputAction::SetKeyerPitchHz, "1050");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetKeyerPitchHz(1050))
        );
    }

    #[test]
    fn test_keyer_pitch_290_rejected_below_min() {
        let mut state = text_input(InputAction::SetKeyerPitchHz, "290");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_keyer_pitch_1060_rejected_above_max() {
        let mut state = text_input(InputAction::SetKeyerPitchHz, "1060");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_keyer_pitch_305_rejected_not_on_10hz_step() {
        let mut state = text_input(InputAction::SetKeyerPitchHz, "305");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- Validation: keyer memory channel (1-5), read/play ---

    #[test]
    fn test_read_keyer_memory_min_boundary_1_accepted() {
        let mut state = text_input(InputAction::ReadKeyerMemory, "1");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ReadKeyerMemory(1))
        );
    }

    #[test]
    fn test_read_keyer_memory_max_boundary_5_accepted() {
        let mut state = text_input(InputAction::ReadKeyerMemory, "5");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ReadKeyerMemory(5))
        );
    }

    #[test]
    fn test_read_keyer_memory_0_rejected() {
        let mut state = text_input(InputAction::ReadKeyerMemory, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_read_keyer_memory_6_rejected_above_max() {
        let mut state = text_input(InputAction::ReadKeyerMemory, "6");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_play_keyer_memory_valid_channel_accepted() {
        let mut state = text_input(InputAction::PlayKeyerMemory, "3");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::PlayKeyerMemory(3))
        );
    }

    #[test]
    fn test_play_keyer_memory_6_rejected_above_max() {
        let mut state = text_input(InputAction::PlayKeyerMemory, "6");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_play_message_keyer_valid_channel_accepted() {
        let mut state = text_input(InputAction::PlayMessageKeyer, "5");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::PlayMessageKeyer(5))
        );
    }

    #[test]
    fn test_play_message_keyer_0_rejected() {
        let mut state = text_input(InputAction::PlayMessageKeyer, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- "channel:message" parsing for WriteKeyerMemory ---

    #[test]
    fn test_write_keyer_memory_valid_channel_and_message_accepted() {
        let mut state = text_input(InputAction::WriteKeyerMemory, "3:CQ CQ DE KF0UWV");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::WriteKeyerMemory(
                3,
                "CQ CQ DE KF0UWV".to_string()
            ))
        );
    }

    #[test]
    fn test_write_keyer_memory_missing_colon_rejected() {
        let mut state = text_input(InputAction::WriteKeyerMemory, "3CQ");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_write_keyer_memory_invalid_channel_rejected() {
        let mut state = text_input(InputAction::WriteKeyerMemory, "6:HELLO");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_write_keyer_memory_empty_message_rejected() {
        let mut state = text_input(InputAction::WriteKeyerMemory, "3:");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_write_keyer_memory_51_char_message_rejected_above_max() {
        let msg = "A".repeat(51);
        let mut state = text_input(InputAction::WriteKeyerMemory, &format!("1:{msg}"));
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_write_keyer_memory_50_char_message_accepted_at_max() {
        let msg = "A".repeat(50);
        let mut state = text_input(InputAction::WriteKeyerMemory, &format!("2:{msg}"));
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::WriteKeyerMemory(2, msg))
        );
    }

    #[test]
    fn test_write_keyer_memory_semicolon_rejected() {
        let mut state = text_input(InputAction::WriteKeyerMemory, "1:BAD;MSG");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_write_keyer_memory_colon_inside_message_preserved() {
        // Split on the *first* ':' only, so a message that legitimately
        // contains a colon still round-trips — same convention as group 3's
        // `WriteMemoryChannelTagFromVfoA`.
        let mut state = text_input(InputAction::WriteKeyerMemory, "4:DE KF0: HELLO");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::WriteKeyerMemory(
                4,
                "DE KF0: HELLO".to_string()
            ))
        );
    }

    // --- SelectAction cursor<->value mapping for the 3 boolean toggles ---

    #[test]
    fn test_select_action_toggle_break_in_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleBreakIn, 0),
            ExecuteAction::SetBreakInOn(true)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleBreakIn, 1),
            ExecuteAction::SetBreakInOn(false)
        );
    }

    #[test]
    fn test_select_action_toggle_cw_spot_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleCwSpot, 0),
            ExecuteAction::SetCwSpotOn(true)
        );
    }

    #[test]
    fn test_select_action_toggle_keyer_enabled_cursor_1_is_off() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleKeyerEnabled, 1),
            ExecuteAction::SetKeyerEnabled(false)
        );
    }

    // =========================================================================
    // Group 2 (VfoMemoryQuickOps) and Group 9 (BandStepEncoder) — this
    // task's content (Wave 4 dispatch queue item 5, §11.6).
    // =========================================================================

    // --- Reachability from Menu ---

    #[test]
    fn test_vfo_memory_quick_ops_reachable_via_o_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('O')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::VfoMemoryQuickOps,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_band_step_encoder_reachable_via_b_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('B')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::BandStepEncoder,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- Key counts and uniqueness ---

    #[test]
    fn test_vfo_memory_quick_ops_group_has_eleven_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::VfoMemoryQuickOps);
        assert_eq!(labels.len(), 11);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    #[test]
    fn test_band_step_encoder_group_has_nine_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::BandStepEncoder);
        assert_eq!(labels.len(), 9);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- Group-specific no-op regression guards (unbound key within each
    // group's own keymap, matching Task 4's precedent for `KeyerCwBreakIn`
    // once it stopped being a genuinely-empty stub) ---

    #[test]
    fn test_vfo_memory_quick_ops_z_key_is_a_no_op() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::VfoMemoryQuickOps,
                ..
            }
        ));
    }

    #[test]
    fn test_band_step_encoder_z_key_is_a_no_op() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        let result = handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::BandStepEncoder,
                ..
            }
        ));
    }

    // --- Group 2: per-key transitions — every key is an `Immediate`
    // zero-argument trigger, so each test presses the key once and checks
    // the exact `ExecuteAction` produced plus the `Feedback` transition. ---

    #[test]
    fn test_vfo_memory_quick_ops_a_is_immediate_copy_vfo_a_to_b() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('a')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::CopyVfoAToB));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_vfo_memory_quick_ops_b_is_immediate_copy_vfo_b_to_a() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('b')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::CopyVfoBToA));
    }

    #[test]
    fn test_vfo_memory_quick_ops_w_is_immediate_swap_vfos() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('w')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SwapVfos));
    }

    #[test]
    fn test_vfo_memory_quick_ops_s_is_immediate_store_vfo_to_memory() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('s')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::StoreVfoToMemory));
    }

    #[test]
    fn test_vfo_memory_quick_ops_r_is_immediate_recall_memory_to_vfo() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('r')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::RecallMemoryToVfo));
    }

    #[test]
    fn test_vfo_memory_quick_ops_u_is_immediate_memory_channel_up() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('u')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::MemoryChannelUp));
    }

    #[test]
    fn test_vfo_memory_quick_ops_d_is_immediate_memory_channel_down() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('d')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::MemoryChannelDown));
    }

    #[test]
    fn test_vfo_memory_quick_ops_m_is_immediate_toggle_vfo_memory_mode() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('m')), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleVfoMemoryMode)
        );
    }

    #[test]
    fn test_vfo_memory_quick_ops_i_is_immediate_qmb_store() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('i')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::QmbStore));
    }

    #[test]
    fn test_vfo_memory_quick_ops_q_is_immediate_qmb_recall() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('q')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::QmbRecall));
    }

    #[test]
    fn test_vfo_memory_quick_ops_p_is_immediate_quick_split() {
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('p')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::QuickSplit));
    }

    #[test]
    fn test_vfo_memory_quick_ops_lowercase_q_inside_group_menu_is_not_global_quit() {
        // `Q` is `VfoMemoryQuickOps`'s own "QMB recall" key — confirms the
        // `Menu`-level Quit binding and a `GroupMenu`-level command key can
        // safely reuse the same letter (different `ControlState` match
        // arms), same namespace-separation precedent group 1 (`M`) and
        // group 5 (`K`) already established.
        let mut state = group_menu(CommandGroup::VfoMemoryQuickOps);
        let result = handle_key(key(KeyCode::Char('q')), &mut state, &display());
        assert_ne!(result, KeyResult::Quit);
        assert_eq!(result, KeyResult::Execute(ExecuteAction::QmbRecall));
    }

    // --- Group 9: per-key transitions ---

    #[test]
    fn test_band_step_encoder_b_transitions_to_list_select_band_with_sixteen_options() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        handle_key(key(KeyCode::Char('b')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 16);
                assert_eq!(action, SelectAction::SetBand);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_band_step_encoder_u_is_immediate_band_up() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        let result = handle_key(key(KeyCode::Char('u')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::BandUp));
    }

    #[test]
    fn test_band_step_encoder_d_is_immediate_band_down() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        let result = handle_key(key(KeyCode::Char('d')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::BandDown));
    }

    #[test]
    fn test_band_step_encoder_f_transitions_to_list_select_fine_step() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        handle_key(key(KeyCode::Char('f')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["On", "Off"]);
                assert_eq!(action, SelectAction::ToggleFineStep);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_band_step_encoder_p_is_immediate_mic_up() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        let result = handle_key(key(KeyCode::Char('p')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::MicUp));
    }

    #[test]
    fn test_band_step_encoder_n_is_immediate_mic_down() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        let result = handle_key(key(KeyCode::Char('n')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::MicDown));
    }

    #[test]
    fn test_band_step_encoder_j_transitions_to_text_input_encoder_down() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        handle_key(key(KeyCode::Char('j')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::EncoderDown,
                ..
            }
        ));
    }

    #[test]
    fn test_band_step_encoder_k_transitions_to_text_input_encoder_up() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        handle_key(key(KeyCode::Char('k')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::EncoderUp,
                ..
            }
        ));
    }

    #[test]
    fn test_band_step_encoder_e_is_immediate_ent_key() {
        let mut state = group_menu(CommandGroup::BandStepEncoder);
        let result = handle_key(key(KeyCode::Char('e')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::EntKey));
    }

    // --- Group 9: SetBand cursor<->table-value lockstep ---

    #[test]
    fn test_set_band_cursor_0_maps_to_one_eight_mhz() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetBand, 0),
            ExecuteAction::SetBand(Band::OneEightMHz)
        );
    }

    #[test]
    fn test_set_band_cursor_15_maps_to_four_three_zero_mhz() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetBand, 15),
            ExecuteAction::SetBand(Band::FourThreeZeroMHz)
        );
    }

    #[test]
    fn test_set_band_cursor_out_of_range_falls_back_to_first_band() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetBand, 99),
            ExecuteAction::SetBand(Band::OneEightMHz)
        );
    }

    #[test]
    fn test_band_options_and_band_order_have_matching_length() {
        assert_eq!(band_options().len(), BAND_ORDER.len());
        assert_eq!(band_options().len(), 16);
    }

    #[test]
    fn test_initial_list_cursor_set_band_always_defaults_to_zero() {
        // `BS` has no Read/Answer form at all — structurally, not just
        // "not polled yet" — see `band_step_encoder_commands`'s doc
        // comment.
        assert_eq!(initial_list_cursor(SelectAction::SetBand, &display()), 0);
    }

    // --- Group 9: ToggleFineStep cursor<->value mapping ---

    #[test]
    fn test_select_action_toggle_fine_step_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleFineStep, 0),
            ExecuteAction::SetFineStep(true)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleFineStep, 1),
            ExecuteAction::SetFineStep(false)
        );
    }

    // --- Group 9: encoder "encoder:steps" text-input validation ---

    #[test]
    fn test_encoder_down_valid_main_and_steps_accepted() {
        let mut state = text_input(InputAction::EncoderDown, "main:5");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::EncoderDown(EncoderSelector::Main, 5))
        );
    }

    #[test]
    fn test_encoder_up_valid_sub_and_steps_accepted() {
        let mut state = text_input(InputAction::EncoderUp, "sub:10");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::EncoderUp(EncoderSelector::Sub, 10))
        );
    }

    #[test]
    fn test_encoder_selector_multi_accepted_case_insensitively() {
        let mut state = text_input(InputAction::EncoderDown, "MULTI:1");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::EncoderDown(EncoderSelector::Multi, 1))
        );
    }

    #[test]
    fn test_encoder_steps_min_boundary_1_accepted() {
        let mut state = text_input(InputAction::EncoderUp, "main:1");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::EncoderUp(EncoderSelector::Main, 1))
        );
    }

    #[test]
    fn test_encoder_steps_max_boundary_99_accepted() {
        let mut state = text_input(InputAction::EncoderUp, "main:99");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::EncoderUp(EncoderSelector::Main, 99))
        );
    }

    #[test]
    fn test_encoder_steps_0_rejected_below_min() {
        let mut state = text_input(InputAction::EncoderDown, "main:0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_encoder_steps_100_rejected_above_max() {
        let mut state = text_input(InputAction::EncoderDown, "main:100");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_encoder_missing_colon_rejected() {
        let mut state = text_input(InputAction::EncoderDown, "main5");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_encoder_invalid_selector_name_rejected() {
        let mut state = text_input(InputAction::EncoderDown, "bogus:5");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_encoder_non_numeric_steps_rejected() {
        let mut state = text_input(InputAction::EncoderDown, "main:abc");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // =========================================================================
    // Group 7 (AttenuatorNoiseAgcNotchFilter) and Group 8 (SpeechMicMonitor)
    // — this task's content (Wave 4 dispatch queue item 6, §11.6).
    // =========================================================================

    // --- Reachability from Menu ---

    #[test]
    fn test_attenuator_noise_agc_notch_filter_reachable_via_a_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('A')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::AttenuatorNoiseAgcNotchFilter,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_speech_mic_monitor_reachable_via_p_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('P')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::SpeechMicMonitor,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- Key counts and uniqueness ---

    #[test]
    fn test_attenuator_noise_agc_notch_filter_group_has_sixteen_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        assert_eq!(labels.len(), 16);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    #[test]
    fn test_speech_mic_monitor_group_has_six_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::SpeechMicMonitor);
        assert_eq!(labels.len(), 6);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- Group-specific no-op regression guards ---

    #[test]
    fn test_attenuator_noise_agc_notch_filter_q_key_is_a_no_op() {
        // 'Q' is not one of group 7's 16 keys (A P B L N R G U W F C H X Y M
        // Z) — must not panic and must not transition state.
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        let result = handle_key(key(KeyCode::Char('Q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::AttenuatorNoiseAgcNotchFilter,
                ..
            }
        ));
    }

    #[test]
    fn test_speech_mic_monitor_q_key_is_a_no_op() {
        // 'Q' is not one of group 8's 6 keys (G L S M V E) — must not panic
        // and must not transition state.
        let mut state = group_menu(CommandGroup::SpeechMicMonitor);
        let result = handle_key(key(KeyCode::Char('Q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::SpeechMicMonitor,
                ..
            }
        ));
    }

    // --- Group 7: per-key state transitions ---

    #[test]
    fn test_attenuator_a_transitions_to_list_select_attenuator() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('a')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleAttenuator,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_p_transitions_to_list_select_preamp_mode_with_three_options() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('p')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["IPO", "AMP1", "AMP2"]);
                assert_eq!(action, SelectAction::SetPreampMode);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_attenuator_b_transitions_to_list_select_noise_blanker_on() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('b')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleNoiseBlanker,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_l_transitions_to_text_input_noise_blanker_level() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('l')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetNoiseBlankerLevel,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_n_transitions_to_list_select_noise_reduction_on() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('n')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleNoiseReduction,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_r_transitions_to_text_input_noise_reduction_level() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('r')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetNoiseReductionLevel,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_g_transitions_to_list_select_agc_mode_with_five_options() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('g')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["Off", "Fast", "Mid", "Slow", "Auto"]);
                assert_eq!(action, SelectAction::SetAgcMode);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_attenuator_u_transitions_to_list_select_auto_notch_on() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('u')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleAutoNotch,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_w_transitions_to_list_select_narrow_on() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('w')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleNarrow,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_f_transitions_to_list_select_filter_width_with_twenty_two_options() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('f')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 22);
                assert_eq!(action, SelectAction::SetFilterWidthIndex);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_attenuator_c_transitions_to_list_select_contour_on() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('c')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleContour,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_h_transitions_to_text_input_contour_frequency() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('h')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetContourFrequencyHz,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_x_transitions_to_list_select_apf_on() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('x')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleApf,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_y_transitions_to_text_input_apf_frequency() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('y')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetApfFrequencyHz,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_m_transitions_to_list_select_manual_notch_on() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('m')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleManualNotch,
                ..
            }
        ));
    }

    #[test]
    fn test_attenuator_z_transitions_to_text_input_manual_notch_frequency() {
        let mut state = group_menu(CommandGroup::AttenuatorNoiseAgcNotchFilter);
        handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetManualNotchFrequencyHz,
                ..
            }
        ));
    }

    // --- Group 7: SelectAction cursor<->value mapping ---

    #[test]
    fn test_select_action_toggle_attenuator_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleAttenuator, 0),
            ExecuteAction::SetAttenuatorOn(true)
        );
    }

    #[test]
    fn test_select_action_set_preamp_mode_cursor_maps_ipo_amp1_amp2_in_order() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetPreampMode, 0),
            ExecuteAction::SetPreampMode(PreampMode::Ipo)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetPreampMode, 1),
            ExecuteAction::SetPreampMode(PreampMode::Amp1)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetPreampMode, 2),
            ExecuteAction::SetPreampMode(PreampMode::Amp2)
        );
    }

    #[test]
    fn test_select_action_set_agc_mode_cursor_maps_off_fast_mid_slow_auto_in_order() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetAgcMode, 0),
            ExecuteAction::SetAgcMode(AgcMode::Off)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetAgcMode, 1),
            ExecuteAction::SetAgcMode(AgcMode::Fast)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetAgcMode, 2),
            ExecuteAction::SetAgcMode(AgcMode::Mid)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetAgcMode, 3),
            ExecuteAction::SetAgcMode(AgcMode::Slow)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetAgcMode, 4),
            ExecuteAction::SetAgcMode(AgcMode::AutoFast)
        );
    }

    #[test]
    fn test_select_action_set_filter_width_index_cursor_maps_directly_to_index() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetFilterWidthIndex, 0),
            ExecuteAction::SetFilterWidthIndex(0)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetFilterWidthIndex, 21),
            ExecuteAction::SetFilterWidthIndex(21)
        );
    }

    #[test]
    fn test_filter_width_options_has_twenty_two_entries_matching_sh_bandwidth_table() {
        assert_eq!(filter_width_options().len(), SH_BANDWIDTH_TABLE.len());
        assert_eq!(filter_width_options().len(), 22);
    }

    #[test]
    fn test_initial_list_cursor_group7_and_group8_default_to_zero() {
        let d = display();
        for action in [
            SelectAction::ToggleAttenuator,
            SelectAction::SetPreampMode,
            SelectAction::ToggleNoiseBlanker,
            SelectAction::ToggleNoiseReduction,
            SelectAction::SetAgcMode,
            SelectAction::ToggleAutoNotch,
            SelectAction::ToggleNarrow,
            SelectAction::SetFilterWidthIndex,
            SelectAction::ToggleContour,
            SelectAction::ToggleApf,
            SelectAction::ToggleManualNotch,
            SelectAction::ToggleSpeechProcessor,
            SelectAction::ToggleMonitor,
            SelectAction::ToggleParametricMicEq,
        ] {
            assert_eq!(initial_list_cursor(action, &d), 0);
        }
    }

    // --- Group 7: validation ranges ---

    #[test]
    fn test_noise_blanker_level_0_accepted_at_min() {
        let mut state = text_input(InputAction::SetNoiseBlankerLevel, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetNoiseBlankerLevel(0))
        );
    }

    #[test]
    fn test_noise_blanker_level_10_accepted_at_max() {
        let mut state = text_input(InputAction::SetNoiseBlankerLevel, "10");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetNoiseBlankerLevel(10))
        );
    }

    #[test]
    fn test_noise_blanker_level_11_rejected_above_max() {
        let mut state = text_input(InputAction::SetNoiseBlankerLevel, "11");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_noise_reduction_level_1_accepted_at_min() {
        let mut state = text_input(InputAction::SetNoiseReductionLevel, "1");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetNoiseReductionLevel(1))
        );
    }

    #[test]
    fn test_noise_reduction_level_15_accepted_at_max() {
        let mut state = text_input(InputAction::SetNoiseReductionLevel, "15");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetNoiseReductionLevel(15))
        );
    }

    #[test]
    fn test_noise_reduction_level_0_rejected_below_min() {
        // Unlike `NL`, `RL` has no `0` — `NR`'s own on/off gate covers that.
        let mut state = text_input(InputAction::SetNoiseReductionLevel, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_noise_reduction_level_16_rejected_above_max() {
        let mut state = text_input(InputAction::SetNoiseReductionLevel, "16");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_contour_frequency_min_boundary_10_accepted() {
        let mut state = text_input(InputAction::SetContourFrequencyHz, "10");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetContourFrequencyHz(10))
        );
    }

    #[test]
    fn test_contour_frequency_max_boundary_3200_accepted() {
        let mut state = text_input(InputAction::SetContourFrequencyHz, "3200");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetContourFrequencyHz(3200))
        );
    }

    #[test]
    fn test_contour_frequency_odd_value_accepted_no_step_constraint() {
        // Unlike APF/manual notch, contour frequency has no documented step
        // constraint (`RadioError::InvalidContourFrequency`'s own message:
        // "valid: 10-3200", no step mentioned).
        let mut state = text_input(InputAction::SetContourFrequencyHz, "1234");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetContourFrequencyHz(1234))
        );
    }

    #[test]
    fn test_contour_frequency_9_rejected_below_min() {
        let mut state = text_input(InputAction::SetContourFrequencyHz, "9");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_contour_frequency_3201_rejected_above_max() {
        let mut state = text_input(InputAction::SetContourFrequencyHz, "3201");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_apf_frequency_min_boundary_negative_250_accepted() {
        let mut state = text_input(InputAction::SetApfFrequencyHz, "-250");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetApfFrequencyHz(-250))
        );
    }

    #[test]
    fn test_apf_frequency_max_boundary_250_accepted() {
        let mut state = text_input(InputAction::SetApfFrequencyHz, "250");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetApfFrequencyHz(250))
        );
    }

    #[test]
    fn test_apf_frequency_zero_accepted() {
        let mut state = text_input(InputAction::SetApfFrequencyHz, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetApfFrequencyHz(0))
        );
    }

    #[test]
    fn test_apf_frequency_below_min_rejected() {
        let mut state = text_input(InputAction::SetApfFrequencyHz, "-260");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_apf_frequency_above_max_rejected() {
        let mut state = text_input(InputAction::SetApfFrequencyHz, "260");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_apf_frequency_non_multiple_of_10_rejected() {
        let mut state = text_input(InputAction::SetApfFrequencyHz, "5");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_manual_notch_frequency_min_boundary_10_accepted() {
        let mut state = text_input(InputAction::SetManualNotchFrequencyHz, "10");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetManualNotchFrequencyHz(10))
        );
    }

    #[test]
    fn test_manual_notch_frequency_max_boundary_3200_accepted() {
        let mut state = text_input(InputAction::SetManualNotchFrequencyHz, "3200");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetManualNotchFrequencyHz(3200))
        );
    }

    #[test]
    fn test_manual_notch_frequency_below_min_rejected() {
        let mut state = text_input(InputAction::SetManualNotchFrequencyHz, "9");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_manual_notch_frequency_above_max_rejected() {
        let mut state = text_input(InputAction::SetManualNotchFrequencyHz, "3201");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_manual_notch_frequency_non_multiple_of_10_rejected() {
        let mut state = text_input(InputAction::SetManualNotchFrequencyHz, "15");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    // --- Group 8: per-key state transitions ---

    #[test]
    fn test_speech_g_transitions_to_text_input_mic_gain() {
        let mut state = group_menu(CommandGroup::SpeechMicMonitor);
        handle_key(key(KeyCode::Char('g')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetMicGain,
                ..
            }
        ));
    }

    #[test]
    fn test_speech_l_transitions_to_text_input_speech_processor_level() {
        let mut state = group_menu(CommandGroup::SpeechMicMonitor);
        handle_key(key(KeyCode::Char('l')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetSpeechProcessorLevel,
                ..
            }
        ));
    }

    #[test]
    fn test_speech_s_transitions_to_list_select_speech_processor_on() {
        let mut state = group_menu(CommandGroup::SpeechMicMonitor);
        handle_key(key(KeyCode::Char('s')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleSpeechProcessor,
                ..
            }
        ));
    }

    #[test]
    fn test_speech_m_transitions_to_list_select_monitor_on() {
        let mut state = group_menu(CommandGroup::SpeechMicMonitor);
        handle_key(key(KeyCode::Char('m')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleMonitor,
                ..
            }
        ));
    }

    #[test]
    fn test_speech_v_transitions_to_text_input_monitor_level() {
        let mut state = group_menu(CommandGroup::SpeechMicMonitor);
        handle_key(key(KeyCode::Char('v')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetMonitorLevel,
                ..
            }
        ));
    }

    #[test]
    fn test_speech_e_transitions_to_list_select_parametric_mic_eq_on() {
        let mut state = group_menu(CommandGroup::SpeechMicMonitor);
        handle_key(key(KeyCode::Char('e')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleParametricMicEq,
                ..
            }
        ));
    }

    // --- Group 8: SelectAction cursor<->value mapping ---

    #[test]
    fn test_select_action_toggle_speech_processor_cursor_1_is_off() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleSpeechProcessor, 1),
            ExecuteAction::SetSpeechProcessorOn(false)
        );
    }

    #[test]
    fn test_select_action_toggle_monitor_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleMonitor, 0),
            ExecuteAction::SetMonitorOn(true)
        );
    }

    #[test]
    fn test_select_action_toggle_parametric_mic_eq_cursor_1_is_off() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleParametricMicEq, 1),
            ExecuteAction::SetParametricMicEqOn(false)
        );
    }

    // --- Group 8: validation ranges (mic gain / speech processor level /
    // monitor level all share the plain 0-100 shape) ---

    #[test]
    fn test_mic_gain_0_accepted_at_min() {
        let mut state = text_input(InputAction::SetMicGain, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetMicGain(0)));
    }

    #[test]
    fn test_mic_gain_100_accepted_at_max() {
        let mut state = text_input(InputAction::SetMicGain, "100");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetMicGain(100)));
    }

    #[test]
    fn test_mic_gain_101_rejected_above_max() {
        let mut state = text_input(InputAction::SetMicGain, "101");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_speech_processor_level_0_accepted_at_min() {
        let mut state = text_input(InputAction::SetSpeechProcessorLevel, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetSpeechProcessorLevel(0))
        );
    }

    #[test]
    fn test_speech_processor_level_100_accepted_at_max() {
        let mut state = text_input(InputAction::SetSpeechProcessorLevel, "100");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetSpeechProcessorLevel(100))
        );
    }

    #[test]
    fn test_speech_processor_level_101_rejected_above_max() {
        let mut state = text_input(InputAction::SetSpeechProcessorLevel, "101");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_monitor_level_0_accepted_at_min() {
        let mut state = text_input(InputAction::SetMonitorLevel, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetMonitorLevel(0))
        );
    }

    #[test]
    fn test_monitor_level_100_accepted_at_max() {
        let mut state = text_input(InputAction::SetMonitorLevel, "100");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetMonitorLevel(100))
        );
    }

    #[test]
    fn test_monitor_level_101_rejected_above_max() {
        let mut state = text_input(InputAction::SetMonitorLevel, "101");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // =========================================================================
    // Group 10 (MetersStatus)
    // =========================================================================

    // --- Reachability from Menu ---

    #[test]
    fn test_meters_status_reachable_via_t_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('T')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::MetersStatus,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- Key counts and uniqueness ---

    #[test]
    fn test_meters_status_group_has_eight_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::MetersStatus);
        assert_eq!(labels.len(), 8);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- No-op regression guard ---

    #[test]
    fn test_meters_status_q_key_is_a_no_op() {
        // 'Q' is not one of group 10's 8 keys (M C D F I R N U) — must not
        // panic and must not transition state.
        let mut state = group_menu(CommandGroup::MetersStatus);
        let result = handle_key(key(KeyCode::Char('Q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::MetersStatus,
                ..
            }
        ));
    }

    // --- Per-key state transitions ---

    #[test]
    fn test_meters_status_m_transitions_to_list_select_select_meter_with_six_options() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        handle_key(key(KeyCode::Char('m')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["COMP", "ALC", "PO", "SWR", "ID", "VDD"]);
                assert_eq!(action, SelectAction::SelectMeter);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_meters_status_c_is_immediate_get_selected_meter() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        let result = handle_key(key(KeyCode::Char('c')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::GetSelectedMeter));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_meters_status_d_transitions_to_list_select_read_meter_direct_with_six_options() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        handle_key(key(KeyCode::Char('d')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["COMP", "ALC", "PO", "SWR", "ID", "VDD"]);
                assert_eq!(action, SelectAction::ReadMeterDirect);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_meters_status_f_is_immediate_get_active_meter_reading() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        let result = handle_key(key(KeyCode::Char('f')), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::GetActiveMeterReading)
        );
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_meters_status_i_is_immediate_get_information() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        let result = handle_key(key(KeyCode::Char('i')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::GetInformation));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_meters_status_r_transitions_to_list_select_read_radio_indicator_with_seven_options() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        handle_key(key(KeyCode::Char('r')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(
                    options,
                    vec!["Hi-SWR", "REC", "PLAY", "VFO-A TX", "VFO-B TX", "VFO-A RX", "TX LED"]
                );
                assert_eq!(action, SelectAction::ReadRadioIndicator);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_meters_status_n_is_immediate_get_menu_mode_active() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        let result = handle_key(key(KeyCode::Char('n')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::GetMenuModeActive));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_meters_status_u_is_immediate_get_pll_unlocked() {
        let mut state = group_menu(CommandGroup::MetersStatus);
        let result = handle_key(key(KeyCode::Char('u')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::GetPllUnlocked));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    // --- SelectAction cursor<->value mapping ---

    #[test]
    fn test_select_action_select_meter_cursor_maps_in_ms_wire_order() {
        assert_eq!(
            select_action_to_execute(SelectAction::SelectMeter, 0),
            ExecuteAction::SelectMeter(Meter::Comp)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SelectMeter, 5),
            ExecuteAction::SelectMeter(Meter::Vdd)
        );
    }

    #[test]
    fn test_select_action_read_meter_direct_cursor_maps_in_ms_wire_order() {
        assert_eq!(
            select_action_to_execute(SelectAction::ReadMeterDirect, 3),
            ExecuteAction::ReadMeterDirect(Meter::Swr)
        );
    }

    #[test]
    fn test_select_action_read_radio_indicator_cursor_maps_in_ri_wire_order() {
        assert_eq!(
            select_action_to_execute(SelectAction::ReadRadioIndicator, 0),
            ExecuteAction::GetRadioIndicator(RadioIndicator::HiSwr)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::ReadRadioIndicator, 6),
            ExecuteAction::GetRadioIndicator(RadioIndicator::TxLed)
        );
    }

    #[test]
    fn test_initial_list_cursor_group10_defaults_to_zero() {
        let d = display();
        for action in [
            SelectAction::SelectMeter,
            SelectAction::ReadMeterDirect,
            SelectAction::ReadRadioIndicator,
        ] {
            assert_eq!(initial_list_cursor(action, &d), 0);
        }
    }

    // =========================================================================
    // Group 11 (SystemTunerDvs)
    // =========================================================================

    // --- Reachability from Menu ---

    #[test]
    fn test_system_tuner_dvs_reachable_via_s_and_esc_returns() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('S')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::SystemTunerDvs,
                ..
            }
        ));
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    // --- Key counts and uniqueness ---

    #[test]
    fn test_system_tuner_dvs_group_has_twenty_two_commands_all_unique_keys() {
        let labels = group_command_labels(CommandGroup::SystemTunerDvs);
        assert_eq!(labels.len(), 22);
        let mut keys: Vec<char> = labels.into_iter().map(|(k, _)| k).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }

    // --- No-op regression guard ---

    #[test]
    fn test_system_tuner_dvs_q_key_is_a_no_op() {
        // 'Q' is not one of group 11's 22 keys — must not panic and must
        // not transition state.
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('Q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::SystemTunerDvs,
                ..
            }
        ));
    }

    // --- Per-key state transitions ---

    #[test]
    fn test_system_tuner_dvs_a_transitions_to_list_select_toggle_auto_info() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('a')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleAutoInfo,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_l_transitions_to_list_select_toggle_frequency_lock() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('l')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleFrequencyLock,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_r_transitions_to_list_select_repeater_shift_with_three_options() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('r')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["Simplex", "Plus", "Minus"]);
                assert_eq!(action, SelectAction::SetRepeaterShift);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_system_tuner_dvs_v_transitions_to_list_select_tx_vfo_with_two_options() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('v')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["VFO A", "VFO B"]);
                assert_eq!(action, SelectAction::SetTxVfo);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_system_tuner_dvs_x_transitions_to_list_select_toggle_mox() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('x')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleMox,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_t_transitions_to_list_select_antenna_tuner_state_with_three_options() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('t')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options, vec!["Off", "On", "Tune Start/Stop"]);
                assert_eq!(action, SelectAction::SetAntennaTunerState);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_system_tuner_dvs_b_transitions_to_text_input_set_dimmer() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('b')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetDimmer,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_k_is_immediate_get_dimmer() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('k')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::GetDimmer));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_d_transitions_to_text_input_set_date() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('d')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetDate,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_y_is_immediate_read_date() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('y')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::ReadDate));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_h_transitions_to_text_input_set_time() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('h')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetTime,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_n_is_immediate_read_time() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('n')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::ReadTime));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_z_transitions_to_text_input_set_time_zone_offset() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetTimeZoneOffset,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_f_is_immediate_read_time_zone_offset() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('f')), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ReadTimeZoneOffset)
        );
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_i_is_immediate_get_opposite_band_information() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('i')), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::GetOppositeBandInformation)
        );
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_w_transitions_to_list_select_toggle_txw() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('w')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::ToggleTxw,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_c_transitions_to_text_input_start_dvs_recording() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('c')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::StartDvsRecording,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_e_is_immediate_stop_dvs_recording() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('e')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::StopDvsRecording));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_g_is_immediate_get_dvs_recording_channel() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('g')), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::GetDvsRecordingChannel)
        );
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_p_transitions_to_text_input_start_dvs_playback() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        handle_key(key(KeyCode::Char('p')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::StartDvsPlayback,
                ..
            }
        ));
    }

    #[test]
    fn test_system_tuner_dvs_s_is_immediate_stop_dvs_playback() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('s')), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::StopDvsPlayback));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_system_tuner_dvs_u_is_immediate_get_dvs_playback_channel() {
        let mut state = group_menu(CommandGroup::SystemTunerDvs);
        let result = handle_key(key(KeyCode::Char('u')), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::GetDvsPlaybackChannel)
        );
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    // --- SelectAction cursor<->value mapping ---

    #[test]
    fn test_select_action_toggle_auto_info_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleAutoInfo, 0),
            ExecuteAction::SetAutoInfoOn(true)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleAutoInfo, 1),
            ExecuteAction::SetAutoInfoOn(false)
        );
    }

    #[test]
    fn test_select_action_toggle_frequency_lock_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleFrequencyLock, 0),
            ExecuteAction::SetFrequencyLock(true)
        );
    }

    #[test]
    fn test_select_action_set_repeater_shift_cursor_maps_simplex_plus_minus_in_order() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetRepeaterShift, 0),
            ExecuteAction::SetRepeaterShift(RepeaterShift::Simplex)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetRepeaterShift, 1),
            ExecuteAction::SetRepeaterShift(RepeaterShift::Plus)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetRepeaterShift, 2),
            ExecuteAction::SetRepeaterShift(RepeaterShift::Minus)
        );
    }

    #[test]
    fn test_select_action_set_tx_vfo_cursor_maps_vfo_a_vfo_b() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetTxVfo, 0),
            ExecuteAction::SetTxVfo(0)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetTxVfo, 1),
            ExecuteAction::SetTxVfo(1)
        );
    }

    #[test]
    fn test_select_action_toggle_mox_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleMox, 0),
            ExecuteAction::SetMoxOn(true)
        );
    }

    #[test]
    fn test_select_action_set_antenna_tuner_state_cursor_maps_directly_to_state() {
        assert_eq!(
            select_action_to_execute(SelectAction::SetAntennaTunerState, 0),
            ExecuteAction::SetAntennaTunerState(0)
        );
        assert_eq!(
            select_action_to_execute(SelectAction::SetAntennaTunerState, 2),
            ExecuteAction::SetAntennaTunerState(2)
        );
    }

    #[test]
    fn test_select_action_toggle_txw_cursor_0_is_on() {
        assert_eq!(
            select_action_to_execute(SelectAction::ToggleTxw, 0),
            ExecuteAction::SetTxwOn(true)
        );
    }

    #[test]
    fn test_initial_list_cursor_group11_defaults_to_zero() {
        let d = display();
        for action in [
            SelectAction::ToggleAutoInfo,
            SelectAction::ToggleFrequencyLock,
            SelectAction::SetRepeaterShift,
            SelectAction::SetTxVfo,
            SelectAction::ToggleMox,
            SelectAction::SetAntennaTunerState,
            SelectAction::ToggleTxw,
        ] {
            assert_eq!(initial_list_cursor(action, &d), 0);
        }
    }

    // --- Validation: dimmer ("led:tft") ---

    #[test]
    fn test_dimmer_valid_input_accepted() {
        let mut state = text_input(InputAction::SetDimmer, "2:8");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetDimmer(2, 8)));
    }

    #[test]
    fn test_dimmer_led_below_min_rejected() {
        let mut state = text_input(InputAction::SetDimmer, "0:8");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_dimmer_led_above_max_rejected() {
        let mut state = text_input(InputAction::SetDimmer, "3:8");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_dimmer_tft_above_max_rejected() {
        let mut state = text_input(InputAction::SetDimmer, "2:16");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_dimmer_tft_min_boundary_0_accepted() {
        let mut state = text_input(InputAction::SetDimmer, "1:0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetDimmer(1, 0)));
    }

    #[test]
    fn test_dimmer_missing_colon_rejected() {
        let mut state = text_input(InputAction::SetDimmer, "28");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- Validation: date ("YYYYMMDD") ---

    #[test]
    fn test_date_valid_input_accepted() {
        let mut state = text_input(InputAction::SetDate, "20260719");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetDate(2026, 7, 19))
        );
    }

    #[test]
    fn test_date_min_boundary_month_day_accepted() {
        let mut state = text_input(InputAction::SetDate, "20260101");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetDate(2026, 1, 1))
        );
    }

    #[test]
    fn test_date_max_boundary_month_day_accepted() {
        let mut state = text_input(InputAction::SetDate, "20261231");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetDate(2026, 12, 31))
        );
    }

    #[test]
    fn test_date_month_13_rejected() {
        let mut state = text_input(InputAction::SetDate, "20261301");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_date_day_32_rejected() {
        let mut state = text_input(InputAction::SetDate, "20260732");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_date_month_0_rejected() {
        let mut state = text_input(InputAction::SetDate, "20260015");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_date_wrong_length_rejected() {
        let mut state = text_input(InputAction::SetDate, "2026719");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_date_non_numeric_rejected() {
        let mut state = text_input(InputAction::SetDate, "2026071a");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- Validation: time ("HHMMSS") ---

    #[test]
    fn test_time_valid_input_accepted() {
        let mut state = text_input(InputAction::SetTime, "143000");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetTime(14, 30, 0))
        );
    }

    #[test]
    fn test_time_min_boundary_000000_accepted() {
        let mut state = text_input(InputAction::SetTime, "000000");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Execute(ExecuteAction::SetTime(0, 0, 0)));
    }

    #[test]
    fn test_time_max_boundary_235959_accepted() {
        let mut state = text_input(InputAction::SetTime, "235959");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetTime(23, 59, 59))
        );
    }

    #[test]
    fn test_time_hour_24_rejected() {
        let mut state = text_input(InputAction::SetTime, "240000");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_minute_60_rejected() {
        let mut state = text_input(InputAction::SetTime, "126000");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_second_60_rejected() {
        let mut state = text_input(InputAction::SetTime, "120060");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_wrong_length_rejected() {
        let mut state = text_input(InputAction::SetTime, "14300");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_non_numeric_rejected() {
        let mut state = text_input(InputAction::SetTime, "14300a");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- Validation: time zone offset ("+HHMM"/"-HHMM") ---

    #[test]
    fn test_time_zone_offset_positive_valid_input_accepted() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "+0930");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetTimeZoneOffset(570))
        );
    }

    #[test]
    fn test_time_zone_offset_negative_valid_input_accepted() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "-0500");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetTimeZoneOffset(-300))
        );
    }

    #[test]
    fn test_time_zone_offset_max_boundary_plus_1400_accepted() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "+1400");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetTimeZoneOffset(840))
        );
    }

    #[test]
    fn test_time_zone_offset_min_boundary_minus_1200_accepted() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "-1200");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetTimeZoneOffset(-720))
        );
    }

    #[test]
    fn test_time_zone_offset_above_max_rejected() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "+1415");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_zone_offset_below_min_rejected() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "-1215");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_zone_offset_non_multiple_of_30_rejected() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "+0915");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_zone_offset_missing_sign_rejected() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "0930");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_zone_offset_wrong_length_rejected() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "+930");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_time_zone_offset_non_numeric_rejected() {
        let mut state = text_input(InputAction::SetTimeZoneOffset, "+09a0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- Validation: DVS channel (1-5), shared by StartDvsRecording/
    // StartDvsPlayback ---

    #[test]
    fn test_dvs_recording_channel_min_boundary_1_accepted() {
        let mut state = text_input(InputAction::StartDvsRecording, "1");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::StartDvsRecording(1))
        );
    }

    #[test]
    fn test_dvs_recording_channel_max_boundary_5_accepted() {
        let mut state = text_input(InputAction::StartDvsRecording, "5");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::StartDvsRecording(5))
        );
    }

    #[test]
    fn test_dvs_recording_channel_0_rejected() {
        let mut state = text_input(InputAction::StartDvsRecording, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_dvs_recording_channel_6_rejected() {
        let mut state = text_input(InputAction::StartDvsRecording, "6");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_dvs_playback_channel_valid_accepted() {
        let mut state = text_input(InputAction::StartDvsPlayback, "3");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::StartDvsPlayback(3))
        );
    }

    #[test]
    fn test_dvs_playback_channel_0_rejected() {
        let mut state = text_input(InputAction::StartDvsPlayback, "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_dvs_playback_channel_6_rejected() {
        let mut state = text_input(InputAction::StartDvsPlayback, "6");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // -----------------------------------------------------------------------
    // Group 12 (`ExMenu`), path (b): number-entry escape hatch (§11.4,
    // Wave 4 Task 8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_ex_number_entry_key_is_unique() {
        // Same uniqueness check `test_menu_group_keys_and_quit_all_unique`
        // already runs for the 12 group keys + Q, extended to also cover
        // `EX_NUMBER_ENTRY_KEY` — guards against a future group reusing
        // 'N' at the top level.
        let mut keys: Vec<char> = ALL_GROUPS.iter().map(|&g| group_key(g)).collect();
        keys.push('Q');
        keys.push(EX_NUMBER_ENTRY_KEY);
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            before,
            "EX_NUMBER_ENTRY_KEY must not collide with any group key or Quit"
        );
    }

    #[test]
    fn test_menu_ex_key_reachable() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('N')), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(
            state,
            ControlState::ExNumberEntry {
                buffer: String::new(),
                error: None,
            }
        );
    }

    #[test]
    fn test_menu_ex_key_lowercase_also_reachable() {
        let mut state = ControlState::Menu;
        handle_key(key(KeyCode::Char('n')), &mut state, &display());
        assert!(matches!(state, ControlState::ExNumberEntry { .. }));
    }

    #[test]
    fn test_ex_number_entry_digits_grow_buffer() {
        let mut state = ControlState::ExNumberEntry {
            buffer: String::new(),
            error: None,
        };
        handle_key(key(KeyCode::Char('6')), &mut state, &display());
        handle_key(key(KeyCode::Char('0')), &mut state, &display());
        assert_eq!(
            state,
            ControlState::ExNumberEntry {
                buffer: "60".to_string(),
                error: None,
            }
        );
    }

    #[test]
    fn test_ex_number_entry_buffer_capped_at_3_digits() {
        let mut state = ControlState::ExNumberEntry {
            buffer: String::new(),
            error: None,
        };
        for c in "1234".chars() {
            handle_key(key(KeyCode::Char(c)), &mut state, &display());
        }
        match &state {
            ControlState::ExNumberEntry { buffer, .. } => assert_eq!(buffer, "123"),
            other => panic!("expected ExNumberEntry, got {other:?}"),
        }
    }

    #[test]
    fn test_ex_number_entry_non_digit_ignored() {
        let mut state = ControlState::ExNumberEntry {
            buffer: String::new(),
            error: None,
        };
        handle_key(key(KeyCode::Char('a')), &mut state, &display());
        match &state {
            ControlState::ExNumberEntry { buffer, .. } => assert!(buffer.is_empty()),
            other => panic!("expected ExNumberEntry, got {other:?}"),
        }
    }

    #[test]
    fn test_ex_number_entry_backspace_clears_buffer_and_error() {
        let mut state = ControlState::ExNumberEntry {
            buffer: "60".to_string(),
            error: Some("stale".to_string()),
        };
        handle_key(key(KeyCode::Backspace), &mut state, &display());
        assert_eq!(
            state,
            ControlState::ExNumberEntry {
                buffer: "6".to_string(),
                error: None,
            }
        );
    }

    #[test]
    fn test_ex_number_entry_esc_returns_to_menu() {
        let mut state = ControlState::ExNumberEntry {
            buffer: "60".to_string(),
            error: None,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(state, ControlState::Menu);
    }

    #[test]
    fn test_ex_number_entry_enter_empty_buffer_errors_and_stays() {
        let mut state = ControlState::ExNumberEntry {
            buffer: String::new(),
            error: None,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        match &state {
            ControlState::ExNumberEntry { buffer, error } => {
                assert!(buffer.is_empty());
                assert!(error.is_some());
            }
            other => panic!("expected ExNumberEntry, got {other:?}"),
        }
    }

    #[test]
    fn test_ex_number_entry_enter_unknown_p1_errors_and_stays() {
        // 999 is not a landed EX_MENU_TABLE row (max is 153).
        let mut state = ControlState::ExNumberEntry {
            buffer: "999".to_string(),
            error: None,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        match &state {
            ControlState::ExNumberEntry { buffer, error } => {
                assert_eq!(buffer, "999");
                assert_eq!(error.as_deref(), Some("No such menu item"));
            }
            other => panic!("expected ExNumberEntry, got {other:?}"),
        }
    }

    #[test]
    fn test_ex_number_entry_enter_valid_enumerated_p1_forks_to_list_select() {
        // Item 060 "PC KEYING": Enumerated [OFF, DAKY, RTS, DTR].
        let mut state = ControlState::ExNumberEntry {
            buffer: "60".to_string(),
            error: None,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        match &state {
            ControlState::ListSelect {
                options,
                cursor,
                action,
            } => {
                assert_eq!(*cursor, 0);
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
    }

    #[test]
    fn test_ex_number_entry_enter_valid_range_p1_forks_to_text_input() {
        // Item 001 "AGC FAST DELAY": Range { min: 20, max: 4000, step: 20 }.
        let mut state = ControlState::ExNumberEntry {
            buffer: "1".to_string(),
            error: None,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        match &state {
            ControlState::TextInput {
                prompt,
                buffer,
                error,
                action,
            } => {
                assert_eq!(*action, InputAction::SetExMenuItem(1));
                assert!(buffer.is_empty());
                assert!(error.is_none());
                assert!(prompt.contains("AGC FAST DELAY"), "prompt: {prompt}");
                assert!(prompt.contains("20..=4000"), "prompt: {prompt}");
            }
            other => panic!("expected TextInput, got {other:?}"),
        }
    }

    #[test]
    fn test_ex_number_entry_leading_zeros_accepted() {
        // "060" and "60" must resolve to the same item (u16 parse ignores
        // leading zeros).
        let mut state = ControlState::ExNumberEntry {
            buffer: "060".to_string(),
            error: None,
        };
        handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::ListSelect {
                action: SelectAction::SetExMenuItem(60),
                ..
            }
        ));
    }

    #[test]
    fn test_ex_value_entry_list_select_esc_returns_to_ex_number_entry_with_p1() {
        let mut state = ControlState::ListSelect {
            options: vec!["OFF".to_string(), "DAKY".to_string()],
            cursor: 1,
            action: SelectAction::SetExMenuItem(60),
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(
            state,
            ControlState::ExNumberEntry {
                buffer: "60".to_string(),
                error: None,
            }
        );
    }

    #[test]
    fn test_ex_value_entry_text_input_esc_returns_to_ex_number_entry_with_p1() {
        let mut state = ControlState::TextInput {
            prompt: "AGC FAST DELAY (20..=4000, step 20):".to_string(),
            buffer: "100".to_string(),
            error: None,
            action: InputAction::SetExMenuItem(1),
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(
            state,
            ControlState::ExNumberEntry {
                buffer: "1".to_string(),
                error: None,
            }
        );
    }

    #[test]
    fn test_ex_value_entry_other_text_input_esc_still_returns_to_menu() {
        // Non-EX `TextInput` actions must be unaffected by the new
        // EX-specific Esc redirect.
        let mut state = text_input(InputAction::SetVfoA, "14250000");
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(state, ControlState::Menu);
    }

    #[test]
    fn test_ex_value_entry_other_list_select_esc_still_returns_to_menu() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 0,
            action: SelectAction::SetMode,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(state, ControlState::Menu);
    }

    #[test]
    fn test_ex_value_entry_enumerated_confirm_produces_execute_action() {
        let mut state = ControlState::ListSelect {
            options: vec![
                "OFF".to_string(),
                "DAKY".to_string(),
                "RTS".to_string(),
                "DTR".to_string(),
            ],
            cursor: 2, // RTS -> wire "2"
            action: SelectAction::SetExMenuItem(60),
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetExMenuItem(60, 2))
        );
    }

    #[test]
    fn test_ex_value_entry_range_min_boundary_20_accepted() {
        let mut state = text_input(InputAction::SetExMenuItem(1), "20");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetExMenuItem(1, 20))
        );
    }

    #[test]
    fn test_ex_value_entry_range_max_boundary_4000_accepted() {
        let mut state = text_input(InputAction::SetExMenuItem(1), "4000");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::SetExMenuItem(1, 4000))
        );
    }

    #[test]
    fn test_ex_value_entry_range_below_min_rejected() {
        let mut state = text_input(InputAction::SetExMenuItem(1), "0");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        match &state {
            ControlState::TextInput { error, .. } => assert!(error.is_some()),
            other => panic!("expected TextInput, got {other:?}"),
        }
    }

    #[test]
    fn test_ex_value_entry_range_above_max_rejected() {
        let mut state = text_input(InputAction::SetExMenuItem(1), "4020");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_ex_value_entry_range_off_step_rejected() {
        // 21 is in [20, 4000] but not a multiple of the 20 step.
        let mut state = text_input(InputAction::SetExMenuItem(1), "21");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_ex_value_entry_range_non_numeric_rejected() {
        let mut state = text_input(InputAction::SetExMenuItem(1), "abc");
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_validate_text_input_ex_menu_item_unknown_p1_errors() {
        let err = validate_text_input(InputAction::SetExMenuItem(9999), "100").unwrap_err();
        assert_eq!(err, "No such menu item");
    }

    #[test]
    fn test_select_action_to_execute_ex_menu_item_out_of_range_cursor_defaults_to_zero() {
        // Defensive fallback path (documented in `select_action_to_execute`'s
        // own comment) — cursor 99 is out of bounds for item 60's 4-value
        // Enumerated list.
        let exec = select_action_to_execute(SelectAction::SetExMenuItem(60), 99);
        assert_eq!(exec, ExecuteAction::SetExMenuItem(60, 0));
    }

    #[test]
    fn test_initial_list_cursor_ex_menu_item_defaults_to_zero() {
        assert_eq!(
            initial_list_cursor(SelectAction::SetExMenuItem(60), &display()),
            0
        );
    }

    // -----------------------------------------------------------------------
    // Group 12 (`ExMenu`), path (a): themed browsing (§11.4, Wave 4 Task 9)
    // -----------------------------------------------------------------------

    #[test]
    fn test_ex_menu_group_has_seven_commands_all_unique_keys() {
        // 6 themed sub-groups + the number-entry escape hatch folded in.
        let labels = group_command_labels(CommandGroup::ExMenu);
        assert_eq!(labels.len(), 7);
        let mut keys: Vec<char> = labels.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 7, "ExMenu group keys must all be unique");
    }

    #[test]
    fn test_ex_theme_ranges_partition_ex_menu_table_with_no_overlap() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        let mut total = 0;
        for &theme in ALL_EX_THEMES.iter() {
            let items = ex_theme_items(theme);
            let (lo, hi) = ex_theme_range(theme);
            for item in &items {
                assert!(
                    item.p1 >= lo && item.p1 <= hi,
                    "item {} bucketed into {theme:?} outside its own range {lo}..={hi}",
                    item.p1
                );
                assert!(
                    seen.insert(item.p1),
                    "p1 {} bucketed into more than one theme",
                    item.p1
                );
            }
            total += items.len();
        }
        assert_eq!(
            total,
            EX_MENU_TABLE.len(),
            "themed buckets must cover every landed EX_MENU_TABLE row exactly once"
        );
    }

    #[test]
    fn test_ex_theme_boundary_spot_checks() {
        assert_eq!(ex_theme_range(ExTheme::GeneralAgcCw), (1, 46));
        assert_eq!(ex_theme_range(ExTheme::TxAudioChain), (47, 79));
        assert_eq!(ex_theme_range(ExTheme::Mixed), (80, 91));
        assert_eq!(ex_theme_range(ExTheme::RttySsbTxChain), (92, 110));
        assert_eq!(ex_theme_range(ExTheme::MeterScope), (111, 136));
        assert_eq!(ex_theme_range(ExTheme::BandLimitVox), (137, 153));

        let has = |theme: ExTheme, p1: u16| ex_theme_items(theme).iter().any(|i| i.p1 == p1);

        // 46/47 straddle GeneralAgcCw/TxAudioChain.
        assert!(has(ExTheme::GeneralAgcCw, 46));
        assert!(!has(ExTheme::GeneralAgcCw, 47));
        assert!(has(ExTheme::TxAudioChain, 47));
        // 79/80 straddle TxAudioChain/Mixed.
        assert!(has(ExTheme::TxAudioChain, 79));
        assert!(has(ExTheme::Mixed, 80));
        // 91/92 straddle Mixed/RttySsbTxChain.
        assert!(has(ExTheme::Mixed, 91));
        assert!(has(ExTheme::RttySsbTxChain, 92));
        // 110/111 straddle RttySsbTxChain/MeterScope.
        assert!(has(ExTheme::RttySsbTxChain, 110));
        assert!(has(ExTheme::MeterScope, 111));
        // 136/137 straddle MeterScope/BandLimitVox.
        assert!(has(ExTheme::MeterScope, 136));
        assert!(has(ExTheme::BandLimitVox, 137));
        // 1 and 153 are the table's own first/last items.
        assert!(has(ExTheme::GeneralAgcCw, 1));
        assert!(has(ExTheme::BandLimitVox, 153));
    }

    #[test]
    fn test_ex_theme_general_agc_cw_excludes_known_gap_027() {
        // 027 "TIME ZONE" is permanently unresolvable from the manual
        // (`radio`'s own `EX_MENU_TABLE` doc comment) — the runtime bucket
        // must reflect that real gap, not assume a full 46-item range.
        let items = ex_theme_items(ExTheme::GeneralAgcCw);
        assert_eq!(items.len(), 45);
        assert!(!items.iter().any(|i| i.p1 == 27));
    }

    #[test]
    fn test_ex_theme_mixed_excludes_known_gap_087() {
        // 087 "RADIO ID" is permanently unresolvable, same treatment.
        let items = ex_theme_items(ExTheme::Mixed);
        assert_eq!(items.len(), 11);
        assert!(!items.iter().any(|i| i.p1 == 87));
    }

    #[test]
    fn test_group_menu_ex_theme_key_opens_ex_sub_group_menu() {
        for &theme in ALL_EX_THEMES.iter() {
            let mut state = group_menu(CommandGroup::ExMenu);
            let k = ex_theme_key(theme);
            let result = handle_key(key(KeyCode::Char(k)), &mut state, &display());
            assert_eq!(result, KeyResult::Continue);
            assert_eq!(state, ControlState::ExSubGroupMenu { theme, cursor: 0 });
        }
    }

    #[test]
    fn test_group_menu_ex_number_entry_key_reachable_from_within_group() {
        let mut state = group_menu(CommandGroup::ExMenu);
        let result = handle_key(
            key(KeyCode::Char(EX_NUMBER_ENTRY_KEY)),
            &mut state,
            &display(),
        );
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(
            state,
            ControlState::ExNumberEntry {
                buffer: String::new(),
                error: None,
            }
        );
    }

    #[test]
    fn test_ex_sub_group_menu_esc_returns_to_theme_picker() {
        let mut state = ControlState::ExSubGroupMenu {
            theme: ExTheme::TxAudioChain,
            cursor: 5,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::ExMenu,
                cursor: 0,
            }
        );
    }

    #[test]
    fn test_ex_theme_picker_esc_returns_to_menu() {
        // Completes the "sub-group list -> theme-picker -> Menu" chain the
        // task requires — this leg is generic `GroupMenu` `Esc` behavior,
        // asserted explicitly here so the full chain has its own coverage.
        let mut state = group_menu(CommandGroup::ExMenu);
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(state, ControlState::Menu);
    }

    #[test]
    fn test_ex_sub_group_menu_down_moves_cursor_forward() {
        let mut state = ControlState::ExSubGroupMenu {
            theme: ExTheme::TxAudioChain,
            cursor: 0,
        };
        handle_key(key(KeyCode::Down), &mut state, &display());
        assert_eq!(
            state,
            ControlState::ExSubGroupMenu {
                theme: ExTheme::TxAudioChain,
                cursor: 1,
            }
        );
    }

    #[test]
    fn test_ex_sub_group_menu_up_does_not_go_below_zero() {
        let mut state = ControlState::ExSubGroupMenu {
            theme: ExTheme::TxAudioChain,
            cursor: 0,
        };
        handle_key(key(KeyCode::Up), &mut state, &display());
        assert_eq!(
            state,
            ControlState::ExSubGroupMenu {
                theme: ExTheme::TxAudioChain,
                cursor: 0,
            }
        );
    }

    #[test]
    fn test_ex_sub_group_menu_down_does_not_exceed_last_item() {
        let last = ex_theme_items(ExTheme::Mixed).len() - 1; // smallest theme
        let mut state = ControlState::ExSubGroupMenu {
            theme: ExTheme::Mixed,
            cursor: last,
        };
        handle_key(key(KeyCode::Down), &mut state, &display());
        assert_eq!(
            state,
            ControlState::ExSubGroupMenu {
                theme: ExTheme::Mixed,
                cursor: last,
            }
        );
    }

    #[test]
    fn test_ex_sub_group_menu_vim_keys_jk_also_scroll() {
        let mut state = ControlState::ExSubGroupMenu {
            theme: ExTheme::TxAudioChain,
            cursor: 1,
        };
        handle_key(key(KeyCode::Char('j')), &mut state, &display());
        assert_eq!(
            state,
            ControlState::ExSubGroupMenu {
                theme: ExTheme::TxAudioChain,
                cursor: 2,
            }
        );
        handle_key(key(KeyCode::Char('k')), &mut state, &display());
        handle_key(key(KeyCode::Char('k')), &mut state, &display());
        assert_eq!(
            state,
            ControlState::ExSubGroupMenu {
                theme: ExTheme::TxAudioChain,
                cursor: 0,
            }
        );
    }

    #[test]
    fn test_ex_sub_group_menu_enter_on_enumerated_item_forks_to_list_select() {
        let items = ex_theme_items(ExTheme::TxAudioChain);
        let idx = items
            .iter()
            .position(|i| i.p1 == 60)
            .expect("item 60 lands in TxAudioChain");
        let mut state = ControlState::ExSubGroupMenu {
            theme: ExTheme::TxAudioChain,
            cursor: idx,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        match &state {
            ControlState::ListSelect {
                options,
                cursor,
                action,
            } => {
                assert_eq!(*cursor, 0);
                assert_eq!(
                    *action,
                    SelectAction::SetExMenuItemFromTheme(60, ExTheme::TxAudioChain, idx)
                );
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
    }

    #[test]
    fn test_ex_sub_group_menu_enter_on_range_item_forks_to_text_input() {
        let items = ex_theme_items(ExTheme::GeneralAgcCw);
        let idx = items
            .iter()
            .position(|i| i.p1 == 1)
            .expect("item 1 lands in GeneralAgcCw");
        let mut state = ControlState::ExSubGroupMenu {
            theme: ExTheme::GeneralAgcCw,
            cursor: idx,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        match &state {
            ControlState::TextInput {
                prompt,
                buffer,
                error,
                action,
            } => {
                assert_eq!(
                    *action,
                    InputAction::SetExMenuItemFromTheme(1, ExTheme::GeneralAgcCw, idx)
                );
                assert!(buffer.is_empty());
                assert!(error.is_none());
                assert!(prompt.contains("AGC FAST DELAY"), "prompt: {prompt}");
                assert!(prompt.contains("20..=4000"), "prompt: {prompt}");
            }
            other => panic!("expected TextInput, got {other:?}"),
        }
    }

    #[test]
    fn test_ex_value_entry_from_theme_list_select_esc_returns_to_sub_group_menu() {
        let mut state = ControlState::ListSelect {
            options: vec!["OFF".to_string(), "DAKY".to_string()],
            cursor: 1,
            action: SelectAction::SetExMenuItemFromTheme(60, ExTheme::TxAudioChain, 7),
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(
            state,
            ControlState::ExSubGroupMenu {
                theme: ExTheme::TxAudioChain,
                cursor: 7,
            }
        );
    }

    #[test]
    fn test_ex_value_entry_from_theme_text_input_esc_returns_to_sub_group_menu() {
        let mut state = ControlState::TextInput {
            prompt: "AGC FAST DELAY (20..=4000, step 20):".to_string(),
            buffer: "100".to_string(),
            error: None,
            action: InputAction::SetExMenuItemFromTheme(1, ExTheme::GeneralAgcCw, 0),
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert_eq!(
            state,
            ControlState::ExSubGroupMenu {
                theme: ExTheme::GeneralAgcCw,
                cursor: 0,
            }
        );
    }

    #[test]
    fn test_ex_value_entry_from_theme_confirm_produces_same_execute_action_as_plain() {
        let exec = select_action_to_execute(
            SelectAction::SetExMenuItemFromTheme(60, ExTheme::TxAudioChain, 7),
            2,
        );
        assert_eq!(exec, ExecuteAction::SetExMenuItem(60, 2));
    }

    // --- Path (a) vs path (b) convergence (§11.4: "the two paths converge
    // on the same value-entry state once an item is selected, they only
    // differ in how the item is found") ---

    #[test]
    fn test_path_a_and_path_b_converge_for_enumerated_item_60() {
        // Path (b): [N] -> "060" -> Enter.
        let mut state_b = ControlState::ExNumberEntry {
            buffer: "060".to_string(),
            error: None,
        };
        handle_key(key(KeyCode::Enter), &mut state_b, &display());

        // Path (a): GroupMenu{ExMenu} -> [T] (TxAudioChain, contains 060) ->
        // scroll to item 60 -> Enter.
        let items = ex_theme_items(ExTheme::TxAudioChain);
        let idx = items.iter().position(|i| i.p1 == 60).unwrap();
        let mut state_a = ControlState::ExSubGroupMenu {
            theme: ExTheme::TxAudioChain,
            cursor: idx,
        };
        handle_key(key(KeyCode::Enter), &mut state_a, &display());

        // Both forks must be the same *shape* — same ListSelect options in
        // the same order, i.e. the rendered UI is identical either way.
        let (options_a, cursor_a, action_a) = match &state_a {
            ControlState::ListSelect {
                options,
                cursor,
                action,
            } => (options.clone(), *cursor, *action),
            other => panic!("path (a): expected ListSelect, got {other:?}"),
        };
        let (options_b, cursor_b, action_b) = match &state_b {
            ControlState::ListSelect {
                options,
                cursor,
                action,
            } => (options.clone(), *cursor, *action),
            other => panic!("path (b): expected ListSelect, got {other:?}"),
        };
        assert_eq!(options_a, options_b);
        assert_eq!(cursor_a, cursor_b);

        // The two `action`s are necessarily different variants (path (a)
        // additionally tags its origin for `Esc` routing — see
        // `ExValueEntryOrigin`), but must resolve to the identical
        // `ExecuteAction` for the same list selection, proving the two
        // paths are behaviorally equivalent, not just visually similar.
        assert_eq!(
            action_a,
            SelectAction::SetExMenuItemFromTheme(60, ExTheme::TxAudioChain, idx)
        );
        assert_eq!(action_b, SelectAction::SetExMenuItem(60));
        let selected_cursor = 2; // "RTS"
        assert_eq!(
            select_action_to_execute(action_a, selected_cursor),
            select_action_to_execute(action_b, selected_cursor)
        );
    }

    #[test]
    fn test_path_a_and_path_b_converge_for_range_item_1() {
        // Path (b): [N] -> "001" -> Enter.
        let mut state_b = ControlState::ExNumberEntry {
            buffer: "001".to_string(),
            error: None,
        };
        handle_key(key(KeyCode::Enter), &mut state_b, &display());

        // Path (a): GroupMenu{ExMenu} -> [G] (GeneralAgcCw, contains 001) ->
        // item 1 is first in the theme -> Enter.
        let items = ex_theme_items(ExTheme::GeneralAgcCw);
        let idx = items.iter().position(|i| i.p1 == 1).unwrap();
        let mut state_a = ControlState::ExSubGroupMenu {
            theme: ExTheme::GeneralAgcCw,
            cursor: idx,
        };
        handle_key(key(KeyCode::Enter), &mut state_a, &display());

        let (prompt_a, action_a) = match &state_a {
            ControlState::TextInput { prompt, action, .. } => (prompt.clone(), *action),
            other => panic!("path (a): expected TextInput, got {other:?}"),
        };
        let (prompt_b, action_b) = match &state_b {
            ControlState::TextInput { prompt, action, .. } => (prompt.clone(), *action),
            other => panic!("path (b): expected TextInput, got {other:?}"),
        };
        assert_eq!(prompt_a, prompt_b);

        assert_eq!(
            action_a,
            InputAction::SetExMenuItemFromTheme(1, ExTheme::GeneralAgcCw, idx)
        );
        assert_eq!(action_b, InputAction::SetExMenuItem(1));

        // Same typed value through `validate_text_input` (the same function
        // `handle_key`'s `TextInput` `Enter` arm calls for either path)
        // must produce the identical `ExecuteAction` regardless of path.
        assert_eq!(
            validate_text_input(action_a, "100"),
            validate_text_input(action_b, "100")
        );
    }

    // -----------------------------------------------------------------------
    // Profiles (`planning/architect/task_plan.md` §12.3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_profile_list_key_is_unique() {
        let mut keys: Vec<char> = ALL_GROUPS.iter().map(|&g| group_key(g)).collect();
        keys.push('Q');
        keys.push(EX_NUMBER_ENTRY_KEY);
        keys.push(PROFILE_LIST_KEY);
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            before,
            "PROFILE_LIST_KEY must not collide with any group key, Quit, or EX_NUMBER_ENTRY_KEY"
        );
    }

    #[test]
    fn test_menu_profile_list_key_reachable() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char(PROFILE_LIST_KEY)), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert!(matches!(state, ControlState::ProfileList { .. }));
    }

    #[test]
    fn test_menu_profile_list_key_is_case_insensitive() {
        let mut state = ControlState::Menu;
        let result = handle_key(
            key(KeyCode::Char(PROFILE_LIST_KEY.to_ascii_lowercase())),
            &mut state,
            &display(),
        );
        assert_eq!(result, KeyResult::Continue);
        assert!(matches!(state, ControlState::ProfileList { .. }));
    }

    fn sample_profiles() -> Vec<(String, radio::Profile)> {
        vec![
            (
                "contest".to_string(),
                radio::Profile {
                    mode: Some(Mode::CwU),
                    ..Default::default()
                },
            ),
            (
                "dx".to_string(),
                radio::Profile {
                    mode: Some(Mode::Usb),
                    squelch: Some(10),
                    ..Default::default()
                },
            ),
        ]
    }

    #[test]
    fn test_profile_list_cursor_clamped_at_bounds() {
        let mut state = ControlState::ProfileList {
            profiles: sample_profiles(),
            cursor: 0,
            error: None,
        };
        handle_key(key(KeyCode::Up), &mut state, &display());
        assert!(matches!(state, ControlState::ProfileList { cursor: 0, .. }));

        handle_key(key(KeyCode::Down), &mut state, &display());
        assert!(matches!(state, ControlState::ProfileList { cursor: 1, .. }));

        // Already at the last entry (2 profiles, max index 1) — stays put.
        handle_key(key(KeyCode::Down), &mut state, &display());
        assert!(matches!(state, ControlState::ProfileList { cursor: 1, .. }));
    }

    #[test]
    fn test_profile_list_enter_produces_apply_profile_action() {
        let profiles = sample_profiles();
        let expected_profile = profiles[1].1.clone();
        let mut state = ControlState::ProfileList {
            profiles,
            cursor: 1,
            error: None,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ApplyProfile(
                "dx".to_string(),
                expected_profile
            ))
        );
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_profile_list_enter_with_no_profiles_sets_error() {
        let mut state = ControlState::ProfileList {
            profiles: Vec::new(),
            cursor: 0,
            error: None,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        match state {
            ControlState::ProfileList { error, .. } => assert!(error.is_some()),
            other => panic!("expected ProfileList, got {other:?}"),
        }
    }

    #[test]
    fn test_profile_list_esc_returns_to_menu() {
        let mut state = ControlState::ProfileList {
            profiles: sample_profiles(),
            cursor: 0,
            error: None,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert_eq!(result, KeyResult::Continue);
        assert!(matches!(state, ControlState::Menu));
    }
}
