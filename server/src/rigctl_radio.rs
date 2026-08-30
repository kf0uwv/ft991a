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

//! `impl cat_rigctl::RigctlRadio` for the FT-991A — the one piece of
//! genuinely FT-991A-specific glue the generic `cat-rigctl` bridge needs
//! (`planning/architect/task_plan.md` §12.2/§9-in-radio-cat-rs). Everything
//! else (dispatch, `\dump_state`, line framing, listener orchestration,
//! error propagation) lives in `cat-rigctl`, shared with `ts570d`.
//!
//! **Deviation from the original sketch**: the task description asked for
//! `impl cat_rigctl::RigctlRadio for radio::Ft991a<S>` directly, but that
//! is an orphan-rules violation (`E0117`) — neither `RigctlRadio` (from
//! `cat_rigctl`) nor `Ft991a` (from `radio`) is local to this crate, and
//! Rust's coherence rules require at least one of them to be. [`Ft991aRigctl`]
//! is a minimal local newtype wrapper around `radio::Ft991a<S>` purely to
//! give the impl a local type to attach to — every method still just
//! delegates straight through to the wrapped `Ft991a<S>`'s existing,
//! already-correct, already-tested inherent async methods; this module
//! never constructs a raw FT-991A wire frame itself. The Hamlib mode-name
//! tables and frequency-range values below are ported verbatim from this
//! crate's former `rigctl.rs` (deleted as part of this migration) — they
//! are genuinely radio-specific and were already correct.

use async_trait::async_trait;
use cat_rigctl::RigctlRadio;
use cat_transport_core::{CatSession, TransportError};
use radio::{Frequency, Ft991a, Mode, TxState};

/// Local newtype wrapper around `radio::Ft991a<S>`, solely so
/// `impl RigctlRadio` below has a type local to this crate to attach to
/// (see this module's doc comment on the orphan-rules deviation).
pub struct Ft991aRigctl<S: CatSession>(Ft991a<S>);

impl<S: CatSession> Ft991aRigctl<S> {
    pub fn new(inner: Ft991a<S>) -> Self {
        Self(inner)
    }
}

#[async_trait(?Send)]
impl<S> RigctlRadio for Ft991aRigctl<S>
where
    S: CatSession<Error = TransportError>,
{
    type Mode = Mode;
    type Error = radio::RadioError;

    async fn get_vfo_a_hz(&mut self) -> Result<u64, Self::Error> {
        self.0.get_vfo_a().await.map(Frequency::hz)
    }

    async fn set_vfo_a_hz(&mut self, hz: u64) -> Result<(), Self::Error> {
        let freq = Frequency::new(hz)?;
        self.0.set_vfo_a(freq).await
    }

    async fn get_mode(&mut self) -> Result<Self::Mode, Self::Error> {
        self.0.get_mode().await
    }

    async fn set_mode(&mut self, mode: Self::Mode) -> Result<(), Self::Error> {
        self.0.set_mode(mode).await
    }

    async fn get_transmitting(&mut self) -> Result<bool, Self::Error> {
        match self.0.get_tx_state().await? {
            TxState::Off => Ok(false),
            _ => Ok(true),
        }
    }

    async fn transmit(&mut self) -> Result<(), Self::Error> {
        self.0.transmit().await
    }

    async fn receive(&mut self) -> Result<(), Self::Error> {
        self.0.receive().await
    }

    /// Map a [`Mode`] to the Hamlib rig-mode name `m`/`M` exchange on the
    /// wire. Best-effort for modes with no exact Hamlib counterpart
    /// (`C4fm`, `AmN`) — documented per-arm below, not silently assumed
    /// correct.
    fn hamlib_mode_name(mode: Self::Mode) -> &'static str {
        match mode {
            Mode::Lsb => "LSB",
            Mode::Usb => "USB",
            Mode::CwU => "CW",
            Mode::CwL => "CWR",
            Mode::Fm => "FM",
            Mode::FmN => "FMN",
            Mode::Am => "AM",
            // Hamlib has no distinct narrow-AM mode name in common use;
            // `AM` is the closest match.
            Mode::AmN => "AM",
            Mode::RttyLsb => "RTTY",
            Mode::RttyUsb => "RTTYR",
            Mode::DataLsb => "PKTLSB",
            Mode::DataUsb => "PKTUSB",
            Mode::DataFm => "PKTFM",
            // No Hamlib equivalent for Yaesu's C4FM digital voice mode;
            // `USB` is a safe, inert fallback (never actually selected by
            // a WSJT-X user, who has no reason to request C4FM over this
            // bridge).
            Mode::C4fm => "USB",
        }
    }

    fn hamlib_mode_from_name(name: &str) -> Option<Self::Mode> {
        match name.to_ascii_uppercase().as_str() {
            "LSB" => Some(Mode::Lsb),
            "USB" => Some(Mode::Usb),
            "CW" => Some(Mode::CwU),
            "CWR" => Some(Mode::CwL),
            "FM" => Some(Mode::Fm),
            "FMN" => Some(Mode::FmN),
            "AM" => Some(Mode::Am),
            "RTTY" => Some(Mode::RttyLsb),
            "RTTYR" => Some(Mode::RttyUsb),
            "PKTLSB" => Some(Mode::DataLsb),
            "PKTUSB" => Some(Mode::DataUsb),
            "PKTFM" => Some(Mode::DataFm),
            _ => None,
        }
    }

    /// Read out of [`radio::capabilities::FT991A`] rather than restated,
    /// so there is one declaration of this radio's coverage and not two.
    ///
    /// `cat_rigctl` only consults this on the placeholder path, which
    /// [`Self::capabilities`] takes us off. Keeping it correct anyway
    /// costs nothing, and a silently-wrong fallback would be nasty.
    fn freq_range_hz() -> (u64, u64) {
        let range = radio::capabilities::FT991A.rx_range;
        (range.min_hz, range.max_hz)
    }

    /// Publish what this radio is, so `\dump_state`'s capability tail is
    /// **generated** rather than a placeholder.
    ///
    /// Until now every Hamlib client was told the same invented story: a
    /// single 10 Hz tuning step, one 2400 Hz filter, and RIT/XIT limits of
    /// 1200 Hz. This radio has eight tuning steps, thirty-four selectable
    /// filter widths and +/-9999 Hz of clarifier, and it covers 30 kHz to
    /// 470 MHz rather than the 1.8-30 MHz a client might reasonably assume
    /// of something answering a Kenwood-shaped protocol.
    ///
    /// This is a deliberate behaviour change to a compatibility layer, and
    /// it fails in the nastiest way available: a `\dump_state` reply
    /// Hamlib disagrees with about length makes `netrigctl_open()` block
    /// forever rather than fail, and nothing in the symptom points at the
    /// cause (radio-cat-rs ADR 0005). So it is verified against a real
    /// client in `tests/hamlib_interop.rs` rather than reasoned about --
    /// this radio's tail is far longer than any fixture upstream tests
    /// with, and length is precisely what that bug was about.
    fn capabilities() -> Option<&'static cat_framework::capabilities::RadioCapabilities> {
        Some(&radio::capabilities::FT991A)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_transport_core::test_support::{Exchange, ScriptedCatSession};

    fn wrap(session: ScriptedCatSession) -> Ft991aRigctl<ScriptedCatSession> {
        Ft991aRigctl::new(Ft991a::new(session))
    }

    #[test]
    fn hamlib_mode_round_trips_for_every_supported_mode() {
        for mode in [
            Mode::Lsb,
            Mode::Usb,
            Mode::CwU,
            Mode::CwL,
            Mode::Fm,
            Mode::FmN,
            Mode::Am,
            Mode::RttyLsb,
            Mode::RttyUsb,
            Mode::DataLsb,
            Mode::DataUsb,
            Mode::DataFm,
        ] {
            let name = <Ft991aRigctl<ScriptedCatSession> as RigctlRadio>::hamlib_mode_name(mode);
            assert_eq!(
                <Ft991aRigctl<ScriptedCatSession> as RigctlRadio>::hamlib_mode_from_name(name),
                Some(mode),
                "mode {mode:?} -> {name} did not round-trip"
            );
        }
    }

    #[test]
    fn the_bridge_publishes_this_radios_capabilities() {
        // Compared by value, not by address: `FT991A` is a `const`, so
        // each `&FT991A` is a separately promoted temporary and pointer
        // identity is not a property it has.
        let caps = <Ft991aRigctl<ScriptedCatSession> as RigctlRadio>::capabilities()
            .expect("the bridge must publish capabilities, not a placeholder");
        assert_eq!(*caps, radio::capabilities::FT991A);
    }

    #[test]
    fn the_generated_dump_state_will_not_be_structurally_empty() {
        // `dump_state` generation lives upstream and is tested there, but
        // it is only as good as what this radio hands it. These are the
        // lists Hamlib reads to a sentinel: were either empty, the reply
        // would fall back to a filler row and quietly stop describing the
        // radio.
        let caps = &radio::capabilities::FT991A;
        assert!(!caps.tuning_steps_hz.is_empty());
        assert!(caps.filters.widths_hz.is_some_and(|w| !w.is_empty()));
        assert!(caps.vfos.rit_hz.is_some());
        assert!(caps.rx_range.min_hz < caps.rx_range.max_hz);
    }

    #[test]
    fn freq_range_hz_matches_frequency_constants() {
        let (min, max) = <Ft991aRigctl<ScriptedCatSession> as RigctlRadio>::freq_range_hz();
        assert_eq!(min, Frequency::MIN_HZ);
        assert_eq!(max, Frequency::MAX_HZ);
    }

    #[test]
    fn set_mode_delegates_to_set_mode() {
        futures::executor::block_on(async {
            let session = ScriptedCatSession::with_script(vec![Exchange::new("MD01;", "")]);
            let mut radio = wrap(session);
            RigctlRadio::set_mode(&mut radio, Mode::Lsb).await.unwrap();
        });
    }

    #[test]
    fn get_vfo_a_hz_delegates_to_get_vfo_a() {
        futures::executor::block_on(async {
            let session =
                ScriptedCatSession::with_script(vec![Exchange::new("FA;", "FA014250000;")]);
            let mut radio = wrap(session);
            let hz = RigctlRadio::get_vfo_a_hz(&mut radio).await.unwrap();
            assert_eq!(hz, 14_250_000);
        });
    }

    #[test]
    fn set_vfo_a_hz_delegates_to_set_vfo_a() {
        futures::executor::block_on(async {
            let session = ScriptedCatSession::with_script(vec![Exchange::new("FA014250000;", "")]);
            let mut radio = wrap(session);
            RigctlRadio::set_vfo_a_hz(&mut radio, 14_250_000)
                .await
                .unwrap();
        });
    }

    #[test]
    fn get_transmitting_maps_tx_state_off_to_false() {
        futures::executor::block_on(async {
            let session = ScriptedCatSession::with_script(vec![Exchange::new("TX;", "TX0;")]);
            let mut radio = wrap(session);
            assert!(!RigctlRadio::get_transmitting(&mut radio).await.unwrap());
        });
    }

    #[test]
    fn get_transmitting_maps_tx_state_on_to_true() {
        futures::executor::block_on(async {
            let session = ScriptedCatSession::with_script(vec![Exchange::new("TX;", "TX1;")]);
            let mut radio = wrap(session);
            assert!(RigctlRadio::get_transmitting(&mut radio).await.unwrap());
        });
    }
}
