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

//! Capability sets to render against, without a radio or a socket.
//!
//! Not `#[cfg(test)]`: `examples/render.rs` needs these too, and a still
//! of the console showing a disconnected state says nothing about whether
//! the layout is right.
//!
//! These are `CapabilitiesWire` — the shape that arrives from a server —
//! rather than the `radio` crate's declaration, because that is what this
//! crate can see. `gui` never depends on `radio`: it is network-only, and
//! the radio on the other end might not be a TS-570D at all.
//!
//! `caps_bare` is the more useful of the two. A console tested only
//! against a well-equipped radio quietly grows assumptions that it has a
//! memory, a menu, a spectrum source — and then draws a control that can
//! never work the first time somebody points it at something simpler.

use cat_native::{
    CapabilitiesWire, FilterWire, FrequencyRange, Installation, MemoryCapability, MenuCapability,
    MeterDescriptorWire, MeterKind, ModeId, ModeKind, ModeWire, RawRange, SUnitScale, Sideband,
    SignalSupport, VfoCapability,
};

fn meter(
    kind: MeterKind,
    raw_max: u16,
    active_on_transmit: bool,
    s_units: Option<SUnitScale>,
) -> MeterDescriptorWire {
    MeterDescriptorWire {
        kind,
        raw_range: RawRange::new(0, raw_max),
        active_on_transmit,
        s_units,
    }
}

fn mode(id: ModeId, label: &str, kind: ModeKind, sideband: Option<Sideband>, bw: u32) -> ModeWire {
    ModeWire {
        id,
        label: label.to_string(),
        kind,
        sideband,
        default_bandwidth_hz: bw,
    }
}

/// An FT-991A as a server describes it.
///
/// The real declaration, transcribed: this radio's own modes, menu extent,
/// memory range and meters. A fixture with plausible-but-wrong numbers
/// would make a still that says nothing about whether the console handles
/// *this* radio — and this one differs from the TS-570D in every way that
/// matters to a console. Fourteen modes against eight, 151 menu items
/// against 52, five meters against four, selectable filter widths where
/// the Kenwood has none, and **no spectrum at all**.
pub fn ft991a() -> CapabilitiesWire {
    CapabilitiesWire {
        model: "Yaesu FT-991A".to_string(),
        endpoints: Vec::new(),
        vfos: VfoCapability {
            count: 2,
            split: true,
            rit_hz: Some(9999),
            xit_hz: Some(9999),
        },
        modes: vec![
            mode(
                ModeId::Lsb,
                "LSB",
                ModeKind::Ssb,
                Some(Sideband::Lower),
                2400,
            ),
            mode(
                ModeId::Usb,
                "USB",
                ModeKind::Ssb,
                Some(Sideband::Upper),
                2400,
            ),
            mode(
                ModeId::CwUpper,
                "CW-U",
                ModeKind::Cw,
                Some(Sideband::Upper),
                500,
            ),
            mode(ModeId::Fm, "FM", ModeKind::Fm, None, 12000),
            mode(ModeId::Am, "AM", ModeKind::Am, None, 6000),
            mode(
                ModeId::RttyLsb,
                "RTTY-LSB",
                ModeKind::Data,
                Some(Sideband::Lower),
                500,
            ),
            mode(
                ModeId::CwLower,
                "CW-L",
                ModeKind::Cw,
                Some(Sideband::Lower),
                500,
            ),
            mode(
                ModeId::DataLsb,
                "DATA-LSB",
                ModeKind::Data,
                Some(Sideband::Lower),
                3000,
            ),
            mode(
                ModeId::RttyUsb,
                "RTTY-USB",
                ModeKind::Data,
                Some(Sideband::Upper),
                500,
            ),
            mode(ModeId::DataFm, "DATA-FM", ModeKind::Data, None, 12000),
            mode(ModeId::FmNarrow, "FM-N", ModeKind::Fm, None, 6000),
            mode(
                ModeId::DataUsb,
                "DATA-USB",
                ModeKind::Data,
                Some(Sideband::Upper),
                3000,
            ),
            mode(ModeId::AmNarrow, "AM-N", ModeKind::Am, None, 3000),
            mode(ModeId::C4fm, "C4FM", ModeKind::DigitalVoice, None, 12500),
        ],
        tuning_steps_hz: vec![10, 100, 1_000, 5_000, 6_250, 10_000, 12_500, 25_000],
        // HF through UHF, against the TS-570D's HF-only range.
        rx_range: FrequencyRange::new(30_000, 470_000_000),
        filters: FilterWire {
            if_shift_hz: Some(1_000),
            // Thirty-four selectable widths. The quick bar shows a FILTER
            // control for this radio and none for the TS-570D, and that
            // difference is derived from here rather than coded anywhere.
            widths_hz: Some(vec![
                50, 100, 150, 200, 250, 300, 350, 400, 450, 500, 600, 800, 850, 1100, 1200, 1350,
                1400, 1500, 1650, 1700, 1800, 1950, 2000, 2100, 2200, 2300, 2400, 2500, 2600, 2700,
                2800, 2900, 3000, 3200,
            ]),
            notch: true,
        },
        meters: vec![
            // No S-unit scale: this radio reports 0-255 and the manual
            // gives no calibration, so a console shows the raw value
            // rather than inventing an S-number for it.
            meter(MeterKind::S, 255, false, None),
            meter(MeterKind::Po, 255, true, None),
            meter(MeterKind::Swr, 255, true, None),
            meter(MeterKind::Alc, 255, true, None),
            meter(MeterKind::Id, 255, true, None),
        ],
        memory: Some(MemoryCapability {
            // Starts at 1, not 0 — which is why this is a range and not a
            // count, and why a console must not assume either.
            channels: RawRange::new(1, 117),
            named: true,
            stores_mode: true,
            scan: true,
        }),
        menu: Some(MenuCapability {
            item_count: 151,
            writable: true,
        }),
        // No IF tap and no bandscope over CAT. A model fact, and a
        // negative one: it is why this console has no SPECTRUM workspace,
        // and the tab list derives that from here without anyone writing
        // an FT-991A special case.
        signal: SignalSupport::None,
        // A fixture for a still, not a server: the arrangement is the
        // server's to author, so a console drawn from this uses its own
        // default.
        layout: None,
        theme: None,
        installation: Installation::default(),
    }
}
