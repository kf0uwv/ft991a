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
//! # Scope (Wave 1, first slice)
//!
//! Exactly 11 commands (`FA`, `FB`, `MD`, `TX`, `SM`, `PS`, `AG`, `RG`,
//! `SQ`, `PC`, `ID`), derived command-by-command from
//! `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` — see
//! `planning/yaesu/task_plan.md` for the full citation table and one
//! correction found (and flagged) against the architect's original
//! transcription. Later waves grow both `Ft991aCommandId` and the `Radio`
//! trait alongside additional manual-cited command coverage (`IF`,
//! `RM`/`RI`, memory channels, the `EX` menu, etc.).
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

pub mod ft991a;
pub mod ft991a_radio;
pub mod radio_trait;

pub use ft991a::Ft991a;
pub use ft991a_radio::{
    Ft991aCommandId, Ft991aEvent, Ft991aRadio, Ft991aState, FT991A_COMMAND_TABLE, FT991A_ID,
};
pub use radio_trait::{Frequency, Mode, NopRadio, Radio, RadioError, RadioResult, TxState};
