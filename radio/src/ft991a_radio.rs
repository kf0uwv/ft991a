//! FT-991A emulator state machine behind the generic CAT framework.
//!
//! `FT991A_COMMAND_TABLE` is derived from
//! `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf`, re-verified page-by-page
//! against the manual's own per-command Set/Read/Answer tables (not just the
//! p.3 master O/X table) — see `planning/yaesu/task_plan.md` for the full
//! citation list and one confirmed correction to the architect's original
//! transcription (`AG`/`RG`/`SQ` are plain zero-width-query commands, not
//! "selector reads" like `MD`/`SM`).
//!
//! # Wire formats used (11 first-slice commands)
//!
//! | Command | Code | Query          | Set                    | Notes |
//! |---------|------|----------------|-------------------------|-------|
//! | VFO A   | FA   | `FA;`          | `FA<9 digits>;`         | p.9, range 30,000-470,000,000 Hz |
//! | VFO B   | FB   | `FB;`          | `FB<9 digits>;`         | p.9, same shape as FA |
//! | Mode    | MD   | `MD0;` (selector read) | `MD0<hex digit>;` | p.11, modes 1(LSB)-E(C4FM) |
//! | TX      | TX   | `TX;` → 0/1/2  | `TX<0/1>;`              | p.17, 3-valued answer |
//! | S-meter | SM   | `SM0;` (selector read) | none (read-only) | p.17 |
//! | Power   | PS   | `PS;`          | `PS<0/1>;`              | p.14, wake-sequence quirk (see `Ft991a::set_power_on`'s doc comment) |
//! | AF gain | AG   | `AG;`          | `AG0<3 digits>;`        | p.4, 000-255 |
//! | RF gain | RG   | `RG;`          | `RG0<3 digits>;`        | p.15, 000-255 |
//! | Squelch | SQ   | `SQ;`          | `SQ0<3 digits>;`        | p.17, 000-100 |
//! | TX power| PC   | `PC;`          | `PC<3 digits>;`         | p.14, 005-100 watts |
//! | ID      | ID   | `ID;`          | none (read-only)        | p.10, fixed `0670` |

use std::convert::Infallible;

use cat_framework::{
    CatCommandCatalog, CatRadio, CommandDefinition, CommandForm, CommandOperation, CommandOutcome,
    CommandRequest, CommandTable, ProtocolErrorKind, ResponseBuilder, ResponseDisposition,
};

/// FT-991A command identifier owned by the radio crate.
///
/// Exactly the 11 first-slice commands (see module docs). Deliberately NOT
/// padded with placeholder variants for commands outside this wave's scope
/// (`IF`, `RM`/`RI`, memory channels, the `EX` menu, etc.) — the enum grows
/// alongside command coverage in later waves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ft991aCommandId {
    Fa,
    Fb,
    Md,
    Tx,
    Sm,
    Ps,
    Ag,
    Rg,
    Sq,
    Pc,
    Id,
}

const QUERY0: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Query, 0)];
const SET_1: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 1)];
const SET_3: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 3)];
const SET_4: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 4)];
const SET_9: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 9)];
const NONE: &[CommandForm] = &[];

/// `MD`'s `set_forms`: the selector-only read width (1 char: `"0"`) plus the
/// real write width (2 chars: `"0"` + mode hex digit).
///
/// This is the one genuine "selector read" in the first slice (along with
/// `SM`): the manual's Read row for `MD` is `MD0;`, not the zero-width `MD;`
/// `cat-framework`'s parser would otherwise require to classify a frame as
/// `Query`. Any non-empty parameter is matched against `set_forms` instead
/// (see `cat_framework::CommandTable::parse`), so both widths must live
/// here, and `handle_command` disambiguates read-vs-write by
/// `request.parameters.raw().len()` rather than `request.operation` alone.
const MD_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

macro_rules! definition {
    // Explicit controller read/write capability (for the two genuine
    // "selector read" commands, MD and SM).
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr, $readable:expr, $writable:expr) => {
        CommandDefinition {
            id: Ft991aCommandId::$id,
            code: $code,
            name: $name,
            description: $name,
            query_forms: $query,
            set_forms: $set,
            action_forms: NONE,
            response_forms: NONE,
            readable: $readable,
            writable: $writable,
        }
    };
    // Derive read/write from the presence of query / set forms.
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr) => {
        definition!(
            $id,
            $code,
            $name,
            $query,
            $set,
            !$query.is_empty(),
            !$set.is_empty()
        )
    };
}

static DEFINITIONS: &[CommandDefinition<Ft991aCommandId>] = &[
    definition!(Fa, "FA", "VFO A Frequency", QUERY0, SET_9),
    definition!(Fb, "FB", "VFO B Frequency", QUERY0, SET_9),
    definition!(Md, "MD", "Operating Mode", NONE, MD_SET_FORMS, true, true),
    definition!(Tx, "TX", "TX Set", QUERY0, SET_1),
    // SM: selector read (`SM0;`), no write at all (manual p.17 Set row is
    // blank) — readable/writable stated explicitly, not derived.
    definition!(Sm, "SM", "S-Meter Reading", NONE, SET_1, true, false),
    definition!(Ps, "PS", "Power Switch", QUERY0, SET_1),
    definition!(Ag, "AG", "AF Gain", QUERY0, SET_4),
    definition!(Rg, "RG", "RF Gain", QUERY0, SET_4),
    definition!(Sq, "SQ", "Squelch Level", QUERY0, SET_4),
    definition!(Pc, "PC", "Power Control", QUERY0, SET_3),
    definition!(Id, "ID", "Identification", QUERY0, NONE),
];

/// FT-991A command table used by the generic framework.
pub static FT991A_COMMAND_TABLE: CommandTable<Ft991aCommandId> = CommandTable::new(DEFINITIONS);

/// FT-991A's fixed radio identifier (manual p.10). Not stored state — the
/// 4-character answer is treated as an opaque string, not parsed as hex or
/// decimal (the manual gives no basis to prefer either interpretation).
pub const FT991A_ID: &str = "0670";

/// Simulated FT-991A radio state (first slice only).
#[derive(Debug, Clone)]
pub struct Ft991aState {
    pub vfo_a_hz: u64,
    pub vfo_b_hz: u64,
    /// Raw hex-nibble mode value, 1 (LSB) ..= 0xE (C4FM). See manual p.11.
    pub mode: u8,
    /// CAT-asserted PTT state (0/1). Distinct from any front-panel-asserted
    /// TX a later wave's emulator might model (the `TX;` answer's `2`
    /// value, "RADIO TX ON / CAT TX OFF") — this state machine only ever
    /// reports 0/1, never 2.
    pub cat_tx: u8,
    pub af_gain: u8,
    pub rf_gain: u8,
    pub squelch: u8,
    /// `PC` command value, watts (005-100).
    pub power_control: u8,
    pub smeter: u8,
    pub power_on: bool,
}

impl Default for Ft991aState {
    fn default() -> Self {
        Self {
            vfo_a_hz: 14_000_000,
            vfo_b_hz: 14_100_000,
            mode: 0x2, // USB
            cat_tx: 0,
            af_gain: 128,
            rf_gain: 255,
            squelch: 0,
            power_control: 100,
            smeter: 0,
            power_on: true,
        }
    }
}

/// Radio-specific state change event used by emulator logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ft991aEvent {
    pub field: &'static str,
    pub value: String,
}

/// FT-991A emulator radio implementation.
#[derive(Debug, Default)]
pub struct Ft991aRadio {
    state: Ft991aState,
}

impl Ft991aRadio {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &Ft991aState {
        &self.state
    }
}

impl CatCommandCatalog for Ft991aRadio {
    type CommandId = Ft991aCommandId;

    fn command_table(&self) -> &'static CommandTable<Self::CommandId> {
        &FT991A_COMMAND_TABLE
    }
}

/// Write `text` (including the trailing `;`) as a complete response and
/// return the disposition for a successfully-written (possibly `"?;"`
/// content-error) response.
fn respond(response: &mut ResponseBuilder<'_>, text: &str) -> ResponseDisposition {
    response
        .write_complete(text)
        .expect("response write cannot fail before finish");
    ResponseDisposition::ResponseWritten
}

impl CatRadio for Ft991aRadio {
    type Event = Ft991aEvent;
    type Error = Infallible;

    fn handle_command(
        &mut self,
        request: CommandRequest<'_, Self::CommandId>,
        response: &mut ResponseBuilder<'_>,
    ) -> Result<CommandOutcome<Self::Event>, Self::Error> {
        use Ft991aCommandId::*;

        let mut events = Vec::new();
        let params = request.parameters.raw();

        let disposition = match request.id {
            Fa => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("FA{:09};", self.state.vfo_a_hz))
                }
                CommandOperation::Set => match params.parse::<u64>() {
                    Ok(hz) if (30_000..=470_000_000).contains(&hz) => {
                        self.state.vfo_a_hz = hz;
                        events.push(Ft991aEvent {
                            field: "vfo_a_hz",
                            value: hz.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Fb => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("FB{:09};", self.state.vfo_b_hz))
                }
                CommandOperation::Set => match params.parse::<u64>() {
                    Ok(hz) if (30_000..=470_000_000).contains(&hz) => {
                        self.state.vfo_b_hz = hz;
                        events.push(Ft991aEvent {
                            field: "vfo_b_hz",
                            value: hz.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Md => match params.len() {
                1 if params == "0" => respond(response, &format!("MD0{:X};", self.state.mode)),
                2 if params.starts_with('0') => {
                    match params[1..2].chars().next().and_then(|c| c.to_digit(16)) {
                        Some(v) if (1..=0xE).contains(&v) => {
                            self.state.mode = v as u8;
                            events.push(Ft991aEvent {
                                field: "mode",
                                value: format!("{:X}", v),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },
            Tx => match request.operation {
                CommandOperation::Query => respond(response, &format!("TX{};", self.state.cat_tx)),
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.cat_tx = params.parse().expect("validated digit");
                        events.push(Ft991aEvent {
                            field: "cat_tx",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Sm => {
                if params == "0" {
                    respond(response, &format!("SM0{:03};", self.state.smeter))
                } else {
                    respond(response, "?;")
                }
            }
            Ps => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("PS{};", u8::from(self.state.power_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.power_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "power_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Ag => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("AG0{:03};", self.state.af_gain))
                }
                CommandOperation::Set => match parse_selector_level(params, 255) {
                    Some(level) => {
                        self.state.af_gain = level;
                        events.push(Ft991aEvent {
                            field: "af_gain",
                            value: level.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    None => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Rg => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("RG0{:03};", self.state.rf_gain))
                }
                CommandOperation::Set => match parse_selector_level(params, 255) {
                    Some(level) => {
                        self.state.rf_gain = level;
                        events.push(Ft991aEvent {
                            field: "rf_gain",
                            value: level.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    None => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Sq => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("SQ0{:03};", self.state.squelch))
                }
                CommandOperation::Set => match parse_selector_level(params, 100) {
                    Some(level) => {
                        self.state.squelch = level;
                        events.push(Ft991aEvent {
                            field: "squelch",
                            value: level.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    None => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Pc => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("PC{:03};", self.state.power_control))
                }
                CommandOperation::Set => match params.parse::<u16>() {
                    Ok(watts) if (5..=100).contains(&watts) => {
                        self.state.power_control = watts as u8;
                        events.push(Ft991aEvent {
                            field: "power_control",
                            value: watts.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Id => respond(response, &format!("ID{};", FT991A_ID)),
        };

        Ok(CommandOutcome {
            response: disposition,
            events,
        })
    }

    /// Write the FT-991A's protocol error response.
    ///
    /// **Assumption, not manual-cited**: the FT-991A CAT manual's 20 pages
    /// never state a protocol-error response format anywhere (unlike
    /// `ts570d`'s Kenwood manual, which documents `?;` directly). `"?;"` is
    /// widely known outside this manual to be the general Yaesu CAT
    /// convention, and matches `ts570d`'s own convention, but this has NOT
    /// been verified against real FT-991A hardware or an official errata —
    /// open item, see `planning/yaesu/task_plan.md`.
    fn write_protocol_error(
        &mut self,
        kind: ProtocolErrorKind,
        response: &mut ResponseBuilder<'_>,
    ) -> Result<CommandOutcome<Self::Event>, Self::Error> {
        response
            .write_complete("?;")
            .expect("response write cannot fail before finish");
        Ok(CommandOutcome {
            response: ResponseDisposition::ProtocolError(kind),
            events: Vec::new(),
        })
    }
}

/// Parse an `AG`/`RG`/`SQ`-shaped 4-character set parameter (`"0" +
/// 3-digit level`), returning the level if the selector is `0` and the
/// level is within `0..=max`.
fn parse_selector_level(params: &str, max: u16) -> Option<u8> {
    if !params.starts_with('0') {
        return None;
    }
    let level: u16 = params.get(1..4)?.parse().ok()?;
    if level <= max {
        Some(level as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cat_framework::CatFramework;

    use super::*;

    #[test]
    fn table_has_unique_codes_and_ids_and_legal_operations() {
        let mut codes = HashSet::new();
        let mut ids = HashSet::new();
        for definition in FT991A_COMMAND_TABLE.definitions() {
            assert!(
                codes.insert(definition.code),
                "duplicate code {}",
                definition.code
            );
            assert!(
                ids.insert(definition.id),
                "duplicate id {:?}",
                definition.id
            );
            assert!(
                !definition.query_forms.is_empty()
                    || !definition.set_forms.is_empty()
                    || !definition.action_forms.is_empty(),
                "{} has no legal operation",
                definition.code
            );
        }
        assert_eq!(codes.len(), 11, "expected exactly 11 first-slice commands");
    }

    #[test]
    fn table_master_flags_match_manual_p3() {
        // Cross-check against the manual's p.3 master Set/Read/Ans/AI table.
        let fa = FT991A_COMMAND_TABLE.find("FA").unwrap();
        assert!(fa.is_readable() && fa.is_writable());
        let sm = FT991A_COMMAND_TABLE.find("SM").unwrap();
        assert!(sm.is_readable() && !sm.is_writable(), "SM is read-only");
        let id = FT991A_COMMAND_TABLE.find("ID").unwrap();
        assert!(id.is_readable() && !id.is_writable(), "ID is read-only");
        let ps = FT991A_COMMAND_TABLE.find("PS").unwrap();
        assert!(ps.is_readable() && ps.is_writable());
    }

    #[test]
    fn framework_fa_query_returns_wire_response() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA014000000;");
    }

    #[test]
    fn framework_fa_set_then_query_preserves_state() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework
            .process_frame("FA014250000;", &mut output)
            .unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA014250000;");
    }

    #[test]
    fn framework_fa_out_of_range_frequency_is_rejected() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // 480,000,000 Hz exceeds the 470,000,000 Hz maximum.
        framework
            .process_frame("FA480000000;", &mut output)
            .unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    #[test]
    fn framework_tx_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("TX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "TX0;");

        output.clear();
        framework.process_frame("TX1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("TX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "TX1;");
    }

    #[test]
    fn framework_tx_never_reports_the_answer_only_value_two() {
        // The emulator's state machine only models CAT-driven TX (0/1);
        // `2` ("radio TX on via a non-CAT cause") is answer-only per the
        // manual and is never produced by this first-slice state machine.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("TX;", &mut output).unwrap();
        assert_ne!(String::from_utf8(output.clone()).unwrap(), "TX2;");
    }

    // -----------------------------------------------------------------
    // The highest-risk part of this design: MD/SM's selector-read shape.
    // -----------------------------------------------------------------

    #[test]
    fn framework_md_selector_read_parses_as_a_read_not_a_write() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // "MD0;" is structurally a Set (1-byte parameter matches
        // MD_SET_FORMS's width-1 form) but semantically a read — the
        // default mode (USB = 0x2) must come back unchanged.
        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD02;");
    }

    #[test]
    fn framework_md_two_char_parameter_parses_as_a_write() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // "MD01;" (2-byte parameter, matches the width-2 form) sets mode to
        // LSB (1) and produces no response, unlike the 1-byte selector read.
        framework.process_frame("MD01;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD01;");
    }

    #[test]
    fn framework_md_hex_mode_c4fm_round_trips() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MD0E;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD0E;");
    }

    #[test]
    fn framework_sm_selector_read_returns_level_and_has_no_write() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("SM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "SM0000;");

        // SM has no set form at all — any 4-char attempt is an unknown
        // parameter width for the SM command.
        output.clear();
        framework.process_frame("SM0123;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    // -----------------------------------------------------------------
    // Corrected (non-selector-read) AG/RG/SQ shape — plain zero-width
    // query, single 4-char set form.
    // -----------------------------------------------------------------

    #[test]
    fn framework_ag_zero_width_query_and_selector_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("AG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AG0128;");

        output.clear();
        framework.process_frame("AG0200;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("AG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AG0200;");
    }

    #[test]
    fn framework_sq_range_is_0_to_100_not_255() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // 150 is valid for AG/RG (max 255) but not SQ (max 100).
        framework.process_frame("SQ0150;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("SQ0075;", &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn framework_pc_zero_width_query_and_plain_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("PC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PC100;");

        output.clear();
        framework.process_frame("PC050;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("PC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PC050;");
    }

    #[test]
    fn framework_id_is_read_only_fixed_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("ID;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "ID0670;");
    }

    #[test]
    fn framework_unknown_command_uses_protocol_error_response() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let outcome = framework.process_frame("ZZ;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
        assert!(matches!(
            outcome.response,
            ResponseDisposition::ProtocolError(ProtocolErrorKind::UnknownCommand)
        ));
    }

    #[test]
    fn framework_ps_wake_sequence_quirk_field_documented_not_tested_here() {
        // PS itself is a plain zero-width query / 1-char set, tested like
        // TX above. The "dummy data, then 1-2s delay" wake sequence (p.14)
        // is a caller-side sequencing concern (see `Ft991a` in ft991a.rs),
        // not something the emulator's state machine or command table can
        // express — this test only documents where that behavior lives.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PS1;");
    }
}
