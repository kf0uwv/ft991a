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

//! The FT-991A GPU console.
//!
//! Per ADR 0008 this crate owns **this radio's layout, feature set,
//! keybindings and visual identity, and nothing else**. The waterfall
//! pass, meter scaling, S-unit formatting and settings rendering come from
//! `cat-ui` and `cat-ui-egui`; the wire comes from `cat-native`. There is
//! no dependency on `radio` or on any transport: the console is
//! network-only, and the thing on the other end might not even be a
//! FT-991A.
//!
//! # Where the logic lives, and why
//!
//! Everything with a decision in it — which workspaces exist, where a
//! click tunes to, what a typed command means, which quick controls this
//! radio has — is a plain function over a `CapabilitiesWire`, tested
//! without a window. What is left in the egui code is placement.
//!
//! That split is not tidiness. A GUI's rendering is the part hardest to
//! assert on and easiest to eyeball; its behaviour is the reverse. Putting
//! the frequency mapping in the draw call would make the one thing that
//! can be *wrong* the one thing nothing can test.

// The console lives in `cat-ui-egui` now. It was 1442 lines with exactly
// one radio-specific mention in it -- a demo status string -- because
// everything it draws it derives from the capability document. Keeping a
// copy per radio would have produced three consoles that agreed until one
// was edited, which is the failure radio-cat-rs ADR 0013 exists to
// prevent. Re-exported so this crate's own paths are unchanged.
pub use cat_ui_egui::{app, devices, readout, theme, tuning};

// The console's *structure* -- which tabs exist, which quick controls
// exist, what the command line accepts -- is derived from the capability
// document and is therefore not this renderer's to own. It moved to
// `cat-ui` the moment the TUI needed the same answers; re-exported here so
// this crate's own paths are unchanged. Two renderers deriving their own
// tab list from the same document would agree until one was edited, which
// is the failure mode radio-cat-rs ADR 0013 exists to prevent.
pub use cat_ui::{command, quick, workspace};

pub mod demo;

pub use command::{Action, ParseError};
pub use quick::Control;
pub use workspace::{Tab, TabEntry};

/// The window this app puts the shared console in.
///
/// `eframe` stays out of `cat-ui-egui` on purpose (ADR 0011's seam): a
/// window and its event loop belong to a binary, not to a widget crate,
/// and keeping them apart is also what lets the offscreen renderer draw a
/// console with no window at all. So the one thing each app supplies is
/// this forwarding impl.
pub struct Window(pub cat_ui_egui::app::Console);

impl eframe::App for Window {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.draw(ctx);
    }
}
