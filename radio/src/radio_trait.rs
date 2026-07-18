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
//! boundary: [`Frequency`], [`Mode`], [`TxState`], [`RadioError`], and
//! [`RadioResult`].
//!
//! # First-slice scope
//!
//! Per `planning/architect/task_plan.md` §4, this trait is deliberately a
//! **subset** of `ts570d`'s `Radio` trait surface — only the concepts the
//! first 11 commands actually back (vfo a/b, mode, ptt, smeter, power
//! on/off, af/rf gain, squelch, tx power, id). It is NOT padded with
//! `unimplemented!()`/default-`NotImplemented` stubs for RIT/XIT, noise
//! blanker, memory channels, CW keyer, etc. — those grow the trait in later
//! waves alongside command coverage, per the `radio-cat-rs`/`ft991a`
//! `CLAUDE.md` "Radio trait scope" section and the yaesu agent guidelines'
//! instruction not to assume TS-570D parity without checking the manual.

use std::fmt;

use async_trait::async_trait;
use cat_client::ClientError;
use cat_transport_core::TransportError;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during FT-991A CAT protocol operations.
#[derive(Debug, Error)]
pub enum RadioError {
    #[error("Invalid mode: {0:#x}")]
    InvalidMode(u8),
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

    /// Flush the session's receive buffer, discarding unsolicited or stale
    /// data. Default implementation is a no-op.
    fn flush_rx(&mut self) {}
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
    }
}
