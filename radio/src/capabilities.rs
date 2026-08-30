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

//! What an FT-991A is, as data.
//!
//! Every number here is a **model fact** — true of any FT-991A, independent
//! of what is plugged into it (radio-cat-rs ADR 0015).
//!
//! Each field cites the code in this crate it agrees with, and the tests at
//! the bottom check the ones that can drift. That is not ceremony: this
//! data was previously described in a `#[cfg(test)]` fixture in
//! `cat-framework`, written by reading this repo, and **two of its numbers
//! were wrong** — see [`MENU`] and [`FILTERS`]. A declaration that lives
//! beside the code it describes, with tests tying the two together, is the
//! fix for that class of error rather than an apology for it.

use cat_framework::capabilities::*;

/// Three endpoints, **none** shareable.
///
/// The USB bridge enumerates two CP210x virtual COM ports — "Enhanced" for
/// CAT, "Standard" for RTS/DTR keying (docs/adr/0002) — plus a USB audio
/// codec. This is the exact inverse of a TS-570D, whose single RS-232C
/// port carries CAT and keying at once, and it is why `shareable_with` is
/// per-endpoint rather than a flag on the set.
const ENDPOINTS: &[EndpointDescriptor] = &[
    EndpointDescriptor {
        role: EndpointRole::Cat,
        required: true,
        shareable_with: &[],
    },
    EndpointDescriptor {
        role: EndpointRole::Keying,
        required: false,
        shareable_with: &[],
    },
    EndpointDescriptor {
        role: EndpointRole::Audio,
        required: false,
        shareable_with: &[],
    },
];

/// Fourteen modes, wire nibbles `0x1`-`0xE`.
///
/// Mirrors [`crate::radio_trait::Mode`] exactly, including the labels its
/// `name()` returns. Two families here have no TS-570D equivalent at all:
/// the DATA modes, and Yaesu's C4FM digital voice.
const MODES: &[ModeDescriptor] = &[
    ModeDescriptor {
        id: ModeId::Lsb,
        label: "LSB",
        kind: ModeKind::Ssb,
        sideband: Some(Sideband::Lower),
        default_bandwidth_hz: 2400,
    },
    ModeDescriptor {
        id: ModeId::Usb,
        label: "USB",
        kind: ModeKind::Ssb,
        sideband: Some(Sideband::Upper),
        default_bandwidth_hz: 2400,
    },
    ModeDescriptor {
        id: ModeId::CwUpper,
        label: "CW-U",
        kind: ModeKind::Cw,
        sideband: Some(Sideband::Upper),
        default_bandwidth_hz: 500,
    },
    ModeDescriptor {
        id: ModeId::Fm,
        label: "FM",
        kind: ModeKind::Fm,
        sideband: None,
        default_bandwidth_hz: 12000,
    },
    ModeDescriptor {
        id: ModeId::Am,
        label: "AM",
        kind: ModeKind::Am,
        sideband: None,
        default_bandwidth_hz: 6000,
    },
    ModeDescriptor {
        id: ModeId::RttyLsb,
        label: "RTTY-LSB",
        kind: ModeKind::Data,
        sideband: Some(Sideband::Lower),
        default_bandwidth_hz: 500,
    },
    ModeDescriptor {
        id: ModeId::CwLower,
        label: "CW-L",
        kind: ModeKind::Cw,
        sideband: Some(Sideband::Lower),
        default_bandwidth_hz: 500,
    },
    ModeDescriptor {
        id: ModeId::DataLsb,
        label: "DATA-LSB",
        kind: ModeKind::Data,
        sideband: Some(Sideband::Lower),
        default_bandwidth_hz: 3000,
    },
    ModeDescriptor {
        id: ModeId::RttyUsb,
        label: "RTTY-USB",
        kind: ModeKind::Data,
        sideband: Some(Sideband::Upper),
        default_bandwidth_hz: 500,
    },
    ModeDescriptor {
        id: ModeId::DataFm,
        label: "DATA-FM",
        kind: ModeKind::Data,
        sideband: None,
        default_bandwidth_hz: 12000,
    },
    ModeDescriptor {
        id: ModeId::FmNarrow,
        label: "FM-N",
        kind: ModeKind::Fm,
        sideband: None,
        default_bandwidth_hz: 6000,
    },
    ModeDescriptor {
        id: ModeId::DataUsb,
        label: "DATA-USB",
        kind: ModeKind::Data,
        sideband: Some(Sideband::Upper),
        default_bandwidth_hz: 3000,
    },
    ModeDescriptor {
        id: ModeId::AmNarrow,
        label: "AM-N",
        kind: ModeKind::Am,
        sideband: None,
        default_bandwidth_hz: 3000,
    },
    ModeDescriptor {
        id: ModeId::C4fm,
        label: "C4FM",
        kind: ModeKind::DigitalVoice,
        sideband: None,
        default_bandwidth_hz: 12500,
    },
];

/// Seven meters, every one reported over **0-255**.
///
/// Contrast a TS-570D, whose meters are 0-30. Same `MeterKind::S`, same
/// raw 15, entirely different signal — which is the reason a reading
/// travels with its range.
///
/// **No `s_units` table.** This radio's TUI has always shown a bar and a
/// raw `nnn/255` with no S-unit against it, and inventing a table here
/// would be asserting a calibration nobody has measured. `None` means the
/// renderer interpolates against the range and says so, which is honest;
/// a fabricated table would not be.
const METERS: &[MeterDescriptor] = &[
    MeterDescriptor {
        kind: MeterKind::S,
        raw_range: RawRange::new(0, 255),
        active_on_transmit: false,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Po,
        raw_range: RawRange::new(0, 255),
        active_on_transmit: true,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Swr,
        raw_range: RawRange::new(0, 255),
        active_on_transmit: true,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Alc,
        raw_range: RawRange::new(0, 255),
        active_on_transmit: true,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Id,
        raw_range: RawRange::new(0, 255),
        active_on_transmit: true,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Vdd,
        raw_range: RawRange::new(0, 255),
        active_on_transmit: true,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Comp,
        raw_range: RawRange::new(0, 255),
        active_on_transmit: true,
        s_units: None,
    },
];

/// The selectable IF bandwidths, as the **union across every mode**.
///
/// # This field does not fit this radio, and the mismatch is recorded
/// rather than hidden
///
/// `widths_hz` is a flat list. The FT-991A's is not: `SH_BANDWIDTH_TABLE`
/// is 22 rows by six columns (SSB/CW/RTTY-PSK, each narrow and wide), and
/// **`SH`'s wire format carries no mode parameter at all** — which column
/// a given `P2` index means is decided by the radio's current mode. 500 Hz
/// is a CW width and a wide RTTY width; 3200 Hz is only ever SSB.
///
/// So there is no single correct answer here, only three wrong ones:
///
/// - one mode's column — wrong for every other mode;
/// - `None` — claims no CAT-selectable widths, which is false;
/// - the union — every width the radio can actually select, losing which
///   mode each belongs to.
///
/// The union is chosen because it is the only one that is *true*, if
/// incomplete, and because the consumer that matters most has the same
/// flat shape: Hamlib's `\dump_state` filter list is per-mode by bitmask,
/// and this bridge sends `-1` (all modes) for the same reason.
///
/// The `cat-framework` fixture that first described this radio listed
/// `[200, 400, 500, 800, 1200, 1500, 1800, 2400, 2900, 3000, 3200]`, which
/// is **not any column of the real table** — it reads like a plausible SSB
/// list assembled by hand. `widths_are_the_real_table_and_not_a_plausible_
/// looking_list` is the test that would have caught it.
const FILTERS: FilterCapability = FilterCapability {
    if_shift_hz: Some(1_000),
    widths_hz: Some(&[
        50, 100, 150, 200, 250, 300, 350, 400, 450, 500, 600, 800, 850, 1100, 1200, 1350, 1400,
        1500, 1650, 1700, 1800, 1950, 2000, 2100, 2200, 2300, 2400, 2500, 2600, 2700, 2800, 2900,
        3000, 3200,
    ]),
    // Both an auto notch (`BC`) and a manual notch (`BP`, 10-3200 Hz).
    notch: true,
};

/// The EX menu: **151 items**, numbered 1-153 with gaps at 27 and 87.
///
/// # The other place the model strains
///
/// `MenuCapability` is a count and a writability flag. That is a fine
/// description of a TS-570D's menu, which is a dense `[u16; 52]`. It is a
/// poor one here, where the menu is 151 heterogeneous *typed* entries —
/// enumerations, levels, frequencies, strings — addressed by a number that
/// is not an index.
///
/// `item_count` is therefore the number of items that exist, not the
/// highest number that addresses one. A consumer that iterated `1..=151`
/// expecting to hit every item would miss 152 and 153 and stumble into the
/// two gaps. Nothing does that today; the test below pins the distinction
/// so that a future consumer finds it stated rather than discovers it.
///
/// The `cat-framework` fixture said 152, which is neither figure.
const MENU: MenuCapability = MenuCapability {
    item_count: 151,
    writable: true,
};

/// The Yaesu FT-991A.
pub const FT991A: RadioCapabilities = RadioCapabilities {
    model: "Yaesu FT-991A",
    endpoints: EndpointSet::new(ENDPOINTS),
    vfos: VfoCapability {
        count: 2,
        split: true,
        // A single shared clarifier offset, -9999..=9999 Hz, gated onto RX
        // and TX independently by `RT`/`XT` (`radio_trait.rs`'s
        // `clarifier_offset_hz`). Reported as a symmetric limit on both,
        // which is what the model can express.
        rit_hz: Some(9999),
        xit_hz: Some(9999),
    },
    modes: MODES,
    tuning_steps_hz: &[10, 100, 1_000, 5_000, 6_250, 10_000, 12_500, 25_000],
    // `Frequency::MIN_HZ` / `MAX_HZ`. HF through UHF, against the
    // TS-570D's HF-only 500 kHz-60 MHz.
    rx_range: FrequencyRange::new(30_000, 470_000_000),
    filters: FILTERS,
    meters: MeterSet::new(METERS),
    memory: Some(MemoryCapability {
        // "Invalid memory channel: valid 1-117" -- note the range starts
        // at 1, not 0. A TS-570D's starts at 0, which is exactly why this
        // is a `RawRange` and not a count.
        channels: RawRange::new(1, 117),
        // Channels carry text tags (`MT`).
        named: true,
        stores_mode: true,
        scan: true,
    }),
    menu: Some(MENU),
    // No IF tap point and no bandscope over CAT. The radio has a scope
    // display and menu items controlling it, but no command that returns
    // scope *data* -- verified against the CAT manual (radio-cat-rs ADR
    // 0010, Context). This is a model fact, and a negative one: it is why
    // this radio gets no waterfall, and saying so beats leaving a console
    // to discover it.
    signal: SignalSupport::None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ft991a_radio::{EX_MENU_TABLE, SH_BANDWIDTH_TABLE};
    use crate::radio_trait::{Frequency, Meter, Mode};

    #[test]
    fn the_declared_modes_are_the_modes_this_crate_can_parse() {
        for nibble in 0x1..=0xEu8 {
            let mode = Mode::try_from(nibble).expect("every nibble 1-E is a mode");
            assert!(
                FT991A.modes.iter().any(|m| m.label == mode.name()),
                "{} is parseable but not declared",
                mode.name()
            );
        }
        assert_eq!(FT991A.modes.len(), 14);
    }

    #[test]
    fn the_declared_coverage_is_the_coverage_this_crate_enforces() {
        assert_eq!(FT991A.rx_range.min_hz, Frequency::MIN_HZ);
        assert_eq!(FT991A.rx_range.max_hz, Frequency::MAX_HZ);
    }

    #[test]
    fn every_meter_this_crate_can_read_is_declared() {
        // Six TX meters from `Meter`, plus the S meter, which is read by
        // its own command and so is not in that enum.
        for meter in [
            Meter::Comp,
            Meter::Alc,
            Meter::Po,
            Meter::Swr,
            Meter::Id,
            Meter::Vdd,
        ] {
            let kind = match meter {
                Meter::Comp => MeterKind::Comp,
                Meter::Alc => MeterKind::Alc,
                Meter::Po => MeterKind::Po,
                Meter::Swr => MeterKind::Swr,
                Meter::Id => MeterKind::Id,
                Meter::Vdd => MeterKind::Vdd,
            };
            assert!(
                FT991A.meters.has(kind),
                "{meter:?} is readable but not declared"
            );
        }
        assert!(FT991A.meters.has(MeterKind::S));
        assert_eq!(FT991A.meters.meters.len(), 7);
    }

    #[test]
    fn the_s_meter_publishes_no_unit_table_rather_than_an_invented_one() {
        // This radio's S-meter calibration has not been measured. A table
        // here would be a fabricated claim about hardware, and it would be
        // believed -- `MeterReading` hands it straight to the renderer.
        let s = FT991A.meters.find(MeterKind::S).expect("has an S meter");
        assert!(s.s_units.is_none());
        assert_eq!(s.raw_range.max, 255);
    }

    #[test]
    fn widths_are_the_real_table_and_not_a_plausible_looking_list() {
        // The test that would have caught the fixture's hand-made list.
        // Every declared width must appear somewhere in the real table,
        // and every width in the real table must be declared.
        let mut actual: Vec<u32> = SH_BANDWIDTH_TABLE
            .iter()
            .flat_map(|row| {
                [
                    row.ssb_narrow,
                    row.ssb_wide,
                    row.cw_narrow,
                    row.cw_wide,
                    row.rtty_psk_narrow,
                    row.rtty_psk_wide,
                ]
            })
            .flatten()
            .map(u32::from)
            .collect();
        actual.sort_unstable();
        actual.dedup();

        let declared = FT991A.filters.widths_hz.expect("widths are declared");
        assert_eq!(
            declared, actual,
            "declared widths are not the union of the real table"
        );
    }

    #[test]
    fn the_menu_item_count_is_a_count_and_not_the_highest_item_number() {
        // The distinction the model cannot express, pinned so it is stated
        // rather than discovered: the menu is numbered past its own length
        // and has holes in it.
        let numbers: Vec<u16> = EX_MENU_TABLE.iter().map(|i| i.p1).collect();
        let menu = FT991A.menu.expect("has a menu");
        assert_eq!(menu.item_count as usize, EX_MENU_TABLE.len());
        assert_eq!(menu.item_count, 151);

        let highest = *numbers.iter().max().unwrap();
        assert_eq!(highest, 153);
        assert!(
            (menu.item_count) < highest,
            "if these ever coincide, the comment above needs rewriting, \
             not the assertion relaxing"
        );
        for gap in [27u16, 87] {
            assert!(!numbers.contains(&gap), "EX {gap} was a gap and is not now");
        }
    }

    #[test]
    fn nothing_here_claims_this_radio_can_produce_a_spectrum() {
        // It has a scope display; it has no command that returns scope
        // data. A console that assumed otherwise would wait forever for
        // frames that are never coming.
        assert_eq!(FT991A.signal, SignalSupport::None);
    }

    #[test]
    fn none_of_the_three_endpoints_may_be_shared() {
        // The inverse of a TS-570D. A supervisor that shared the CAT
        // handle for keying here would be driving the wrong COM port.
        assert_eq!(FT991A.endpoints.endpoints.len(), 3);
        for endpoint in FT991A.endpoints.endpoints {
            assert!(
                endpoint.shareable_with.is_empty(),
                "{:?} must not be shareable",
                endpoint.role
            );
        }
    }
}
