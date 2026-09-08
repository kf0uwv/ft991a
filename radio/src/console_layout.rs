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

//! What an FT-991A's console looks like.
//!
//! Authored here, not derived, and deliberately **not** the TS-570D's.
//! Same components; different radio; different console.
//!
//! # Why this radio gets this arrangement
//!
//! This radio has no spectrum over CAT at all — it has a scope display and
//! menu items controlling it, and no command that returns scope *data*
//! (see [`crate::capabilities::FT991A`]'s `signal`). So there is no
//! spectrum panel, and the room a TS-570D gives to a waterfall goes to
//! what this radio actually has:
//!
//! - **Five meters, not four.** S, PO, SWR, ALC and Id, and Id is the one
//!   a TS-570D cannot show at all. The rail is taller for it.
//! - **151 menu items, against 52**, and 117 memories with text tags. The
//!   workspace is the main event here, so it gets the content pane
//!   outright rather than sharing it.
//! - **Thirty-four selectable filter widths and a notch.** The quick
//!   ribbon has something to say on this radio, so it is two rows and
//!   sits under the readout where it can be read while tuning.
//!
//! The AF panels stay, because this radio's USB codec is a real audio
//! path even though its IF is not tapped — but they sit at the foot of the
//! rail rather than taking a third of it.

use cat_layout::{Child, LayoutSpec, Node, PanelKind, Rgb, Size, Theme};

/// The meter rail's width. Wider than a TS-570D's: five meters, and this
/// radio reports them 0-255 with no S-unit calibration the manual gives,
/// so the raw value is shown beside each and needs the room.
const RAIL_W: u16 = 24;

/// The levels rail's width.
const LEVELS_W: u16 = 26;

/// The console this radio asks for.
pub fn layout() -> LayoutSpec {
    LayoutSpec::new(Node::rows(vec![
        // Five: the tab bar plus the readout's two rows, with room for the
        // GPU console's taller strip. The two renderers draw the same
        // panel at different densities, and the layout has to fit the
        // roomier of them or one clips.
        Child::panel(Size::Fixed(5), PanelKind::Readout),
        // Six rows, and unlike a TS-570D's they are full: this radio has
        // thirty-four filter widths, a notch and a clarifier to show,
        // where the Kenwood's ribbon is mostly em dashes.
        Child::panel(Size::Fixed(6), PanelKind::QuickBar),
        Child::new(
            Size::Min(8),
            Node::columns(vec![
                Child::new(
                    Size::Fixed(RAIL_W),
                    Node::rows(vec![
                        // This radio declares seven meters. The renderer says how
                        // many rows that needs, because the two consoles spend
                        // different amounts on each: the terminal console one row,
                        // the GPU console a label row and a bar.
                        //
                        // It was `Min(n)`, which absorbed every spare row in the
                        // column and still was not enough -- the GPU console drew
                        // five of the seven and simply stopped, and a meter missing
                        // from a rail looks like a radio that does not have one.
                        Child::panel(Size::Natural, PanelKind::MeterRail),
                        Child::panel(Size::Fixed(5), PanelKind::AfScope),
                        Child::panel(Size::Fixed(5), PanelKind::AfFft),
                    ]),
                ),
                // No spectrum, so the workspace is not sharing with one.
                // 151 menu items and 117 named memories are what an
                // operator came to this console for.
                Child::panel(Size::Min(20), PanelKind::Workspace),
                Child::new(
                    Size::Fixed(LEVELS_W),
                    Node::rows(vec![
                        Child::panel(Size::Min(6), PanelKind::LevelsRail),
                        // The band and mode buttons live here rather than
                        // across the top: fourteen modes do not fit on one
                        // row, and wrapping them would move them about as
                        // the window resized.
                        Child::panel(Size::Fixed(4), PanelKind::ModeBar),
                        Child::panel(Size::Fixed(3), PanelKind::BandBar),
                    ]),
                ),
            ]),
        ),
        Child::panel(Size::Fixed(1), PanelKind::Status),
        Child::panel(Size::Fixed(1), PanelKind::CommandLine),
    ]))
}

/// What an FT-991A looks like.
///
/// Deliberately unlike the TS-570D's, because the radios are unlike. This
/// is a modern Yaesu: a black panel and a **full-colour TFT**, and its
/// scope display is blue and cyan with white text. So this console is blue
/// and white where the Kenwood's is amber.
///
/// The difference is not decoration. An operator with both rigs on the
/// bench can tell at a glance which console is which — before reading a
/// single label — and that is worth more than two consoles that match
/// each other.
pub fn theme() -> Theme {
    Theme {
        // The panel: near-black, cool rather than warm.
        background: Rgb::hex(0x05080f),
        // The TFT's own ground, a deep blue.
        panel: Rgb::hex(0x0b1526),
        // White text, as the display uses.
        ink: Rgb::hex(0xdfe9f5),
        // The cyan this radio highlights with.
        accent: Rgb::hex(0x4fc3f7),
        // The scope's trace: this radio draws its bandscope in a
        // blue-green, and the console follows it.
        signal: Rgb::hex(0x35d6a4),
        warning: Rgb::hex(0xff5252),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_layout::Area;

    fn area() -> Area {
        Area::new(0, 0, 120, 40)
    }

    #[test]
    fn this_radio_asks_for_no_spectrum_panel() {
        // The capability set says `SignalSupport::None`; the layout agrees
        // rather than placing a panel that would draw an empty axis
        // looking like a dead receiver.
        assert!(!layout().root.places(&PanelKind::Spectrum));
    }

    #[test]
    fn the_workspace_gets_the_room_the_spectrum_would_have_taken() {
        // 151 menu items and 117 memories are what this console is for.
        let w = layout().find(area(), &PanelKind::Workspace).unwrap();
        assert!(w.width > 60, "workspace only {} wide", w.width);
    }

    #[test]
    fn the_meter_rail_is_taller_than_a_four_meter_radio_would_need() {
        // Five meters, one of which (Id) a TS-570D cannot show at all.
        let rail = layout().find(area(), &PanelKind::MeterRail).unwrap();
        assert!(rail.height >= 6, "rail only {} tall", rail.height);
    }

    #[test]
    fn the_furniture_is_there() {
        let spec = layout();
        for f in [PanelKind::Status, PanelKind::CommandLine] {
            assert!(spec.root.places(&f), "{f:?} missing");
        }
    }

    #[test]
    fn it_is_not_the_other_radios_console() {
        // The point of authoring these separately. If these ever converge
        // it should be because somebody decided they should, not because
        // one was derived from the other.
        let mine = layout().resolve(area());
        assert!(mine.iter().any(|p| p.kind == PanelKind::ModeBar));
        assert!(!mine.iter().any(|p| p.kind == PanelKind::Spectrum));
    }
}
