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

    fn unsupported() -> Self::Error {
        radio::RadioError::NotImplemented
    }

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

    /// Split, from `FT` -- which VFO is the TX band.
    ///
    /// `RigctlRadio` defaults both halves of this to a refusal, and this
    /// impl inherited it while `get_tx_vfo`/`set_tx_vfo` sat one call
    /// away. Rigctl's `s` therefore answered "not split" on a radio in
    /// split, and `S 1` refused: a defaulted method nobody overrode is
    /// indistinguishable from a radio that cannot do the thing.
    async fn get_split(&mut self) -> Result<bool, Self::Error> {
        self.0.get_tx_vfo().await.map(|vfo| vfo != 0)
    }

    async fn set_split(&mut self, on: bool) -> Result<(), Self::Error> {
        self.0.set_tx_vfo(u8::from(on)).await
    }

    /// The clarifier offset, from the `IF` record that already carries it.
    ///
    /// Yaesu calls it the clarifier; hamlib calls it RIT. Zero when RX
    /// CLAR is off: the radio keeps the offset across the switch, and
    /// reporting it while it is not applied would describe a receiver
    /// other than the one listening.
    ///
    /// There is no matching setter. This radio's CAT set has `RC` to
    /// clear and `RU`/`RD` to step, and nothing that takes a frequency --
    /// so `I`/`X` stay refused rather than pretending. Same shape as the
    /// TS-570D, for the same reason.
    async fn get_rit_hz(&mut self) -> Result<i32, Self::Error> {
        let info = self.0.get_information().await?;
        Ok(if info.rx_clarifier_on {
            i32::from(info.clarifier_offset_hz)
        } else {
            0
        })
    }

    async fn get_xit_hz(&mut self) -> Result<i32, Self::Error> {
        // One offset field, two switches: P4 and P5 of the same record
        // say whether it is applied to receive, transmit or both.
        let info = self.0.get_information().await?;
        Ok(if info.tx_clarifier_on {
            i32::from(info.clarifier_offset_hz)
        } else {
            0
        })
    }

    /// The `ModeId` -> `Mode` crossing, so a cached poll can answer `m`.
    ///
    /// Without it every mode read goes to the wire: the cache falls
    /// through rather than guessing, which is safe but spends the link on
    /// a question already answered.
    fn mode_from_id(id: cat_framework::capabilities::ModeId) -> Option<Self::Mode> {
        radio::capabilities::to_mode(id)
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

    /// An `IF` answer with the clarifier fields set as named.
    ///
    /// Built from the manual's P1..P10 layout (CAT reference p.16) rather
    /// than a captured string, so a field that moves shows up here as a
    /// width error instead of as a silently shifted read.
    fn if_answer(offset: i16, rx_clar: bool, tx_clar: bool) -> String {
        format!(
            "IF{channel:03}{freq:09}{sign}{offset:04}{rx}{tx}{mode}{select}{tone}00{shift};",
            channel = 0,
            freq = 14_250_000u64,
            sign = if offset < 0 { '-' } else { '+' },
            offset = offset.unsigned_abs(),
            rx = u8::from(rx_clar),
            tx = u8::from(tx_clar),
            mode = 2, // USB
            select = 0,
            tone = 0,
            shift = 0,
        )
    }

    #[monoio::test(driver = "legacy")]
    async fn split_survives_the_round_trip_through_the_bridge() {
        // `get_split`/`set_split` are defaulted on the trait and this impl
        // inherited both, so rigctl's `s` answered "not split" on a radio
        // in split and `S 1` refused -- while `FT` sat one call away.
        // Asserting the exchange rather than the methods' presence means
        // deleting either impl fails here instead of compiling quietly.
        let mut radio = wrap(ScriptedCatSession::with_script(vec![Exchange::new(
            "FT;", "FT1;",
        )]));
        assert!(RigctlRadio::get_split(&mut radio).await.unwrap());

        let mut radio = wrap(ScriptedCatSession::with_script(vec![Exchange::new(
            "FT;", "FT0;",
        )]));
        assert!(!RigctlRadio::get_split(&mut radio).await.unwrap());

        // `FT`'s set domain is not its answer domain: 0/1 report, 2/3
        // write. The radio crate translates, and this is the assertion
        // that the bridge lets it rather than passing 0/1 through.
        let mut radio = wrap(ScriptedCatSession::with_script(vec![Exchange::new(
            "FT3;", "",
        )]));
        RigctlRadio::set_split(&mut radio, true).await.unwrap();
    }

    #[monoio::test(driver = "legacy")]
    async fn the_clarifier_offset_reaches_rigctl_as_rit_and_xit() {
        // Yaesu's clarifier is hamlib's RIT. Both were refused before,
        // though `IF` had been carrying the offset and both switches all
        // along.
        let mut radio = wrap(ScriptedCatSession::with_script(vec![Exchange::new(
            "IF;",
            if_answer(-1234, true, false),
        )]));
        assert_eq!(RigctlRadio::get_rit_hz(&mut radio).await.unwrap(), -1234);

        let mut radio = wrap(ScriptedCatSession::with_script(vec![Exchange::new(
            "IF;",
            if_answer(500, false, true),
        )]));
        assert_eq!(RigctlRadio::get_xit_hz(&mut radio).await.unwrap(), 500);
    }

    #[monoio::test(driver = "legacy")]
    async fn an_offset_the_radio_is_not_applying_reads_as_zero() {
        // The radio keeps the offset across the switch. Reporting a stored
        // offset that is not being applied would describe a receiver other
        // than the one actually listening, which is worse than saying
        // zero: a client would correct for a shift that is not there.
        let mut radio = wrap(ScriptedCatSession::with_script(vec![Exchange::new(
            "IF;",
            if_answer(-1234, false, false),
        )]));
        assert_eq!(RigctlRadio::get_rit_hz(&mut radio).await.unwrap(), 0);

        let mut radio = wrap(ScriptedCatSession::with_script(vec![Exchange::new(
            "IF;",
            if_answer(-1234, false, false),
        )]));
        assert_eq!(RigctlRadio::get_xit_hz(&mut radio).await.unwrap(), 0);
    }

    #[test]
    fn every_mode_this_radio_declares_crosses_back_from_its_shared_id() {
        // `mode_from_id` lets a cached poll answer `m` without going to
        // the wire. It was never overridden, so it returned `None` for
        // everything and every mode read went out over the link.
        for descriptor in radio::capabilities::FT991A.modes.iter() {
            assert!(
                <Ft991aRigctl<ScriptedCatSession> as RigctlRadio>::mode_from_id(descriptor.id)
                    .is_some(),
                "{} is declared but does not cross back from its ModeId",
                descriptor.label
            );
        }
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
