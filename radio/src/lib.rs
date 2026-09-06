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

//! Yaesu FT-991A CAT Protocol Implementation — first slice.
//!
//! This crate provides the FT-991A-specific pieces of the shared
//! `cat-framework`/`cat-client` CAT engine from `radio-cat-rs`: the single
//! `FT991A_COMMAND_TABLE`, the `Ft991aRadio` emulator state machine, a
//! typed controller client (`Ft991a<S: CatSession>`), and the
//! controller/UI-facing `Radio` trait + domain types.
//!
//! # Scope
//!
//! Wave 1 (first slice): 11 commands (`FA`, `FB`, `MD`, `TX`, `SM`, `PS`,
//! `AG`, `RG`, `SQ`, `PC`, `ID`). Wave 3 batch 9 (meters/status) adds 6
//! more: `IF`, `RM`, `RI`, `RS`, `MS`, `UL`. Wave 3's `EX` first sub-batch
//! adds the `EX` menu command (shared plumbing covering all 6 distinct
//! total-wire-widths in the full 153-item manual table) plus exactly 9
//! menu items (047, 048, 060, 071, 072, 076, 077, 108, 109 — the
//! PTT/keying-relevant ones, including 060 "PC KEYING," which the
//! RTS/DTR CW-keying feature reads/writes) — see [`ExMenuItem`]/
//! [`EX_MENU_TABLE`]. Batch 2 (memory channel records) adds `MC`, `MR`,
//! `MW`, `MT` — see [`MemoryChannelRecord`] (emulator storage),
//! [`MemoryChannelEntry`]/[`MemoryTag`]/[`TaggedMemoryChannel`]
//! (controller-facing domain types). Batch 1 (VFO/split/memory quick-ops)
//! adds `AB`, `BA`, `AM`, `VM`, `MA`, `CH`, `QI`, `QR`, `QS`, `SV` — mostly
//! zero-width `CommandOperation::Action` triggers, plus `Radio` trait
//! growth (`copy_vfo_a_to_b`/`copy_vfo_b_to_a`/`swap_vfos`/
//! `store_vfo_to_memory`/`recall_memory_to_vfo`/`memory_channel_up`/
//! `memory_channel_down`) for the concepts generic enough to belong there.
//! Batch 3 (clarifier/RIT-XIT + tone + IF-shift) adds `RT`, `RC`, `RD`,
//! `RU`, `XT`, `CN`, `CT`, `IS`. Batch 4 (keyer/CW/break-in) adds `KM`,
//! `KP`, `KR`, `KS`, `KY`, `CS`, `ZI`, `BI`, `SD` — `KM` (keyer memory
//! message storage, [`KeyerPlaybackMode`]) and `KY` (stored-message
//! playback, distinct from the not-yet-consumed RTS/DTR real-time CW-keying
//! feature) stay `Ft991a`-inherent-only; break-in, semi break-in delay, CW
//! spot, keyer speed/pitch, keyer on/off, and zero-in are generic enough to
//! join the `Radio` trait. Batch 5 (scan/VOX/busy) adds `SC`, `VX`, `VD`,
//! `VG`, `BY` — all five join the `Radio` trait ([`ScanState`] for `SC`'s
//! 3-valued state); `VD`'s real-world meaning depends on the
//! not-yet-implemented `EX` menu item 142 "VOX SELECT," documented
//! explicitly rather than hidden (see `ft991a_radio.rs`'s module docs).
//! Batch 6 (attenuator/preamp/noise/AGC/notch/filter-width) adds `RA`,
//! `PA`, `NB`, `NL`, `NR`, `RL`, `GT`, `CO`, `BP`, `BC`, `NA`, `SH` — ten of
//! the twelve join the `Radio` trait ([`PreampMode`] for `PA`'s 3-valued
//! state, [`AgcMode`] for `GT`'s write/report domain mismatch); `CO`
//! (Contour/APF) and `BP` (Manual Notch) stay `Ft991a`-inherent-only, same
//! treatment as `EX`/`KM`/`KY`. `NA`'s own per-command box has a genuine
//! wire-diagram typo (`M A` instead of `N A`), resolved in favor of the
//! master table's own code — see `ft991a_radio.rs`'s module docs. `SH`'s
//! full six-column bandwidth table ([`SH_BANDWIDTH_TABLE`]) is transcribed
//! in full, plus [`mode_family_for`]/[`filter_bandwidth_hz`] for resolving
//! it (the table's own family/narrow-wide columns are never referenced by
//! `SH`'s own wire format, only by external state — documented, not
//! implemented as an enforced dependency).
//! Batch 7 (speech processor/mic/monitor) adds `MG`, `PL`, `PR`, `ML` — the
//! architect's own deliberately smallest batch. `PR`'s own per-command box
//! is headed "SPEECH PROCESSOR LEVEL," byte-identical to `PL`'s heading — a
//! genuine manual typo resolved via the master table's own name ("SPEECH
//! PROCESSOR") and the wire content itself (an on/off toggle with an
//! unusual `1`=OFF/`2`=ON encoding), not followed literally. `MG`, `PL`,
//! and `PR`'s speech-processor branch join the `Radio` trait (direct
//! `ts570d::Radio` precedent); `PR`'s Parametric Mic EQ branch stays
//! `Ft991a`-inherent-only (same treatment as `CO`/`BP`); `ML`'s monitor
//! on/off and level join the trait as a documented judgment call (no
//! `ts570d::Radio` precedent exists, but an audio monitor is a
//! near-universal transceiver concept) — see `ft991a_radio.rs`'s module
//! docs for the full citations.
//! Batch 8 (band/step/encoder front-panel controls) adds `BS`, `BU`, `BD`,
//! `FS`, `ED`, `EU`, `EK`, `DN`, `UP`. `BS`'s full 16-band table
//! (`00`-`16`, with a documented gap at `13`) is transcribed in full and
//! modeled as [`Band`]. `DN`'s own per-command box is
//! headed "MIC DWN" despite the master table's plain "DOWN" — resolved via
//! cross-radio corroboration against `ts570d::Radio::mic_down`/`mic_up`
//! (Kenwood's own wire-identical `DN`/`UP` commands), not the heading text
//! alone. `set_band`/`band_up`/`band_down`, `get_fine_step`/`set_fine_step`,
//! and `mic_up`/`mic_down` join the `Radio` trait; `ED`/`EU`/`EK` stay
//! `Ft991a`-inherent-only (FT-991A-specific front-panel encoder/key
//! concepts, no generic precedent) — see `ft991a_radio.rs`'s module docs
//! for the full citations.
//! All derived command-by-command from
//! `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` — see
//! `planning/yaesu/task_plan.md` for the full citation table and
//! corrections found (and flagged) against the architect's original
//! transcription/citations. Later waves grow both `Ft991aCommandId` and the
//! `Radio` trait alongside additional manual-cited command coverage (the
//! remaining ~144 `EX` menu items, etc.).
//!
//! # Architecture
//!
//! - `ft991a`: Typed [`Ft991a`] client, wrapping `cat_client::CatClient` for
//!   sending commands and reading responses.
//! - `ft991a_radio`: The single [`FT991A_COMMAND_TABLE`] and the
//!   `Ft991aRadio` emulator state machine (a `cat_framework::CatRadio`
//!   implementation).
//! - `radio_trait`: Controller/UI-facing `Radio` trait + domain types
//!   (`Frequency`, `Mode`, `TxState`, `RadioError`, `RadioResult`).
//!
//! # Usage
//!
//! ```no_run
//! use radio::FT991A_COMMAND_TABLE;
//!
//! // Look up a command definition in the single command table.
//! let fa = FT991A_COMMAND_TABLE.find("FA").unwrap();
//! assert!(fa.is_readable());
//! assert!(fa.is_writable());
//! ```

pub mod capabilities;
pub mod console_layout;
pub mod ft991a;
pub mod ft991a_radio;
pub mod profile;
pub mod radio_trait;

pub use ft991a::Ft991a;
pub use ft991a_radio::{
    apf_hz_to_raw, apf_raw_to_hz, ctcss_tone_hz, ctcss_tone_index, dcs_code_index, dcs_code_number,
    ex_menu_item, filter_bandwidth_hz, ky_selector_from_wire, ky_selector_to_wire, mode_family_for,
    ChannelStatusFields, EncoderSelector, ExMenuItem, Ft991aCommandId, Ft991aEvent, Ft991aRadio,
    Ft991aState, KeyerPlaybackMode, MemoryChannelRecord, ModeFamily, ShBandwidthRow,
    CTCSS_TONES_DECIHZ, DCS_CODES, EX_MENU_TABLE, FT991A_COMMAND_TABLE, FT991A_ID,
    SH_BANDWIDTH_TABLE,
};
pub use profile::{default_profile_dir, FailedProfiles, LoadedProfiles, Profile, ProfileError};
pub use radio_trait::{
    AgcMode, Band, CwKeying, Frequency, Ft991aExtras, MemoryChannelEntry, MemoryTag, Meter, Mode,
    NopRadio, PreampMode, Radio, RadioError, RadioIndicator, RadioResult, RepeaterShift, ScanState,
    TaggedMemoryChannel, ToneSquelchMode, TxState,
};
