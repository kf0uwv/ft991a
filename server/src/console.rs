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

//! This radio, as the console protocol sees it.
//!
//! The seam `cat_rigctl::run_with_native` asks for: read the radio's whole
//! state in one go, and apply a command the capability set has already
//! accepted. Everything above it — the handshake, validation, spectrum and
//! audio pumps, framing — is `cat-native`'s and is not restated per radio.
//!
//! # One read, not five
//!
//! `IF;` carries the dial, the mode, TX state, the clarifier and the
//! memory channel together, and that is the whole reason to prefer it: a
//! console assembling those from five separate reads would be describing
//! five different moments as though they were one.

use cat_native::{Command, MeterKind, MeterSample, RadioState};
use cat_rigctl::native_bridge::NativeRadio;
use cat_transport_core::{CatSession, TransportError};
use radio::capabilities::{from_mode, to_mode};
use radio::{Frequency, Ft991a};

/// This radio, as the console protocol sees it.
pub struct ConsoleFt991a<S: CatSession>(pub Ft991a<S>);

#[async_trait::async_trait(?Send)]
impl<S> NativeRadio for ConsoleFt991a<S>
where
    S: CatSession<Error = TransportError>,
{
    async fn state(&mut self) -> Option<RadioState> {
        let info = self.0.get_information().await.ok()?;
        let mode = radio::Mode::try_from(info.mode).ok()?;

        // The S-meter is its own command, and the one field that moves
        // fast enough to be worth a second round trip. A failed read drops
        // the meter rather than the whole state: a console can draw a dash
        // for one meter, and can do nothing useful with a frequency it did
        // not get.
        let meters = match self.0.get_smeter().await {
            Ok(raw) => vec![MeterSample {
                kind: MeterKind::S,
                raw: u16::from(raw),
            }],
            Err(_) => Vec::new(),
        };

        // `IF` does not carry TX state on this radio -- `TX;` does, and it
        // distinguishes *this session* keying from the front panel or a
        // footswitch keying. Both are transmitting as far as a console is
        // concerned, and a console that showed RX while the operator held
        // the mic would be worse than one that showed nothing.
        let transmitting = matches!(
            self.0.get_tx_state().await,
            Ok(radio::TxState::CatKeyed | radio::TxState::RadioKeyedNonCat)
        );

        // `select` 0-1 are the VFOs; 2 and up are memory and QMB. A
        // channel number is only meaningful in the first case, and
        // reporting one in VFO mode would have a console highlight a
        // memory the radio is not on.
        let memory_channel = (info.select >= 2).then_some(u16::from(info.channel));

        Some(RadioState {
            vfo_a_hz: info.frequency_hz,
            // `IF` carries one frequency: whichever VFO is active.
            // Reporting it as B as well would be inventing a reading, so B
            // mirrors A until there is a real read for it.
            vfo_b_hz: info.frequency_hz,
            mode: from_mode(mode),
            // Split on this radio is which VFO transmits, which `IF` does
            // not report. Read separately rather than guessed.
            split: self.0.get_tx_vfo().await.map(|v| v != 0).unwrap_or(false),
            transmitting,
            memory_channel,
            // The clarifier is not IF shift: it offsets the receive
            // frequency, where IF shift moves the passband. Reporting one
            // as the other would put a number in a cell that means
            // something else.
            if_shift_hz: None,
            filter_width_hz: None,
            meters,
        })
    }

    async fn apply(&mut self, command: &Command) -> Result<(), String> {
        let result = match command {
            Command::SetFrequency { vfo: 0, hz } | Command::Retune { hz } => {
                match Frequency::new(*hz) {
                    Ok(f) => self.0.set_vfo_a(f).await,
                    Err(e) => return Err(e.to_string()),
                }
            }
            Command::SetFrequency { hz, .. } => match Frequency::new(*hz) {
                Ok(f) => self.0.set_vfo_b(f).await,
                Err(e) => return Err(e.to_string()),
            },
            Command::SetMode { mode } => match to_mode(*mode) {
                Some(m) => self.0.set_mode(m).await,
                None => return Err("this radio has no such mode".to_string()),
            },
            // Split is which VFO transmits: `FT1` puts TX on VFO B, `FT0`
            // returns it to A.
            Command::SetSplit { enabled } => self.0.set_tx_vfo(u8::from(*enabled)).await,
            Command::SetMemoryChannel { channel } => match u8::try_from(*channel) {
                Ok(c) => self.0.set_memory_channel(c).await,
                Err(_) => return Err("memory channel out of range".to_string()),
            },
            // Reads are answered from the published state, never sent.
            Command::ReadMeter { .. } | Command::ReadState | Command::ReadDevices => return Ok(()),
            // Never reaches here: `NativeShared::apply` handles an attach
            // against its device directory and does not queue it. Kept
            // explicit rather than swept into a `_` arm, so the next
            // command added to the protocol fails to compile here instead
            // of being silently accepted and ignored.
            Command::AttachDevice { .. } => {
                return Err("a device attach is not a CAT command".to_string())
            }
            Command::SetIfShift { .. } | Command::SetFilterWidth { .. } => {
                return Err("not wired to CAT on this radio yet".to_string())
            }
        };
        result.map_err(|e| e.to_string())
    }
}
