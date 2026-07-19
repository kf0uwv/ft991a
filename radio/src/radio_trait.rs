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

//! FT-991A controller-facing `Radio` trait and shared protocol domain types.
//!
//! This module defines the [`Radio`] trait that a future `ui` crate and
//! controller consume, plus the FT-991A domain types that cross that
//! boundary: [`Frequency`], [`Mode`], [`TxState`], [`ScanState`], [`Band`],
//! [`Meter`], [`RadioIndicator`], [`MemoryChannelEntry`], [`MemoryTag`],
//! [`TaggedMemoryChannel`], [`ToneSquelchMode`], [`PreampMode`],
//! [`AgcMode`], [`RepeaterShift`], [`RadioError`], and [`RadioResult`].
//!
//! # Scope grows alongside command coverage
//!
//! Per `planning/architect/task_plan.md` §4, this trait started as a
//! deliberate **subset** of `ts570d`'s `Radio` trait surface — only the
//! concepts the first 11 commands actually backed (vfo a/b, mode, ptt,
//! smeter, power on/off, af/rf gain, squelch, tx power, id) — and was NOT
//! padded with `unimplemented!()`/default-`NotImplemented` stubs for
//! RIT/XIT, noise blanker, memory channels, CW keyer, etc. ahead of need.
//! It has since grown alongside landed command batches: meters (batch 9)
//! and memory channels (batch 2, `MC`/`MR`/`MW`/`MT`) are both on the
//! trait now, per the `radio-cat-rs`/`ft991a` `CLAUDE.md` "Radio trait
//! scope" section, which explicitly names both as trait-worthy generic
//! concepts (alongside gain/squelch, already present) — not assumed from
//! `ts570d`'s exact surface without checking the FT-991A's own manual.

use std::fmt;

use async_trait::async_trait;
use cat_client::ClientError;
use cat_transport_core::TransportError;
use thiserror::Error;

use crate::ft991a_radio::{ChannelStatusFields, EncoderSelector, KeyerPlaybackMode};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during FT-991A CAT protocol operations.
#[derive(Debug, Error)]
pub enum RadioError {
    #[error("Invalid mode: {0:#x}")]
    InvalidMode(u8),
    #[error("Invalid meter selector: {0}")]
    InvalidMeter(u8),
    #[error("Invalid memory channel: {0} (valid: 1-117)")]
    InvalidMemoryChannel(u8),
    #[error("Invalid memory tag: {0:?} (up to 12 printable ASCII characters, no ';')")]
    InvalidMemoryTag(String),
    #[error("Invalid CTCSS tone: {0} Hz (not one of the 50 standard tones)")]
    InvalidCtcssTone(f32),
    #[error("Invalid DCS code: {0} (not one of the 104 standard codes)")]
    InvalidDcsCode(u16),
    #[error("Invalid tone squelch mode: {0}")]
    InvalidToneSquelchMode(u8),
    #[error("Invalid IF-shift offset: {0} Hz (valid: -1200..=1200, 20 Hz steps)")]
    InvalidIfShift(i16),
    #[error("Invalid keyer memory channel: {0} (valid: 1-5)")]
    InvalidKeyerMemoryChannel(u8),
    #[error("Invalid keyer memory message: {0:?} (1-50 printable ASCII characters, no ';')")]
    InvalidKeyerMessage(String),
    #[error("Invalid keyer speed: {0} WPM (valid: 4-60)")]
    InvalidKeyerSpeed(u8),
    #[error("Invalid keyer pitch: {0} Hz (valid: 300-1050, 10 Hz steps)")]
    InvalidKeyerPitch(u16),
    #[error("Invalid CW break-in delay: {0} ms (valid: 30-3000)")]
    InvalidBreakInDelay(u16),
    #[error("Invalid VOX gain: {0} (valid: 0-100)")]
    InvalidVoxGain(u8),
    #[error("Invalid VOX delay: {0} ms (valid: 30-3000, 10 ms steps)")]
    InvalidVoxDelay(u16),
    #[error("Invalid pre-amp mode: {0} (valid: 0=IPO, 1=AMP1, 2=AMP2)")]
    InvalidPreampMode(u8),
    #[error("Invalid AGC mode: {0} (valid: 0-6)")]
    InvalidAgcMode(u8),
    #[error("Invalid noise blanker level: {0} (valid: 0-10)")]
    InvalidNoiseBlankerLevel(u8),
    #[error("Invalid noise reduction level: {0} (valid: 1-15)")]
    InvalidNoiseReductionLevel(u8),
    #[error("Invalid filter width index: {0} (valid: 0-21)")]
    InvalidFilterWidthIndex(u8),
    #[error("Invalid contour frequency: {0} Hz (valid: 10-3200)")]
    InvalidContourFrequency(u16),
    #[error("Invalid APF frequency: {0} Hz (valid: -250-250, 10 Hz steps)")]
    InvalidApfFrequency(i16),
    #[error("Invalid manual notch frequency: {0} Hz (valid: 10-3200, 10 Hz steps)")]
    InvalidManualNotchFrequency(u16),
    #[error("Invalid mic gain: {0} (valid: 0-100)")]
    InvalidMicGain(u8),
    #[error("Invalid speech processor level: {0} (valid: 0-100)")]
    InvalidSpeechProcessorLevel(u8),
    #[error("Invalid monitor level: {0} (valid: 0-100)")]
    InvalidMonitorLevel(u8),
    #[error(
        "Invalid band: {0} (valid: 0-12, 14-16; 13 is a documented gap in the manual's own table)"
    )]
    InvalidBand(u8),
    #[error("Invalid encoder step count: {0} (valid: 1-99)")]
    InvalidEncoderSteps(u8),
    #[error("Invalid repeater shift: {0} (valid: 0=Simplex, 1=Plus Shift, 2=Minus Shift)")]
    InvalidRepeaterShift(u8),
    #[error("Invalid date: {year:04}-{month:02}-{day:02} (month: 1-12, day: 1-31)")]
    InvalidDate { year: u16, month: u8, day: u8 },
    #[error("Invalid time: {hour:02}:{minute:02}:{second:02} (24-hour)")]
    InvalidTime { hour: u8, minute: u8, second: u8 },
    #[error("Invalid time zone offset: {0} minutes (valid: -720..=840, 30 minute steps)")]
    InvalidTimeZoneOffset(i16),
    #[error("Invalid antenna tuner state: {0} (valid: 0=OFF, 1=ON, 2=Tuning Start/Stop)")]
    InvalidAntennaTunerState(u8),
    #[error("Invalid dimmer level: LED {led} (valid: 1-2), TFT {tft} (valid: 0-15)")]
    InvalidDimmerLevel { led: u8, tft: u8 },
    #[error("Invalid TX VFO select: {0} (valid: 0=VFO-A, 1=VFO-B)")]
    InvalidTxVfo(u8),
    #[error("Invalid DVS channel: {0} (valid: 1-5)")]
    InvalidDvsChannel(u8),
    #[error("Unknown EX menu item: P1={0:03} (not in EX_MENU_TABLE)")]
    UnknownExMenuItem(u16),
    #[error("Frequency out of range: {0} Hz (valid: {min}-{max})", min = Frequency::MIN_HZ, max = Frequency::MAX_HZ)]
    FrequencyOutOfRange(u64),
    #[error("Invalid protocol string: {0}")]
    InvalidProtocolString(String),
    #[error("Unknown command code: {0}")]
    UnknownCommand(String),
    #[error("Command {0} does not support read (query)")]
    CommandNotReadable(String),
    #[error("Command {0} does not support write (set)")]
    CommandNotWritable(String),
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("Not implemented")]
    NotImplemented,
}

/// Convenience [`Result`] alias for radio operations.
pub type RadioResult<T> = Result<T, RadioError>;

/// Reconciles `cat_client`'s generic client-side errors with this crate's
/// FT-991A-flavored [`RadioError`], at the boundary where [`crate::Ft991a`]
/// delegates to `cat_client::CatClient` (see `radio/src/ft991a.rs`).
///
/// Mirrors `ts570d::RadioError`'s equivalent `From` impl.
impl From<ClientError<TransportError>> for RadioError {
    fn from(err: ClientError<TransportError>) -> Self {
        match err {
            ClientError::UnknownCommand(code) => RadioError::UnknownCommand(code),
            ClientError::CommandNotReadable(code) => RadioError::CommandNotReadable(code),
            ClientError::CommandNotWritable(code) => RadioError::CommandNotWritable(code),
            ClientError::ProtocolError(kind) => RadioError::InvalidProtocolString(format!(
                "session reported a protocol error: {:?}",
                kind
            )),
            ClientError::Transport(e) => RadioError::Transport(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Frequency
// ---------------------------------------------------------------------------

/// Frequency in Hertz, validated to the FT-991A's documented range
/// (manual p.9, FA/FB: `000030000-470000000`).
///
/// **Not** compatible with `ts570d::Frequency`'s range (500 kHz-60 MHz) or
/// wire width (11 digits) — the FT-991A uses a 9-digit field and a wider
/// 30 kHz-470 MHz range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frequency(u64);

impl Frequency {
    pub const MIN_HZ: u64 = 30_000;
    pub const MAX_HZ: u64 = 470_000_000;

    /// Construct a `Frequency`, returning [`RadioError::FrequencyOutOfRange`]
    /// if `hz` is outside the valid range.
    pub fn new(hz: u64) -> Result<Self, RadioError> {
        if !(Self::MIN_HZ..=Self::MAX_HZ).contains(&hz) {
            return Err(RadioError::FrequencyOutOfRange(hz));
        }
        Ok(Frequency(hz))
    }

    /// Return the raw frequency in Hz.
    pub fn hz(self) -> u64 {
        self.0
    }

    /// Format as a 9-digit zero-padded protocol string, e.g. `"014230000"`.
    /// See manual p.9: `FA<9 digits>;`.
    pub fn to_protocol_string(self) -> String {
        format!("{:09}", self.0)
    }

    /// Parse from a 9-digit protocol string.
    pub fn from_protocol_str(s: &str) -> Result<Self, RadioError> {
        let hz = s
            .parse::<u64>()
            .map_err(|_| RadioError::InvalidProtocolString(s.to_string()))?;
        Self::new(hz)
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mhz = self.0 as f64 / 1_000_000.0;
        write!(f, "{:.3} MHz", mhz)
    }
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

/// FT-991A operating modes per CAT protocol specification (manual p.11).
///
/// Hex-nibble-valued (`1`..`E`), **not** compatible with `ts570d::Mode`'s
/// single-decimal-digit scheme (`1`..`9`, no letters) — a fresh type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mode {
    Lsb = 0x1,
    Usb = 0x2,
    CwU = 0x3,
    Fm = 0x4,
    Am = 0x5,
    RttyLsb = 0x6,
    CwL = 0x7,
    DataLsb = 0x8,
    RttyUsb = 0x9,
    DataFm = 0xA,
    FmN = 0xB,
    DataUsb = 0xC,
    AmN = 0xD,
    C4fm = 0xE,
}

impl Mode {
    /// Return the numeric nibble value (1-14) used in the CAT protocol.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Return the single uppercase hex character used on the wire, e.g.
    /// `'E'` for [`Mode::C4fm`].
    pub fn as_wire_char(self) -> char {
        char::from_digit(self.as_u8() as u32, 16)
            .expect("mode nibble is always a valid hex digit")
            .to_ascii_uppercase()
    }

    /// Return the human-readable name string.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Lsb => "LSB",
            Mode::Usb => "USB",
            Mode::CwU => "CW-U",
            Mode::Fm => "FM",
            Mode::Am => "AM",
            Mode::RttyLsb => "RTTY-LSB",
            Mode::CwL => "CW-L",
            Mode::DataLsb => "DATA-LSB",
            Mode::RttyUsb => "RTTY-USB",
            Mode::DataFm => "DATA-FM",
            Mode::FmN => "FM-N",
            Mode::DataUsb => "DATA-USB",
            Mode::AmN => "AM-N",
            Mode::C4fm => "C4FM",
        }
    }
}

impl TryFrom<u8> for Mode {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x1 => Ok(Mode::Lsb),
            0x2 => Ok(Mode::Usb),
            0x3 => Ok(Mode::CwU),
            0x4 => Ok(Mode::Fm),
            0x5 => Ok(Mode::Am),
            0x6 => Ok(Mode::RttyLsb),
            0x7 => Ok(Mode::CwL),
            0x8 => Ok(Mode::DataLsb),
            0x9 => Ok(Mode::RttyUsb),
            0xA => Ok(Mode::DataFm),
            0xB => Ok(Mode::FmN),
            0xC => Ok(Mode::DataUsb),
            0xD => Ok(Mode::AmN),
            0xE => Ok(Mode::C4fm),
            _ => Err(RadioError::InvalidMode(value)),
        }
    }
}

impl TryFrom<char> for Mode {
    type Error = RadioError;

    /// Parse a single wire hex character (`'1'`..`'9'`, `'A'`..`'E'`,
    /// case-insensitive) into a [`Mode`].
    fn try_from(c: char) -> Result<Self, Self::Error> {
        let digit = c
            .to_digit(16)
            .ok_or_else(|| RadioError::InvalidProtocolString(c.to_string()))?;
        Mode::try_from(digit as u8)
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ---------------------------------------------------------------------------
// TxState
// ---------------------------------------------------------------------------

/// The FT-991A's 3-valued `TX;` query answer (manual p.17).
///
/// Unlike `ts570d`'s write-only `TX`/`RX` actions (which collapse cleanly to
/// a bool), the FT-991A's `TX` **read** can report transmit asserted by a
/// non-CAT cause (`RadioKeyedNonCat`) — a value [`Radio::transmit`]/
/// [`Radio::receive`] alone cannot represent, since those only ever send
/// `TX0;`/`TX1;`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxState {
    /// `0`: RADIO TX "OFF", CAT TX "OFF".
    Off,
    /// `1`: RADIO TX "OFF", CAT TX "ON" — this session asserted PTT.
    CatKeyed,
    /// `2`: RADIO TX "ON", CAT TX "OFF" (answer-only) — transmitting via
    /// some other cause (front panel, footswitch, VOX, ...).
    RadioKeyedNonCat,
}

impl TryFrom<u8> for TxState {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TxState::Off),
            1 => Ok(TxState::CatKeyed),
            2 => Ok(TxState::RadioKeyedNonCat),
            _ => Err(RadioError::InvalidProtocolString(value.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// ScanState
// ---------------------------------------------------------------------------

/// The FT-991A's 3-valued `SC` scan state (manual p.16).
///
/// Unlike [`TxState`], whose `2` value is answer-only (never legally set by
/// a controller), all three of `SC`'s values are legally settable — this is
/// a plain three-way enumerated state, not an "answer can report more than
/// a set can express" situation. Modeled as its own `TryFrom<u8>` enum
/// (mirroring `TxState`'s shape) rather than a `bool`, since `ts570d`'s own
/// analogous `Radio::get_scan`/`set_scan` (a plain on/off bool, per that
/// radio's simpler scan model) cannot represent the FT-991A's UP/DOWN scan
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanState {
    /// `0`: Scan "OFF".
    Off,
    /// `1`: Scan "ON" (UP ward).
    Up,
    /// `2`: Scan "ON" (DOWN ward).
    Down,
}

impl ScanState {
    /// Return the `SC` wire digit (`0`/`1`/`2`) for this state.
    pub fn as_u8(self) -> u8 {
        match self {
            ScanState::Off => 0,
            ScanState::Up => 1,
            ScanState::Down => 2,
        }
    }
}

impl TryFrom<u8> for ScanState {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ScanState::Off),
            1 => Ok(ScanState::Up),
            2 => Ok(ScanState::Down),
            _ => Err(RadioError::InvalidProtocolString(value.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// PreampMode
// ---------------------------------------------------------------------------

/// The FT-991A's 3-valued `PA` (pre-amp/IPO) selector (manual p.14).
///
/// Diverges from `ts570d::Radio::get_preamp`/`set_preamp`'s plain `bool` (a
/// simple on/off pre-amp), since the FT-991A genuinely has two distinct
/// gain stages (`AMP1`/`AMP2`) plus IPO (bypass) — a bool would collapse
/// that distinction, same category of divergence as [`ScanState`] vs.
/// `ts570d::Radio::get_scan`/`set_scan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreampMode {
    /// `0`: IPO (Intercept Point Optimization — pre-amp bypassed).
    Ipo,
    /// `1`: AMP 1.
    Amp1,
    /// `2`: AMP 2.
    Amp2,
}

impl PreampMode {
    /// Return the `PA` wire digit (`0`/`1`/`2`) for this mode.
    pub fn as_u8(self) -> u8 {
        match self {
            PreampMode::Ipo => 0,
            PreampMode::Amp1 => 1,
            PreampMode::Amp2 => 2,
        }
    }
}

impl TryFrom<u8> for PreampMode {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(PreampMode::Ipo),
            1 => Ok(PreampMode::Amp1),
            2 => Ok(PreampMode::Amp2),
            _ => Err(RadioError::InvalidPreampMode(value)),
        }
    }
}

// ---------------------------------------------------------------------------
// AgcMode
// ---------------------------------------------------------------------------

/// The FT-991A's `GT` (AGC function) state (manual p.10).
///
/// **Write/report domain mismatch, documented not hidden**: `GT`'s own Set
/// command (`P2`) only ever accepts 5 values (`OFF`/`FAST`/`MID`/`SLOW`/
/// `AUTO`), but its Answer (`P3`) can report 7 (the same four, plus `AUTO`
/// resolved into one of `AUTO-FAST`/`AUTO-MID`/`AUTO-SLOW`) — a real
/// asymmetry in the manual's own wire diagram, not a transcription error
/// (see `ft991a_radio.rs`'s module docs' "GT" section for the citation).
/// This single enum models the wider (Answer/`P3`) domain, used by both
/// [`Radio::get_agc_mode`] and [`Radio::set_agc_mode`]: setting any of
/// [`AgcMode::AutoFast`]/[`AgcMode::AutoMid`]/[`AgcMode::AutoSlow`] wire-
/// encodes identically ([`Self::set_wire_value`] returns `4`, plain
/// "AUTO" — the FT-991A's Set command has no way to request a *specific*
/// AUTO sub-variant), and this emulator's own choice of which sub-variant to
/// report back afterward is [`AgcMode::AutoFast`] — a documented, arbitrary
/// implementation default, not a manual fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgcMode {
    Off,
    Fast,
    Mid,
    Slow,
    AutoFast,
    AutoMid,
    AutoSlow,
}

impl AgcMode {
    /// Wire value in the *reported* (`P3`, `0`-`6`) domain — always safe to
    /// send as `GT`'s Answer.
    pub fn as_u8(self) -> u8 {
        match self {
            AgcMode::Off => 0,
            AgcMode::Fast => 1,
            AgcMode::Mid => 2,
            AgcMode::Slow => 3,
            AgcMode::AutoFast => 4,
            AgcMode::AutoMid => 5,
            AgcMode::AutoSlow => 6,
        }
    }

    /// Wire value in the *settable* (`P2`, `0`-`4`) domain — collapses
    /// `AutoMid`/`AutoSlow` onto the same `4` ("AUTO") `AutoFast` uses, per
    /// this type's own doc comment.
    pub fn set_wire_value(self) -> u8 {
        match self {
            AgcMode::Off => 0,
            AgcMode::Fast => 1,
            AgcMode::Mid => 2,
            AgcMode::Slow => 3,
            AgcMode::AutoFast | AgcMode::AutoMid | AgcMode::AutoSlow => 4,
        }
    }
}

impl TryFrom<u8> for AgcMode {
    type Error = RadioError;

    /// Parse a `P3` (reported, `0`-`6`) wire digit.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(AgcMode::Off),
            1 => Ok(AgcMode::Fast),
            2 => Ok(AgcMode::Mid),
            3 => Ok(AgcMode::Slow),
            4 => Ok(AgcMode::AutoFast),
            5 => Ok(AgcMode::AutoMid),
            6 => Ok(AgcMode::AutoSlow),
            _ => Err(RadioError::InvalidAgcMode(value)),
        }
    }
}

// ---------------------------------------------------------------------------
// Band
// ---------------------------------------------------------------------------

/// The FT-991A's 16 selectable bands (`BS`, manual p.5) — see
/// `ft991a_radio.rs`'s module docs' "BS's full 16-band table" section for
/// the complete transcription.
///
/// **No `ts570d::Radio` precedent** (`ts570d` has no band concept at all,
/// checked directly before adding) — included anyway per this task's own
/// framing that band select/up/down are "fairly generic" transceiver
/// concepts, same "near-universal concept, added anyway, flagged for
/// review" treatment [`ScanState`] and [`Radio::get_monitor_on`] already
/// received. Deliberately excludes wire value `13`, a genuine, documented
/// gap in the manual's own table (not a transcription omission) —
/// [`Band::try_from`] rejects it exactly like any other illegal value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// `00`: 1.8 MHz.
    OneEightMHz,
    /// `01`: 3.5 MHz.
    ThreeFiveMHz,
    /// `02`: 5 MHz.
    FiveMHz,
    /// `03`: 7 MHz.
    SevenMHz,
    /// `04`: 10 MHz.
    TenMHz,
    /// `05`: 14 MHz.
    FourteenMHz,
    /// `06`: 18 MHz.
    EighteenMHz,
    /// `07`: 21 MHz.
    TwentyOneMHz,
    /// `08`: 24.5 MHz.
    TwentyFourPointFiveMHz,
    /// `09`: 28 MHz.
    TwentyEightMHz,
    /// `10`: 50 MHz.
    FiftyMHz,
    /// `11`: GEN (general coverage receive).
    Gen,
    /// `12`: MW (medium wave broadcast receive).
    Mw,
    /// `14`: AIR (aircraft band receive). Note the gap: `13` has no
    /// `Band` variant at all.
    Air,
    /// `15`: 144 MHz.
    OneFourFourMHz,
    /// `16`: 430 MHz.
    FourThreeZeroMHz,
}

impl Band {
    /// Return the `BS` `P1` wire value (`00`-`16`, excluding `13`) for this
    /// band.
    pub fn as_u8(self) -> u8 {
        match self {
            Band::OneEightMHz => 0,
            Band::ThreeFiveMHz => 1,
            Band::FiveMHz => 2,
            Band::SevenMHz => 3,
            Band::TenMHz => 4,
            Band::FourteenMHz => 5,
            Band::EighteenMHz => 6,
            Band::TwentyOneMHz => 7,
            Band::TwentyFourPointFiveMHz => 8,
            Band::TwentyEightMHz => 9,
            Band::FiftyMHz => 10,
            Band::Gen => 11,
            Band::Mw => 12,
            Band::Air => 14,
            Band::OneFourFourMHz => 15,
            Band::FourThreeZeroMHz => 16,
        }
    }
}

impl TryFrom<u8> for Band {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Band::OneEightMHz),
            1 => Ok(Band::ThreeFiveMHz),
            2 => Ok(Band::FiveMHz),
            3 => Ok(Band::SevenMHz),
            4 => Ok(Band::TenMHz),
            5 => Ok(Band::FourteenMHz),
            6 => Ok(Band::EighteenMHz),
            7 => Ok(Band::TwentyOneMHz),
            8 => Ok(Band::TwentyFourPointFiveMHz),
            9 => Ok(Band::TwentyEightMHz),
            10 => Ok(Band::FiftyMHz),
            11 => Ok(Band::Gen),
            12 => Ok(Band::Mw),
            14 => Ok(Band::Air),
            15 => Ok(Band::OneFourFourMHz),
            16 => Ok(Band::FourThreeZeroMHz),
            _ => Err(RadioError::InvalidBand(value)),
        }
    }
}

// ---------------------------------------------------------------------------
// RepeaterShift
// ---------------------------------------------------------------------------

/// The FT-991A's 3-valued `OS` (OFFSET / REPEATER SHIFT) direction (manual
/// p.13). Also the domain of `IF`/`OI`/`MR`/`MW`/`MT`'s shared `P10` field
/// (`Ft991aState::offset_type`, batch 9/2/10) — `OS` is the first, and only,
/// command in this crate that writes it.
///
/// **No `ts570d::Radio` precedent** (`ts570d` has no FM-repeater concept at
/// all — an HF-only rig, checked directly before adding) — included anyway
/// as a standard, near-universal concept on VHF/UHF-capable transceivers
/// like the FT-991A, same "near-universal concept, no direct precedent,
/// flagged for review" treatment [`Band`]/[`ScanState`]/
/// [`Radio::get_monitor_on`] already received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeaterShift {
    /// `0`: Simplex (no shift).
    Simplex,
    /// `1`: Plus Shift.
    Plus,
    /// `2`: Minus Shift.
    Minus,
}

impl RepeaterShift {
    /// Return the `OS`/`offset_type` wire digit (`0`/`1`/`2`) for this
    /// shift.
    pub fn as_u8(self) -> u8 {
        match self {
            RepeaterShift::Simplex => 0,
            RepeaterShift::Plus => 1,
            RepeaterShift::Minus => 2,
        }
    }
}

impl TryFrom<u8> for RepeaterShift {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(RepeaterShift::Simplex),
            1 => Ok(RepeaterShift::Plus),
            2 => Ok(RepeaterShift::Minus),
            _ => Err(RadioError::InvalidRepeaterShift(value)),
        }
    }
}

// ---------------------------------------------------------------------------
// Meter
// ---------------------------------------------------------------------------

/// One of the six physical meters `MS` can select and `RM` can directly
/// read (manual p.12 `MS`, p.15 `RM`). `CLAUDE.md`'s "Radio trait scope"
/// section explicitly lists "meters" as a trait-worthy generic concept
/// (alongside frequency/mode/gain/squelch, already on this trait) — this
/// type is deliberately **not** FT-991A-specific in shape (six named
/// meters a controller can select among and read), even though its exact
/// wire encoding is.
///
/// Deliberately excludes `RM`'s other selectors: `1` (S-meter — already
/// covered by `Radio::get_smeter`/`SM`, not duplicated here) and `0`/`2`
/// ("depends on the front panel", i.e. whatever `Meter` is currently
/// selected — see `Ft991a::get_active_meter_reading` for that, kept as
/// an FT-991A-specific inherent method since it is defined purely in terms
/// of `MS`'s own most-recent selection, not a value a caller chooses per
/// read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Meter {
    Comp = 0,
    Alc = 1,
    Po = 2,
    Swr = 3,
    Id = 4,
    Vdd = 5,
}

impl Meter {
    /// Return the `MS` P1 / `RM` P1 numeric value (0-5 for `MS`; `RM`'s
    /// direct-select values are these plus 3 — see
    /// `Ft991a::get_meter`'s doc comment for that offset).
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Meter::Comp => "COMP",
            Meter::Alc => "ALC",
            Meter::Po => "PO",
            Meter::Swr => "SWR",
            Meter::Id => "ID",
            Meter::Vdd => "VDD",
        }
    }
}

impl TryFrom<u8> for Meter {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Meter::Comp),
            1 => Ok(Meter::Alc),
            2 => Ok(Meter::Po),
            3 => Ok(Meter::Swr),
            4 => Ok(Meter::Id),
            5 => Ok(Meter::Vdd),
            _ => Err(RadioError::InvalidMeter(value)),
        }
    }
}

impl fmt::Display for Meter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ---------------------------------------------------------------------------
// RadioIndicator
// ---------------------------------------------------------------------------

/// One of `RI`'s status-flag selectors (manual p.15). Kept as a plain
/// domain type (not a `Radio` trait method) — this is an FT-991A CAT
/// protocol artifact (a fixed, manual-specific list of status flags with a
/// documented gap), not a generic radio concept in the sense `CLAUDE.md`'s
/// "Radio trait scope" section means (frequency/mode/meters/gain/etc.).
///
/// The manual's P1 legend lists `0`, `3`-`7`, and `A` only — `1`, `2`,
/// `8`, `9`, and `B`-`F` are not documented and are deliberately **not**
/// represented here (transcribed exactly, including the gap, rather than
/// guessed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RadioIndicator {
    HiSwr = 0x0,
    Rec = 0x3,
    Play = 0x4,
    VfoATx = 0x5,
    VfoBTx = 0x6,
    VfoARx = 0x7,
    TxLed = 0xA,
}

impl RadioIndicator {
    /// Return the `RI` P1 numeric value.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            RadioIndicator::HiSwr => "Hi-SWR",
            RadioIndicator::Rec => "REC",
            RadioIndicator::Play => "PLAY",
            RadioIndicator::VfoATx => "VFO-A TX",
            RadioIndicator::VfoBTx => "VFO-B TX",
            RadioIndicator::VfoARx => "VFO-A RX",
            RadioIndicator::TxLed => "TX LED",
        }
    }
}

impl TryFrom<u8> for RadioIndicator {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x0 => Ok(RadioIndicator::HiSwr),
            0x3 => Ok(RadioIndicator::Rec),
            0x4 => Ok(RadioIndicator::Play),
            0x5 => Ok(RadioIndicator::VfoATx),
            0x6 => Ok(RadioIndicator::VfoBTx),
            0x7 => Ok(RadioIndicator::VfoARx),
            0xA => Ok(RadioIndicator::TxLed),
            _ => Err(RadioError::InvalidProtocolString(format!("{value:#x}"))),
        }
    }
}

impl fmt::Display for RadioIndicator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ---------------------------------------------------------------------------
// ToneSquelchMode (CT)
// ---------------------------------------------------------------------------

/// `CT`'s tone squelch mode (manual p.5). A generic ham radio concept
/// (CTCSS/DCS encode/decode selection) — kept trait-worthy per
/// `CLAUDE.md`'s "Radio trait scope" section, distinct from the specific
/// tone/code *value* (see [`crate::Ft991a::get_ctcss_tone_hz`]/
/// [`crate::Ft991a::get_dcs_code`], which are FT-991A-table-backed but
/// still domain-typed at this trait's boundary — Hz/code number, not a raw
/// wire index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ToneSquelchMode {
    Off = 0,
    CtcssEncDec = 1,
    CtcssEnc = 2,
    DcsEncDec = 3,
    DcsEnc = 4,
}

impl ToneSquelchMode {
    /// Return the `CT` P2 numeric value.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            ToneSquelchMode::Off => "OFF",
            ToneSquelchMode::CtcssEncDec => "CTCSS ENC/DEC",
            ToneSquelchMode::CtcssEnc => "CTCSS ENC",
            ToneSquelchMode::DcsEncDec => "DCS ENC/DEC",
            ToneSquelchMode::DcsEnc => "DCS ENC",
        }
    }
}

impl TryFrom<u8> for ToneSquelchMode {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ToneSquelchMode::Off),
            1 => Ok(ToneSquelchMode::CtcssEncDec),
            2 => Ok(ToneSquelchMode::CtcssEnc),
            3 => Ok(ToneSquelchMode::DcsEncDec),
            4 => Ok(ToneSquelchMode::DcsEnc),
            _ => Err(RadioError::InvalidToneSquelchMode(value)),
        }
    }
}

impl fmt::Display for ToneSquelchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ---------------------------------------------------------------------------
// Memory channel records (MC, MR, MW, MT)
// ---------------------------------------------------------------------------

/// Contents of a single memory channel, minus its tag (`MR`/`MW` — manual
/// p.12; `CLAUDE.md`'s "Radio trait scope" section explicitly lists
/// "memory channels" as a trait-worthy generic concept, mirroring
/// `ts570d::MemoryChannelEntry`'s shape and naming, though not its exact
/// field set — the FT-991A's own manual determines the fields here).
///
/// `frequency_hz`/`mode` deliberately mirror
/// `crate::ft991a_radio::ChannelStatusFields`'s own field types
/// (`frequency_hz: u64` raw/unvalidated rather than the [`Frequency`]
/// newtype, since `MR`/`MW`'s own manual page states no narrower range
/// than "Frequency (Hz)") — except `mode`, which IS upgraded to the
/// domain [`Mode`] type here (unlike `ChannelStatusFields`'s raw nibble),
/// since this struct is part of the trait-facing, controller-oriented API
/// (matching `Radio::get_mode`/`set_mode`'s own domain-typed shape), while
/// `ChannelStatusFields` stays a low-level wire-shape struct internal to
/// `ft991a_radio`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryChannelEntry {
    /// P1: memory channel number, 1-117 (manual p.12: `001`-`117`, no `0`
    /// VFO-mode case — unlike `IF`'s channel field).
    pub channel: u8,
    /// P2: frequency, Hz.
    pub frequency_hz: u64,
    /// P3: combined sign+offset, range -9999..=9999 Hz.
    pub clarifier_offset_hz: i16,
    /// P4: RX CLAR on/off.
    pub rx_clarifier_on: bool,
    /// P5: TX CLAR on/off.
    pub tx_clarifier_on: bool,
    /// P6: operating mode.
    pub mode: Mode,
    /// P8: CTCSS/DCS status, 0-4 (manual p.12's `CN`/`CT` tone-related
    /// legend; the tone *number* itself is a different, not-yet-landed
    /// command — see `planning/architect/task_plan.md` §10.5 batch 3).
    pub tone_status: u8,
    /// P10: offset type, 0-2 (Simplex/Plus/Minus).
    pub offset_type: u8,
}

/// `MT`'s up-to-12-character ASCII memory channel tag (manual p.12, P12).
///
/// Validated at construction time against the character-set restriction
/// `crate::ft991a_radio::MemoryChannelRecord`'s doc comment explains in
/// full (manual p.12 itself states only `"(up to 12 characters) (ASCII)"`,
/// no explicit character-set restriction or padding convention — this
/// applies the CAT Operation section's *general* parameter rule from
/// manual p.2 instead, a documented judgment call): printable ASCII space
/// (0x20) through tilde (0x7E), excluding `;` (which would otherwise
/// corrupt the wire frame if ever formatted directly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTag(String);

impl MemoryTag {
    /// Fixed wire width of the P12 field (manual p.12's column diagram).
    pub const MAX_LEN: usize = 12;

    /// Construct a tag, validating length and character set. Returns
    /// [`RadioError::InvalidMemoryTag`] if `s` is longer than
    /// [`Self::MAX_LEN`] characters or contains a character outside the
    /// legal set (see struct docs).
    pub fn new(s: &str) -> Result<Self, RadioError> {
        if s.chars().count() > Self::MAX_LEN
            || !s.chars().all(|c| (' '..='~').contains(&c) && c != ';')
        {
            return Err(RadioError::InvalidMemoryTag(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    /// Borrow the tag's content (never padded — see struct docs).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Right-pad with ASCII spaces to the fixed [`Self::MAX_LEN`]-byte wire
    /// width (manual p.12, P12).
    pub(crate) fn to_wire_string(&self) -> String {
        format!("{:<width$}", self.0, width = Self::MAX_LEN)
    }
}

impl fmt::Display for MemoryTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A [`MemoryChannelEntry`] plus its [`MemoryTag`] (`MT`, manual p.12) — a
/// genuine superset of `MR`/`MW`'s plain [`MemoryChannelEntry`], not a
/// duplicate of it: `MT`'s Set/Answer wire shape carries the full channel
/// record AND the tag in one composite frame (there is no
/// tag-only-update form on real hardware either).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedMemoryChannel {
    pub entry: MemoryChannelEntry,
    pub tag: MemoryTag,
}

// ---------------------------------------------------------------------------
// Radio trait
// ---------------------------------------------------------------------------

/// Abstraction over an FT-991A (or compatible) radio — first-slice subset.
///
/// Implemented by `radio::Ft991a<S: CatSession>`. A future `ui` crate and
/// other framework consumers depend only on this trait, not on the concrete
/// radio crate.
///
/// Uses `#[async_trait(?Send)]` — no `Send` bounds, compatible with monoio's
/// thread-per-core (`!Send`) futures.
#[async_trait(?Send)]
pub trait Radio {
    async fn get_vfo_a(&mut self) -> RadioResult<Frequency> {
        Err(RadioError::NotImplemented)
    }
    async fn set_vfo_a(&mut self, _freq: Frequency) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_vfo_b(&mut self) -> RadioResult<Frequency> {
        Err(RadioError::NotImplemented)
    }
    async fn set_vfo_b(&mut self, _freq: Frequency) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_mode(&mut self) -> RadioResult<Mode> {
        Err(RadioError::NotImplemented)
    }
    async fn set_mode(&mut self, _mode: Mode) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Assert CAT-driven PTT (`TX1;`).
    async fn transmit(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Release CAT-driven PTT (`TX0;`).
    async fn receive(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the 3-valued `TX;` answer. See [`TxState`] — `transmit`/
    /// `receive` alone cannot represent "radio TX on via a non-CAT cause".
    async fn get_tx_state(&mut self) -> RadioResult<TxState> {
        Err(RadioError::NotImplemented)
    }

    async fn get_smeter(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }

    /// Select which physical meter (`MS`) subsequent front-panel-following
    /// reads report — see [`Meter`].
    async fn select_meter(&mut self, _meter: Meter) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query which physical meter `MS` currently has selected.
    async fn get_selected_meter(&mut self) -> RadioResult<Meter> {
        Err(RadioError::NotImplemented)
    }
    /// Directly read one named meter (`RM`'s direct-select values),
    /// independent of `MS`'s current selection.
    async fn get_meter(&mut self, _meter: Meter) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }

    async fn get_power_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    async fn set_power_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    async fn get_af_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    async fn set_af_gain(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_rf_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    async fn set_rf_gain(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_squelch(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    async fn set_squelch(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    async fn get_power(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    async fn set_power(&mut self, _watts: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the radio's fixed model identifier (`"0670"` for the FT-991A).
    async fn get_id(&mut self) -> RadioResult<String> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Memory channels (MC, MR, MW, MT)
    // -----------------------------------------------------------------------

    /// Get the currently selected memory channel number (`MC`, manual
    /// p.11; 1-117).
    async fn get_memory_channel(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Select a memory channel (`MC`).
    async fn set_memory_channel(&mut self, _ch: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Read a memory channel's contents (`MR`, manual p.12; read-only —
    /// see [`Self::read_memory_channel_tag`] for the tagged variant).
    async fn read_memory_channel(&mut self, _ch: u8) -> RadioResult<MemoryChannelEntry> {
        Err(RadioError::NotImplemented)
    }
    /// Write a memory channel's contents (`MW`, manual p.12; write-only,
    /// no answer, no tag).
    async fn write_memory_channel(&mut self, _entry: MemoryChannelEntry) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Read a memory channel's contents plus its tag (`MT`, manual p.12).
    async fn read_memory_channel_tag(&mut self, _ch: u8) -> RadioResult<TaggedMemoryChannel> {
        Err(RadioError::NotImplemented)
    }
    /// Write a memory channel's contents plus its tag (`MT`) — a single
    /// composite write, not an incremental tag-only update (see
    /// [`TaggedMemoryChannel`]'s doc comment).
    async fn write_memory_channel_tag(&mut self, _channel: TaggedMemoryChannel) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // VFO A/B copy, swap, and memory-channel quick-ops (batch 1: `AB BA AM
    // MA CH`)
    // -----------------------------------------------------------------------
    //
    // Trait-worthy per `CLAUDE.md`'s "Radio trait scope" section: dual-VFO
    // copy/swap and memory-channel step operations are generic ham radio
    // concepts, not FT-991A-specific (this trait already exposes
    // `get_vfo_a`/`get_vfo_b`/memory-channel get/set — these round out that
    // family with the quick-op triggers batch 1 adds). `VM`, `QI`/`QR`, and
    // `QS` deliberately stay `Ft991a`-inherent-only, not on this trait —
    // see `Ft991a`'s own doc comments for the reasoning (VM's meaning is a
    // documented judgment call resting on manual-heading evidence, not a
    // crisply specified concept; QI/QR/QS are FT-991A-named "Quick"
    // features distinct from the generic memory-channel/split concepts
    // already covered elsewhere).

    /// Copy VFO-A's frequency into VFO-B (`AB`, manual p.4).
    async fn copy_vfo_a_to_b(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Copy VFO-B's frequency into VFO-A (`BA`, manual p.4).
    async fn copy_vfo_b_to_a(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Swap VFO-A's and VFO-B's frequencies (`SV`, manual p.17).
    async fn swap_vfos(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Store VFO-A into the currently selected memory channel (`AM`, manual
    /// p.4). Selection is set via [`Self::set_memory_channel`].
    async fn store_vfo_to_memory(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Recall the currently selected memory channel into VFO-A (`MA`,
    /// manual p.11).
    async fn recall_memory_to_vfo(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Step the selected memory channel up (`CH0`, manual p.5).
    async fn memory_channel_up(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Step the selected memory channel down (`CH1`, manual p.5).
    async fn memory_channel_down(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Clarifier / RIT-XIT (batch 3: `RT RC RD RU XT`)
    // -----------------------------------------------------------------------
    //
    // Trait-worthy per `CLAUDE.md`'s "Radio trait scope" section, which
    // explicitly names "RIT/XIT" as a generic radio concept. `RT`/`XT`
    // independently gate a single shared clarifier offset's effect on
    // RX/TX; `RD`/`RU`/`RC` adjust/clear that one offset — see
    // `ft991a_radio.rs`'s module docs' "RX/TX clarifier relationship"
    // section for the full manual citation.

    /// Query whether the RX clarifier is applied (`RT`, manual p.16).
    async fn get_rx_clarifier_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the RX clarifier (`RT`).
    async fn set_rx_clarifier_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query whether the TX clarifier is applied (`XT`, manual p.19).
    async fn get_tx_clarifier_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the TX clarifier (`XT`).
    async fn set_tx_clarifier_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Zero the shared clarifier offset (`RC`, manual p.15). Does not
    /// change [`Self::get_rx_clarifier_on`]/[`Self::get_tx_clarifier_on`]'s
    /// state.
    async fn clarifier_clear(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Set the shared clarifier offset to `offset_hz` below the tuned
    /// frequency (`RD`, manual p.15; `0..=9999` Hz). An absolute set, not
    /// an incremental step.
    async fn clarifier_down(&mut self, _offset_hz: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Set the shared clarifier offset to `offset_hz` above the tuned
    /// frequency (`RU`, manual p.16; `0..=9999` Hz). An absolute set, not
    /// an incremental step.
    async fn clarifier_up(&mut self, _offset_hz: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // IF-shift (batch 3: `IS`)
    // -----------------------------------------------------------------------

    /// Query the IF-shift offset, Hz (`IS`, manual p.10; -1200..=1200, 20 Hz
    /// steps).
    async fn get_if_shift_hz(&mut self) -> RadioResult<i16> {
        Err(RadioError::NotImplemented)
    }
    /// Set the IF-shift offset, Hz (`IS`).
    async fn set_if_shift_hz(&mut self, _hz: i16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Tone squelch mode + CTCSS/DCS value (batch 3: `CT CN`)
    // -----------------------------------------------------------------------
    //
    // Trait-worthy per `CLAUDE.md`'s "Radio trait scope" section
    // (CTCSS/DCS tone selection is a generic ham radio concept) — but the
    // specific 50/104-entry lookup tables `CN`'s wire index resolves
    // through are FT-991A manual data, not trait-level; this trait exposes
    // domain values (Hz for CTCSS, the 3-digit code number for DCS), never
    // the raw table index (see `ft991a_radio.rs`'s [`CTCSS_TONES_DECIHZ`]/
    // [`DCS_CODES`] and `ft991a.rs`'s conversion at the client boundary).

    /// Query the tone squelch mode (`CT`, manual p.5).
    async fn get_tone_squelch_mode(&mut self) -> RadioResult<ToneSquelchMode> {
        Err(RadioError::NotImplemented)
    }
    /// Set the tone squelch mode (`CT`).
    async fn set_tone_squelch_mode(&mut self, _mode: ToneSquelchMode) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the currently selected CTCSS tone, Hz (`CN` with `P2=0`,
    /// manual p.5/p.6 Table 1).
    async fn get_ctcss_tone_hz(&mut self) -> RadioResult<f32> {
        Err(RadioError::NotImplemented)
    }
    /// Select a CTCSS tone by its frequency in Hz — must be one of the 50
    /// standard tones (`CN` with `P2=0`).
    async fn set_ctcss_tone_hz(&mut self, _hz: f32) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the currently selected DCS code (`CN` with `P2=1`, manual
    /// p.5/p.6 Table 2).
    async fn get_dcs_code(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Select a DCS code — must be one of the 104 standard codes (`CN`
    /// with `P2=1`).
    async fn set_dcs_code(&mut self, _code: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // CW keyer speed/pitch/on-off, break-in, CW spot, zero-in (batch 4: `KM
    // KP KR KS KY CS ZI BI SD`)
    // -----------------------------------------------------------------------
    //
    // Trait-worthy per `CLAUDE.md`'s "Radio trait scope" section and
    // `ts570d::Radio`'s own precedent (`get_keyer_speed`/`set_keyer_speed`,
    // `get_semi_break_in_delay`/`set_semi_break_in_delay` already exist
    // there for the analogous Kenwood concepts, per that trait's own
    // module): break-in mode, semi break-in delay, CW spot, keyer
    // speed/pitch, and the electronic-keyer on/off toggle are generic
    // CW-operating concepts present across many radios, not FT-991A-
    // specific. `KM` (keyer memory message storage) and `KY` (stored-
    // message playback trigger) are deliberately **not** here — see
    // `Ft991a::read_keyer_memory`/`write_keyer_memory`/`play_keyer_memory`'s
    // own doc comments (`radio/src/ft991a.rs`) for why those stay
    // FT-991A-inherent-only, per this task's explicit brief.

    /// Query whether break-in is enabled (`BI`, manual p.5).
    async fn get_break_in_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable break-in (`BI`).
    async fn set_break_in_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the CW (semi) break-in delay time, ms (`SD`, manual p.16;
    /// `30`-`3000`).
    async fn get_semi_break_in_delay(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Set the CW (semi) break-in delay time, ms (`SD`).
    async fn set_semi_break_in_delay(&mut self, _ms: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether CW spot is enabled (`CS`, manual p.6).
    async fn get_cw_spot_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable CW spot (`CS`).
    async fn set_cw_spot_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the electronic keyer's on/off state (`KR`, manual p.10).
    async fn get_keyer_enabled(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the electronic keyer (`KR`).
    async fn set_keyer_enabled(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the keyer speed, WPM (`KS`, manual p.11; `4`-`60`).
    async fn get_keyer_speed(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the keyer speed, WPM (`KS`).
    async fn set_keyer_speed(&mut self, _wpm: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the keyer (CW sidetone) pitch, Hz (`KP`, manual p.10;
    /// `300`-`1050`, 10 Hz steps).
    async fn get_keyer_pitch_hz(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Set the keyer pitch, Hz (`KP`).
    async fn set_keyer_pitch_hz(&mut self, _hz: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Trigger the CW auto zero-in function (`ZI`, manual p.18) — a
    /// zero-width, write-only trigger with no persisted on/off state
    /// (unlike `ts570d::Radio::set_cw_auto_zerobeat`'s toggle shape; this
    /// radio's own manual gives `ZI` a blank Read/Answer row, confirmed
    /// write-only per the p.3 master table).
    async fn zero_in(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Scan / VOX / busy (batch 5: `SC VX VD VG BY`)
    // -----------------------------------------------------------------------
    //
    // Trait-worthy per `CLAUDE.md`'s "Radio trait scope" section, which
    // explicitly lists "scan" and "VOX" as generic radio concepts, and per
    // `ts570d::Radio`'s own precedent (`get_scan`/`set_scan`, `get_vox`/
    // `set_vox`, `get_vox_gain`/`set_vox_gain`, `get_vox_delay`/
    // `set_vox_delay`, `is_busy` already exist there for the analogous
    // Kenwood concepts). `get_scan_state`/`set_scan_state` use [`ScanState`]
    // rather than `ts570d::Radio::get_scan`/`set_scan`'s plain `bool`, since
    // the FT-991A's own manual genuinely gives `SC` three legal values
    // (OFF/UP/DOWN), not just on/off — a bool would lose the scan direction.

    /// Query the scan state (`SC`, manual p.16). See [`ScanState`].
    async fn get_scan_state(&mut self) -> RadioResult<ScanState> {
        Err(RadioError::NotImplemented)
    }
    /// Set the scan state (`SC`).
    async fn set_scan_state(&mut self, _state: ScanState) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether VOX is enabled (`VX`, manual p.18).
    async fn get_vox_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable VOX (`VX`).
    async fn set_vox_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the VOX gain, `0`-`100` (`VG`, manual p.18).
    async fn get_vox_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the VOX gain (`VG`). Returns [`RadioError::InvalidVoxGain`] if
    /// `gain` is outside `0..=100`.
    async fn set_vox_gain(&mut self, _gain: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the VOX delay time, ms (`VD`, manual p.17; `30`-`3000`, 10 ms
    /// steps).
    ///
    /// **Doc-noted dependency on `EX` menu item 142 "VOX SELECT," which
    /// this crate does not implement**: `VD`'s own manual box states that
    /// this command's parameter means the ordinary VOX delay when menu 142
    /// is set to `"MIC"`, or the DATA VOX delay when set to `"DATA"` — two
    /// different physical settings sharing this one CAT command. Menu 142
    /// (and its adjacent items 143/144/146/147, which the front-panel menu
    /// system uses to store the MIC/DATA settings separately) is not among
    /// the `EX` items landed so far (see `ft991a_radio.rs`'s
    /// `EX_MENU_TABLE`) — this implementation exposes exactly **one** value,
    /// addressed unconditionally, regardless of what menu 142 would (if
    /// implemented) currently select. Same category of "meaning depends on
    /// something outside this command's own wire bytes" open item as
    /// `TxState::RadioKeyedNonCat` (`TX`'s answer-only `2` value, whose
    /// real-world cause is likewise not resolvable from `TX` alone) — stated
    /// explicitly here per this crate's own documentation practice, not
    /// hidden.
    async fn get_vox_delay(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Set the VOX delay time, ms (`VD`). See [`Self::get_vox_delay`]'s doc
    /// comment for the `EX` menu 142 dependency this crate does not
    /// resolve. Returns [`RadioError::InvalidVoxDelay`] if `ms` is outside
    /// `30..=3000` or not a multiple of 10.
    async fn set_vox_delay(&mut self, _ms: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether the receiver is busy (`BY`, manual p.5; read-only).
    /// This emulator has no simulated received-signal/squelch-open
    /// condition, so [`Ft991a`](crate::Ft991a)'s implementation always
    /// reports `false` — a documented simplification of the emulator, not a
    /// manual-specified default.
    async fn get_rx_busy(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Attenuator / preamp / noise / AGC / notch / filter-width
    // (batch 6: `RA PA NB NL NR RL GT CO BP BC NA SH`)
    // -----------------------------------------------------------------------
    //
    // Ten of the twelve landed commands are trait-worthy per `CLAUDE.md`'s
    // "Radio trait scope" section (attenuator/preamp/noise blanker are
    // explicitly named there) and `ts570d::Radio`'s own precedent
    // (`get_attenuator`/`set_attenuator`, `get_preamp`/`set_preamp`,
    // `get_noise_blanker`/`set_noise_blanker`, `get_noise_reduction`/
    // `set_noise_reduction`, `get_agc`/`set_agc` all exist there for the
    // analogous Kenwood concepts — checked before deciding, same practice
    // prior batches used). This crate's own method shapes diverge from
    // `ts570d::Radio`'s where the FT-991A's wire format genuinely differs:
    // [`PreampMode`] (3-valued) instead of a bool, noise reduction split
    // into a separate on/off (`NR`) and level (`RL`) instead of `ts570d`'s
    // single combined `0`-`2` field, and [`AgcMode`] (7-valued) instead of
    // a raw time-constant `u8`. `CO` (Contour/APF) and `BP` (Manual Notch)
    // are deliberately **not** on this trait — FT-991A-named parametric-EQ/
    // audio-peaking features with no generic-radio-concept precedent in
    // either `CLAUDE.md`'s list or `ts570d::Radio` — kept `Ft991a`-inherent-
    // only, same treatment as `EX`/`KM`/`KY`. `BC` (Auto Notch, a plain
    // bool) IS on the trait, a deliberate asymmetry against `BP` documented
    // in `ft991a_radio.rs`'s module docs' "BP" section.

    /// Query whether the RF attenuator is on (`RA`, manual p.15).
    async fn get_attenuator_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the RF attenuator (`RA`).
    async fn set_attenuator_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the pre-amp/IPO mode (`PA`, manual p.14). See [`PreampMode`].
    async fn get_preamp_mode(&mut self) -> RadioResult<PreampMode> {
        Err(RadioError::NotImplemented)
    }
    /// Set the pre-amp/IPO mode (`PA`).
    async fn set_preamp_mode(&mut self, _mode: PreampMode) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether the noise blanker is on (`NB`, manual p.13).
    async fn get_noise_blanker_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the noise blanker (`NB`).
    async fn set_noise_blanker_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the noise blanker level, `0`-`10` (`NL`, manual p.13).
    async fn get_noise_blanker_level(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the noise blanker level (`NL`). Returns
    /// [`RadioError::InvalidNoiseBlankerLevel`] if `level` is outside
    /// `0..=10`.
    async fn set_noise_blanker_level(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether noise reduction is on (`NR`, manual p.13). Distinct
    /// from the noise reduction *level* ([`Self::get_noise_reduction_level`]
    /// / `RL`) — the FT-991A splits these into two commands, unlike
    /// `ts570d::Radio::get_noise_reduction`'s single combined `0`-`2` field.
    async fn get_noise_reduction_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable noise reduction (`NR`).
    async fn set_noise_reduction_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the noise reduction level, `1`-`15` (`RL`, manual p.15; no
    /// `0` — [`Self::get_noise_reduction_on`]/`NR` is the separate on/off
    /// gate).
    async fn get_noise_reduction_level(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the noise reduction level (`RL`). Returns
    /// [`RadioError::InvalidNoiseReductionLevel`] if `level` is outside
    /// `1..=15`.
    async fn set_noise_reduction_level(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the AGC mode (`GT`, manual p.10). See [`AgcMode`]'s doc
    /// comment for the write/report domain mismatch this type documents.
    async fn get_agc_mode(&mut self) -> RadioResult<AgcMode> {
        Err(RadioError::NotImplemented)
    }
    /// Set the AGC mode (`GT`). See [`AgcMode::set_wire_value`] for how
    /// `AutoMid`/`AutoSlow` collapse onto the same wire value `AutoFast`
    /// uses.
    async fn set_agc_mode(&mut self, _mode: AgcMode) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether the auto notch filter is on (`BC`, manual p.4).
    async fn get_auto_notch_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the auto notch filter (`BC`).
    async fn set_auto_notch_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether the narrow filter is on (`NA`, manual p.13). See
    /// `ft991a_radio.rs`'s module docs' "NA, a genuine manual wire-diagram
    /// typo" section for why the wire code is `NA`, not the `MA` its own
    /// per-command box's diagram literally shows.
    async fn get_narrow_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the narrow filter (`NA`).
    async fn set_narrow_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the raw filter width table index, `0`-`21` (`SH`, manual
    /// p.16). **The actual bandwidth in Hz this index represents depends on
    /// the radio's current mode and narrow/wide (`NA`) state, neither of
    /// which is part of `SH`'s own wire bytes** — see
    /// `ft991a_radio.rs`'s [`crate::ft991a_radio::SH_BANDWIDTH_TABLE`]/
    /// [`crate::ft991a_radio::filter_bandwidth_hz`] for the full lookup
    /// table and module docs' "SH" section for the citation. This method
    /// returns only the raw index — resolving it to Hz is a separate,
    /// pure (non-I/O) lookup a caller performs itself.
    async fn get_filter_width_index(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the raw filter width table index (`SH`). Returns
    /// [`RadioError::InvalidFilterWidthIndex`] if `index` is outside
    /// `0..=21`. Does not cross-validate against the current mode/`NA`
    /// state — see [`Self::get_filter_width_index`]'s doc comment.
    async fn set_filter_width_index(&mut self, _index: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Batch 7: speech processor/mic/monitor (MG, PL, PR, ML)
    // -----------------------------------------------------------------------

    /// Query the microphone gain, `0`-`100` (`MG`, manual p.11). Direct
    /// `ts570d::Radio::get_mic_gain` precedent (checked before adding).
    async fn get_mic_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the microphone gain (`MG`). Returns
    /// [`RadioError::InvalidMicGain`] if `level` is outside `0..=100`.
    async fn set_mic_gain(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the speech processor level, `0`-`100` (`PL`, manual p.14).
    async fn get_speech_processor_level(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the speech processor level (`PL`). Returns
    /// [`RadioError::InvalidSpeechProcessorLevel`] if `level` is outside
    /// `0..=100`.
    async fn set_speech_processor_level(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether the speech processor is on (`PR` with `P1=0`, manual
    /// p.14). See `ft991a_radio.rs`'s module docs' "PR, a genuine manual
    /// heading typo" section for why this command's own per-command box is
    /// headed "SPEECH PROCESSOR LEVEL" despite being an on/off toggle, not a
    /// level. Direct `ts570d::Radio::get_speech_processor` precedent.
    async fn get_speech_processor_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the speech processor (`PR` with `P1=0`). `PR`'s `P1=1`
    /// item (Parametric Microphone Equalizer) is deliberately **not** on
    /// this trait — an FT-991A-named parametric-EQ feature with no generic
    /// concept precedent, same treatment batch 6 gave `CO`/`BP` — see
    /// [`crate::Ft991a::get_parametric_mic_eq_on`]/
    /// [`crate::Ft991a::set_parametric_mic_eq_on`] instead.
    async fn set_speech_processor_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether the audio monitor is on (`ML` with `P1=0`, manual
    /// p.12). **Judgment call, not backed by a `ts570d::Radio` precedent**
    /// (that trait has no "monitor" concept at all) — added anyway since an
    /// audio monitor (hearing one's own transmitted signal) is a standard,
    /// near-universal transceiver concept, not an FT-991A-named feature the
    /// way CONTOUR/APF are; flagged for architect review rather than
    /// assumed correct.
    async fn get_monitor_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the audio monitor (`ML` with `P1=0`).
    async fn set_monitor_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the audio monitor level, `0`-`100` (`ML` with `P1=1`, manual
    /// p.12). Same judgment-call status as [`Self::get_monitor_on`].
    async fn get_monitor_level(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the audio monitor level (`ML` with `P1=1`). Returns
    /// [`RadioError::InvalidMonitorLevel`] if `level` is outside `0..=100`.
    async fn set_monitor_level(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Batch 8: band/step/encoder front-panel controls (BS BU BD FS DN UP)
    // -----------------------------------------------------------------------
    //
    // `ED`/`EU`/`EK` (front-panel encoder step / ENT key) are deliberately
    // **not** on this trait — FT-991A-specific concepts with no
    // `ts570d::Radio` precedent and no clean generic abstraction (`P1`
    // selects among three physical encoders, `P2` is an opaque step count)
    // — see [`crate::Ft991a::encoder_down`]/[`crate::Ft991a::encoder_up`]/
    // [`crate::Ft991a::ent_key`] instead.

    /// Select a band (`BS`, manual p.5). Write-only on the wire — no
    /// `Read`/`Answer` form exists for `BS` at all (manual p.3: `Set O Read
    /// X Ans X`), so this trait has no paired `get_band`. **Judgment call,
    /// not backed by a `ts570d::Radio` precedent** — see [`Band`]'s own doc
    /// comment.
    async fn set_band(&mut self, _band: Band) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Step to the next band up (`BU`, manual p.4). Wraps past the highest
    /// band back to the lowest, skipping the documented gap at wire value
    /// `13` — manual-silent judgment call, same category as
    /// [`Self::memory_channel_up`]'s wrap-around.
    async fn band_up(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Step to the next band down (`BD`, manual p.4). Wraps past the lowest
    /// band back to the highest, skipping the gap.
    async fn band_down(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query whether the VFO-A "FAST" step key is on (`FS`, manual p.9).
    /// Direct `ts570d::Radio::get_fine_step` precedent (checked before
    /// adding).
    async fn get_fine_step(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the VFO-A "FAST" step key (`FS`).
    async fn set_fine_step(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Emulate a press of the hand mic's "UP" button (`UP`, manual p.17).
    /// Direct `ts570d::Radio::mic_up` precedent (same method name) — see
    /// `ft991a_radio.rs`'s module docs' "DN/UP" section for the full
    /// cross-radio corroboration behind resolving `UP`/`DN` to mic-button
    /// commands at all.
    async fn mic_up(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Emulate a press of the hand mic's "DWN" button (`DN`, manual p.6 —
    /// own per-command box heading "MIC DWN," despite the master table's
    /// plain "DOWN"). Direct `ts570d::Radio::mic_down` precedent.
    async fn mic_down(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS
    // -----------------------------------------------------------------------
    //
    // `AC` (antenna tuner), `DA` (dimmer), `DT` (date/time), `OI` (opposite
    // band info), `TS` (meaning genuinely uncertain even after reading the
    // manual — see `ft991a_radio.rs`'s module docs' "TS" section), and
    // `LM`/`PB` (DVS record/playback, FT-991A-specific like `KM`/`KY`) are
    // deliberately **not** on this trait — see `ft991a_radio.rs`'s module
    // docs' "Batch 10" section for the per-command reasoning. Use
    // [`crate::Ft991a::get_antenna_tuner_state`]/`set_antenna_tuner_state`,
    // `get_dimmer`/`set_dimmer`, `read_date`/`write_date`/`read_time`/
    // `write_time`/`read_time_zone_offset`/`write_time_zone_offset`,
    // `get_opposite_band_information`, `get_txw_on`/`set_txw_on`, and
    // `start_dvs_recording`/`stop_dvs_recording`/`get_dvs_recording_channel`/
    // `start_dvs_playback`/`stop_dvs_playback`/`get_dvs_playback_channel`
    // instead.

    /// Query auto-information broadcast on/off (`AI`, manual p.4). Direct
    /// `ts570d::Radio::set_auto_info(mode: u8)` precedent (checked before
    /// adding) — the FT-991A's own `AI` is a plain 2-valued on/off, unlike
    /// `ts570d`'s 4-valued `0`-`3` "mode," a deliberate, documented
    /// divergence from mirroring that signature exactly.
    async fn get_auto_info_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable auto-information broadcast (`AI`). The manual's own
    /// note that this resets to `false` when the transceiver powers off is
    /// **not** enforced by this crate — see `ft991a_radio.rs`'s module docs'
    /// "AI" section.
    async fn set_auto_info_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the VFO-A dial lock state (`LK`, manual p.11). Direct
    /// `ts570d::Radio::get_frequency_lock` precedent (checked before
    /// adding).
    async fn get_frequency_lock(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the VFO-A dial lock (`LK`). Direct
    /// `ts570d::Radio::set_frequency_lock` precedent.
    async fn set_frequency_lock(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query the FM repeater shift direction (`OS`, manual p.13 — "*This
    /// command can be activated only with an FM mode," a front-panel-context
    /// caveat this crate does not enforce). **Judgment call, not backed by a
    /// `ts570d::Radio` precedent** — see [`RepeaterShift`]'s own doc
    /// comment.
    async fn get_repeater_shift(&mut self) -> RadioResult<RepeaterShift> {
        Err(RadioError::NotImplemented)
    }
    /// Set the FM repeater shift direction (`OS`).
    async fn set_repeater_shift(&mut self, _shift: RepeaterShift) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query which VFO/band is the TX (transmitter) band (`FT`, manual p.9;
    /// `0`=VFO-A, `1`=VFO-B — the Answer-domain values; `FT`'s own `Set`
    /// wire encoding is `2`/`3` for the identical two states, translated at
    /// the `Ft991a`/`Ft991aRadio` boundary, not exposed here — see
    /// `ft991a_radio.rs`'s module docs' "FT" section). Direct
    /// `ts570d::Radio::get_tx_vfo`/`set_tx_vfo` precedent (checked before
    /// adding: `ts570d`'s own signature is `0`=VFO A, `1`=VFO B, `2`=Memory
    /// — the FT-991A's `FT` has no memory-channel-TX option, a documented,
    /// deliberate narrowing of that 3-valued domain to 2, not an oversight).
    async fn get_tx_vfo(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Select which VFO/band is the TX band (`FT`). `vfo` must be `0` or
    /// `1`.
    async fn set_tx_vfo(&mut self, _vfo: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Query MOX (manual transmitter-key) on/off state (`MX`, manual p.13).
    /// **Judgment call, not backed by a `ts570d::Radio` precedent** —
    /// structurally and conceptually adjacent to the already-trait-level PTT
    /// concept (`transmit`/`receive`/`get_tx_state`), added anyway as a
    /// near-universal transceiver concept, same treatment as
    /// [`Self::get_repeater_shift`]/[`Self::get_monitor_on`].
    async fn get_mox_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable MOX (`MX`).
    async fn set_mox_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Flush the session's receive buffer, discarding unsolicited or stale
    /// data. Default implementation is a no-op.
    fn flush_rx(&mut self) {}
}

// ---------------------------------------------------------------------------
// Ft991aExtras trait
// ---------------------------------------------------------------------------

/// FT-991A-specific capabilities that stayed `Ft991a`-inherent-only through
/// Waves 1-3 (per `CLAUDE.md`'s "Radio trait scope" — keyer memory
/// playback, QMB, encoder nudges, antenna tuner, dimmer, date/time, DVS,
/// contour/APF/manual notch, the `IF`/`OI` composite status payloads, and
/// the `EX` menu escape hatch), re-exposed here as trait methods so a
/// generic `ui::run<R: Radio + Ft991aExtras + ...>` can still reach them —
/// see `planning/architect/task_plan.md` §11.3 point 3.
///
/// Every method below (other than [`Self::get_ex_menu_item`]/
/// [`Self::set_ex_menu_item`], which are genuinely new — see §11.4) already
/// exists as an inherent method on [`crate::Ft991a`] (Waves 1-3): this
/// trait is a **re-export surface**, not new functionality. Default bodies
/// return [`RadioError::NotImplemented`], exactly [`Radio`]'s own idiom;
/// [`crate::Ft991a`]'s impl (in `ft991a.rs`) simply forwards each method to
/// its own already-landed inherent method of the same name.
///
/// Implemented **unconditionally** for `impl<S: CatSession<Error =
/// TransportError>> Ft991aExtras for Ft991a<S>` — the exact same bound
/// [`Radio`]'s own impl uses. This is a single, non-overlapping impl with
/// zero coherence risk: unlike [`CwKeying`] (which genuinely needs the
/// extra `S: ModemControlLines` bound and so gets its own, separate impl
/// block), every method here can be implemented against a plain
/// `S: CatSession` alone, so there is no second, overlapping
/// `Ft991aExtras for Ft991a<S>` impl to collide with this one — confirmed
/// by this crate's own `cargo build` (see `planning/yaesu/findings.md`),
/// not just asserted.
///
/// Uses `#[async_trait(?Send)]`, matching [`Radio`] — compatible with
/// monoio's thread-per-core (`!Send`) futures.
#[async_trait(?Send)]
pub trait Ft991aExtras {
    // -----------------------------------------------------------------------
    // Composite status payloads (IF, OI)
    // -----------------------------------------------------------------------

    /// Query the composite VFO/memory-channel status payload (`IF`, manual
    /// p.10).
    async fn get_information(&mut self) -> RadioResult<ChannelStatusFields> {
        Err(RadioError::NotImplemented)
    }
    /// Query the composite opposite-band (VFO-B) status payload (`OI`,
    /// manual p.13; read-only).
    async fn get_opposite_band_information(&mut self) -> RadioResult<ChannelStatusFields> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Meters/status (RM direct reading, RI, RS, UL)
    // -----------------------------------------------------------------------

    /// Read whatever meter is currently shown on the front panel (`RM`
    /// `P1=0`, manual p.15).
    async fn get_active_meter_reading(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Query one status-flag indicator (`RI`, manual p.15).
    async fn get_radio_indicator(&mut self, _indicator: RadioIndicator) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Query whether the radio is currently in MENU MODE (`RS`, manual
    /// p.16).
    async fn get_menu_mode_active(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Query whether the PLL is unlocked (`UL`, manual p.18).
    async fn get_pll_unlocked(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // VFO/memory quick-ops with no generic-`Radio` home (VM, QI, QR, QS)
    // -----------------------------------------------------------------------

    /// Emulate pressing the front-panel `[V/M]` key (`VM`, manual p.18).
    /// **Judgment call, not manual-proven** — see `ft991a_radio.rs`'s
    /// module docs' "VM/AM" section.
    async fn toggle_vfo_memory_mode(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Store VFO-A into the dedicated Quick Memory Bank slot (`QI`, manual
    /// p.14).
    async fn qmb_store(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Recall the Quick Memory Bank slot into VFO-A (`QR`, manual p.14).
    async fn qmb_recall(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Toggle Quick Split (`QS`, manual p.15). **Judgment call** — see
    /// `ft991a_radio.rs`'s module docs' "QS" section.
    async fn quick_split(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Keyer memory store/playback (KM, KY)
    // -----------------------------------------------------------------------

    /// Read one `KM` keyer memory channel's stored message (manual p.10;
    /// channel `1`-`5`).
    async fn read_keyer_memory(&mut self, _channel: u8) -> RadioResult<String> {
        Err(RadioError::NotImplemented)
    }
    /// Write one `KM` keyer memory channel's message (manual p.10; channel
    /// `1`-`5`, message 1-50 printable ASCII characters, no `;`).
    async fn write_keyer_memory(&mut self, _channel: u8, _message: &str) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Trigger playback of a stored `KM` keyer memory channel (`KY`, manual
    /// p.11) — distinct from the real-time RTS/DTR CW-keying feature
    /// ([`CwKeying`]), see `ft991a.rs`'s doc comment on the equivalent
    /// inherent method for the full distinction.
    async fn play_keyer_memory(
        &mut self,
        _channel: u8,
        _mode: KeyerPlaybackMode,
    ) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Contour/APF (CO) and manual notch (BP)
    // -----------------------------------------------------------------------

    /// Query whether CONTOUR is on (`CO` `P2=0`, manual p.5).
    async fn get_contour_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable CONTOUR (`CO` `P2=0`).
    async fn set_contour_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the CONTOUR frequency, Hz, `10`-`3200` (`CO` `P2=1`, manual
    /// p.5).
    async fn get_contour_frequency_hz(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Set the CONTOUR frequency (`CO` `P2=1`).
    async fn set_contour_frequency_hz(&mut self, _hz: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query whether APF is on (`CO` `P2=2`, manual p.5).
    async fn get_apf_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable APF (`CO` `P2=2`).
    async fn set_apf_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the APF frequency, Hz, `-250`..=`250` in 10 Hz steps (`CO`
    /// `P2=3`, manual p.5).
    async fn get_apf_frequency_hz(&mut self) -> RadioResult<i16> {
        Err(RadioError::NotImplemented)
    }
    /// Set the APF frequency (`CO` `P2=3`).
    async fn set_apf_frequency_hz(&mut self, _hz: i16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query whether the manual notch is on (`BP` `P2=0`, manual p.5).
    async fn get_manual_notch_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the manual notch (`BP` `P2=0`).
    async fn set_manual_notch_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the manual notch frequency, Hz, `10`-`3200` in 10 Hz steps
    /// (`BP` `P2=1`, manual p.5).
    async fn get_manual_notch_frequency_hz(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Set the manual notch frequency (`BP` `P2=1`).
    async fn set_manual_notch_frequency_hz(&mut self, _hz: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Parametric Microphone Equalizer (PR P1=1)
    // -----------------------------------------------------------------------

    /// Query whether the Parametric Microphone Equalizer is on (`PR`
    /// `P1=1`, manual p.14).
    async fn get_parametric_mic_eq_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable the Parametric Microphone Equalizer (`PR` `P1=1`).
    async fn set_parametric_mic_eq_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Front-panel encoder/key emulation (ED, EU, EK)
    // -----------------------------------------------------------------------

    /// Step the specified front-panel encoder down (`ED`, manual p.7;
    /// `steps` must be `1`-`99`).
    async fn encoder_down(&mut self, _encoder: EncoderSelector, _steps: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Step the specified front-panel encoder up (`EU`, manual p.7).
    async fn encoder_up(&mut self, _encoder: EncoderSelector, _steps: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Emulate a press of the front-panel ENT key (`EK`, manual p.7;
    /// zero-width Action trigger).
    async fn ent_key(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Antenna tuner (AC)
    // -----------------------------------------------------------------------

    /// Query the antenna tuner state (`AC`, manual p.4; `0`=OFF, `1`=ON,
    /// `2`=Tuning Start/Stop).
    async fn get_antenna_tuner_state(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set the antenna tuner state (`AC`). `state` must be `0`-`2`.
    async fn set_antenna_tuner_state(&mut self, _state: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Dimmer (DA)
    // -----------------------------------------------------------------------

    /// Query the dimmer levels (`DA`, manual p.6): `(led_brightness,
    /// tft_brightness)` — LED `1`-`2`, TFT `0`-`15`.
    async fn get_dimmer(&mut self) -> RadioResult<(u8, u8)> {
        Err(RadioError::NotImplemented)
    }
    /// Set the dimmer levels (`DA`).
    async fn set_dimmer(&mut self, _led: u8, _tft: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Date/time/time-zone (DT)
    // -----------------------------------------------------------------------

    /// Read the current date (`DT` `P1=0`, manual p.6): `(year, month,
    /// day)`.
    async fn read_date(&mut self) -> RadioResult<(u16, u8, u8)> {
        Err(RadioError::NotImplemented)
    }
    /// Write the date (`DT` `P1=0`).
    async fn write_date(&mut self, _year: u16, _month: u8, _day: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Read the current time (`DT` `P1=1`, manual p.6): `(hour, minute,
    /// second)`, 24-hour, UTC.
    async fn read_time(&mut self) -> RadioResult<(u8, u8, u8)> {
        Err(RadioError::NotImplemented)
    }
    /// Write the time (`DT` `P1=1`).
    async fn write_time(&mut self, _hour: u8, _minute: u8, _second: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Read the current time zone offset in minutes (`DT` `P1=2`, manual
    /// p.6; `-720..=840`, 30-minute steps).
    async fn read_time_zone_offset(&mut self) -> RadioResult<i16> {
        Err(RadioError::NotImplemented)
    }
    /// Write the time zone offset (`DT` `P1=2`).
    async fn write_time_zone_offset(&mut self, _minutes: i16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // "TXW" (TS)
    // -----------------------------------------------------------------------

    /// Query the "TXW" on/off state (`TS`, manual p.17).
    async fn get_txw_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Enable/disable "TXW" (`TS`).
    async fn set_txw_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // DVS record/playback (LM, PB)
    // -----------------------------------------------------------------------

    /// Query the DVS recording state (`LM`, manual p.11): `None` = stopped,
    /// `Some(channel)` = actively recording that channel (`1`-`5`).
    async fn get_dvs_recording_channel(&mut self) -> RadioResult<Option<u8>> {
        Err(RadioError::NotImplemented)
    }
    /// Start (or toggle-stop, if already recording `channel`) DVS
    /// recording on the given channel (`LM`, `1`-`5`).
    async fn start_dvs_recording(&mut self, _channel: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Stop DVS recording (`LM` with `P2=0`).
    async fn stop_dvs_recording(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Query the DVS playback state (`PB`, manual p.14): `None` = stopped,
    /// `Some(channel)` = actively playing that channel (`1`-`5`).
    async fn get_dvs_playback_channel(&mut self) -> RadioResult<Option<u8>> {
        Err(RadioError::NotImplemented)
    }
    /// Start DVS playback on the given channel (`PB`, `1`-`5`;
    /// unconditional, not a toggle).
    async fn start_dvs_playback(&mut self, _channel: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Stop DVS playback (`PB` with `P2=0`).
    async fn stop_dvs_playback(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // EX menu escape hatch (§11.4) — new, not a re-export of an existing
    // inherent method
    // -----------------------------------------------------------------------

    /// Read an `EX` menu item's raw integer value by its `P1` menu number
    /// (manual p.7-9). Returns [`RadioError::UnknownExMenuItem`] if `p1`
    /// isn't a landed `EX_MENU_TABLE` row.
    async fn get_ex_menu_item(&mut self, _p1: u16) -> RadioResult<i32> {
        Err(RadioError::NotImplemented)
    }
    /// Write an `EX` menu item's raw integer value by its `P1` menu
    /// number. Returns [`RadioError::UnknownExMenuItem`] if `p1` isn't a
    /// landed `EX_MENU_TABLE` row, or [`RadioError::InvalidProtocolString`]
    /// if `value` is not legal for that item's own value kind/range.
    async fn set_ex_menu_item(&mut self, _p1: u16, _value: i32) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
}

// ---------------------------------------------------------------------------
// CwKeying trait
// ---------------------------------------------------------------------------

/// Real-time RS-232 modem control/status line access for CW keying —
/// distinct from [`Ft991aExtras::play_keyer_memory`] (`KY`, which plays
/// back a *pre-stored* `KM` message via CAT). This backs `EX` menu item
/// 060 "PC KEYING" when set to `2: RTS` or `3: DTR` (manual p.8): the PC
/// asserts/clears RTS or DTR directly, with no CAT command involved at
/// all, to key CW in real time.
///
/// Per `planning/architect/task_plan.md` §10.3, these are plain sync
/// `fn`s, not `#[async_trait]` — matching the precedent
/// `cat_transport_core::ModemControlLines` itself sets (direct
/// `ioctl(2)` calls, no I/O wait), not [`Radio`]/[`Ft991aExtras`]'s async
/// idiom.
///
/// Implemented for `impl<S: CatSession<Error = TransportError> +
/// ModemControlLines> CwKeying for Ft991a<S>` — the same bound the
/// existing (Wave 3) `assert_rts`/`assert_dtr`/`read_cts`/`read_dsr`/
/// `read_dcd` inherent-method impl block already uses. This is the
/// **one** trait in this pair that stays genuinely conditional on the
/// transport, by design: a future non-serial transport without modem
/// control lines simply won't implement it — a compile-time signal,
/// not a silent runtime [`RadioError::NotImplemented`] (though the
/// default bodies below still return that, for any `S` that *does*
/// implement this trait but wants to no-op a subset).
pub trait CwKeying {
    /// Assert or clear RTS. Present on the RS-232C 9-pin CAT connector,
    /// pin 7 (manual p.1).
    fn assert_rts(&self, _asserted: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Assert or clear DTR. **Not present on the RS-232C 9-pin CAT
    /// connector** — only reachable via a USB Dual-UART bridge connection.
    fn assert_dtr(&self, _asserted: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Read the current CTS (Clear To Send) status line state. Present on
    /// the RS-232C CAT connector, pin 8 (manual p.1).
    fn read_cts(&self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Read the current DSR (Data Set Ready) status line state. **Not
    /// present on the RS-232C 9-pin CAT connector.**
    fn read_dsr(&self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Read the current DCD (Data Carrier Detect) status line state.
    /// **Not present on the RS-232C 9-pin CAT connector.**
    fn read_dcd(&self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
}

// ---------------------------------------------------------------------------
// NopRadio
// ---------------------------------------------------------------------------

/// A no-op [`Radio`] implementation. All methods return
/// [`RadioError::NotImplemented`]. Useful as a starting point — implement
/// only the methods your radio supports.
pub struct NopRadio;

#[async_trait(?Send)]
impl Radio for NopRadio {}

// Also satisfies the widened `ui::run<R: Radio + Ft991aExtras + CwKeying +
// 'static>` bound (`planning/architect/task_plan.md` §11.3 point 3's last
// bullet) via the traits' own `NotImplemented` defaults — kept in sync with
// this crate's own `NopRadio`, same mechanical addition a future `ui`
// crate's `MockRadio` test double needs.
#[async_trait(?Send)]
impl Ft991aExtras for NopRadio {}

impl CwKeying for NopRadio {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Frequency tests ---

    #[test]
    fn test_valid_frequency() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.hz(), 14_230_000);
    }

    #[test]
    fn test_boundary_min() {
        let freq = Frequency::new(Frequency::MIN_HZ).unwrap();
        assert_eq!(freq.hz(), 30_000);
    }

    #[test]
    fn test_boundary_max() {
        let freq = Frequency::new(Frequency::MAX_HZ).unwrap();
        assert_eq!(freq.hz(), 470_000_000);
    }

    #[test]
    fn test_out_of_range_below() {
        let result = Frequency::new(29_999);
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(v) => assert_eq!(v, 29_999),
            other => panic!("expected FrequencyOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn test_out_of_range_above() {
        let result = Frequency::new(470_000_001);
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(v) => assert_eq!(v, 470_000_001),
            other => panic!("expected FrequencyOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn test_protocol_string_format_is_9_digits() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "014230000");
        assert_eq!(freq.to_protocol_string().len(), 9);
    }

    #[test]
    fn test_protocol_string_min() {
        let freq = Frequency::new(30_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "000030000");
    }

    #[test]
    fn test_protocol_string_max() {
        let freq = Frequency::new(470_000_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "470000000");
    }

    #[test]
    fn test_from_protocol_str_round_trip() {
        let freq = Frequency::new(14_230_000).unwrap();
        let s = freq.to_protocol_string();
        let recovered = Frequency::from_protocol_str(&s).unwrap();
        assert_eq!(recovered, freq);
    }

    #[test]
    fn test_from_protocol_str_invalid() {
        let result = Frequency::from_protocol_str("not_a_number");
        assert!(matches!(
            result.unwrap_err(),
            RadioError::InvalidProtocolString(_)
        ));
    }

    #[test]
    fn test_display_format() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.to_string(), "14.230 MHz");
    }

    // --- Mode tests ---

    #[test]
    fn test_all_valid_modes_round_trip_u8() {
        let modes = [
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
        for mode in modes {
            let byte = mode.as_u8();
            assert_eq!(Mode::try_from(byte).unwrap(), mode);
        }
        assert_eq!(modes.len(), 14, "manual p.11 lists 14 modes, 1..=E");
    }

    #[test]
    fn test_wire_char_uses_uppercase_hex() {
        assert_eq!(Mode::Lsb.as_wire_char(), '1');
        assert_eq!(Mode::DataFm.as_wire_char(), 'A');
        assert_eq!(Mode::C4fm.as_wire_char(), 'E');
    }

    #[test]
    fn test_mode_from_wire_char_round_trip() {
        assert_eq!(Mode::try_from('1').unwrap(), Mode::Lsb);
        assert_eq!(Mode::try_from('a').unwrap(), Mode::DataFm);
        assert_eq!(Mode::try_from('E').unwrap(), Mode::C4fm);
    }

    #[test]
    fn test_invalid_mode_zero() {
        assert!(matches!(
            Mode::try_from(0u8).unwrap_err(),
            RadioError::InvalidMode(0)
        ));
    }

    #[test]
    fn test_invalid_mode_too_high() {
        assert!(matches!(
            Mode::try_from(0x0Fu8).unwrap_err(),
            RadioError::InvalidMode(0x0F)
        ));
    }

    #[test]
    fn test_mode_display() {
        assert_eq!(Mode::Lsb.to_string(), "LSB");
        assert_eq!(Mode::C4fm.to_string(), "C4FM");
    }

    // --- TxState tests ---

    #[test]
    fn test_tx_state_from_u8() {
        assert_eq!(TxState::try_from(0).unwrap(), TxState::Off);
        assert_eq!(TxState::try_from(1).unwrap(), TxState::CatKeyed);
        assert_eq!(TxState::try_from(2).unwrap(), TxState::RadioKeyedNonCat);
        assert!(TxState::try_from(3).is_err());
    }

    // --- Meter tests ---

    #[test]
    fn test_meter_round_trips_u8() {
        for meter in [
            Meter::Comp,
            Meter::Alc,
            Meter::Po,
            Meter::Swr,
            Meter::Id,
            Meter::Vdd,
        ] {
            assert_eq!(Meter::try_from(meter.as_u8()).unwrap(), meter);
        }
    }

    #[test]
    fn test_meter_values_match_ms_p1_table() {
        // Manual p.12 MS: 0:COMP 1:ALC 2:PO 3:SWR 4:ID 5:VDD.
        assert_eq!(Meter::try_from(0).unwrap(), Meter::Comp);
        assert_eq!(Meter::try_from(1).unwrap(), Meter::Alc);
        assert_eq!(Meter::try_from(2).unwrap(), Meter::Po);
        assert_eq!(Meter::try_from(3).unwrap(), Meter::Swr);
        assert_eq!(Meter::try_from(4).unwrap(), Meter::Id);
        assert_eq!(Meter::try_from(5).unwrap(), Meter::Vdd);
    }

    #[test]
    fn test_meter_invalid_value() {
        assert!(matches!(
            Meter::try_from(6u8).unwrap_err(),
            RadioError::InvalidMeter(6)
        ));
    }

    #[test]
    fn test_meter_display() {
        assert_eq!(Meter::Comp.to_string(), "COMP");
        assert_eq!(Meter::Vdd.to_string(), "VDD");
    }

    // --- RadioIndicator tests ---

    #[test]
    fn test_radio_indicator_round_trips_u8() {
        for indicator in [
            RadioIndicator::HiSwr,
            RadioIndicator::Rec,
            RadioIndicator::Play,
            RadioIndicator::VfoATx,
            RadioIndicator::VfoBTx,
            RadioIndicator::VfoARx,
            RadioIndicator::TxLed,
        ] {
            assert_eq!(
                RadioIndicator::try_from(indicator.as_u8()).unwrap(),
                indicator
            );
        }
    }

    #[test]
    fn test_radio_indicator_rejects_documented_gap_values() {
        // 1, 2, 8, 9, B-F are not in the manual's P1 legend.
        for value in [1u8, 2, 8, 9, 0xB, 0xF] {
            assert!(RadioIndicator::try_from(value).is_err());
        }
    }

    #[test]
    fn test_radio_indicator_display() {
        assert_eq!(RadioIndicator::HiSwr.to_string(), "Hi-SWR");
        assert_eq!(RadioIndicator::TxLed.to_string(), "TX LED");
    }

    // --- ToneSquelchMode tests ---

    #[test]
    fn test_tone_squelch_mode_round_trips_u8() {
        for mode in [
            ToneSquelchMode::Off,
            ToneSquelchMode::CtcssEncDec,
            ToneSquelchMode::CtcssEnc,
            ToneSquelchMode::DcsEncDec,
            ToneSquelchMode::DcsEnc,
        ] {
            assert_eq!(ToneSquelchMode::try_from(mode.as_u8()).unwrap(), mode);
        }
    }

    #[test]
    fn test_tone_squelch_mode_values_match_ct_p2_table() {
        // Manual p.5 CT: 0:OFF 1:CTCSS ENC/DEC 2:CTCSS ENC 3:DCS ENC/DEC
        // 4:DCS ENC.
        assert_eq!(ToneSquelchMode::try_from(0).unwrap(), ToneSquelchMode::Off);
        assert_eq!(
            ToneSquelchMode::try_from(1).unwrap(),
            ToneSquelchMode::CtcssEncDec
        );
        assert_eq!(
            ToneSquelchMode::try_from(2).unwrap(),
            ToneSquelchMode::CtcssEnc
        );
        assert_eq!(
            ToneSquelchMode::try_from(3).unwrap(),
            ToneSquelchMode::DcsEncDec
        );
        assert_eq!(
            ToneSquelchMode::try_from(4).unwrap(),
            ToneSquelchMode::DcsEnc
        );
    }

    #[test]
    fn test_tone_squelch_mode_invalid_value() {
        assert!(matches!(
            ToneSquelchMode::try_from(5u8).unwrap_err(),
            RadioError::InvalidToneSquelchMode(5)
        ));
    }

    #[test]
    fn test_tone_squelch_mode_display() {
        assert_eq!(ToneSquelchMode::Off.to_string(), "OFF");
        assert_eq!(ToneSquelchMode::DcsEnc.to_string(), "DCS ENC");
    }

    // --- MemoryTag tests ---

    #[test]
    fn test_memory_tag_accepts_empty_string() {
        let tag = MemoryTag::new("").unwrap();
        assert_eq!(tag.as_str(), "");
        assert_eq!(tag.to_wire_string(), "            "); // 12 spaces
    }

    #[test]
    fn test_memory_tag_accepts_exactly_twelve_characters() {
        let tag = MemoryTag::new("ABCDEFGHIJKL").unwrap();
        assert_eq!(tag.as_str(), "ABCDEFGHIJKL");
        assert_eq!(tag.to_wire_string(), "ABCDEFGHIJKL"); // no padding needed
    }

    #[test]
    fn test_memory_tag_rejects_thirteen_characters() {
        assert!(matches!(
            MemoryTag::new("ABCDEFGHIJKLM").unwrap_err(),
            RadioError::InvalidMemoryTag(_)
        ));
    }

    #[test]
    fn test_memory_tag_pads_shorter_content_on_the_wire() {
        let tag = MemoryTag::new("REPEATER 1").unwrap(); // 10 chars
        assert_eq!(tag.to_wire_string(), "REPEATER 1  "); // + 2 trailing spaces
    }

    #[test]
    fn test_memory_tag_rejects_control_characters() {
        assert!(matches!(
            MemoryTag::new("BAD\u{1}TAG").unwrap_err(),
            RadioError::InvalidMemoryTag(_)
        ));
    }

    #[test]
    fn test_memory_tag_rejects_semicolon() {
        // Would corrupt the wire frame's terminator if ever formatted
        // directly, even though real wire content can never actually
        // contain one (framing already splits on the first `;`).
        assert!(matches!(
            MemoryTag::new("BAD;TAG").unwrap_err(),
            RadioError::InvalidMemoryTag(_)
        ));
    }

    #[test]
    fn test_memory_tag_display() {
        let tag = MemoryTag::new("N0CALL").unwrap();
        assert_eq!(tag.to_string(), "N0CALL");
    }

    // --- NopRadio tests ---

    #[monoio::test(driver = "legacy")]
    async fn test_nop_radio_returns_not_implemented() {
        let mut radio = NopRadio;
        assert!(matches!(
            radio.get_vfo_a().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_mode().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.transmit().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_tx_state().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_id().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.select_meter(Meter::Swr).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_selected_meter().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_meter(Meter::Swr).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_memory_channel().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_memory_channel(1).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.read_memory_channel(1).await,
            Err(RadioError::NotImplemented)
        ));
        let entry = MemoryChannelEntry {
            channel: 1,
            frequency_hz: 14_000_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: Mode::Usb,
            tone_status: 0,
            offset_type: 0,
        };
        assert!(matches!(
            radio.write_memory_channel(entry).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.read_memory_channel_tag(1).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio
                .write_memory_channel_tag(TaggedMemoryChannel {
                    entry,
                    tag: MemoryTag::new("TEST").unwrap(),
                })
                .await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.copy_vfo_a_to_b().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.copy_vfo_b_to_a().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.swap_vfos().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.store_vfo_to_memory().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.recall_memory_to_vfo().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.memory_channel_up().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.memory_channel_down().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_rx_clarifier_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_rx_clarifier_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_tx_clarifier_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_tx_clarifier_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.clarifier_clear().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.clarifier_down(100).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.clarifier_up(100).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_if_shift_hz().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_if_shift_hz(200).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_tone_squelch_mode().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_tone_squelch_mode(ToneSquelchMode::CtcssEnc).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_ctcss_tone_hz().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_ctcss_tone_hz(100.0).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_dcs_code().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_dcs_code(23).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_break_in_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_break_in_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_semi_break_in_delay().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_semi_break_in_delay(500).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_cw_spot_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_cw_spot_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_keyer_enabled().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_keyer_enabled(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_keyer_speed().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_keyer_speed(20).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_keyer_pitch_hz().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_keyer_pitch_hz(600).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.zero_in().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_scan_state().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_scan_state(ScanState::Up).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_vox_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_vox_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_vox_gain().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_vox_gain(50).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_vox_delay().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_vox_delay(500).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_rx_busy().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_attenuator_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_attenuator_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_preamp_mode().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_preamp_mode(PreampMode::Amp1).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_noise_blanker_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_noise_blanker_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_noise_blanker_level().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_noise_blanker_level(5).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_noise_reduction_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_noise_reduction_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_noise_reduction_level().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_noise_reduction_level(5).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_agc_mode().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_agc_mode(AgcMode::Fast).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_auto_notch_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_auto_notch_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_narrow_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_narrow_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_filter_width_index().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_filter_width_index(5).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_mic_gain().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_mic_gain(50).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_speech_processor_level().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_speech_processor_level(50).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_speech_processor_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_speech_processor_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_monitor_on().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_monitor_on(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_monitor_level().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_monitor_level(50).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_band(Band::OneEightMHz).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.band_up().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.band_down().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_fine_step().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_fine_step(true).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.mic_up().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.mic_down().await,
            Err(RadioError::NotImplemented)
        ));
    }

    // --- Band tests ---

    #[test]
    fn test_band_try_from_u8_first_and_last_valid_codes() {
        assert_eq!(Band::try_from(0).unwrap(), Band::OneEightMHz);
        assert_eq!(Band::try_from(16).unwrap(), Band::FourThreeZeroMHz);
    }

    #[test]
    fn test_band_try_from_u8_rejects_the_documented_gap_at_13() {
        assert!(matches!(
            Band::try_from(13),
            Err(RadioError::InvalidBand(13))
        ));
    }

    #[test]
    fn test_band_try_from_u8_rejects_out_of_range_value() {
        assert!(matches!(
            Band::try_from(17),
            Err(RadioError::InvalidBand(17))
        ));
    }

    #[test]
    fn test_band_as_u8_round_trips_all_sixteen_valid_bands() {
        for band in [
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
        ] {
            assert_eq!(Band::try_from(band.as_u8()).unwrap(), band);
        }
    }

    // --- PreampMode tests ---

    #[test]
    fn test_preamp_mode_try_from_u8() {
        assert_eq!(PreampMode::try_from(0).unwrap(), PreampMode::Ipo);
        assert_eq!(PreampMode::try_from(1).unwrap(), PreampMode::Amp1);
        assert_eq!(PreampMode::try_from(2).unwrap(), PreampMode::Amp2);
        assert!(matches!(
            PreampMode::try_from(3),
            Err(RadioError::InvalidPreampMode(3))
        ));
    }

    #[test]
    fn test_preamp_mode_as_u8_round_trips() {
        for mode in [PreampMode::Ipo, PreampMode::Amp1, PreampMode::Amp2] {
            assert_eq!(PreampMode::try_from(mode.as_u8()).unwrap(), mode);
        }
    }

    // --- AgcMode tests ---

    #[test]
    fn test_agc_mode_try_from_u8_full_report_domain() {
        assert_eq!(AgcMode::try_from(0).unwrap(), AgcMode::Off);
        assert_eq!(AgcMode::try_from(1).unwrap(), AgcMode::Fast);
        assert_eq!(AgcMode::try_from(2).unwrap(), AgcMode::Mid);
        assert_eq!(AgcMode::try_from(3).unwrap(), AgcMode::Slow);
        assert_eq!(AgcMode::try_from(4).unwrap(), AgcMode::AutoFast);
        assert_eq!(AgcMode::try_from(5).unwrap(), AgcMode::AutoMid);
        assert_eq!(AgcMode::try_from(6).unwrap(), AgcMode::AutoSlow);
        assert!(matches!(
            AgcMode::try_from(7),
            Err(RadioError::InvalidAgcMode(7))
        ));
    }

    #[test]
    fn test_agc_mode_as_u8_round_trips_full_domain() {
        for mode in [
            AgcMode::Off,
            AgcMode::Fast,
            AgcMode::Mid,
            AgcMode::Slow,
            AgcMode::AutoFast,
            AgcMode::AutoMid,
            AgcMode::AutoSlow,
        ] {
            assert_eq!(AgcMode::try_from(mode.as_u8()).unwrap(), mode);
        }
    }

    #[test]
    fn test_agc_mode_set_wire_value_collapses_auto_variants() {
        assert_eq!(AgcMode::Off.set_wire_value(), 0);
        assert_eq!(AgcMode::Fast.set_wire_value(), 1);
        assert_eq!(AgcMode::Mid.set_wire_value(), 2);
        assert_eq!(AgcMode::Slow.set_wire_value(), 3);
        // All three AUTO sub-variants wire-encode identically — the
        // FT-991A's `GT` Set command has no way to request a specific one.
        assert_eq!(AgcMode::AutoFast.set_wire_value(), 4);
        assert_eq!(AgcMode::AutoMid.set_wire_value(), 4);
        assert_eq!(AgcMode::AutoSlow.set_wire_value(), 4);
    }

    // --- ScanState tests ---

    #[test]
    fn test_scan_state_try_from_u8() {
        assert_eq!(ScanState::try_from(0).unwrap(), ScanState::Off);
        assert_eq!(ScanState::try_from(1).unwrap(), ScanState::Up);
        assert_eq!(ScanState::try_from(2).unwrap(), ScanState::Down);
        assert!(ScanState::try_from(3).is_err());
    }

    #[test]
    fn test_scan_state_as_u8_round_trips() {
        for state in [ScanState::Off, ScanState::Up, ScanState::Down] {
            assert_eq!(ScanState::try_from(state.as_u8()).unwrap(), state);
        }
    }
}
