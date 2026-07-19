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

//! Typed FT-991A radio client.
//!
//! [`Ft991a`] wraps a [`CatClient`] and provides strongly-typed convenience
//! methods for every first-slice radio operation. Each getter queries the
//! radio and parses the response into the appropriate Rust type; each
//! setter formats the typed value back into a CAT parameter string before
//! delegating to [`CatClient::set`].
//!
//! # Wire formats used
//!
//! See `ft991a_radio.rs`'s module docs for the full per-command citation
//! table; summarized here for the client-side call shape:
//!
//! | Operation       | Code | Query call                    | Set call                  |
//! |-----------------|------|--------------------------------|----------------------------|
//! | VFO A frequency | FA   | `query("FA")`                  | `set("FA", "<9 digits>")`  |
//! | VFO B frequency | FB   | `query("FB")`                  | `set("FB", "<9 digits>")`  |
//! | Mode            | MD   | `query_with_param("MD","0")`   | `set("MD", "0<hex>")`      |
//! | PTT / TX state  | TX   | `query("TX")` (3-valued)       | `set("TX", "0"/"1")`       |
//! | S-meter         | SM   | `query_with_param("SM","0")`   | none (read-only)           |
//! | Power on/off    | PS   | `query("PS")`                  | `set("PS", "0"/"1")`       |
//! | AF gain         | AG   | `query("AG")`                  | `set("AG", "0<3 digits>")` |
//! | RF gain         | RG   | `query("RG")`                  | `set("RG", "0<3 digits>")` |
//! | Squelch         | SQ   | `query("SQ")`                  | `set("SQ", "0<3 digits>")` |
//! | TX power        | PC   | `query("PC")`                  | `set("PC", "<3 digits>")`  |
//! | Radio ID        | ID   | `query("ID")`                  | none (read-only)           |
//!
//! Note `AG`/`RG`/`SQ` use a **plain** `query()` (zero-width `"AG;"`, not
//! `query_with_param("AG", "0")`) — see `ft991a_radio.rs`'s module docs for
//! why these three are NOT "selector reads" despite carrying a selector
//! byte in their *set* form. Only `MD`/`SM` are genuine selector reads.
//!
//! # Wire formats used (batch 9: meters/status)
//!
//! | Operation | Code | Query call | Set call |
//! |-----------|------|------------|----------|
//! | Composite status | IF | `query("IF")` (28-byte answer, see [`ChannelStatusFields`]) | none (read-only) |
//! | Read meter (direct) | RM | `query_with_param("RM", "<3-8>")` | none (read-only) |
//! | Read meter (front panel) | RM | `query_with_param("RM", "0")` | none (read-only) |
//! | Radio indicator | RI | `query_with_param("RI", "<selector>")` | none (read-only) |
//! | Radio status | RS | `query("RS")` | none (read-only) |
//! | Meter select | MS | `query("MS")` | `set("MS", "0"-"5")` |
//! | PLL unlock | UL | `query("UL")` | none (read-only) |
//!
//! `RM`/`RI` are genuine selector reads (like `MD`/`SM`) — the query call
//! carries a selector parameter even though it's a read.
//!
//! # Wire formats used (batch 2: memory channel records)
//!
//! | Operation | Code | Query/Read call | Set/Write call |
//! |-----------|------|-------------------|-------------------|
//! | Memory channel select | MC | `query("MC")` | `set("MC", "<3 digits>")` |
//! | Memory channel read   | MR | `query_with_param("MR", "<3 digits>")` | none (read-only) |
//! | Memory channel write  | MW | none (write-only) | `set("MW", "<25-byte body>")` |
//! | Memory channel write/tag | MT | `query_with_param("MT", "<3 digits>")` | `set("MT", "<38-byte body>")` |
//!
//! `MR`/`MW` reuse [`ChannelStatusFields`]'s 25-byte P1-P10 wire shape
//! directly (same as `IF`'s answer); `MT` extends it with a fixed reserved
//! byte and the 12-character [`MemoryTag`] — see `ft991a_radio.rs`'s
//! module docs and [`crate::ft991a_radio::MemoryChannelRecord`]'s doc
//! comment for the full citation and the narrow divergences from `IF`'s
//! use of the same shape (channel range, P7/select semantics).
//!
//! # Wire formats used (batch 1: VFO/split/memory quick-ops)
//!
//! | Operation         | Code | Set call            |
//! |-------------------|------|----------------------|
//! | VFO-A to VFO-B    | AB   | `set("AB", "")`       |
//! | VFO-B to VFO-A    | BA   | `set("BA", "")`       |
//! | VFO-A to memory   | AM   | `set("AM", "")`       |
//! | `[V/M]` key       | VM   | `set("VM", "")`       |
//! | Memory to VFO-A   | MA   | `set("MA", "")`       |
//! | Channel up        | CH   | `set("CH", "0")`      |
//! | Channel down      | CH   | `set("CH", "1")`      |
//! | QMB store         | QI   | `set("QI", "")`       |
//! | QMB recall        | QR   | `set("QR", "")`       |
//! | Quick split       | QS   | `set("QS", "")`       |
//! | Swap VFO          | SV   | `set("SV", "")`       |
//!
//! All ten are write-only `CommandOperation::Action` triggers (`cat-client`'s
//! `set(code, "")` formats the bare `"<code>;"` wire frame `cat-framework`
//! parses as an Action, since `ClientError::CommandNotWritable` only checks
//! `is_writable()`, which is `true` for these — no dedicated "action" client
//! method was needed). See `ft991a_radio.rs`'s module docs for the full
//! manual citations, the `VM`/`AM` heading-inconsistency resolution, and the
//! judgment calls (`CH`'s wrap-around, `QI`/`QR`'s dedicated QMB slot,
//! `QS`'s toggle semantics).
//!
//! # Wire formats used (batch 4: keyer/CW/break-in)
//!
//! | Operation        | Code | Query/Read call                | Set/Write call |
//! |------------------|------|----------------------------------|-----------------|
//! | Keyer memory read | KM  | `query_with_param("KM", "<channel>")` | none |
//! | Keyer memory write | KM | none | `set("KM", "<channel><message>")` |
//! | Key pitch        | KP   | `query("KP")`                    | `set("KP", "<2 digits>")` |
//! | Keyer on/off     | KR   | `query("KR")`                    | `set("KR", "0"/"1")` |
//! | Key speed        | KS   | `query("KS")`                    | `set("KS", "<3 digits>")` |
//! | CW keying        | KY   | none                              | `set("KY", "<1 char>")` |
//! | CW spot          | CS   | `query("CS")`                    | `set("CS", "0"/"1")` |
//! | Zero in          | ZI   | none                              | `set("ZI", "")` (Action) |
//! | Break-in         | BI   | `query("BI")`                    | `set("BI", "0"/"1")` |
//! | CW break-in delay | SD  | `query("SD")`                    | `set("SD", "<4 digits>")` |
//!
//! `KM`/`KY` are `Ft991a`-inherent methods, not `Radio` trait methods — see
//! `ft991a_radio.rs`'s module docs' "KM"/"KY" sections and
//! [`Ft991a::read_keyer_memory`]/[`Ft991a::write_keyer_memory`]/
//! [`Ft991a::play_keyer_memory`]'s own doc comments for the full reasoning
//! (the stored-message system and its playback trigger are FT-991A-specific,
//! distinct from the generic CW-operating concepts — break-in, semi
//! break-in delay, CW spot, keyer speed/pitch/on-off, zero-in — that ARE on
//! the `Radio` trait, per `CLAUDE.md`'s "Radio trait scope" section and
//! `ts570d::Radio`'s own precedent for the analogous concepts).
//!
//! # Wire formats used (batch 5: scan/VOX/busy)
//!
//! | Operation  | Code | Query call     | Set call                |
//! |------------|------|-----------------|---------------------------|
//! | Scan       | SC   | `query("SC")` (3-valued) | `set("SC", "0"/"1"/"2")` |
//! | VOX status | VX   | `query("VX")`  | `set("VX", "0"/"1")`     |
//! | VOX delay  | VD   | `query("VD")`  | `set("VD", "<4 digits>")` |
//! | VOX gain   | VG   | `query("VG")`  | `set("VG", "<3 digits>")` |
//! | Busy       | BY   | `query("BY")`  | none (read-only)         |
//!
//! [`Ft991a::get_vox_delay`]/[`Ft991a::set_vox_delay`] carry the same `EX`
//! menu item 142 "VOX SELECT" dependency documented on
//! [`crate::Radio::get_vox_delay`] and in `ft991a_radio.rs`'s module docs —
//! not resolvable by this crate, stated explicitly rather than hidden.
//!
//! # Wire formats used (batch 6: attenuator/preamp/noise/AGC/notch/
//! filter-width)
//!
//! | Operation | Code | Query call | Set call |
//! |-----------|------|------------|----------|
//! | Attenuator | RA | `query_with_param("RA","0")` | `set("RA", "0<0/1>")` |
//! | Pre-amp/IPO | PA | `query_with_param("PA","0")` | `set("PA", "0<0-2>")` |
//! | Noise blanker on/off | NB | `query_with_param("NB","0")` | `set("NB", "0<0/1>")` |
//! | Noise blanker level | NL | `query_with_param("NL","0")` | `set("NL", "0<3 digits>")` |
//! | Noise reduction on/off | NR | `query_with_param("NR","0")` | `set("NR", "0<0/1>")` |
//! | Noise reduction level | RL | `query_with_param("RL","0")` | `set("RL", "0<2 digits>")` |
//! | AGC mode | GT | `query_with_param("GT","0")` (7-valued) | `set("GT", "0<0-4>")` (5-valued) |
//! | Contour on/off | CO | `query_with_param("CO","00")` | `set("CO", "00<4 digits>")` |
//! | Contour frequency | CO | `query_with_param("CO","01")` | `set("CO", "01<4 digits>")` |
//! | APF on/off | CO | `query_with_param("CO","02")` | `set("CO", "02<4 digits>")` |
//! | APF frequency | CO | `query_with_param("CO","03")` | `set("CO", "03<4 digits>")` |
//! | Manual notch on/off | BP | `query_with_param("BP","00")` | `set("BP", "00<3 digits>")` |
//! | Manual notch frequency | BP | `query_with_param("BP","01")` | `set("BP", "01<3 digits>")` |
//! | Auto notch | BC | `query_with_param("BC","0")` | `set("BC", "0<0/1>")` |
//! | Narrow | NA | `query_with_param("NA","0")` | `set("NA", "0<0/1>")` |
//! | Filter width index | SH | `query_with_param("SH","0")` | `set("SH", "0<2 digits>")` |
//!
//! `CO`'s four items and `BP`'s two items are `Ft991a`-inherent-only, not on
//! the `Radio` trait — see `ft991a_radio.rs`'s module docs' "CO"/"BP"
//! sections for why. `GT`'s Query/Set methods use [`AgcMode`], whose 7-valued
//! Answer domain is wider than its 5-valued Set domain — see [`AgcMode`]'s
//! own doc comment.
//!
//! # Wire formats used (batch 7: speech processor/mic/monitor)
//!
//! | Operation | Code | Query call | Set call |
//! |-----------|------|------------|----------|
//! | Mic gain | MG | `query("MG")` | `set("MG", "<3 digits>")` |
//! | Speech processor level | PL | `query("PL")` | `set("PL", "<3 digits>")` |
//! | Speech processor on/off | PR | `query_with_param("PR","0")` | `set("PR", "0<1/2>")` |
//! | Parametric mic EQ on/off | PR | `query_with_param("PR","1")` | `set("PR", "1<1/2>")` |
//! | Monitor on/off | ML | `query_with_param("ML","0")` | `set("ML", "0<3 digits>")` |
//! | Monitor level | ML | `query_with_param("ML","1")` | `set("ML", "1<3 digits>")` |
//!
//! `MG`/`PL` use a **plain** `query()`/`set()` (no selector byte at all,
//! like `PC`) — unlike `PR`/`ML`, genuine selector reads. `PR`'s on/off
//! wire encoding is `1`=OFF/`2`=ON (not the usual `0`/`1` — manual p.14,
//! see `ft991a_radio.rs`'s module docs' "PR, a genuine manual heading
//! typo" section). `PR`'s Parametric Mic EQ item (`P1=1`) is
//! `Ft991a`-inherent-only, same treatment as `CO`/`BP`.
//!
//! # Wire formats used (batch 8: band/step/encoder front-panel controls)
//!
//! | Operation | Code | Query call | Set call |
//! |-----------|------|------------|----------|
//! | Band select | BS | none (write-only) | `set("BS", "<2 digits>")` |
//! | Band up | BU | none (write-only) | `set("BU", "0")` |
//! | Band down | BD | none (write-only) | `set("BD", "0")` |
//! | Fast step | FS | `query("FS")` | `set("FS", "0"/"1")` |
//! | Encoder down | ED | none (write-only) | `set("ED", "<P1><2 digits>")` |
//! | Encoder up | EU | none (write-only) | `set("EU", "<P1><2 digits>")` |
//! | Ent key | EK | none | `set("EK", "")` (Action) |
//! | Mic down | DN | none | `set("DN", "")` (Action) |
//! | Mic up | UP | none | `set("UP", "")` (Action) |
//!
//! `BS` has no `Read`/`Answer` form at all, so [`Ft991a::set_band`] has no
//! paired getter. `ED`/`EU`/`EK` are `Ft991a`-inherent-only (no
//! `Radio`-trait exposure) — see `ft991a_radio.rs`'s module docs' "ED/EU"
//! and "EK" sections. `DN`/`UP` are resolved to
//! [`Ft991a::mic_down`]/[`Ft991a::mic_up`], mirroring
//! `ts570d::Radio::mic_down`/`mic_up` exactly — see `ft991a_radio.rs`'s
//! module docs' "DN/UP" section for the full cross-radio corroboration
//! behind that resolution.
//!
//! # Wire formats used (batch 10, last of the 10 core batches: misc
//! system/TX/tuner/DVS)
//!
//! | Operation | Code | Query call | Set call |
//! |-----------|------|------------|----------|
//! | Antenna tuner state | AC | `query("AC")` | `set("AC", "00<0/1/2>")` |
//! | Auto information on/off | AI | `query("AI")` | `set("AI", "0"/"1")` |
//! | Dimmer | DA | `query("DA")` | `set("DA", "00<2 digits><2 digits>")` |
//! | Date | DT | `query_with_param("DT","0")` | `set("DT", "0<8 digits>")` |
//! | Time | DT | `query_with_param("DT","1")` | `set("DT", "1<6 digits>")` |
//! | Time zone offset | DT | `query_with_param("DT","2")` | `set("DT", "2<sign><4 digits>")` |
//! | Frequency lock | LK | `query("LK")` | `set("LK", "0"/"1")` |
//! | Opposite band info | OI | `query("OI")` | none (read-only) |
//! | Repeater shift | OS | `query_with_param("OS","0")` | `set("OS", "0<0/1/2>")` |
//! | TX VFO select | FT | `query("FT")` | `set("FT", "2"/"3")` (Answer domain `0`/`1`) |
//! | TXW ("TS") | TS | `query("TS")` | `set("TS", "0"/"1")` |
//! | MOX | MX | `query("MX")` | `set("MX", "0"/"1")` |
//! | DVS recording channel | LM | `query_with_param("LM","0")` | `set("LM", "0<0-5>")` |
//! | DVS playback channel | PB | `query_with_param("PB","0")` | `set("PB", "0<0-5>")` |
//!
//! `AC`/`DA`/`DT`/`OI`/`TS`/`LM`/`PB` are `Ft991a`-inherent-only (no
//! `Radio`-trait exposure) — see `ft991a_radio.rs`'s module docs' "Batch 10"
//! section for the per-command reasoning, including why `TS` is not modeled
//! as "tuning step" despite the architect's dispatch-prompt guess. `FT`'s
//! `Set` wire domain (`2`/`3`) is translated to/from the Answer domain
//! (`0`/`1`) at this client's boundary — see [`Ft991a::get_tx_vfo`]/
//! [`Ft991a::set_tx_vfo`].

use std::cell::RefCell;
use std::rc::Rc;

use cat_client::CatClient;
use cat_transport_core::{CatSession, ModemControlLines, ResponseDisposition, TransportError};

use crate::ft991a_radio::{
    apf_hz_to_raw, apf_raw_to_hz, ctcss_tone_hz, ctcss_tone_index, dcs_code_index, dcs_code_number,
    ky_selector_to_wire, ChannelStatusFields, EncoderSelector, Ft991aCommandId, KeyerPlaybackMode,
    FT991A_COMMAND_TABLE,
};
use crate::{
    AgcMode, Band, Frequency, MemoryChannelEntry, MemoryTag, Meter, Mode, PreampMode, RadioError,
    RadioIndicator, RadioResult, RepeaterShift, ScanState, TaggedMemoryChannel, ToneSquelchMode,
    TxState,
};

/// Shares one [`CatSession`] between the internal [`CatClient`] (whose
/// `session` field is private to the `cat_client` crate) and [`Ft991a`]'s
/// own direct access — needed for [`Ft991a::flush_rx`] and for this
/// module's wire-byte-level test assertions, neither of which `CatClient`
/// exposes a passthrough for. Local to `radio`, not a redesign of
/// `cat_client`.
///
/// Copied near-verbatim from `ts570d/radio/src/ts570d.rs`'s
/// `SharedSession<S>` (per `planning/architect/task_plan.md` §4) — it
/// solves a generic monoio-`!Send`-futures-vs-`RefCell`-borrow problem, not
/// an FT-991A-specific one.
///
/// Holds `Option<S>` rather than `S` so that `execute`/`send` can take
/// ownership of the session out of the `RefCell` *before* awaiting (and put
/// it back synchronously afterwards) instead of holding a `RefCell` borrow
/// across an `.await` point, which clippy's `await_holding_refcell_ref`
/// correctly flags as a re-entrancy/panic hazard. `Ft991a` never calls two
/// session-touching methods concurrently against the same handle (all of
/// its own methods take `&mut self`), so the session is always present
/// when `take()` runs.
pub(crate) struct SharedSession<S>(Rc<RefCell<Option<S>>>);

impl<S> SharedSession<S> {
    fn new(session: S) -> Self {
        Self(Rc::new(RefCell::new(Some(session))))
    }

    /// Take ownership of the session for the duration of one synchronous or
    /// asynchronous operation, then hand it back with [`Self::put_back`].
    fn take(&self) -> S {
        self.0
            .borrow_mut()
            .take()
            .expect("SharedSession: session unavailable (unexpected re-entrant use)")
    }

    fn put_back(&self, session: S) {
        *self.0.borrow_mut() = Some(session);
    }

    /// Read-only, non-consuming access to the underlying session, for this
    /// module's wire-byte-level test assertions (e.g.
    /// `radio.session.borrow().transport.written()`). Never called while
    /// `take()` is outstanding — same non-reentrancy guarantee as above.
    #[cfg(test)]
    fn borrow(&self) -> impl std::ops::Deref<Target = S> + '_ {
        std::cell::Ref::map(self.0.borrow(), |opt| {
            opt.as_ref()
                .expect("SharedSession: session unavailable (unexpected re-entrant use)")
        })
    }
}

impl<S> Clone for SharedSession<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[async_trait::async_trait(?Send)]
impl<S: CatSession> CatSession for SharedSession<S> {
    type Error = S::Error;

    async fn execute(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8>,
    ) -> Result<ResponseDisposition, Self::Error> {
        let mut session = self.take();
        let result = session.execute(request, response).await;
        self.put_back(session);
        result
    }

    async fn send(&mut self, request: &[u8]) -> Result<(), Self::Error> {
        let mut session = self.take();
        let result = session.send(request).await;
        self.put_back(session);
        result
    }

    fn flush_rx(&mut self) {
        let mut session = self.take();
        session.flush_rx();
        self.put_back(session);
    }
}

/// Blanket delegation, mirroring [`CatSession for SharedSession<S>`](
/// SharedSession)'s `take`/call/`put_back` shape exactly: whenever the
/// wrapped `S` also implements [`ModemControlLines`] (e.g.
/// `cat-transport-serial::SerialPort`, or `SerialCatSession<SerialPort>` via
/// that crate's own blanket delegation), `SharedSession<S>` forwards the
/// capability unchanged. `ModemControlLines`'s methods take `&self` (direct
/// synchronous `ioctl(2)` calls, no `.await` point — see
/// `planning/architect/task_plan.md` §10.3), so there is no re-entrancy
/// hazard here the way there could be across an `.await`; `take`/`put_back`
/// are used anyway, purely for consistency with the `CatSession` delegation
/// above and to keep exactly one borrow pattern in this file.
impl<S: ModemControlLines> ModemControlLines for SharedSession<S> {
    fn set_rts(&self, asserted: bool) -> Result<(), TransportError> {
        let session = self.take();
        let result = session.set_rts(asserted);
        self.put_back(session);
        result
    }

    fn set_dtr(&self, asserted: bool) -> Result<(), TransportError> {
        let session = self.take();
        let result = session.set_dtr(asserted);
        self.put_back(session);
        result
    }

    fn read_cts(&self) -> Result<bool, TransportError> {
        let session = self.take();
        let result = session.read_cts();
        self.put_back(session);
        result
    }

    fn read_dsr(&self) -> Result<bool, TransportError> {
        let session = self.take();
        let result = session.read_dsr();
        self.put_back(session);
        result
    }

    fn read_dcd(&self) -> Result<bool, TransportError> {
        let session = self.take();
        let result = session.read_dcd();
        self.put_back(session);
        result
    }
}

/// Strip a wire response's leading command code and trailing `;`,
/// returning the parameter body, or [`RadioError::InvalidProtocolString`]
/// if the response isn't shaped like `"<code><body>;"`.
fn parse_frame<'a>(raw: &'a str, code: &str) -> RadioResult<&'a str> {
    raw.strip_prefix(code)
        .and_then(|rest| rest.strip_suffix(';'))
        .ok_or_else(|| RadioError::InvalidProtocolString(raw.to_string()))
}

/// Parse an `RM<selector><3 digits>;` answer, verifying the echoed
/// selector matches what was requested before returning the 3-digit level.
fn parse_rm_answer(raw: &str, expected_selector: u8) -> RadioResult<u8> {
    let body = parse_frame(raw, "RM")?;
    let echoed = body
        .chars()
        .next()
        .and_then(|c| c.to_digit(10))
        .ok_or_else(|| RadioError::InvalidProtocolString(raw.to_string()))?;
    if echoed as u8 != expected_selector {
        return Err(RadioError::InvalidProtocolString(raw.to_string()));
    }
    body.get(1..4)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| RadioError::InvalidProtocolString(raw.to_string()))
}

/// Parse an `IS0<sign><4 digits>;` answer/echo body (post-`IS` prefix,
/// e.g. `"0+1000"`) into a signed Hz value. See `ft991a_radio.rs`'s module
/// docs' "IS, a resolved manual discrepancy" section for the 4-digit width.
fn parse_is_body(body: &str) -> Option<i16> {
    if body.len() != 6 || !body.starts_with('0') {
        return None;
    }
    let sign = body.get(1..2)?;
    let magnitude: i16 = body.get(2..6)?.parse().ok()?;
    match sign {
        "+" => Some(magnitude),
        "-" => Some(-magnitude),
        _ => None,
    }
}

/// Parse a `CN<P1><P2><3 digits>;` answer/echo body (post-`CN` prefix, e.g.
/// `"00042"`), verifying the echoed table selector (`'0'`=CTCSS, `'1'`=DCS)
/// matches `expected_table` before returning the 3-digit table index.
fn parse_cn_index(body: &str, expected_table: char) -> Option<u8> {
    if body.len() != 5 || !body.starts_with('0') {
        return None;
    }
    if body.get(1..2)?.chars().next()? != expected_table {
        return None;
    }
    body.get(2..5)?.parse().ok()
}

/// Parse a `PR<P1><P2>;` answer/echo, verifying the echoed `P1` selector
/// (`'0'`=Speech Processor, `'1'`=Parametric Mic EQ) matches `expected_p1`,
/// then decoding `P2`'s on/off encoding (`1`=OFF, `2`=ON — manual p.14, not
/// the usual `0`/`1`).
fn parse_pr_answer(raw: &str, expected_p1: char) -> RadioResult<bool> {
    let body = parse_frame(raw, "PR")?;
    if body.len() != 2 || !body.starts_with(expected_p1) {
        return Err(RadioError::InvalidProtocolString(raw.to_string()));
    }
    match body.get(1..2) {
        Some("1") => Ok(false),
        Some("2") => Ok(true),
        _ => Err(RadioError::InvalidProtocolString(raw.to_string())),
    }
}

/// Parse an `ML<P1><3 digits>;` answer/echo body (post-`ML` prefix, e.g.
/// `"0001"`), verifying the echoed selector (`'0'`=MONI on/off, `'1'`=MONI
/// level) matches `expected_p1` before returning the raw 3-digit value.
fn parse_ml_body(body: &str, expected_p1: char) -> Option<u16> {
    if body.len() != 4 || body.chars().next()? != expected_p1 {
        return None;
    }
    body.get(1..4)?.parse().ok()
}

/// Convert a parsed [`ChannelStatusFields`] (`MR`'s or `MT`'s answer body)
/// into the trait-facing [`MemoryChannelEntry`], upgrading the raw mode
/// nibble into the domain [`Mode`] type.
fn memory_entry_from_fields(
    fields: ChannelStatusFields,
    raw: &str,
) -> RadioResult<MemoryChannelEntry> {
    Ok(MemoryChannelEntry {
        channel: fields.channel,
        frequency_hz: fields.frequency_hz,
        clarifier_offset_hz: fields.clarifier_offset_hz,
        rx_clarifier_on: fields.rx_clarifier_on,
        tx_clarifier_on: fields.tx_clarifier_on,
        mode: Mode::try_from(fields.mode)
            .map_err(|_| RadioError::InvalidProtocolString(raw.to_string()))?,
        tone_status: fields.tone_status,
        offset_type: fields.offset_type,
    })
}

/// Convert a [`MemoryChannelEntry`] into the [`ChannelStatusFields`] shape
/// `MW`'s and `MT`'s Set forms require, with `select` fixed to `0` (manual
/// p.12: both `MW`'s and `MT`'s Set-direction P7 are `"(Fixed)"` — see
/// `crate::ft991a_radio::MemoryChannelRecord`'s doc comment).
fn channel_status_fields_for_write(entry: MemoryChannelEntry) -> ChannelStatusFields {
    ChannelStatusFields {
        channel: entry.channel,
        frequency_hz: entry.frequency_hz,
        clarifier_offset_hz: entry.clarifier_offset_hz,
        rx_clarifier_on: entry.rx_clarifier_on,
        tx_clarifier_on: entry.tx_clarifier_on,
        mode: entry.mode.as_u8(),
        select: 0,
        tone_status: entry.tone_status,
        offset_type: entry.offset_type,
    }
}

/// Strongly-typed client for the Yaesu FT-991A CAT interface.
///
/// Wraps [`CatClient<Ft991aCommandId, S>`] and converts raw protocol
/// strings into typed Rust values. All async methods are monoio-compatible
/// (`!Send`).
pub struct Ft991a<S: CatSession> {
    pub(crate) client: CatClient<Ft991aCommandId, SharedSession<S>>,
    session: SharedSession<S>,
}

impl<S> Ft991a<S>
where
    S: CatSession<Error = TransportError>,
{
    /// Create a new `Ft991a` wrapping the given session.
    pub fn new(session: S) -> Self {
        let session = SharedSession::new(session);
        Self {
            client: CatClient::new(session.clone(), &FT991A_COMMAND_TABLE),
            session,
        }
    }

    // -----------------------------------------------------------------------
    // VFO A frequency
    // -----------------------------------------------------------------------

    /// Query the current VFO A frequency.
    pub async fn get_vfo_a(&mut self) -> RadioResult<Frequency> {
        let raw = self.client.query("FA").await?;
        let body = parse_frame(&raw, "FA")?;
        Frequency::from_protocol_str(body)
    }

    /// Set the VFO A frequency.
    pub async fn set_vfo_a(&mut self, freq: Frequency) -> RadioResult<()> {
        self.client
            .set("FA", &freq.to_protocol_string())
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // VFO B frequency
    // -----------------------------------------------------------------------

    /// Query the current VFO B frequency.
    pub async fn get_vfo_b(&mut self) -> RadioResult<Frequency> {
        let raw = self.client.query("FB").await?;
        let body = parse_frame(&raw, "FB")?;
        Frequency::from_protocol_str(body)
    }

    /// Set the VFO B frequency.
    pub async fn set_vfo_b(&mut self, freq: Frequency) -> RadioResult<()> {
        self.client
            .set("FB", &freq.to_protocol_string())
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Operating mode
    // -----------------------------------------------------------------------

    /// Query the current operating mode (main receiver, selector `0`).
    ///
    /// Manual p.11: read is `MD0;` (selector included) → answer `MD0<hex
    /// digit>;` — a genuine "selector read", not a zero-width query.
    pub async fn get_mode(&mut self) -> RadioResult<Mode> {
        let raw = self.client.query_with_param("MD", "0").await?;
        let body = parse_frame(&raw, "MD")?;
        let mode_char = body
            .chars()
            .nth(1)
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        Mode::try_from(mode_char)
    }

    /// Set the operating mode (main receiver, selector `0`).
    pub async fn set_mode(&mut self, mode: Mode) -> RadioResult<()> {
        self.client
            .set("MD", &format!("0{}", mode.as_wire_char()))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // PTT / TX state
    // -----------------------------------------------------------------------

    /// Assert CAT-driven PTT (`TX1;`).
    pub async fn transmit(&mut self) -> RadioResult<()> {
        self.client.set("TX", "1").await.map_err(Into::into)
    }

    /// Release CAT-driven PTT (`TX0;`).
    pub async fn receive(&mut self) -> RadioResult<()> {
        self.client.set("TX", "0").await.map_err(Into::into)
    }

    /// Query the 3-valued `TX;` answer. See [`TxState`] — the manual's `2`
    /// value ("RADIO TX ON / CAT TX OFF") means the radio is transmitting
    /// via a non-CAT cause; [`Self::transmit`]/[`Self::receive`] alone
    /// cannot represent that, since they only ever send `TX0;`/`TX1;`.
    pub async fn get_tx_state(&mut self) -> RadioResult<TxState> {
        let raw = self.client.query("TX").await?;
        let body = parse_frame(&raw, "TX")?;
        let digit: u8 = body
            .parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))?;
        TxState::try_from(digit)
    }

    // -----------------------------------------------------------------------
    // S-meter (read-only)
    // -----------------------------------------------------------------------

    /// Query the S-meter reading (main receiver, selector `0`).
    ///
    /// Manual p.17: read is `SM0;` (selector read) → answer `SM0<3
    /// digits>;`, range 000-255. There is no `SM` set form at all.
    pub async fn get_smeter(&mut self) -> RadioResult<u8> {
        let raw = self.client.query_with_param("SM", "0").await?;
        let body = parse_frame(&raw, "SM")?;
        body.get(1..4)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    // -----------------------------------------------------------------------
    // Power on/off
    // -----------------------------------------------------------------------

    /// Query transceiver power on/off state.
    pub async fn get_power_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("PS").await?;
        let body = parse_frame(&raw, "PS")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Set transceiver power on/off.
    ///
    /// **Note**: manual p.14 states this command "requires dummy data be
    /// initially sent. Then after one second and before two seconds the
    /// command is sent" when waking the radio from standby. That
    /// caller-side sequencing quirk is deliberately NOT baked into this
    /// method (which stays a faithful 1:1 `PS<0/1>;` wire mapping) — a
    /// dedicated `wake_and_power_on()` helper was scoped in
    /// `planning/architect/task_plan.md` §4 as a first-slice
    /// nice-to-have, not a hard blocker, and is deferred to a follow-on
    /// wave rather than guessed at without a verified `monoio` timer API
    /// precedent anywhere else in this codebase or `ts570d`.
    pub async fn set_power_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("PS", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // AF gain
    // -----------------------------------------------------------------------

    /// Query the AF (audio) gain level (main receiver).
    ///
    /// Manual p.4: read is `AG;` (zero-width — **not** a selector read,
    /// unlike `MD`/`SM`), answer `AG0<3 digits>;`, range 000-255.
    pub async fn get_af_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("AG").await?;
        let body = parse_frame(&raw, "AG")?;
        body.get(1..4)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the AF gain level for the main receiver (selector `0` baked in).
    pub async fn set_af_gain(&mut self, level: u8) -> RadioResult<()> {
        self.client
            .set("AG", &format!("0{:03}", level))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // RF gain
    // -----------------------------------------------------------------------

    /// Query the RF gain level. Manual p.15: zero-width read, range 000-255.
    pub async fn get_rf_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("RG").await?;
        let body = parse_frame(&raw, "RG")?;
        body.get(1..4)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the RF gain level (0-255).
    pub async fn set_rf_gain(&mut self, level: u8) -> RadioResult<()> {
        self.client
            .set("RG", &format!("0{:03}", level))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Squelch
    // -----------------------------------------------------------------------

    /// Query squelch level. Manual p.17: zero-width read, range 000-100
    /// (**not** 255 — different upper bound from AF/RF gain).
    pub async fn get_squelch(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("SQ").await?;
        let body = parse_frame(&raw, "SQ")?;
        body.get(1..4)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set squelch level (0-100).
    pub async fn set_squelch(&mut self, level: u8) -> RadioResult<()> {
        self.client
            .set("SQ", &format!("0{:03}", level))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Transmit power
    // -----------------------------------------------------------------------

    /// Query the transmit power setting (watts, 005-100). No selector byte
    /// at all — unlike AG/RG/SQ, PC's set form is a plain 3-digit value.
    pub async fn get_power(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("PC").await?;
        let body = parse_frame(&raw, "PC")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the transmit power (watts, 005-100).
    pub async fn set_power(&mut self, watts: u8) -> RadioResult<()> {
        self.client
            .set("PC", &format!("{:03}", watts))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Radio identification (read-only)
    // -----------------------------------------------------------------------

    /// Query the radio's fixed model identifier. Manual p.10: `ID;` →
    /// `ID0670;` for the FT-991A. Returned as an opaque 4-character string
    /// — the manual gives no basis to parse it as hex or decimal.
    pub async fn get_id(&mut self) -> RadioResult<String> {
        let raw = self.client.query("ID").await?;
        let body = parse_frame(&raw, "ID")?;
        Ok(body.to_string())
    }

    // -----------------------------------------------------------------------
    // Composite VFO/memory-channel status (IF, read-only)
    // -----------------------------------------------------------------------

    /// Query the composite VFO/memory-channel status payload. Manual p.10:
    /// `IF;` → 28-byte answer (`IF` + 25-byte body + `;`) — see
    /// [`ChannelStatusFields`] for the settled field layout (re-verified
    /// column-by-column against the manual page image, resolving the
    /// ambiguity Wave 1 deferred `IF` over).
    pub async fn get_information(&mut self) -> RadioResult<ChannelStatusFields> {
        let raw = self.client.query("IF").await?;
        let body = parse_frame(&raw, "IF")?;
        ChannelStatusFields::parse(body).ok_or(RadioError::InvalidProtocolString(raw))
    }

    // -----------------------------------------------------------------------
    // Meters (MS, RM)
    // -----------------------------------------------------------------------

    /// Select which physical meter (`MS`) [`Self::get_active_meter_reading`]
    /// subsequently reports. Manual p.12.
    pub async fn select_meter(&mut self, meter: Meter) -> RadioResult<()> {
        self.client
            .set("MS", &meter.as_u8().to_string())
            .await
            .map_err(Into::into)
    }

    /// Query which physical meter `MS` currently has selected.
    pub async fn get_selected_meter(&mut self) -> RadioResult<Meter> {
        let raw = self.client.query("MS").await?;
        let body = parse_frame(&raw, "MS")?;
        let digit: u8 = body
            .parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))?;
        Meter::try_from(digit)
    }

    /// Directly read one named meter (`RM`'s P1 direct-select values,
    /// `3`-`8` — manual p.15), independent of `MS`'s current selection.
    /// To read the S-meter (`RM` P1=1), use [`Self::get_smeter`] instead —
    /// `SM` reports the identical value via a dedicated command.
    pub async fn get_meter(&mut self, meter: Meter) -> RadioResult<u8> {
        // MS's 0-5 selection maps onto RM's direct-select 3-8 (manual
        // p.12's MS legend and p.15's RM legend agree: 0=COMP..5=VDD and
        // 3=COMP..8=VDD respectively — RM's direct selectors are simply
        // offset by 3 from MS's).
        let selector = meter.as_u8() + 3;
        let raw = self
            .client
            .query_with_param("RM", &selector.to_string())
            .await?;
        parse_rm_answer(&raw, selector)
    }

    /// Read whatever meter is currently shown on the front panel — `RM`
    /// P1=0 ("Depends on the front panel METER"), which tracks `MS`'s most
    /// recent selection. The manual documents `0` and `2` as equivalent
    /// (both "Depends on the front panel METER"); this method always sends
    /// `0`.
    pub async fn get_active_meter_reading(&mut self) -> RadioResult<u8> {
        let raw = self.client.query_with_param("RM", "0").await?;
        parse_rm_answer(&raw, 0)
    }

    // -----------------------------------------------------------------------
    // Radio indicators (RI, read-only)
    // -----------------------------------------------------------------------

    /// Query one status-flag indicator (`RI`, manual p.15). See
    /// [`RadioIndicator`] for the legal selector set (the manual's P1
    /// legend has a documented gap).
    pub async fn get_radio_indicator(&mut self, indicator: RadioIndicator) -> RadioResult<bool> {
        let selector = indicator.as_u8();
        let selector_char = char::from_digit(selector as u32, 16)
            .expect("RadioIndicator::as_u8 is always a valid hex digit")
            .to_ascii_uppercase();
        let raw = self
            .client
            .query_with_param("RI", &selector_char.to_string())
            .await?;
        let body = parse_frame(&raw, "RI")?;
        match body.get(1..2) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    // -----------------------------------------------------------------------
    // Radio status (RS, read-only)
    // -----------------------------------------------------------------------

    /// Query whether the radio is currently in MENU MODE (`RS`, manual
    /// p.16). `true` = MENU MODE, `false` = NORMAL MODE.
    pub async fn get_menu_mode_active(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("RS").await?;
        let body = parse_frame(&raw, "RS")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    // -----------------------------------------------------------------------
    // PLL unlock status (UL, read-only)
    // -----------------------------------------------------------------------

    /// Query whether the PLL is unlocked (`UL`, manual p.18). `true` =
    /// Unlock, `false` = Lock.
    pub async fn get_pll_unlocked(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("UL").await?;
        let body = parse_frame(&raw, "UL")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    // -----------------------------------------------------------------------
    // Memory channel select (MC)
    // -----------------------------------------------------------------------

    /// Query the currently selected memory channel (`MC`, manual p.11).
    pub async fn get_memory_channel(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("MC").await?;
        let body = parse_frame(&raw, "MC")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Select a memory channel (`MC`), range 1-117 (manual p.11:
    /// 001-099 regular, 100=P-1L .. 117=P-9U).
    pub async fn set_memory_channel(&mut self, ch: u8) -> RadioResult<()> {
        if !(1..=117).contains(&ch) {
            return Err(RadioError::InvalidMemoryChannel(ch));
        }
        self.client
            .set("MC", &format!("{:03}", ch))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Memory channel read/write (MR, MW)
    // -----------------------------------------------------------------------

    /// Read a memory channel's contents (`MR`, manual p.12; read-only —
    /// no tag, see [`Self::read_memory_channel_tag`]).
    pub async fn read_memory_channel(&mut self, ch: u8) -> RadioResult<MemoryChannelEntry> {
        if !(1..=117).contains(&ch) {
            return Err(RadioError::InvalidMemoryChannel(ch));
        }
        let raw = self
            .client
            .query_with_param("MR", &format!("{:03}", ch))
            .await?;
        let body = parse_frame(&raw, "MR")?;
        let fields = ChannelStatusFields::parse(body)
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        memory_entry_from_fields(fields, &raw)
    }

    /// Write a memory channel's contents (`MW`, manual p.12; write-only,
    /// no answer, no tag).
    pub async fn write_memory_channel(&mut self, entry: MemoryChannelEntry) -> RadioResult<()> {
        if !(1..=117).contains(&entry.channel) {
            return Err(RadioError::InvalidMemoryChannel(entry.channel));
        }
        let fields = channel_status_fields_for_write(entry);
        self.client
            .set("MW", &fields.to_wire_string())
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Memory channel write/tag (MT)
    // -----------------------------------------------------------------------

    /// Read a memory channel's contents plus its tag (`MT`, manual p.12).
    pub async fn read_memory_channel_tag(&mut self, ch: u8) -> RadioResult<TaggedMemoryChannel> {
        if !(1..=117).contains(&ch) {
            return Err(RadioError::InvalidMemoryChannel(ch));
        }
        let raw = self
            .client
            .query_with_param("MT", &format!("{:03}", ch))
            .await?;
        let body = parse_frame(&raw, "MT")?;
        if body.len() != 38 {
            return Err(RadioError::InvalidProtocolString(raw.clone()));
        }
        let fields_body = body
            .get(0..ChannelStatusFields::WIRE_WIDTH)
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        let raw_tag = body
            .get(ChannelStatusFields::WIRE_WIDTH + 1..38)
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        let fields = ChannelStatusFields::parse(fields_body)
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        let entry = memory_entry_from_fields(fields, &raw)?;
        let tag = MemoryTag::new(raw_tag.trim_end_matches(' '))
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))?;
        Ok(TaggedMemoryChannel { entry, tag })
    }

    /// Write a memory channel's contents plus its tag (`MT`) — a single
    /// composite write, not an incremental tag-only update (manual p.12;
    /// see [`TaggedMemoryChannel`]'s doc comment).
    pub async fn write_memory_channel_tag(
        &mut self,
        channel: TaggedMemoryChannel,
    ) -> RadioResult<()> {
        if !(1..=117).contains(&channel.entry.channel) {
            return Err(RadioError::InvalidMemoryChannel(channel.entry.channel));
        }
        let fields = channel_status_fields_for_write(channel.entry);
        let wire = format!(
            "{}0{}",
            fields.to_wire_string(),
            channel.tag.to_wire_string()
        );
        self.client.set("MT", &wire).await.map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 1: VFO/split/memory quick-ops (AB, BA, AM, VM, MA, CH, QI, QR,
    // QS, SV)
    // -----------------------------------------------------------------------

    /// Copy VFO-A's frequency into VFO-B (`AB`, manual p.4).
    pub async fn copy_vfo_a_to_b(&mut self) -> RadioResult<()> {
        self.client.set("AB", "").await.map_err(Into::into)
    }

    /// Copy VFO-B's frequency into VFO-A (`BA`, manual p.4).
    pub async fn copy_vfo_b_to_a(&mut self) -> RadioResult<()> {
        self.client.set("BA", "").await.map_err(Into::into)
    }

    /// Store VFO-A into the currently selected memory channel (`AM`, manual
    /// p.4; selection is set via [`Self::set_memory_channel`]).
    pub async fn store_vfo_to_memory(&mut self) -> RadioResult<()> {
        self.client.set("AM", "").await.map_err(Into::into)
    }

    /// Emulate pressing the front-panel `[V/M]` key, toggling between VFO
    /// and Memory operating mode (`VM`, manual p.18).
    ///
    /// **Judgment call, not manual-proven** — see `ft991a_radio.rs`'s
    /// module docs' "VM/AM manual heading inconsistency" section for the
    /// full reasoning. `VM`'s own per-command box is headed identically to
    /// `AM`'s ("VFO-A TO MEMORY CHANNEL"), which the wire shape alone
    /// cannot disambiguate from `AM`'s genuine store behavior — this method
    /// name and its FT-991A-inherent (not `Radio`-trait) placement both
    /// reflect that residual uncertainty.
    pub async fn toggle_vfo_memory_mode(&mut self) -> RadioResult<()> {
        self.client.set("VM", "").await.map_err(Into::into)
    }

    /// Recall the currently selected memory channel into VFO-A (`MA`,
    /// manual p.11).
    pub async fn recall_memory_to_vfo(&mut self) -> RadioResult<()> {
        self.client.set("MA", "").await.map_err(Into::into)
    }

    /// Step the selected memory channel up (`CH0`, manual p.5). Wraps
    /// 117→1 (documented judgment call, see `ft991a_radio.rs`'s module
    /// docs).
    pub async fn memory_channel_up(&mut self) -> RadioResult<()> {
        self.client.set("CH", "0").await.map_err(Into::into)
    }

    /// Step the selected memory channel down (`CH1`, manual p.5). Wraps
    /// 1→117.
    pub async fn memory_channel_down(&mut self) -> RadioResult<()> {
        self.client.set("CH", "1").await.map_err(Into::into)
    }

    /// Store VFO-A into the dedicated Quick Memory Bank slot (`QI`, manual
    /// p.14) — distinct from the 117 numbered memory channels (see
    /// `ft991a_radio.rs`'s module docs' "QI/QR" section).
    pub async fn qmb_store(&mut self) -> RadioResult<()> {
        self.client.set("QI", "").await.map_err(Into::into)
    }

    /// Recall the Quick Memory Bank slot into VFO-A (`QR`, manual p.14).
    pub async fn qmb_recall(&mut self) -> RadioResult<()> {
        self.client.set("QR", "").await.map_err(Into::into)
    }

    /// Toggle Quick Split (`QS`, manual p.15). **Judgment call**: the
    /// manual has no dedicated "split on"/"split off" command anywhere in
    /// the master table, and `QS` itself has no Read/Answer row — modeled
    /// as a plain toggle, see `ft991a_radio.rs`'s module docs' "QS"
    /// section.
    pub async fn quick_split(&mut self) -> RadioResult<()> {
        self.client.set("QS", "").await.map_err(Into::into)
    }

    /// Swap VFO-A's and VFO-B's frequencies (`SV`, manual p.17).
    pub async fn swap_vfos(&mut self) -> RadioResult<()> {
        self.client.set("SV", "").await.map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 3: clarifier/RIT-XIT (RT, RC, RD, RU, XT)
    // -----------------------------------------------------------------------

    /// Query whether the RX clarifier is applied (`RT`, manual p.16).
    pub async fn get_rx_clarifier_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("RT").await?;
        let body = parse_frame(&raw, "RT")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the RX clarifier (`RT`).
    pub async fn set_rx_clarifier_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("RT", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query whether the TX clarifier is applied (`XT`, manual p.19).
    pub async fn get_tx_clarifier_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("XT").await?;
        let body = parse_frame(&raw, "XT")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the TX clarifier (`XT`).
    pub async fn set_tx_clarifier_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("XT", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Zero the shared clarifier offset (`RC`, manual p.15). Does not
    /// change [`Self::get_rx_clarifier_on`]/[`Self::get_tx_clarifier_on`]'s
    /// state — see `ft991a_radio.rs`'s module docs' "RC" section.
    pub async fn clarifier_clear(&mut self) -> RadioResult<()> {
        self.client.set("RC", "").await.map_err(Into::into)
    }

    /// Set the shared clarifier offset below the tuned frequency (`RD`,
    /// manual p.15; `0..=9999` Hz). An absolute set, not an incremental
    /// step — see `ft991a_radio.rs`'s module docs' "RD/RU's direction
    /// encoding" section.
    pub async fn clarifier_down(&mut self, offset_hz: u16) -> RadioResult<()> {
        if offset_hz > 9999 {
            return Err(RadioError::InvalidProtocolString(offset_hz.to_string()));
        }
        self.client
            .set("RD", &format!("{offset_hz:04}"))
            .await
            .map_err(Into::into)
    }

    /// Set the shared clarifier offset above the tuned frequency (`RU`,
    /// manual p.16, "RX CLARIFIER PLUS OFFSET"; `0..=9999` Hz). An absolute
    /// set, not an incremental step.
    pub async fn clarifier_up(&mut self, offset_hz: u16) -> RadioResult<()> {
        if offset_hz > 9999 {
            return Err(RadioError::InvalidProtocolString(offset_hz.to_string()));
        }
        self.client
            .set("RU", &format!("{offset_hz:04}"))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 3: IF-shift (IS)
    // -----------------------------------------------------------------------

    /// Query the IF-shift offset, Hz (`IS`, manual p.10; -1200..=1200,
    /// 20 Hz steps). Manual p.10's read is `IS0;` (selector read) — see
    /// `ft991a_radio.rs`'s module docs' "IS, a resolved manual discrepancy"
    /// section for the 4-digit (not the box's literal 3-digit) magnitude
    /// width this crate uses.
    pub async fn get_if_shift_hz(&mut self) -> RadioResult<i16> {
        let raw = self.client.query_with_param("IS", "0").await?;
        let body = parse_frame(&raw, "IS")?;
        parse_is_body(body).ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the IF-shift offset, Hz (`IS`). Returns
    /// [`RadioError::InvalidIfShift`] if `hz` is outside `-1200..=1200` or
    /// not a multiple of 20.
    pub async fn set_if_shift_hz(&mut self, hz: i16) -> RadioResult<()> {
        if !(-1200..=1200).contains(&hz) || hz % 20 != 0 {
            return Err(RadioError::InvalidIfShift(hz));
        }
        let sign = if hz < 0 { '-' } else { '+' };
        self.client
            .set("IS", &format!("0{sign}{:04}", hz.abs()))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 3: tone squelch mode + CTCSS/DCS value (CT, CN)
    // -----------------------------------------------------------------------

    /// Query the tone squelch mode (`CT`, manual p.5; selector read,
    /// `CT0;`).
    pub async fn get_tone_squelch_mode(&mut self) -> RadioResult<ToneSquelchMode> {
        let raw = self.client.query_with_param("CT", "0").await?;
        let body = parse_frame(&raw, "CT")?;
        let digit = body
            .get(1..2)
            .and_then(|s| s.parse::<u8>().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        ToneSquelchMode::try_from(digit)
    }

    /// Set the tone squelch mode (`CT`).
    pub async fn set_tone_squelch_mode(&mut self, mode: ToneSquelchMode) -> RadioResult<()> {
        self.client
            .set("CT", &format!("0{}", mode.as_u8()))
            .await
            .map_err(Into::into)
    }

    /// Query the currently selected CTCSS tone, Hz (`CN` with `P2=0`,
    /// manual p.5/p.6 Table 1).
    pub async fn get_ctcss_tone_hz(&mut self) -> RadioResult<f32> {
        let raw = self.client.query_with_param("CN", "00").await?;
        let body = parse_frame(&raw, "CN")?;
        let index = parse_cn_index(body, '0')
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        ctcss_tone_hz(index).ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Select a CTCSS tone by its frequency in Hz — must be one of the 50
    /// standard tones (`CN` with `P2=0`). Returns
    /// [`RadioError::InvalidCtcssTone`] otherwise.
    pub async fn set_ctcss_tone_hz(&mut self, hz: f32) -> RadioResult<()> {
        let index = ctcss_tone_index(hz).ok_or(RadioError::InvalidCtcssTone(hz))?;
        self.client
            .set("CN", &format!("00{index:03}"))
            .await
            .map_err(Into::into)
    }

    /// Query the currently selected DCS code (`CN` with `P2=1`, manual
    /// p.5/p.6 Table 2).
    pub async fn get_dcs_code(&mut self) -> RadioResult<u16> {
        let raw = self.client.query_with_param("CN", "01").await?;
        let body = parse_frame(&raw, "CN")?;
        let index = parse_cn_index(body, '1')
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        dcs_code_number(index).ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Select a DCS code — must be one of the 104 standard codes (`CN`
    /// with `P2=1`). Returns [`RadioError::InvalidDcsCode`] otherwise.
    pub async fn set_dcs_code(&mut self, code: u16) -> RadioResult<()> {
        let index = dcs_code_index(code).ok_or(RadioError::InvalidDcsCode(code))?;
        self.client
            .set("CN", &format!("01{index:03}"))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 4: keyer/CW/break-in (KM KP KR KS KY CS ZI BI SD)
    // -----------------------------------------------------------------------

    /// Read one `KM` keyer memory channel's stored message (manual p.10;
    /// channel `1`-`5`). Returns an empty string for a vacant channel.
    pub async fn read_keyer_memory(&mut self, channel: u8) -> RadioResult<String> {
        if !(1..=5).contains(&channel) {
            return Err(RadioError::InvalidKeyerMemoryChannel(channel));
        }
        let raw = self
            .client
            .query_with_param("KM", &channel.to_string())
            .await?;
        let body = parse_frame(&raw, "KM")?;
        let echoed = body
            .chars()
            .next()
            .and_then(|c| c.to_digit(10))
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        if echoed as u8 != channel {
            return Err(RadioError::InvalidProtocolString(raw.clone()));
        }
        Ok(body.get(1..).unwrap_or("").to_string())
    }

    /// Write one `KM` keyer memory channel's message (manual p.10; channel
    /// `1`-`5`, message 1-50 printable ASCII characters, no `;`). This
    /// implementation cannot write an empty (0-character) message — see
    /// `ft991a_radio.rs`'s module docs' "KM" section for why (the wire
    /// shape's read-vs-write disambiguation requires a non-empty message).
    pub async fn write_keyer_memory(&mut self, channel: u8, message: &str) -> RadioResult<()> {
        if !(1..=5).contains(&channel) {
            return Err(RadioError::InvalidKeyerMemoryChannel(channel));
        }
        let len = message.chars().count();
        if !(1..=50).contains(&len)
            || !message
                .chars()
                .all(|c| (' '..='~').contains(&c) && c != ';')
        {
            return Err(RadioError::InvalidKeyerMessage(message.to_string()));
        }
        self.client
            .set("KM", &format!("{channel}{message}"))
            .await
            .map_err(Into::into)
    }

    /// Trigger playback of a stored `KM` keyer memory channel (`KY`, manual
    /// p.11). **Distinct from the RTS/DTR real-time CW-keying feature**
    /// (`planning/architect/task_plan.md` §10.2-10.4, not yet consumed on
    /// this repo's side) — `KY` asks the radio to autonomously transmit a
    /// *pre-stored* message (written via [`Self::write_keyer_memory`]) in
    /// one of two playback families ([`KeyerPlaybackMode`]); RTS/DTR is
    /// real-time, PC-driven keying of arbitrary Morse timing with no
    /// pre-stored content and no CAT command involved at all. See
    /// `ft991a_radio.rs`'s module docs' "KY" section for the full
    /// citation, including the cross-reference to `EX` menu items 018-022
    /// confirming the two playback families both address the *same*
    /// 5-channel `KM` store, not two independent stores.
    ///
    /// Kept `Ft991a`-inherent, not on the `Radio` trait — this feature is
    /// tightly coupled to the FT-991A-specific `KM` message store, the same
    /// scope reasoning [`Self::read_keyer_memory`]/[`Self::write_keyer_memory`]
    /// document.
    pub async fn play_keyer_memory(
        &mut self,
        channel: u8,
        mode: KeyerPlaybackMode,
    ) -> RadioResult<()> {
        let wire = ky_selector_to_wire(channel, mode)
            .ok_or(RadioError::InvalidKeyerMemoryChannel(channel))?;
        self.client
            .set("KY", &wire.to_string())
            .await
            .map_err(Into::into)
    }

    /// Query the keyer pitch, Hz (`KP`, manual p.10; `300`-`1050`, 10 Hz
    /// steps).
    pub async fn get_keyer_pitch_hz(&mut self) -> RadioResult<u16> {
        let raw = self.client.query("KP").await?;
        let body = parse_frame(&raw, "KP")?;
        let raw_value: u16 = body
            .parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))?;
        Ok(300 + raw_value * 10)
    }

    /// Set the keyer pitch, Hz (`KP`). Returns
    /// [`RadioError::InvalidKeyerPitch`] if `hz` is outside `300..=1050` or
    /// not a multiple of 10 Hz above 300.
    pub async fn set_keyer_pitch_hz(&mut self, hz: u16) -> RadioResult<()> {
        if !(300..=1050).contains(&hz) || (hz - 300) % 10 != 0 {
            return Err(RadioError::InvalidKeyerPitch(hz));
        }
        let raw_value = (hz - 300) / 10;
        self.client
            .set("KP", &format!("{raw_value:02}"))
            .await
            .map_err(Into::into)
    }

    /// Query the electronic keyer's on/off state (`KR`, manual p.10).
    pub async fn get_keyer_enabled(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("KR").await?;
        let body = parse_frame(&raw, "KR")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the electronic keyer (`KR`).
    pub async fn set_keyer_enabled(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("KR", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query the keyer speed, WPM (`KS`, manual p.11; `4`-`60`).
    pub async fn get_keyer_speed(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("KS").await?;
        let body = parse_frame(&raw, "KS")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the keyer speed, WPM (`KS`). Returns
    /// [`RadioError::InvalidKeyerSpeed`] if `wpm` is outside `4..=60`.
    pub async fn set_keyer_speed(&mut self, wpm: u8) -> RadioResult<()> {
        if !(4..=60).contains(&wpm) {
            return Err(RadioError::InvalidKeyerSpeed(wpm));
        }
        self.client
            .set("KS", &format!("{wpm:03}"))
            .await
            .map_err(Into::into)
    }

    /// Query whether CW spot is enabled (`CS`, manual p.6).
    pub async fn get_cw_spot_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("CS").await?;
        let body = parse_frame(&raw, "CS")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable CW spot (`CS`).
    pub async fn set_cw_spot_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("CS", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Trigger the CW auto zero-in function (`ZI`, manual p.18;
    /// zero-width, write-only).
    pub async fn zero_in(&mut self) -> RadioResult<()> {
        self.client.set("ZI", "").await.map_err(Into::into)
    }

    /// Query whether break-in is enabled (`BI`, manual p.5).
    pub async fn get_break_in_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("BI").await?;
        let body = parse_frame(&raw, "BI")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable break-in (`BI`).
    pub async fn set_break_in_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("BI", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query the CW (semi) break-in delay time, ms (`SD`, manual p.16;
    /// `30`-`3000`).
    pub async fn get_semi_break_in_delay(&mut self) -> RadioResult<u16> {
        let raw = self.client.query("SD").await?;
        let body = parse_frame(&raw, "SD")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the CW (semi) break-in delay time, ms (`SD`). Returns
    /// [`RadioError::InvalidBreakInDelay`] if `ms` is outside `30..=3000`.
    pub async fn set_semi_break_in_delay(&mut self, ms: u16) -> RadioResult<()> {
        if !(30..=3000).contains(&ms) {
            return Err(RadioError::InvalidBreakInDelay(ms));
        }
        self.client
            .set("SD", &format!("{ms:04}"))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 5: scan/VOX/busy (SC VX VD VG BY)
    // -----------------------------------------------------------------------

    /// Query the scan state (`SC`, manual p.16; 3-valued, see [`ScanState`]).
    pub async fn get_scan_state(&mut self) -> RadioResult<ScanState> {
        let raw = self.client.query("SC").await?;
        let body = parse_frame(&raw, "SC")?;
        let digit: u8 = body
            .parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))?;
        ScanState::try_from(digit)
    }

    /// Set the scan state (`SC`).
    pub async fn set_scan_state(&mut self, state: ScanState) -> RadioResult<()> {
        self.client
            .set("SC", &state.as_u8().to_string())
            .await
            .map_err(Into::into)
    }

    /// Query whether VOX is enabled (`VX`, manual p.18).
    pub async fn get_vox_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("VX").await?;
        let body = parse_frame(&raw, "VX")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable VOX (`VX`).
    pub async fn set_vox_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("VX", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query the VOX gain, `0`-`100` (`VG`, manual p.18).
    pub async fn get_vox_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("VG").await?;
        let body = parse_frame(&raw, "VG")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the VOX gain (`VG`). Returns [`RadioError::InvalidVoxGain`] if
    /// `gain` is outside `0..=100`.
    pub async fn set_vox_gain(&mut self, gain: u8) -> RadioResult<()> {
        if gain > 100 {
            return Err(RadioError::InvalidVoxGain(gain));
        }
        self.client
            .set("VG", &format!("{gain:03}"))
            .await
            .map_err(Into::into)
    }

    /// Query the VOX delay time, ms (`VD`, manual p.17; `30`-`3000`, 10 ms
    /// steps).
    ///
    /// **Doc-noted dependency on `EX` menu item 142 "VOX SELECT," which
    /// this crate does not implement** — see
    /// [`crate::Radio::get_vox_delay`]'s doc comment and `ft991a_radio.rs`'s
    /// module docs' "VD, the batch's highest-risk item" section for the
    /// full manual citation. This method returns the emulator's single
    /// shared `vox_delay_ms` value regardless of which physical setting
    /// (MIC VOX delay or DATA VOX delay) it would represent on real
    /// hardware.
    pub async fn get_vox_delay(&mut self) -> RadioResult<u16> {
        let raw = self.client.query("VD").await?;
        let body = parse_frame(&raw, "VD")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the VOX delay time, ms (`VD`). See [`Self::get_vox_delay`]'s doc
    /// comment for the `EX` menu 142 dependency this crate does not
    /// resolve. Returns [`RadioError::InvalidVoxDelay`] if `ms` is outside
    /// `30..=3000` or not a multiple of 10.
    pub async fn set_vox_delay(&mut self, ms: u16) -> RadioResult<()> {
        if !(30..=3000).contains(&ms) || ms % 10 != 0 {
            return Err(RadioError::InvalidVoxDelay(ms));
        }
        self.client
            .set("VD", &format!("{ms:04}"))
            .await
            .map_err(Into::into)
    }

    /// Query whether the receiver is busy (`BY`, manual p.5; read-only).
    /// This emulator has no simulated received-signal/squelch-open
    /// condition, so this always reports `false` — a documented
    /// simplification, not a manual-specified default (see
    /// `ft991a_radio.rs`'s module docs' "BY" section).
    pub async fn get_rx_busy(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("BY").await?;
        let body = parse_frame(&raw, "BY")?;
        match body.get(0..1) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    // -----------------------------------------------------------------------
    // Batch 6: attenuator/preamp/noise/AGC/notch/filter-width
    // (RA PA NB NL NR RL GT CO BP BC NA SH)
    // -----------------------------------------------------------------------

    /// Query whether the RF attenuator is on (`RA`, manual p.15; selector
    /// read, `RA0;`).
    pub async fn get_attenuator_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("RA", "0").await?;
        let body = parse_frame(&raw, "RA")?;
        match body.get(1..2) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the RF attenuator (`RA`).
    pub async fn set_attenuator_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("RA", &format!("0{}", u8::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the pre-amp/IPO mode (`PA`, manual p.14; selector read,
    /// `PA0;`). See [`PreampMode`].
    pub async fn get_preamp_mode(&mut self) -> RadioResult<PreampMode> {
        let raw = self.client.query_with_param("PA", "0").await?;
        let body = parse_frame(&raw, "PA")?;
        let digit = body
            .get(1..2)
            .and_then(|s| s.parse::<u8>().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        PreampMode::try_from(digit)
    }

    /// Set the pre-amp/IPO mode (`PA`).
    pub async fn set_preamp_mode(&mut self, mode: PreampMode) -> RadioResult<()> {
        self.client
            .set("PA", &format!("0{}", mode.as_u8()))
            .await
            .map_err(Into::into)
    }

    /// Query whether the noise blanker is on (`NB`, manual p.13; selector
    /// read, `NB0;`).
    pub async fn get_noise_blanker_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("NB", "0").await?;
        let body = parse_frame(&raw, "NB")?;
        match body.get(1..2) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the noise blanker (`NB`).
    pub async fn set_noise_blanker_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("NB", &format!("0{}", u8::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the noise blanker level, `0`-`10` (`NL`, manual p.13; selector
    /// read, `NL0;`).
    pub async fn get_noise_blanker_level(&mut self) -> RadioResult<u8> {
        let raw = self.client.query_with_param("NL", "0").await?;
        let body = parse_frame(&raw, "NL")?;
        body.get(1..4)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the noise blanker level (`NL`). Returns
    /// [`RadioError::InvalidNoiseBlankerLevel`] if `level` is outside
    /// `0..=10`.
    pub async fn set_noise_blanker_level(&mut self, level: u8) -> RadioResult<()> {
        if level > 10 {
            return Err(RadioError::InvalidNoiseBlankerLevel(level));
        }
        self.client
            .set("NL", &format!("0{level:03}"))
            .await
            .map_err(Into::into)
    }

    /// Query whether noise reduction is on (`NR`, manual p.13; selector
    /// read, `NR0;`).
    pub async fn get_noise_reduction_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("NR", "0").await?;
        let body = parse_frame(&raw, "NR")?;
        match body.get(1..2) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable noise reduction (`NR`).
    pub async fn set_noise_reduction_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("NR", &format!("0{}", u8::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the noise reduction level, `1`-`15` (`RL`, manual p.15;
    /// selector read, `RL0;`).
    pub async fn get_noise_reduction_level(&mut self) -> RadioResult<u8> {
        let raw = self.client.query_with_param("RL", "0").await?;
        let body = parse_frame(&raw, "RL")?;
        body.get(1..3)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the noise reduction level (`RL`). Returns
    /// [`RadioError::InvalidNoiseReductionLevel`] if `level` is outside
    /// `1..=15`.
    pub async fn set_noise_reduction_level(&mut self, level: u8) -> RadioResult<()> {
        if !(1..=15).contains(&level) {
            return Err(RadioError::InvalidNoiseReductionLevel(level));
        }
        self.client
            .set("RL", &format!("0{level:02}"))
            .await
            .map_err(Into::into)
    }

    /// Query the AGC mode (`GT`, manual p.10; selector read, `GT0;`). See
    /// [`AgcMode`]'s doc comment for the write/report domain mismatch.
    pub async fn get_agc_mode(&mut self) -> RadioResult<AgcMode> {
        let raw = self.client.query_with_param("GT", "0").await?;
        let body = parse_frame(&raw, "GT")?;
        let digit = body
            .get(1..2)
            .and_then(|s| s.parse::<u8>().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        AgcMode::try_from(digit)
    }

    /// Set the AGC mode (`GT`). See [`AgcMode::set_wire_value`] for how
    /// `AutoMid`/`AutoSlow` collapse onto the same wire value `AutoFast`
    /// uses.
    pub async fn set_agc_mode(&mut self, mode: AgcMode) -> RadioResult<()> {
        self.client
            .set("GT", &format!("0{}", mode.set_wire_value()))
            .await
            .map_err(Into::into)
    }

    /// Query whether CONTOUR is on (`CO` `P2=0`, manual p.5). `Ft991a`-
    /// inherent-only — see `ft991a_radio.rs`'s module docs' "CO" section
    /// for why this isn't on the `Radio` trait.
    pub async fn get_contour_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("CO", "00").await?;
        let body = parse_frame(&raw, "CO")?;
        match body.get(2..6).and_then(|s| s.parse::<u16>().ok()) {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable CONTOUR (`CO` `P2=0`).
    pub async fn set_contour_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("CO", &format!("00{:04}", u16::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the CONTOUR frequency, Hz, `10`-`3200` (`CO` `P2=1`, manual
    /// p.5).
    pub async fn get_contour_frequency_hz(&mut self) -> RadioResult<u16> {
        let raw = self.client.query_with_param("CO", "01").await?;
        let body = parse_frame(&raw, "CO")?;
        body.get(2..6)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the CONTOUR frequency (`CO` `P2=1`). Returns
    /// [`RadioError::InvalidContourFrequency`] if `hz` is outside
    /// `10..=3200`.
    pub async fn set_contour_frequency_hz(&mut self, hz: u16) -> RadioResult<()> {
        if !(10..=3200).contains(&hz) {
            return Err(RadioError::InvalidContourFrequency(hz));
        }
        self.client
            .set("CO", &format!("01{hz:04}"))
            .await
            .map_err(Into::into)
    }

    /// Query whether APF is on (`CO` `P2=2`, manual p.5).
    pub async fn get_apf_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("CO", "02").await?;
        let body = parse_frame(&raw, "CO")?;
        match body.get(2..6).and_then(|s| s.parse::<u16>().ok()) {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable APF (`CO` `P2=2`).
    pub async fn set_apf_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("CO", &format!("02{:04}", u16::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the APF frequency, Hz, `-250`..=`250` in 10 Hz steps (`CO`
    /// `P2=3`, manual p.5). See [`apf_raw_to_hz`] for the raw wire-value
    /// mapping.
    pub async fn get_apf_frequency_hz(&mut self) -> RadioResult<i16> {
        let raw = self.client.query_with_param("CO", "03").await?;
        let body = parse_frame(&raw, "CO")?;
        let raw_val: u8 = body
            .get(2..6)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        Ok(apf_raw_to_hz(raw_val))
    }

    /// Set the APF frequency (`CO` `P2=3`). Returns
    /// [`RadioError::InvalidApfFrequency`] if `hz` is outside `-250..=250`
    /// or not a multiple of 10.
    pub async fn set_apf_frequency_hz(&mut self, hz: i16) -> RadioResult<()> {
        if !(-250..=250).contains(&hz) || hz % 10 != 0 {
            return Err(RadioError::InvalidApfFrequency(hz));
        }
        let raw_val = apf_hz_to_raw(hz);
        self.client
            .set("CO", &format!("03{raw_val:04}"))
            .await
            .map_err(Into::into)
    }

    /// Query whether the manual notch is on (`BP` `P2=0`, manual p.5).
    /// `Ft991a`-inherent-only — see `ft991a_radio.rs`'s module docs' "BP"
    /// section for why this isn't on the `Radio` trait.
    pub async fn get_manual_notch_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("BP", "00").await?;
        let body = parse_frame(&raw, "BP")?;
        match body.get(2..5).and_then(|s| s.parse::<u16>().ok()) {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the manual notch (`BP` `P2=0`).
    pub async fn set_manual_notch_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("BP", &format!("00{:03}", u16::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the manual notch frequency, Hz, `10`-`3200` in 10 Hz steps
    /// (`BP` `P2=1`, manual p.5; "NOTCH Frequency: x 10 Hz").
    pub async fn get_manual_notch_frequency_hz(&mut self) -> RadioResult<u16> {
        let raw = self.client.query_with_param("BP", "01").await?;
        let body = parse_frame(&raw, "BP")?;
        let raw_val: u16 = body
            .get(2..5)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        Ok(raw_val * 10)
    }

    /// Set the manual notch frequency (`BP` `P2=1`). Returns
    /// [`RadioError::InvalidManualNotchFrequency`] if `hz` is outside
    /// `10..=3200` or not a multiple of 10.
    pub async fn set_manual_notch_frequency_hz(&mut self, hz: u16) -> RadioResult<()> {
        if !(10..=3200).contains(&hz) || hz % 10 != 0 {
            return Err(RadioError::InvalidManualNotchFrequency(hz));
        }
        self.client
            .set("BP", &format!("01{:03}", hz / 10))
            .await
            .map_err(Into::into)
    }

    /// Query whether the auto notch filter is on (`BC`, manual p.4;
    /// selector read, `BC0;`).
    pub async fn get_auto_notch_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("BC", "0").await?;
        let body = parse_frame(&raw, "BC")?;
        match body.get(1..2) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the auto notch filter (`BC`).
    pub async fn set_auto_notch_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("BC", &format!("0{}", u8::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query whether the narrow filter is on (`NA`, manual p.13; selector
    /// read, `NA0;`). See `ft991a_radio.rs`'s module docs' "NA, a genuine
    /// manual wire-diagram typo" section for why the wire code is `NA`.
    pub async fn get_narrow_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("NA", "0").await?;
        let body = parse_frame(&raw, "NA")?;
        match body.get(1..2) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the narrow filter (`NA`).
    pub async fn set_narrow_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("NA", &format!("0{}", u8::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the raw filter width table index, `0`-`21` (`SH`, manual p.16;
    /// selector read, `SH0;`). See [`crate::ft991a_radio::filter_bandwidth_hz`]
    /// for resolving this index to an actual Hz value.
    pub async fn get_filter_width_index(&mut self) -> RadioResult<u8> {
        let raw = self.client.query_with_param("SH", "0").await?;
        let body = parse_frame(&raw, "SH")?;
        body.get(1..3)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the raw filter width table index (`SH`). Returns
    /// [`RadioError::InvalidFilterWidthIndex`] if `index` is outside
    /// `0..=21`.
    pub async fn set_filter_width_index(&mut self, index: u8) -> RadioResult<()> {
        if index > 21 {
            return Err(RadioError::InvalidFilterWidthIndex(index));
        }
        self.client
            .set("SH", &format!("0{index:02}"))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Mic gain
    // -----------------------------------------------------------------------

    /// Query the microphone gain (`MG`, manual p.11; zero-width read,
    /// `000`-`100`, no selector byte — same shape as `PC`/`PL`).
    pub async fn get_mic_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("MG").await?;
        let body = parse_frame(&raw, "MG")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the microphone gain (`MG`). Returns
    /// [`RadioError::InvalidMicGain`] if `level` is outside `0..=100`.
    pub async fn set_mic_gain(&mut self, level: u8) -> RadioResult<()> {
        if level > 100 {
            return Err(RadioError::InvalidMicGain(level));
        }
        self.client
            .set("MG", &format!("{level:03}"))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Speech processor level (PL)
    // -----------------------------------------------------------------------

    /// Query the speech processor level (`PL`, manual p.14; zero-width
    /// read, `000`-`100`).
    pub async fn get_speech_processor_level(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("PL").await?;
        let body = parse_frame(&raw, "PL")?;
        body.parse()
            .map_err(|_| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the speech processor level (`PL`). Returns
    /// [`RadioError::InvalidSpeechProcessorLevel`] if `level` is outside
    /// `0..=100`.
    pub async fn set_speech_processor_level(&mut self, level: u8) -> RadioResult<()> {
        if level > 100 {
            return Err(RadioError::InvalidSpeechProcessorLevel(level));
        }
        self.client
            .set("PL", &format!("{level:03}"))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Speech processor / Parametric Mic EQ on-off (PR)
    // -----------------------------------------------------------------------

    /// Query whether the speech processor is on (`PR` with `P1=0`, manual
    /// p.14; selector read, `PR0;`). See `ft991a_radio.rs`'s module docs'
    /// "PR, a genuine manual heading typo" section — this is an on/off
    /// toggle, not a level, despite the per-command box's own heading.
    pub async fn get_speech_processor_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("PR", "0").await?;
        parse_pr_answer(&raw, '0')
    }

    /// Enable/disable the speech processor (`PR` with `P1=0`). **`P2`'s
    /// wire encoding is `1`=OFF, `2`=ON** (not the usual `0`/`1` — manual
    /// p.14, transcribed exactly).
    pub async fn set_speech_processor_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("PR", &format!("0{}", if on { 2 } else { 1 }))
            .await
            .map_err(Into::into)
    }

    /// Query whether the Parametric Microphone Equalizer is on (`PR` with
    /// `P1=1`, manual p.14). `Ft991a`-inherent only — no generic-concept
    /// precedent in `CLAUDE.md`'s "Radio trait scope" or `ts570d::Radio`,
    /// same treatment batch 6 gave `CO`/`BP`.
    pub async fn get_parametric_mic_eq_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("PR", "1").await?;
        parse_pr_answer(&raw, '1')
    }

    /// Enable/disable the Parametric Microphone Equalizer (`PR` with
    /// `P1=1`). Same `1`=OFF/`2`=ON encoding as
    /// [`Self::set_speech_processor_on`].
    pub async fn set_parametric_mic_eq_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("PR", &format!("1{}", if on { 2 } else { 1 }))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Monitor on-off / level (ML)
    // -----------------------------------------------------------------------

    /// Query whether the audio monitor is on (`ML` with `P1=0`, manual
    /// p.12; selector read, `ML0;`).
    pub async fn get_monitor_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query_with_param("ML", "0").await?;
        let body = parse_frame(&raw, "ML")?;
        match parse_ml_body(body, '0') {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the audio monitor (`ML` with `P1=0`).
    pub async fn set_monitor_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("ML", &format!("0{:03}", u8::from(on)))
            .await
            .map_err(Into::into)
    }

    /// Query the audio monitor level (`ML` with `P1=1`, manual p.12;
    /// selector read, `ML1;`; `000`-`100`).
    pub async fn get_monitor_level(&mut self) -> RadioResult<u8> {
        let raw = self.client.query_with_param("ML", "1").await?;
        let body = parse_frame(&raw, "ML")?;
        parse_ml_body(body, '1')
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the audio monitor level (`ML` with `P1=1`). Returns
    /// [`RadioError::InvalidMonitorLevel`] if `level` is outside `0..=100`.
    pub async fn set_monitor_level(&mut self, level: u8) -> RadioResult<()> {
        if level > 100 {
            return Err(RadioError::InvalidMonitorLevel(level));
        }
        self.client
            .set("ML", &format!("1{level:03}"))
            .await
            .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 8: band/step/encoder front-panel controls (BS BU BD FS ED EU EK
    // DN UP)
    // -----------------------------------------------------------------------

    /// Select a band (`BS`, manual p.5; write-only, 2-digit band code — no
    /// `Read`/`Answer` form exists, so there is no paired `get_band`).
    pub async fn set_band(&mut self, band: Band) -> RadioResult<()> {
        self.client
            .set("BS", &format!("{:02}", band.as_u8()))
            .await
            .map_err(Into::into)
    }

    /// Step to the next band up (`BU0`, manual p.4). Wraps past the highest
    /// band back to the lowest, skipping the documented gap at wire value
    /// `13`.
    pub async fn band_up(&mut self) -> RadioResult<()> {
        self.client.set("BU", "0").await.map_err(Into::into)
    }

    /// Step to the next band down (`BD0`, manual p.4). Wraps past the
    /// lowest band back to the highest, skipping the gap.
    pub async fn band_down(&mut self) -> RadioResult<()> {
        self.client.set("BD", "0").await.map_err(Into::into)
    }

    /// Query whether the VFO-A "FAST" step key is on (`FS`, manual p.9).
    pub async fn get_fine_step(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("FS").await?;
        let body = parse_frame(&raw, "FS")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the VFO-A "FAST" step key (`FS`).
    pub async fn set_fine_step(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("FS", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Step the specified front-panel encoder down (`ED`, manual p.7).
    /// `Ft991a`-inherent only — FT-991A-specific concept, no
    /// `ts570d::Radio` precedent (see `ft991a_radio.rs`'s module docs'
    /// "ED/EU" section). Returns [`RadioError::InvalidEncoderSteps`] if
    /// `steps` is outside `1..=99`.
    pub async fn encoder_down(&mut self, encoder: EncoderSelector, steps: u8) -> RadioResult<()> {
        if !(1..=99).contains(&steps) {
            return Err(RadioError::InvalidEncoderSteps(steps));
        }
        self.client
            .set("ED", &format!("{}{:02}", encoder.as_wire_digit(), steps))
            .await
            .map_err(Into::into)
    }

    /// Step the specified front-panel encoder up (`EU`, manual p.7). Same
    /// shape/validation as [`Self::encoder_down`].
    pub async fn encoder_up(&mut self, encoder: EncoderSelector, steps: u8) -> RadioResult<()> {
        if !(1..=99).contains(&steps) {
            return Err(RadioError::InvalidEncoderSteps(steps));
        }
        self.client
            .set("EU", &format!("{}{:02}", encoder.as_wire_digit(), steps))
            .await
            .map_err(Into::into)
    }

    /// Emulate a press of the front-panel ENT key (`EK`, manual p.7;
    /// zero-width Action trigger). `Ft991a`-inherent only — see
    /// `ft991a_radio.rs`'s module docs' "EK" section.
    pub async fn ent_key(&mut self) -> RadioResult<()> {
        self.client.set("EK", "").await.map_err(Into::into)
    }

    /// Emulate a press of the hand mic's "UP" button (`UP`, manual p.17).
    /// See `ft991a_radio.rs`'s module docs' "DN/UP" section for the full
    /// cross-radio corroboration behind resolving `UP` to this concept.
    pub async fn mic_up(&mut self) -> RadioResult<()> {
        self.client.set("UP", "").await.map_err(Into::into)
    }

    /// Emulate a press of the hand mic's "DWN" button (`DN`, manual p.6 —
    /// own per-command box heading "MIC DWN," despite the master table's
    /// plain "DOWN"). See `ft991a_radio.rs`'s module docs' "DN/UP" section.
    pub async fn mic_down(&mut self) -> RadioResult<()> {
        self.client.set("DN", "").await.map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS
    // -----------------------------------------------------------------------

    /// Query the antenna tuner state (`AC`, manual p.4; `0`=OFF, `1`=ON,
    /// `2`=Tuning Start/Stop). `Ft991a`-inherent only — see
    /// `ft991a_radio.rs`'s module docs' "AC" section (antenna tuner is
    /// explicitly excluded from the `Radio` trait by both this repo's and
    /// `ts570d`'s own `CLAUDE.md`).
    pub async fn get_antenna_tuner_state(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("AC").await?;
        let body = parse_frame(&raw, "AC")?;
        body.get(2..3)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))
    }

    /// Set the antenna tuner state (`AC`). `state` must be `0`-`2`.
    pub async fn set_antenna_tuner_state(&mut self, state: u8) -> RadioResult<()> {
        if state > 2 {
            return Err(RadioError::InvalidAntennaTunerState(state));
        }
        self.client
            .set("AC", &format!("00{state}"))
            .await
            .map_err(Into::into)
    }

    /// Query auto-information broadcast on/off (`AI`, manual p.4).
    pub async fn get_auto_info_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("AI").await?;
        let body = parse_frame(&raw, "AI")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable auto-information broadcast (`AI`). The manual's own
    /// note that this resets to `false` when the transceiver powers off is
    /// **not** enforced by this crate — see `ft991a_radio.rs`'s module docs'
    /// "AI" section.
    pub async fn set_auto_info_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("AI", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query the dimmer levels (`DA`, manual p.6): `(led_brightness,
    /// tft_brightness)` — LED `1`-`2`, TFT `0`-`15`. `Ft991a`-inherent only.
    pub async fn get_dimmer(&mut self) -> RadioResult<(u8, u8)> {
        let raw = self.client.query("DA").await?;
        let body = parse_frame(&raw, "DA")?;
        let led: Option<u8> = body.get(2..4).and_then(|s| s.parse().ok());
        let tft: Option<u8> = body.get(4..6).and_then(|s| s.parse().ok());
        match (led, tft) {
            (Some(l), Some(t)) => Ok((l, t)),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Set the dimmer levels (`DA`). `led` must be `1`-`2`, `tft` must be
    /// `0`-`15`.
    pub async fn set_dimmer(&mut self, led: u8, tft: u8) -> RadioResult<()> {
        if !(1..=2).contains(&led) || tft > 15 {
            return Err(RadioError::InvalidDimmerLevel { led, tft });
        }
        self.client
            .set("DA", &format!("00{led:02}{tft:02}"))
            .await
            .map_err(Into::into)
    }

    /// Read the current date (`DT` P1=0, manual p.6): `(year, month, day)`.
    /// `Ft991a`-inherent only — see `ft991a_radio.rs`'s module docs' "DT"
    /// section.
    pub async fn read_date(&mut self) -> RadioResult<(u16, u8, u8)> {
        let raw = self.client.query_with_param("DT", "0").await?;
        let body = parse_frame(&raw, "DT")?;
        let year: Option<u16> = body.get(1..5).and_then(|s| s.parse().ok());
        let month: Option<u8> = body.get(5..7).and_then(|s| s.parse().ok());
        let day: Option<u8> = body.get(7..9).and_then(|s| s.parse().ok());
        match (year, month, day) {
            (Some(y), Some(m), Some(d)) => Ok((y, m, d)),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Write the date (`DT` P1=0). `month` must be `1`-`12`, `day` must be
    /// `1`-`31` (no leap-year/month-length calendar validation — see module
    /// docs).
    pub async fn write_date(&mut self, year: u16, month: u8, day: u8) -> RadioResult<()> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(RadioError::InvalidDate { year, month, day });
        }
        self.client
            .set("DT", &format!("0{year:04}{month:02}{day:02}"))
            .await
            .map_err(Into::into)
    }

    /// Read the current time (`DT` P1=1, manual p.6): `(hour, minute,
    /// second)`, 24-hour, UTC.
    pub async fn read_time(&mut self) -> RadioResult<(u8, u8, u8)> {
        let raw = self.client.query_with_param("DT", "1").await?;
        let body = parse_frame(&raw, "DT")?;
        let hour: Option<u8> = body.get(1..3).and_then(|s| s.parse().ok());
        let minute: Option<u8> = body.get(3..5).and_then(|s| s.parse().ok());
        let second: Option<u8> = body.get(5..7).and_then(|s| s.parse().ok());
        match (hour, minute, second) {
            (Some(h), Some(mi), Some(se)) => Ok((h, mi, se)),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Write the time (`DT` P1=1). `hour` must be `0`-`23`, `minute`/
    /// `second` must be `0`-`59`.
    pub async fn write_time(&mut self, hour: u8, minute: u8, second: u8) -> RadioResult<()> {
        if hour > 23 || minute > 59 || second > 59 {
            return Err(RadioError::InvalidTime {
                hour,
                minute,
                second,
            });
        }
        self.client
            .set("DT", &format!("1{hour:02}{minute:02}{second:02}"))
            .await
            .map_err(Into::into)
    }

    /// Read the current time zone (time differential) offset in minutes
    /// (`DT` P1=2, manual p.6; `-720..=840`, 30-minute steps).
    pub async fn read_time_zone_offset(&mut self) -> RadioResult<i16> {
        let raw = self.client.query_with_param("DT", "2").await?;
        let body = parse_frame(&raw, "DT")?;
        let sign = body.get(1..2);
        let hh: Option<i16> = body.get(2..4).and_then(|s| s.parse().ok());
        let mm: Option<i16> = body.get(4..6).and_then(|s| s.parse().ok());
        match (sign, hh, mm) {
            (Some(s), Some(h), Some(m)) if matches!(s, "+" | "-") => {
                let total = h * 60 + m;
                Ok(if s == "-" { -total } else { total })
            }
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Write the time zone offset (`DT` P1=2). `minutes` must be within
    /// `-720..=840` in 30-minute steps.
    pub async fn write_time_zone_offset(&mut self, minutes: i16) -> RadioResult<()> {
        if !(-720..=840).contains(&minutes) || minutes % 30 != 0 {
            return Err(RadioError::InvalidTimeZoneOffset(minutes));
        }
        let sign = if minutes < 0 { '-' } else { '+' };
        let mag = minutes.unsigned_abs();
        self.client
            .set("DT", &format!("2{sign}{:02}{:02}", mag / 60, mag % 60))
            .await
            .map_err(Into::into)
    }

    /// Query the VFO-A dial lock state (`LK`, manual p.11).
    pub async fn get_frequency_lock(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("LK").await?;
        let body = parse_frame(&raw, "LK")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable the VFO-A dial lock (`LK`).
    pub async fn set_frequency_lock(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("LK", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query the composite opposite-band (VFO-B) status payload (`OI`,
    /// manual p.13; read-only). Shares [`ChannelStatusFields`]'s shape with
    /// [`Self::get_information`] — see `ft991a_radio.rs`'s module docs'
    /// "OI" section for the documented judgment call that all fields but
    /// the frequency reuse the same shared state `IF` reports.
    pub async fn get_opposite_band_information(&mut self) -> RadioResult<ChannelStatusFields> {
        let raw = self.client.query("OI").await?;
        let body = parse_frame(&raw, "OI")?;
        ChannelStatusFields::parse(body).ok_or(RadioError::InvalidProtocolString(raw))
    }

    /// Query the FM repeater shift direction (`OS`, manual p.13).
    pub async fn get_repeater_shift(&mut self) -> RadioResult<RepeaterShift> {
        let raw = self.client.query_with_param("OS", "0").await?;
        let body = parse_frame(&raw, "OS")?;
        let digit: u8 = body
            .get(1..2)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        RepeaterShift::try_from(digit)
    }

    /// Set the FM repeater shift direction (`OS`).
    pub async fn set_repeater_shift(&mut self, shift: RepeaterShift) -> RadioResult<()> {
        self.client
            .set("OS", &format!("0{}", shift.as_u8()))
            .await
            .map_err(Into::into)
    }

    /// Query which VFO/band is the TX band (`FT`, manual p.9); `0`=VFO-A,
    /// `1`=VFO-B (the Answer domain — see `ft991a_radio.rs`'s module docs'
    /// "FT" section for the write/report domain mismatch this translates).
    pub async fn get_tx_vfo(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("FT").await?;
        let body = parse_frame(&raw, "FT")?;
        match body {
            "0" => Ok(0),
            "1" => Ok(1),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Select which VFO/band is the TX band (`FT`). `vfo` must be `0` or
    /// `1` — translated to `FT`'s own `Set`-domain wire values (`2`/`3`)
    /// internally.
    pub async fn set_tx_vfo(&mut self, vfo: u8) -> RadioResult<()> {
        let wire = match vfo {
            0 => "2",
            1 => "3",
            _ => return Err(RadioError::InvalidTxVfo(vfo)),
        };
        self.client.set("FT", wire).await.map_err(Into::into)
    }

    /// Query the "TXW" on/off state (`TS`, manual p.17 — see
    /// `ft991a_radio.rs`'s module docs' "TS" section for why this is not
    /// modeled as "tuning step"). `Ft991a`-inherent only.
    pub async fn get_txw_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("TS").await?;
        let body = parse_frame(&raw, "TS")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable "TXW" (`TS`).
    pub async fn set_txw_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("TS", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query MOX (manual transmitter-key) on/off state (`MX`, manual p.13).
    pub async fn get_mox_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("MX").await?;
        let body = parse_frame(&raw, "MX")?;
        match body {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(RadioError::InvalidProtocolString(raw.clone())),
        }
    }

    /// Enable/disable MOX (`MX`).
    pub async fn set_mox_on(&mut self, on: bool) -> RadioResult<()> {
        self.client
            .set("MX", if on { "1" } else { "0" })
            .await
            .map_err(Into::into)
    }

    /// Query the DVS recording state (`LM`, manual p.11): `None` = stopped,
    /// `Some(channel)` = actively recording that channel (`1`-`5`).
    /// `Ft991a`-inherent only — see `ft991a_radio.rs`'s module docs'
    /// "LM/PB" section.
    pub async fn get_dvs_recording_channel(&mut self) -> RadioResult<Option<u8>> {
        let raw = self.client.query_with_param("LM", "0").await?;
        let body = parse_frame(&raw, "LM")?;
        let v: u8 = body
            .get(1..2)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        Ok(if v == 0 { None } else { Some(v) })
    }

    /// Start (or toggle-stop, if already recording `channel`) DVS recording
    /// on the given channel (`LM`, `1`-`5`). See module docs' "LM/PB"
    /// section for the toggle semantics.
    pub async fn start_dvs_recording(&mut self, channel: u8) -> RadioResult<()> {
        if !(1..=5).contains(&channel) {
            return Err(RadioError::InvalidDvsChannel(channel));
        }
        self.client
            .set("LM", &format!("0{channel}"))
            .await
            .map_err(Into::into)
    }

    /// Stop DVS recording (`LM` with `P2=0`).
    pub async fn stop_dvs_recording(&mut self) -> RadioResult<()> {
        self.client.set("LM", "00").await.map_err(Into::into)
    }

    /// Query the DVS playback state (`PB`, manual p.14): `None` = stopped,
    /// `Some(channel)` = actively playing that channel (`1`-`5`).
    /// `Ft991a`-inherent only.
    pub async fn get_dvs_playback_channel(&mut self) -> RadioResult<Option<u8>> {
        let raw = self.client.query_with_param("PB", "0").await?;
        let body = parse_frame(&raw, "PB")?;
        let v: u8 = body
            .get(1..2)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| RadioError::InvalidProtocolString(raw.clone()))?;
        Ok(if v == 0 { None } else { Some(v) })
    }

    /// Start DVS playback on the given channel (`PB`, `1`-`5`;
    /// unconditional, not a toggle — see module docs' "LM/PB" section).
    pub async fn start_dvs_playback(&mut self, channel: u8) -> RadioResult<()> {
        if !(1..=5).contains(&channel) {
            return Err(RadioError::InvalidDvsChannel(channel));
        }
        self.client
            .set("PB", &format!("0{channel}"))
            .await
            .map_err(Into::into)
    }

    /// Stop DVS playback (`PB` with `P2=0`).
    pub async fn stop_dvs_playback(&mut self) -> RadioResult<()> {
        self.client.set("PB", "00").await.map_err(Into::into)
    }

    /// Flush the session's receive buffer, discarding unsolicited or stale
    /// data.
    pub fn flush_rx(&mut self) {
        let mut session = self.session.take();
        session.flush_rx();
        self.session.put_back(session);
    }
}

// ---------------------------------------------------------------------------
// Modem control lines (RTS/DTR/CTS/DSR/DCD) — additive only
// ---------------------------------------------------------------------------

/// RS-232 modem control/status line access, independent of CAT byte
/// framing. Per `planning/architect/task_plan.md` §10.2-10.4: this is what
/// backs `EX` menu item 060 "PC KEYING" (already CAT-implemented) when the
/// radio is configured to watch RTS or DTR for real-time CW keying instead
/// of any CAT command.
///
/// This is a **separate, additive** impl block, bounded on `S:
/// CatSession<Error = TransportError> + ModemControlLines` — it does NOT
/// touch or narrow the main `impl<S: CatSession<Error = TransportError>>
/// Ft991a<S>` block above, nor the `Radio` trait or its impl below. Mock/
/// fake test sessions that implement only `CatSession` (not
/// `ModemControlLines`) are unaffected and keep working exactly as before;
/// these methods simply aren't reachable on `Ft991a<S>` for such an `S`.
/// The concrete type satisfying both bounds is decided entirely at the
/// wiring layer (`app/src/main.rs`), per `CLAUDE.md` rule 5 — see this
/// crate's own findings.md for confirmation that today's single-`SerialPort`
/// wiring already satisfies this bound with zero `main.rs` changes.
impl<S> Ft991a<S>
where
    S: CatSession<Error = TransportError> + ModemControlLines,
{
    /// Assert or clear RTS.
    ///
    /// Usable for real-time CW keying when `EX` menu item 060 "PC KEYING"
    /// is set to `2: RTS` (manual p.8) — asserting/clearing this line then
    /// keys the CW element and PTT directly, without any CAT command.
    /// RTS is present on the RS-232C 9-pin CAT connector, pin 7 (manual
    /// p.1) — reachable over the same serial connection this app already
    /// opens.
    pub fn assert_rts(&self, asserted: bool) -> RadioResult<()> {
        self.session.set_rts(asserted).map_err(Into::into)
    }

    /// Assert or clear DTR.
    ///
    /// Usable when `EX` menu item 060 "PC KEYING" is set to `3: DTR`.
    /// **Not present on the RS-232C 9-pin CAT connector** — confirmed
    /// against the manual's own pinout table (p.1, Figure 1): only RTS
    /// (pin 7) and CTS (pin 8) are wired on that connector alongside the
    /// two data lines and ground; there is no DTR or DSR/DCD pin at all.
    /// DTR is only reachable via a USB Dual-UART bridge connection, which
    /// this repo does not yet support as a modem-control path (see
    /// `planning/architect/task_plan.md` §10.4's "USB dual-port case"
    /// deferral) — calling this against today's single-`SerialPort` wiring
    /// will reach `SerialPort::set_dtr`, but the resulting DTR line has no
    /// physical connection to the radio over RS-232C.
    pub fn assert_dtr(&self, asserted: bool) -> RadioResult<()> {
        self.session.set_dtr(asserted).map_err(Into::into)
    }

    /// Read the current CTS (Clear To Send) status line state.
    ///
    /// Present on the RS-232C CAT connector, pin 8 (manual p.1).
    pub fn read_cts(&self) -> RadioResult<bool> {
        self.session.read_cts().map_err(Into::into)
    }

    /// Read the current DSR (Data Set Ready) status line state.
    ///
    /// **Not present on the RS-232C 9-pin CAT connector** — same caveat as
    /// [`Self::assert_dtr`]: only reachable via a USB connection, not this
    /// repo's current RS-232C serial wiring. Exposed for completeness and
    /// future transports.
    pub fn read_dsr(&self) -> RadioResult<bool> {
        self.session.read_dsr().map_err(Into::into)
    }

    /// Read the current DCD (Data Carrier Detect) status line state.
    ///
    /// **Not present on the RS-232C 9-pin CAT connector** — same caveat as
    /// [`Self::assert_dtr`]: only reachable via a USB connection, not this
    /// repo's current RS-232C serial wiring. Exposed for completeness and
    /// future transports.
    pub fn read_dcd(&self) -> RadioResult<bool> {
        self.session.read_dcd().map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Radio trait implementation
// ---------------------------------------------------------------------------

#[async_trait::async_trait(?Send)]
impl<S> crate::Radio for Ft991a<S>
where
    S: CatSession<Error = TransportError>,
{
    async fn get_vfo_a(&mut self) -> crate::RadioResult<crate::Frequency> {
        Ft991a::get_vfo_a(self).await
    }

    async fn set_vfo_a(&mut self, freq: crate::Frequency) -> crate::RadioResult<()> {
        Ft991a::set_vfo_a(self, freq).await
    }

    async fn get_vfo_b(&mut self) -> crate::RadioResult<crate::Frequency> {
        Ft991a::get_vfo_b(self).await
    }

    async fn set_vfo_b(&mut self, freq: crate::Frequency) -> crate::RadioResult<()> {
        Ft991a::set_vfo_b(self, freq).await
    }

    async fn get_mode(&mut self) -> crate::RadioResult<crate::Mode> {
        Ft991a::get_mode(self).await
    }

    async fn set_mode(&mut self, mode: crate::Mode) -> crate::RadioResult<()> {
        Ft991a::set_mode(self, mode).await
    }

    async fn transmit(&mut self) -> crate::RadioResult<()> {
        Ft991a::transmit(self).await
    }

    async fn receive(&mut self) -> crate::RadioResult<()> {
        Ft991a::receive(self).await
    }

    async fn get_tx_state(&mut self) -> crate::RadioResult<crate::TxState> {
        Ft991a::get_tx_state(self).await
    }

    async fn get_smeter(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_smeter(self).await
    }

    async fn select_meter(&mut self, meter: crate::Meter) -> crate::RadioResult<()> {
        Ft991a::select_meter(self, meter).await
    }

    async fn get_selected_meter(&mut self) -> crate::RadioResult<crate::Meter> {
        Ft991a::get_selected_meter(self).await
    }

    async fn get_meter(&mut self, meter: crate::Meter) -> crate::RadioResult<u8> {
        Ft991a::get_meter(self, meter).await
    }

    async fn get_power_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_power_on(self).await
    }

    async fn set_power_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_power_on(self, on).await
    }

    async fn get_af_gain(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_af_gain(self).await
    }

    async fn set_af_gain(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_af_gain(self, level).await
    }

    async fn get_rf_gain(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_rf_gain(self).await
    }

    async fn set_rf_gain(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_rf_gain(self, level).await
    }

    async fn get_squelch(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_squelch(self).await
    }

    async fn set_squelch(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_squelch(self, level).await
    }

    async fn get_power(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_power(self).await
    }

    async fn set_power(&mut self, watts: u8) -> crate::RadioResult<()> {
        Ft991a::set_power(self, watts).await
    }

    async fn get_id(&mut self) -> crate::RadioResult<String> {
        Ft991a::get_id(self).await
    }

    async fn get_memory_channel(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_memory_channel(self).await
    }

    async fn set_memory_channel(&mut self, ch: u8) -> crate::RadioResult<()> {
        Ft991a::set_memory_channel(self, ch).await
    }

    async fn read_memory_channel(&mut self, ch: u8) -> crate::RadioResult<MemoryChannelEntry> {
        Ft991a::read_memory_channel(self, ch).await
    }

    async fn write_memory_channel(&mut self, entry: MemoryChannelEntry) -> crate::RadioResult<()> {
        Ft991a::write_memory_channel(self, entry).await
    }

    async fn read_memory_channel_tag(&mut self, ch: u8) -> crate::RadioResult<TaggedMemoryChannel> {
        Ft991a::read_memory_channel_tag(self, ch).await
    }

    async fn write_memory_channel_tag(
        &mut self,
        channel: TaggedMemoryChannel,
    ) -> crate::RadioResult<()> {
        Ft991a::write_memory_channel_tag(self, channel).await
    }

    async fn copy_vfo_a_to_b(&mut self) -> crate::RadioResult<()> {
        Ft991a::copy_vfo_a_to_b(self).await
    }

    async fn copy_vfo_b_to_a(&mut self) -> crate::RadioResult<()> {
        Ft991a::copy_vfo_b_to_a(self).await
    }

    async fn swap_vfos(&mut self) -> crate::RadioResult<()> {
        Ft991a::swap_vfos(self).await
    }

    async fn store_vfo_to_memory(&mut self) -> crate::RadioResult<()> {
        Ft991a::store_vfo_to_memory(self).await
    }

    async fn recall_memory_to_vfo(&mut self) -> crate::RadioResult<()> {
        Ft991a::recall_memory_to_vfo(self).await
    }

    async fn memory_channel_up(&mut self) -> crate::RadioResult<()> {
        Ft991a::memory_channel_up(self).await
    }

    async fn memory_channel_down(&mut self) -> crate::RadioResult<()> {
        Ft991a::memory_channel_down(self).await
    }

    async fn get_rx_clarifier_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_rx_clarifier_on(self).await
    }

    async fn set_rx_clarifier_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_rx_clarifier_on(self, on).await
    }

    async fn get_tx_clarifier_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_tx_clarifier_on(self).await
    }

    async fn set_tx_clarifier_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_tx_clarifier_on(self, on).await
    }

    async fn clarifier_clear(&mut self) -> crate::RadioResult<()> {
        Ft991a::clarifier_clear(self).await
    }

    async fn clarifier_down(&mut self, offset_hz: u16) -> crate::RadioResult<()> {
        Ft991a::clarifier_down(self, offset_hz).await
    }

    async fn clarifier_up(&mut self, offset_hz: u16) -> crate::RadioResult<()> {
        Ft991a::clarifier_up(self, offset_hz).await
    }

    async fn get_if_shift_hz(&mut self) -> crate::RadioResult<i16> {
        Ft991a::get_if_shift_hz(self).await
    }

    async fn set_if_shift_hz(&mut self, hz: i16) -> crate::RadioResult<()> {
        Ft991a::set_if_shift_hz(self, hz).await
    }

    async fn get_tone_squelch_mode(&mut self) -> crate::RadioResult<ToneSquelchMode> {
        Ft991a::get_tone_squelch_mode(self).await
    }

    async fn set_tone_squelch_mode(&mut self, mode: ToneSquelchMode) -> crate::RadioResult<()> {
        Ft991a::set_tone_squelch_mode(self, mode).await
    }

    async fn get_ctcss_tone_hz(&mut self) -> crate::RadioResult<f32> {
        Ft991a::get_ctcss_tone_hz(self).await
    }

    async fn set_ctcss_tone_hz(&mut self, hz: f32) -> crate::RadioResult<()> {
        Ft991a::set_ctcss_tone_hz(self, hz).await
    }

    async fn get_dcs_code(&mut self) -> crate::RadioResult<u16> {
        Ft991a::get_dcs_code(self).await
    }

    async fn set_dcs_code(&mut self, code: u16) -> crate::RadioResult<()> {
        Ft991a::set_dcs_code(self, code).await
    }

    async fn get_break_in_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_break_in_on(self).await
    }

    async fn set_break_in_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_break_in_on(self, on).await
    }

    async fn get_semi_break_in_delay(&mut self) -> crate::RadioResult<u16> {
        Ft991a::get_semi_break_in_delay(self).await
    }

    async fn set_semi_break_in_delay(&mut self, ms: u16) -> crate::RadioResult<()> {
        Ft991a::set_semi_break_in_delay(self, ms).await
    }

    async fn get_cw_spot_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_cw_spot_on(self).await
    }

    async fn set_cw_spot_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_cw_spot_on(self, on).await
    }

    async fn get_keyer_enabled(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_keyer_enabled(self).await
    }

    async fn set_keyer_enabled(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_keyer_enabled(self, on).await
    }

    async fn get_keyer_speed(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_keyer_speed(self).await
    }

    async fn set_keyer_speed(&mut self, wpm: u8) -> crate::RadioResult<()> {
        Ft991a::set_keyer_speed(self, wpm).await
    }

    async fn get_keyer_pitch_hz(&mut self) -> crate::RadioResult<u16> {
        Ft991a::get_keyer_pitch_hz(self).await
    }

    async fn set_keyer_pitch_hz(&mut self, hz: u16) -> crate::RadioResult<()> {
        Ft991a::set_keyer_pitch_hz(self, hz).await
    }

    async fn zero_in(&mut self) -> crate::RadioResult<()> {
        Ft991a::zero_in(self).await
    }

    async fn get_scan_state(&mut self) -> crate::RadioResult<ScanState> {
        Ft991a::get_scan_state(self).await
    }

    async fn set_scan_state(&mut self, state: ScanState) -> crate::RadioResult<()> {
        Ft991a::set_scan_state(self, state).await
    }

    async fn get_vox_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_vox_on(self).await
    }

    async fn set_vox_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_vox_on(self, on).await
    }

    async fn get_vox_gain(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_vox_gain(self).await
    }

    async fn set_vox_gain(&mut self, gain: u8) -> crate::RadioResult<()> {
        Ft991a::set_vox_gain(self, gain).await
    }

    async fn get_vox_delay(&mut self) -> crate::RadioResult<u16> {
        Ft991a::get_vox_delay(self).await
    }

    async fn set_vox_delay(&mut self, ms: u16) -> crate::RadioResult<()> {
        Ft991a::set_vox_delay(self, ms).await
    }

    async fn get_rx_busy(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_rx_busy(self).await
    }

    async fn get_attenuator_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_attenuator_on(self).await
    }

    async fn set_attenuator_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_attenuator_on(self, on).await
    }

    async fn get_preamp_mode(&mut self) -> crate::RadioResult<PreampMode> {
        Ft991a::get_preamp_mode(self).await
    }

    async fn set_preamp_mode(&mut self, mode: PreampMode) -> crate::RadioResult<()> {
        Ft991a::set_preamp_mode(self, mode).await
    }

    async fn get_noise_blanker_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_noise_blanker_on(self).await
    }

    async fn set_noise_blanker_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_noise_blanker_on(self, on).await
    }

    async fn get_noise_blanker_level(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_noise_blanker_level(self).await
    }

    async fn set_noise_blanker_level(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_noise_blanker_level(self, level).await
    }

    async fn get_noise_reduction_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_noise_reduction_on(self).await
    }

    async fn set_noise_reduction_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_noise_reduction_on(self, on).await
    }

    async fn get_noise_reduction_level(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_noise_reduction_level(self).await
    }

    async fn set_noise_reduction_level(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_noise_reduction_level(self, level).await
    }

    async fn get_agc_mode(&mut self) -> crate::RadioResult<AgcMode> {
        Ft991a::get_agc_mode(self).await
    }

    async fn set_agc_mode(&mut self, mode: AgcMode) -> crate::RadioResult<()> {
        Ft991a::set_agc_mode(self, mode).await
    }

    async fn get_auto_notch_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_auto_notch_on(self).await
    }

    async fn set_auto_notch_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_auto_notch_on(self, on).await
    }

    async fn get_narrow_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_narrow_on(self).await
    }

    async fn set_narrow_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_narrow_on(self, on).await
    }

    async fn get_filter_width_index(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_filter_width_index(self).await
    }

    async fn set_filter_width_index(&mut self, index: u8) -> crate::RadioResult<()> {
        Ft991a::set_filter_width_index(self, index).await
    }

    async fn get_mic_gain(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_mic_gain(self).await
    }

    async fn set_mic_gain(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_mic_gain(self, level).await
    }

    async fn get_speech_processor_level(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_speech_processor_level(self).await
    }

    async fn set_speech_processor_level(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_speech_processor_level(self, level).await
    }

    async fn get_speech_processor_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_speech_processor_on(self).await
    }

    async fn set_speech_processor_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_speech_processor_on(self, on).await
    }

    async fn get_monitor_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_monitor_on(self).await
    }

    async fn set_monitor_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_monitor_on(self, on).await
    }

    async fn get_monitor_level(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_monitor_level(self).await
    }

    async fn set_monitor_level(&mut self, level: u8) -> crate::RadioResult<()> {
        Ft991a::set_monitor_level(self, level).await
    }

    async fn set_band(&mut self, band: crate::Band) -> crate::RadioResult<()> {
        Ft991a::set_band(self, band).await
    }

    async fn band_up(&mut self) -> crate::RadioResult<()> {
        Ft991a::band_up(self).await
    }

    async fn band_down(&mut self) -> crate::RadioResult<()> {
        Ft991a::band_down(self).await
    }

    async fn get_fine_step(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_fine_step(self).await
    }

    async fn set_fine_step(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_fine_step(self, on).await
    }

    async fn mic_up(&mut self) -> crate::RadioResult<()> {
        Ft991a::mic_up(self).await
    }

    async fn mic_down(&mut self) -> crate::RadioResult<()> {
        Ft991a::mic_down(self).await
    }

    async fn get_auto_info_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_auto_info_on(self).await
    }

    async fn set_auto_info_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_auto_info_on(self, on).await
    }

    async fn get_frequency_lock(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_frequency_lock(self).await
    }

    async fn set_frequency_lock(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_frequency_lock(self, on).await
    }

    async fn get_repeater_shift(&mut self) -> crate::RadioResult<crate::RepeaterShift> {
        Ft991a::get_repeater_shift(self).await
    }

    async fn set_repeater_shift(&mut self, shift: crate::RepeaterShift) -> crate::RadioResult<()> {
        Ft991a::set_repeater_shift(self, shift).await
    }

    async fn get_tx_vfo(&mut self) -> crate::RadioResult<u8> {
        Ft991a::get_tx_vfo(self).await
    }

    async fn set_tx_vfo(&mut self, vfo: u8) -> crate::RadioResult<()> {
        Ft991a::set_tx_vfo(self, vfo).await
    }

    async fn get_mox_on(&mut self) -> crate::RadioResult<bool> {
        Ft991a::get_mox_on(self).await
    }

    async fn set_mox_on(&mut self, on: bool) -> crate::RadioResult<()> {
        Ft991a::set_mox_on(self, on).await
    }

    fn flush_rx(&mut self) {
        Ft991a::flush_rx(self)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use cat_transport_core::Transport;
    use cat_transport_serial::SerialCatSession;
    use std::cell::Cell;
    use std::collections::VecDeque;

    // -----------------------------------------------------------------------
    // In-memory fake transport (mirrors ts570d/radio/src/ts570d.rs's own
    // test module — wire-framing-level tests, exercising the real
    // `cat-transport-serial::SerialCatSession` read-until-`;` framing
    // against a local fake `Transport`, never a production transport).
    //
    // Also implements `ModemControlLines` (via `Cell`s for interior
    // mutability, since the trait's methods take `&self`) — mirrors
    // `cat-transport-serial::session`'s own test-module `FakeTransport`
    // exactly (`radio-cat-rs/cat-transport-serial/src/session.rs`), the
    // canonical fake for this trait. Because `SerialCatSession<T: Transport
    // + ModemControlLines>: ModemControlLines` already exists as a blanket
    // impl in `cat-transport-serial`, giving this fake `ModemControlLines`
    // is enough to make `Ft991a<SerialCatSession<FakeTransport>>` satisfy
    // this crate's own new `S: CatSession<Error = TransportError> +
    // ModemControlLines` bound — no separate test double/session type is
    // needed.
    // -----------------------------------------------------------------------

    struct FakeTransport {
        writes: Vec<u8>,
        reads: VecDeque<u8>,
        last_set_rts: Cell<Option<bool>>,
        last_set_dtr: Cell<Option<bool>>,
        cts: Cell<bool>,
        dsr: Cell<bool>,
        dcd: Cell<bool>,
    }

    impl FakeTransport {
        fn new() -> Self {
            Self {
                writes: Vec::new(),
                reads: VecDeque::new(),
                last_set_rts: Cell::new(None),
                last_set_dtr: Cell::new(None),
                cts: Cell::new(false),
                dsr: Cell::new(false),
                dcd: Cell::new(false),
            }
        }

        fn enqueue_response(&mut self, response: &str) {
            self.reads.extend(response.as_bytes());
        }

        fn written(&self) -> &[u8] {
            &self.writes
        }

        fn written_str(&self) -> &str {
            std::str::from_utf8(&self.writes).expect("non-UTF-8 in writes")
        }
    }

    #[async_trait(?Send)]
    impl Transport for FakeTransport {
        async fn write(&mut self, data: &[u8]) -> Result<usize, TransportError> {
            self.writes.extend_from_slice(data);
            Ok(data.len())
        }

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
            if let Some(byte) = self.reads.pop_front() {
                buf[0] = byte;
                Ok(1)
            } else {
                Ok(0)
            }
        }

        async fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    impl ModemControlLines for FakeTransport {
        fn set_rts(&self, asserted: bool) -> Result<(), TransportError> {
            self.last_set_rts.set(Some(asserted));
            Ok(())
        }

        fn set_dtr(&self, asserted: bool) -> Result<(), TransportError> {
            self.last_set_dtr.set(Some(asserted));
            Ok(())
        }

        fn read_cts(&self) -> Result<bool, TransportError> {
            Ok(self.cts.get())
        }

        fn read_dsr(&self) -> Result<bool, TransportError> {
            Ok(self.dsr.get())
        }

        fn read_dcd(&self) -> Result<bool, TransportError> {
            Ok(self.dcd.get())
        }
    }

    fn make_radio(response: &str) -> Ft991a<SerialCatSession<FakeTransport>> {
        let mut transport = FakeTransport::new();
        transport.enqueue_response(response);
        Ft991a::new(SerialCatSession::new(transport))
    }

    // -----------------------------------------------------------------------
    // VFO A
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_a_query_sent() {
        let mut radio = make_radio("FA000014250000;");
        let _ = radio.get_vfo_a().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"FA;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_a_frequency_parsed_9_digits() {
        let mut radio = make_radio("FA014250000;");
        let freq = radio.get_vfo_a().await.unwrap();
        assert_eq!(freq, Frequency::new(14_250_000).unwrap());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vfo_a_command_formatted_9_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let freq = Frequency::new(14_250_000).unwrap();
        radio.set_vfo_a(freq).await.unwrap();
        assert_eq!(
            radio.session.borrow().transport.written_str(),
            "FA014250000;"
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vfo_a_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let freq = Frequency::new(30_000).unwrap();
        radio.set_vfo_a(freq).await.unwrap();
        assert_eq!(
            radio.session.borrow().transport.written_str(),
            "FA000030000;"
        );
    }

    // -----------------------------------------------------------------------
    // Mode (selector read)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_mode_sends_selector_zero() {
        let mut radio = make_radio("MD02;");
        let _ = radio.get_mode().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"MD0;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_mode_parses_hex_digit() {
        let mut radio = make_radio("MD0E;");
        let mode = radio.get_mode().await.unwrap();
        assert_eq!(mode, Mode::C4fm);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_mode_formats_selector_and_hex_char() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_mode(Mode::DataFm).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "MD0A;");
    }

    // -----------------------------------------------------------------------
    // TX / PTT (3-valued read)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_transmit_sends_tx1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.transmit().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "TX1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_receive_sends_tx0() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.receive().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "TX0;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_tx_state_parses_answer_only_value_two() {
        let mut radio = make_radio("TX2;");
        let state = radio.get_tx_state().await.unwrap();
        assert_eq!(state, TxState::RadioKeyedNonCat);
    }

    // -----------------------------------------------------------------------
    // AG/RG/SQ — corrected (non-selector-read) shape
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_af_gain_sends_zero_width_query_not_selector() {
        let mut radio = make_radio("AG0200;");
        let level = radio.get_af_gain().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"AG;");
        assert_eq!(level, 200);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_af_gain_bakes_in_selector() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_af_gain(200).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "AG0200;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_squelch_sends_zero_width_query() {
        let mut radio = make_radio("SQ0050;");
        let level = radio.get_squelch().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"SQ;");
        assert_eq!(level, 50);
    }

    // -----------------------------------------------------------------------
    // ID (read-only)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_id_returns_fixed_value() {
        let mut radio = make_radio("ID0670;");
        let id = radio.get_id().await.unwrap();
        assert_eq!(id, "0670");
    }

    // -----------------------------------------------------------------------
    // IF (composite status, read-only)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_information_sends_zero_width_query() {
        let mut radio = make_radio("IF000014000000+000000200000;");
        let _ = radio.get_information().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"IF;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_information_parses_composite_fields() {
        let mut radio = make_radio("IF042014250000+120010332001;");
        let info = radio.get_information().await.unwrap();
        assert_eq!(info.channel, 42);
        assert_eq!(info.frequency_hz, 14_250_000);
        assert_eq!(info.clarifier_offset_hz, 1200);
        assert!(info.rx_clarifier_on);
        assert!(!info.tx_clarifier_on);
        assert_eq!(info.mode, 0x3);
        assert_eq!(info.select, 3);
        assert_eq!(info.tone_status, 2);
        assert_eq!(info.offset_type, 1);
    }

    // -----------------------------------------------------------------------
    // Meters (MS, RM)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_select_meter_formats_ms_selector() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.select_meter(Meter::Swr).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "MS3;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_selected_meter_parses_ms_answer() {
        let mut radio = make_radio("MS2;");
        let meter = radio.get_selected_meter().await.unwrap();
        assert_eq!(meter, Meter::Po);
        assert_eq!(radio.session.borrow().transport.written(), b"MS;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_meter_sends_direct_select_offset_by_three() {
        // Meter::Swr as_u8() == 3, RM's direct-select value is 3+3 == 6.
        let mut radio = make_radio("RM6033;");
        let level = radio.get_meter(Meter::Swr).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"RM6;");
        assert_eq!(level, 33);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_meter_rejects_mismatched_echoed_selector() {
        // Radio echoed selector 5 (PO) when 6 (SWR) was requested.
        let mut radio = make_radio("RM5033;");
        assert!(radio.get_meter(Meter::Swr).await.is_err());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_active_meter_reading_sends_selector_zero() {
        let mut radio = make_radio("RM0111;");
        let level = radio.get_active_meter_reading().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"RM0;");
        assert_eq!(level, 111);
    }

    // -----------------------------------------------------------------------
    // RI (radio indicator, selector read)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_radio_indicator_formats_hex_selector() {
        let mut radio = make_radio("RIA1;");
        let on = radio
            .get_radio_indicator(RadioIndicator::TxLed)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"RIA;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_radio_indicator_reports_off() {
        let mut radio = make_radio("RI00;");
        let on = radio
            .get_radio_indicator(RadioIndicator::HiSwr)
            .await
            .unwrap();
        assert!(!on);
    }

    // -----------------------------------------------------------------------
    // RS (radio status, read-only)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_menu_mode_active_parses_rs_answer() {
        let mut radio = make_radio("RS1;");
        let menu_mode = radio.get_menu_mode_active().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"RS;");
        assert!(menu_mode);
    }

    // -----------------------------------------------------------------------
    // UL (PLL unlock status, read-only)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_pll_unlocked_parses_ul_answer() {
        let mut radio = make_radio("UL0;");
        let unlocked = radio.get_pll_unlocked().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"UL;");
        assert!(!unlocked);
    }

    // -----------------------------------------------------------------------
    // Memory channel select (MC)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_memory_channel_parses_mc_answer() {
        let mut radio = make_radio("MC042;");
        let ch = radio.get_memory_channel().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"MC;");
        assert_eq!(ch, 42);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_memory_channel_formats_three_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_memory_channel(7).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"MC007;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_memory_channel_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_memory_channel(0).await.unwrap_err(),
            RadioError::InvalidMemoryChannel(0)
        ));
        assert!(matches!(
            radio.set_memory_channel(118).await.unwrap_err(),
            RadioError::InvalidMemoryChannel(118)
        ));
        // Validated before touching the wire — nothing sent.
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    // -----------------------------------------------------------------------
    // Memory channel read/write (MR, MW)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_read_memory_channel_parses_mr_answer() {
        let mut radio = make_radio("MR042014250000+120010332001;");
        let entry = radio.read_memory_channel(42).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"MR042;");
        assert_eq!(entry.channel, 42);
        assert_eq!(entry.frequency_hz, 14_250_000);
        assert_eq!(entry.clarifier_offset_hz, 1200);
        assert!(entry.rx_clarifier_on);
        assert!(!entry.tx_clarifier_on);
        assert_eq!(entry.mode, Mode::CwU);
        assert_eq!(entry.tone_status, 2);
        assert_eq!(entry.offset_type, 1);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_read_memory_channel_rejects_out_of_range_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.read_memory_channel(0).await.unwrap_err(),
            RadioError::InvalidMemoryChannel(0)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_memory_channel_formats_mw_body() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let entry = MemoryChannelEntry {
            channel: 5,
            frequency_hz: 7_100_000,
            clarifier_offset_hz: -250,
            rx_clarifier_on: true,
            tx_clarifier_on: false,
            mode: Mode::Fm,
            tone_status: 2,
            offset_type: 1,
        };
        radio.write_memory_channel(entry).await.unwrap();
        let expected_fields = ChannelStatusFields {
            channel: 5,
            frequency_hz: 7_100_000,
            clarifier_offset_hz: -250,
            rx_clarifier_on: true,
            tx_clarifier_on: false,
            mode: Mode::Fm.as_u8(),
            select: 0,
            tone_status: 2,
            offset_type: 1,
        };
        assert_eq!(
            radio.session.borrow().transport.written_str(),
            format!("MW{};", expected_fields.to_wire_string())
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_memory_channel_rejects_out_of_range_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let entry = MemoryChannelEntry {
            channel: 0,
            frequency_hz: 14_000_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: Mode::Usb,
            tone_status: 0,
            offset_type: 0,
        };
        assert!(matches!(
            radio.write_memory_channel(entry).await.unwrap_err(),
            RadioError::InvalidMemoryChannel(0)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    // -----------------------------------------------------------------------
    // Memory channel write/tag (MT)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_read_memory_channel_tag_parses_mt_answer_including_tag() {
        let fields = ChannelStatusFields {
            channel: 42,
            frequency_hz: 14_250_000,
            clarifier_offset_hz: 1200,
            rx_clarifier_on: true,
            tx_clarifier_on: false,
            mode: 0x3,
            select: 1, // radio always reports Memory for MT's answer
            tone_status: 2,
            offset_type: 1,
        };
        let response = format!("MT{}0{:<12};", fields.to_wire_string(), "REPEATER 1");
        let mut radio = make_radio(&response);
        let tagged = radio.read_memory_channel_tag(42).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"MT042;");
        assert_eq!(tagged.entry.channel, 42);
        assert_eq!(tagged.entry.frequency_hz, 14_250_000);
        assert_eq!(tagged.entry.mode, Mode::CwU);
        assert_eq!(tagged.tag.as_str(), "REPEATER 1");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_read_memory_channel_tag_rejects_out_of_range_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.read_memory_channel_tag(118).await.unwrap_err(),
            RadioError::InvalidMemoryChannel(118)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_memory_channel_tag_formats_mt_body_with_padded_tag() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let entry = MemoryChannelEntry {
            channel: 10,
            frequency_hz: 3_573_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: Mode::Usb,
            tone_status: 0,
            offset_type: 0,
        };
        let tag = MemoryTag::new("N0CALL").unwrap();
        radio
            .write_memory_channel_tag(TaggedMemoryChannel { entry, tag })
            .await
            .unwrap();
        let expected_fields = ChannelStatusFields {
            channel: 10,
            frequency_hz: 3_573_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: Mode::Usb.as_u8(),
            select: 0,
            tone_status: 0,
            offset_type: 0,
        };
        assert_eq!(
            radio.session.borrow().transport.written_str(),
            format!("MT{}0{:<12};", expected_fields.to_wire_string(), "N0CALL")
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_memory_channel_tag_rejects_out_of_range_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let entry = MemoryChannelEntry {
            channel: 0,
            frequency_hz: 14_000_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: Mode::Usb,
            tone_status: 0,
            offset_type: 0,
        };
        let tag = MemoryTag::new("BAD").unwrap();
        assert!(matches!(
            radio
                .write_memory_channel_tag(TaggedMemoryChannel { entry, tag })
                .await
                .unwrap_err(),
            RadioError::InvalidMemoryChannel(0)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    // -----------------------------------------------------------------------
    // SharedSession / flush_rx
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_flush_rx_does_not_panic() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.flush_rx();
    }

    // -----------------------------------------------------------------------
    // Batch 1: VFO/split/memory quick-ops (AB, BA, AM, VM, MA, CH, QI, QR,
    // QS, SV) — all zero-width Action triggers, `CH` excepted (1-digit
    // selector).
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_copy_vfo_a_to_b_sends_bare_ab() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.copy_vfo_a_to_b().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "AB;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_copy_vfo_b_to_a_sends_bare_ba() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.copy_vfo_b_to_a().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BA;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_swap_vfos_sends_bare_sv() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.swap_vfos().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "SV;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_store_vfo_to_memory_sends_bare_am() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.store_vfo_to_memory().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "AM;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_toggle_vfo_memory_mode_sends_bare_vm() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.toggle_vfo_memory_mode().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "VM;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_recall_memory_to_vfo_sends_bare_ma() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.recall_memory_to_vfo().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "MA;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_memory_channel_up_sends_ch0() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.memory_channel_up().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CH0;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_memory_channel_down_sends_ch1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.memory_channel_down().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CH1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_qmb_store_sends_bare_qi() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.qmb_store().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "QI;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_qmb_recall_sends_bare_qr() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.qmb_recall().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "QR;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_quick_split_sends_bare_qs() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.quick_split().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "QS;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_radio_trait_delegates_batch_one_quick_ops() {
        use crate::Radio;

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::copy_vfo_a_to_b(&mut radio).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "AB;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::memory_channel_down(&mut radio).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CH1;");
    }

    // -----------------------------------------------------------------------
    // Batch 3: clarifier/RIT-XIT (RT, RC, RD, RU, XT)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_rx_clarifier_on_parses_rt_answer() {
        let mut radio = make_radio("RT1;");
        let on = radio.get_rx_clarifier_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"RT;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_rx_clarifier_on_sends_rt1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_rx_clarifier_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RT1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_rx_clarifier_off_sends_rt0() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_rx_clarifier_on(false).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RT0;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_tx_clarifier_on_parses_xt_answer() {
        let mut radio = make_radio("XT0;");
        let on = radio.get_tx_clarifier_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"XT;");
        assert!(!on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_tx_clarifier_on_sends_xt1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_tx_clarifier_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "XT1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_clarifier_clear_sends_bare_rc() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.clarifier_clear().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RC;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_clarifier_down_formats_four_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.clarifier_down(300).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RD0300;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_clarifier_up_formats_four_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.clarifier_up(1200).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RU1200;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_clarifier_down_rejects_above_9999() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(radio.clarifier_down(10_000).await.is_err());
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_clarifier_up_rejects_above_9999() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(radio.clarifier_up(10_000).await.is_err());
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    // -----------------------------------------------------------------------
    // Batch 3: IF-shift (IS)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_if_shift_hz_sends_selector_and_parses_positive() {
        let mut radio = make_radio("IS0+1000;");
        let hz = radio.get_if_shift_hz().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"IS0;");
        assert_eq!(hz, 1000);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_if_shift_hz_parses_negative() {
        let mut radio = make_radio("IS0-1200;");
        let hz = radio.get_if_shift_hz().await.unwrap();
        assert_eq!(hz, -1200);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_if_shift_hz_formats_sign_and_four_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_if_shift_hz(1000).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "IS0+1000;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_if_shift_hz_negative_formats_minus_sign() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_if_shift_hz(-1200).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "IS0-1200;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_if_shift_hz_rejects_above_1200() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_if_shift_hz(1220).await.unwrap_err(),
            RadioError::InvalidIfShift(1220)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_if_shift_hz_rejects_non_multiple_of_twenty() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_if_shift_hz(10).await.unwrap_err(),
            RadioError::InvalidIfShift(10)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    // -----------------------------------------------------------------------
    // Batch 3: tone squelch mode + CTCSS/DCS value (CT, CN)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_tone_squelch_mode_sends_selector_zero() {
        let mut radio = make_radio("CT03;");
        let mode = radio.get_tone_squelch_mode().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"CT0;");
        assert_eq!(mode, ToneSquelchMode::DcsEncDec);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_tone_squelch_mode_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio
            .set_tone_squelch_mode(ToneSquelchMode::CtcssEnc)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CT02;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_ctcss_tone_hz_sends_table_selector_and_parses_first_entry() {
        let mut radio = make_radio("CN00000;");
        let hz = radio.get_ctcss_tone_hz().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"CN00;");
        assert_eq!(hz, 67.0);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_ctcss_tone_hz_parses_last_entry() {
        let mut radio = make_radio("CN00049;");
        let hz = radio.get_ctcss_tone_hz().await.unwrap();
        assert_eq!(hz, 254.1);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_ctcss_tone_hz_formats_table_index() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_ctcss_tone_hz(100.0).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CN00012;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_ctcss_tone_hz_rejects_unlisted_frequency() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_ctcss_tone_hz(100.5).await.unwrap_err(),
            RadioError::InvalidCtcssTone(_)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_dcs_code_sends_table_selector_and_parses_first_entry() {
        let mut radio = make_radio("CN01000;");
        let code = radio.get_dcs_code().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"CN01;");
        assert_eq!(code, 23);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_dcs_code_parses_last_entry() {
        let mut radio = make_radio("CN01103;");
        let code = radio.get_dcs_code().await.unwrap();
        assert_eq!(code, 754);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_dcs_code_formats_table_index() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_dcs_code(754).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CN01103;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_dcs_code_rejects_unlisted_code() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_dcs_code(999).await.unwrap_err(),
            RadioError::InvalidDcsCode(999)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_radio_trait_delegates_batch_three_clarifier_and_tone() {
        use crate::Radio;

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_rx_clarifier_on(&mut radio, true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RT1;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::clarifier_down(&mut radio, 500).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RD0500;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_dcs_code(&mut radio, 23).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CN01000;");
    }

    // -----------------------------------------------------------------------
    // Batch 4: keyer/CW/break-in (KM, KP, KR, KS, KY, CS, ZI, BI, SD)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_read_keyer_memory_sends_channel_selector_and_parses_message() {
        let mut radio = make_radio("KM3CQ DE N0CALL;");
        let message = radio.read_keyer_memory(3).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"KM3;");
        assert_eq!(message, "CQ DE N0CALL");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_read_keyer_memory_rejects_out_of_range_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.read_keyer_memory(0).await.unwrap_err(),
            RadioError::InvalidKeyerMemoryChannel(0)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_keyer_memory_formats_channel_and_message() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.write_keyer_memory(1, "CQ CQ CQ").await.unwrap();
        assert_eq!(
            radio.session.borrow().transport.written_str(),
            "KM1CQ CQ CQ;"
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_keyer_memory_rejects_out_of_range_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.write_keyer_memory(6, "TEST").await.unwrap_err(),
            RadioError::InvalidKeyerMemoryChannel(6)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_keyer_memory_rejects_empty_message() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.write_keyer_memory(1, "").await.unwrap_err(),
            RadioError::InvalidKeyerMessage(_)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_keyer_memory_rejects_message_over_fifty_chars() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let too_long = "A".repeat(51);
        assert!(matches!(
            radio.write_keyer_memory(1, &too_long).await.unwrap_err(),
            RadioError::InvalidKeyerMessage(_)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_keyer_memory_accepts_exactly_fifty_chars() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        let exactly_fifty = "A".repeat(50);
        radio.write_keyer_memory(1, &exactly_fifty).await.unwrap();
        assert_eq!(
            radio.session.borrow().transport.written_str(),
            format!("KM1{exactly_fifty};")
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_write_keyer_memory_rejects_control_character() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio
                .write_keyer_memory(1, "BAD\u{1}MSG")
                .await
                .unwrap_err(),
            RadioError::InvalidKeyerMessage(_)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_play_keyer_memory_formats_keyer_memory_family() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio
            .play_keyer_memory(3, KeyerPlaybackMode::KeyerMemory)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "KY3;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_play_keyer_memory_formats_message_keyer_family() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio
            .play_keyer_memory(1, KeyerPlaybackMode::MessageKeyer)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "KY6;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio
            .play_keyer_memory(5, KeyerPlaybackMode::MessageKeyer)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "KYA;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_play_keyer_memory_rejects_out_of_range_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio
                .play_keyer_memory(0, KeyerPlaybackMode::KeyerMemory)
                .await
                .unwrap_err(),
            RadioError::InvalidKeyerMemoryChannel(0)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_keyer_pitch_hz_converts_raw_value() {
        let mut radio = make_radio("KP20;");
        let hz = radio.get_keyer_pitch_hz().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"KP;");
        assert_eq!(hz, 500);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_keyer_pitch_hz_formats_raw_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_keyer_pitch_hz(1050).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "KP75;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_keyer_pitch_hz_rejects_non_step_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_keyer_pitch_hz(305).await.unwrap_err(),
            RadioError::InvalidKeyerPitch(305)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_keyer_enabled_parses_kr_answer() {
        let mut radio = make_radio("KR1;");
        let on = radio.get_keyer_enabled().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"KR;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_keyer_enabled_sends_kr1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_keyer_enabled(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "KR1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_keyer_speed_parses_three_digits() {
        let mut radio = make_radio("KS020;");
        let wpm = radio.get_keyer_speed().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"KS;");
        assert_eq!(wpm, 20);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_keyer_speed_formats_three_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_keyer_speed(60).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "KS060;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_keyer_speed_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_keyer_speed(3).await.unwrap_err(),
            RadioError::InvalidKeyerSpeed(3)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_cw_spot_on_parses_cs_answer() {
        let mut radio = make_radio("CS1;");
        let on = radio.get_cw_spot_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"CS;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_cw_spot_on_sends_cs1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_cw_spot_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CS1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_zero_in_sends_bare_zi() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.zero_in().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "ZI;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_break_in_on_parses_bi_answer() {
        let mut radio = make_radio("BI0;");
        let on = radio.get_break_in_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"BI;");
        assert!(!on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_break_in_on_sends_bi1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_break_in_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BI1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_semi_break_in_delay_parses_four_digits() {
        let mut radio = make_radio("SD0500;");
        let ms = radio.get_semi_break_in_delay().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"SD;");
        assert_eq!(ms, 500);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_semi_break_in_delay_formats_four_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_semi_break_in_delay(30).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "SD0030;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_semi_break_in_delay_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_semi_break_in_delay(3001).await.unwrap_err(),
            RadioError::InvalidBreakInDelay(3001)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_radio_trait_delegates_batch_four_generic_cw_methods() {
        use crate::Radio;

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_break_in_on(&mut radio, true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BI1;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_keyer_speed(&mut radio, 25).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "KS025;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::zero_in(&mut radio).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "ZI;");
    }

    // -----------------------------------------------------------------
    // Batch 5: scan/VOX/busy (SC VX VD VG BY).
    // -----------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_scan_state_parses_all_three_values() {
        let mut radio = make_radio("SC0;");
        assert_eq!(radio.get_scan_state().await.unwrap(), ScanState::Off);
        assert_eq!(radio.session.borrow().transport.written(), b"SC;");

        let mut radio = make_radio("SC1;");
        assert_eq!(radio.get_scan_state().await.unwrap(), ScanState::Up);

        let mut radio = make_radio("SC2;");
        assert_eq!(radio.get_scan_state().await.unwrap(), ScanState::Down);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_scan_state_sends_correct_digit() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_scan_state(ScanState::Down).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "SC2;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vox_on_parses_vx_answer() {
        let mut radio = make_radio("VX1;");
        let on = radio.get_vox_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"VX;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_on_sends_vx1() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_vox_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "VX1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vox_gain_parses_three_digits() {
        let mut radio = make_radio("VG050;");
        let gain = radio.get_vox_gain().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"VG;");
        assert_eq!(gain, 50);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_gain_formats_three_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_vox_gain(100).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "VG100;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_gain_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_vox_gain(101).await.unwrap_err(),
            RadioError::InvalidVoxGain(101)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vox_delay_parses_four_digits() {
        let mut radio = make_radio("VD0500;");
        let ms = radio.get_vox_delay().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"VD;");
        assert_eq!(ms, 500);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_delay_formats_four_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_vox_delay(30).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "VD0030;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_delay_rejects_out_of_range_and_non_step_values() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_vox_delay(20).await.unwrap_err(),
            RadioError::InvalidVoxDelay(20)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());

        assert!(matches!(
            radio.set_vox_delay(3001).await.unwrap_err(),
            RadioError::InvalidVoxDelay(3001)
        ));

        assert!(matches!(
            radio.set_vox_delay(35).await.unwrap_err(),
            RadioError::InvalidVoxDelay(35)
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_rx_busy_parses_by_answer() {
        let mut radio = make_radio("BY10;");
        let busy = radio.get_rx_busy().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"BY;");
        assert!(busy);

        let mut radio = make_radio("BY00;");
        let busy = radio.get_rx_busy().await.unwrap();
        assert!(!busy);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_radio_trait_delegates_batch_five_scan_vox_busy_methods() {
        use crate::Radio;

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_scan_state(&mut radio, ScanState::Up)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "SC1;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_vox_on(&mut radio, true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "VX1;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_vox_gain(&mut radio, 25).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "VG025;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_vox_delay(&mut radio, 100).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "VD0100;");

        let mut radio = make_radio("BY10;");
        assert!(Radio::get_rx_busy(&mut radio).await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"BY;");
    }

    // -----------------------------------------------------------------------
    // Batch 6: attenuator/preamp/noise/AGC/notch/filter-width
    // (RA PA NB NL NR RL GT CO BP BC NA SH)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_attenuator_on_sends_selector_zero() {
        let mut radio = make_radio("RA01;");
        let on = radio.get_attenuator_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"RA0;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_attenuator_on_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_attenuator_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RA01;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_preamp_mode_parses_all_three_values() {
        let mut radio = make_radio("PA00;");
        assert_eq!(radio.get_preamp_mode().await.unwrap(), PreampMode::Ipo);
        assert_eq!(radio.session.borrow().transport.written(), b"PA0;");

        let mut radio = make_radio("PA01;");
        assert_eq!(radio.get_preamp_mode().await.unwrap(), PreampMode::Amp1);

        let mut radio = make_radio("PA02;");
        assert_eq!(radio.get_preamp_mode().await.unwrap(), PreampMode::Amp2);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_preamp_mode_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_preamp_mode(PreampMode::Amp2).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PA02;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_noise_blanker_on_sends_selector_zero() {
        let mut radio = make_radio("NB01;");
        let on = radio.get_noise_blanker_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"NB0;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_blanker_on_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_noise_blanker_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "NB01;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_noise_blanker_level_parses_three_digits() {
        let mut radio = make_radio("NL0007;");
        let level = radio.get_noise_blanker_level().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"NL0;");
        assert_eq!(level, 7);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_blanker_level_formats_three_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_noise_blanker_level(10).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "NL0010;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_blanker_level_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_noise_blanker_level(11).await.unwrap_err(),
            RadioError::InvalidNoiseBlankerLevel(11)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_noise_reduction_on_sends_selector_zero() {
        let mut radio = make_radio("NR01;");
        let on = radio.get_noise_reduction_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"NR0;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_reduction_on_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_noise_reduction_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "NR01;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_noise_reduction_level_parses_two_digits() {
        let mut radio = make_radio("RL015;");
        let level = radio.get_noise_reduction_level().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"RL0;");
        assert_eq!(level, 15);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_reduction_level_formats_two_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_noise_reduction_level(1).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RL001;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_reduction_level_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_noise_reduction_level(0).await.unwrap_err(),
            RadioError::InvalidNoiseReductionLevel(0)
        ));
        assert!(matches!(
            radio.set_noise_reduction_level(16).await.unwrap_err(),
            RadioError::InvalidNoiseReductionLevel(16)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_agc_mode_parses_full_seven_valued_domain() {
        let mut radio = make_radio("GT04;");
        assert_eq!(radio.get_agc_mode().await.unwrap(), AgcMode::AutoFast);
        assert_eq!(radio.session.borrow().transport.written(), b"GT0;");

        let mut radio = make_radio("GT06;");
        assert_eq!(radio.get_agc_mode().await.unwrap(), AgcMode::AutoSlow);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_agc_mode_collapses_auto_variants_to_wire_value_four() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_agc_mode(AgcMode::AutoMid).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "GT04;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_agc_mode(AgcMode::Slow).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "GT03;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_contour_on_get_set_send_item_selector_zero() {
        let mut radio = make_radio("CO000001;");
        assert!(radio.get_contour_on().await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"CO00;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_contour_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CO000001;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_contour_frequency_get_set_send_item_selector_one() {
        let mut radio = make_radio("CO013200;");
        assert_eq!(radio.get_contour_frequency_hz().await.unwrap(), 3200);
        assert_eq!(radio.session.borrow().transport.written(), b"CO01;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_contour_frequency_hz(10).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CO010010;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_contour_frequency_hz_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_contour_frequency_hz(9).await.unwrap_err(),
            RadioError::InvalidContourFrequency(9)
        ));
        assert!(matches!(
            radio.set_contour_frequency_hz(3201).await.unwrap_err(),
            RadioError::InvalidContourFrequency(3201)
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_apf_on_get_set_send_item_selector_two() {
        let mut radio = make_radio("CO020001;");
        assert!(radio.get_apf_on().await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"CO02;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_apf_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CO020001;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_apf_frequency_get_set_round_trips_boundary_values() {
        let mut radio = make_radio("CO030000;");
        assert_eq!(radio.get_apf_frequency_hz().await.unwrap(), -250);
        assert_eq!(radio.session.borrow().transport.written(), b"CO03;");

        let mut radio = make_radio("CO030050;");
        assert_eq!(radio.get_apf_frequency_hz().await.unwrap(), 250);

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_apf_frequency_hz(-250).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CO030000;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_apf_frequency_hz(250).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "CO030050;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_apf_frequency_hz_rejects_out_of_range_and_non_step_values() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_apf_frequency_hz(-260).await.unwrap_err(),
            RadioError::InvalidApfFrequency(-260)
        ));
        assert!(matches!(
            radio.set_apf_frequency_hz(255).await.unwrap_err(),
            RadioError::InvalidApfFrequency(255)
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_manual_notch_on_get_set_send_item_selector_zero() {
        let mut radio = make_radio("BP00001;");
        assert!(radio.get_manual_notch_on().await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"BP00;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_manual_notch_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BP00001;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_manual_notch_frequency_get_set_round_trips_boundary_values() {
        let mut radio = make_radio("BP01320;");
        assert_eq!(radio.get_manual_notch_frequency_hz().await.unwrap(), 3200);
        assert_eq!(radio.session.borrow().transport.written(), b"BP01;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_manual_notch_frequency_hz(10).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BP01001;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_manual_notch_frequency_hz_rejects_out_of_range_and_non_step_values() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_manual_notch_frequency_hz(5).await.unwrap_err(),
            RadioError::InvalidManualNotchFrequency(5)
        ));
        assert!(matches!(
            radio.set_manual_notch_frequency_hz(3210).await.unwrap_err(),
            RadioError::InvalidManualNotchFrequency(3210)
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_auto_notch_on_sends_selector_zero() {
        let mut radio = make_radio("BC01;");
        let on = radio.get_auto_notch_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"BC0;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_auto_notch_on_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_auto_notch_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BC01;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_narrow_on_sends_selector_zero() {
        let mut radio = make_radio("NA01;");
        let on = radio.get_narrow_on().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"NA0;");
        assert!(on);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_narrow_on_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_narrow_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "NA01;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_filter_width_index_parses_two_digits() {
        let mut radio = make_radio("SH021;");
        let index = radio.get_filter_width_index().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"SH0;");
        assert_eq!(index, 21);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_filter_width_index_formats_two_digits() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_filter_width_index(21).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "SH021;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_filter_width_index_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_filter_width_index(22).await.unwrap_err(),
            RadioError::InvalidFilterWidthIndex(22)
        ));
        assert!(radio.session.borrow().transport.written().is_empty());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_radio_trait_delegates_batch_six_methods() {
        use crate::Radio;

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_attenuator_on(&mut radio, true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "RA01;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_preamp_mode(&mut radio, PreampMode::Amp1)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PA01;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_noise_blanker_level(&mut radio, 5).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "NL0005;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_agc_mode(&mut radio, AgcMode::Fast)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "GT01;");

        let mut radio = make_radio("SH000;");
        assert_eq!(Radio::get_filter_width_index(&mut radio).await.unwrap(), 0);
        assert_eq!(radio.session.borrow().transport.written(), b"SH0;");
    }

    // -----------------------------------------------------------------------
    // Batch 7: speech processor/mic/monitor (MG PL PR ML)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_mic_gain_sends_zero_width_query_not_selector() {
        let mut radio = make_radio("MG050;");
        let level = radio.get_mic_gain().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"MG;");
        assert_eq!(level, 50);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_mic_gain_formats_plain_value_no_selector() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_mic_gain(50).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "MG050;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_mic_gain_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_mic_gain(101).await,
            Err(RadioError::InvalidMicGain(101))
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_speech_processor_level_sends_zero_width_query() {
        let mut radio = make_radio("PL100;");
        let level = radio.get_speech_processor_level().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"PL;");
        assert_eq!(level, 100);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_speech_processor_level_formats_plain_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_speech_processor_level(100).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PL100;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_speech_processor_level_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_speech_processor_level(101).await,
            Err(RadioError::InvalidSpeechProcessorLevel(101))
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_speech_processor_on_sends_selector_zero_and_decodes_on_off() {
        let mut radio = make_radio("PR02;");
        assert!(radio.get_speech_processor_on().await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"PR0;");

        let mut radio = make_radio("PR01;");
        assert!(!radio.get_speech_processor_on().await.unwrap());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_speech_processor_on_uses_one_two_encoding() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_speech_processor_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PR02;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_speech_processor_on(false).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PR01;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_parametric_mic_eq_on_sends_selector_one() {
        let mut radio = make_radio("PR12;");
        assert!(radio.get_parametric_mic_eq_on().await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"PR1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_parametric_mic_eq_on_uses_one_two_encoding() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_parametric_mic_eq_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PR12;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_speech_processor_on_rejects_mismatched_echoed_selector() {
        // Radio echoed P1=1 (parametric mic EQ) when P1=0 was requested.
        let mut radio = make_radio("PR12;");
        assert!(matches!(
            radio.get_speech_processor_on().await,
            Err(RadioError::InvalidProtocolString(_))
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_monitor_on_sends_selector_zero() {
        let mut radio = make_radio("ML0001;");
        assert!(radio.get_monitor_on().await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"ML0;");

        let mut radio = make_radio("ML0000;");
        assert!(!radio.get_monitor_on().await.unwrap());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_monitor_on_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_monitor_on(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "ML0001;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_monitor_level_sends_selector_one() {
        let mut radio = make_radio("ML1050;");
        let level = radio.get_monitor_level().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written(), b"ML1;");
        assert_eq!(level, 50);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_monitor_level_formats_selector_and_value() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_monitor_level(50).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "ML1050;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_monitor_level_rejects_out_of_range() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.set_monitor_level(101).await,
            Err(RadioError::InvalidMonitorLevel(101))
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_radio_trait_delegates_batch_seven_methods() {
        use crate::Radio;

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_mic_gain(&mut radio, 50).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "MG050;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_speech_processor_level(&mut radio, 100)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PL100;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_speech_processor_on(&mut radio, true)
            .await
            .unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "PR02;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_monitor_on(&mut radio, true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "ML0001;");

        let mut radio = make_radio("ML1050;");
        assert_eq!(Radio::get_monitor_level(&mut radio).await.unwrap(), 50);
        assert_eq!(radio.session.borrow().transport.written(), b"ML1;");
    }

    // -----------------------------------------------------------------------
    // Batch 8: band/step/encoder front-panel controls (BS BU BD FS ED EU EK
    // DN UP)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_set_band_formats_two_digit_code() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_band(Band::OneEightMHz).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BS00;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_band(Band::FourThreeZeroMHz).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BS16;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_band_up_sends_fixed_selector() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.band_up().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BU0;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_band_down_sends_fixed_selector() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.band_down().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BD0;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_fine_step_query_and_decode() {
        let mut radio = make_radio("FS1;");
        assert!(radio.get_fine_step().await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"FS;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_fine_step_formats_zero_or_one() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_fine_step(true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "FS1;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.set_fine_step(false).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "FS0;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_encoder_down_formats_selector_and_steps() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.encoder_down(EncoderSelector::Main, 1).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "ED001;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_encoder_down_rejects_out_of_range_steps() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        assert!(matches!(
            radio.encoder_down(EncoderSelector::Main, 0).await,
            Err(RadioError::InvalidEncoderSteps(0))
        ));
        assert!(matches!(
            radio.encoder_down(EncoderSelector::Main, 100).await,
            Err(RadioError::InvalidEncoderSteps(100))
        ));
    }

    #[monoio::test(driver = "legacy")]
    async fn test_encoder_up_formats_selector_and_steps() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.encoder_up(EncoderSelector::Multi, 99).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "EU899;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_ent_key_sends_zero_width_set() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.ent_key().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "EK;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_mic_up_sends_zero_width_set() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.mic_up().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "UP;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_mic_down_sends_zero_width_set() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.mic_down().await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "DN;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_radio_trait_delegates_batch_eight_methods() {
        use crate::Radio;

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_band(&mut radio, Band::Gen).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BS11;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::band_up(&mut radio).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BU0;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::band_down(&mut radio).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "BD0;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::set_fine_step(&mut radio, true).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "FS1;");

        let mut radio = make_radio("FS0;");
        assert!(!Radio::get_fine_step(&mut radio).await.unwrap());
        assert_eq!(radio.session.borrow().transport.written(), b"FS;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::mic_up(&mut radio).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "UP;");

        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        Radio::mic_down(&mut radio).await.unwrap();
        assert_eq!(radio.session.borrow().transport.written_str(), "DN;");
    }

    // -----------------------------------------------------------------------
    // Modem control lines (RTS/DTR/CTS/DSR/DCD) — §10.4 consumption wiring.
    //
    // `ModemControlLines`'s methods are plain synchronous `fn`s (no
    // `.await` point — direct `ioctl(2)` calls, per
    // `planning/architect/task_plan.md` §10.3), so these tests are plain
    // `#[test]`s, matching `cat-transport-serial::session`'s own
    // `modem_control_lines_delegate_to_transport` test precedent rather
    // than the `#[monoio::test]` used everywhere else in this module for
    // the async `CatSession`-backed methods.
    // -----------------------------------------------------------------------

    #[test]
    fn test_assert_rts_delegates_through_shared_session_to_transport() {
        let transport = FakeTransport::new();
        let radio = Ft991a::new(SerialCatSession::new(transport));

        radio.assert_rts(true).unwrap();
        assert_eq!(
            radio.session.borrow().transport.last_set_rts.get(),
            Some(true)
        );

        radio.assert_rts(false).unwrap();
        assert_eq!(
            radio.session.borrow().transport.last_set_rts.get(),
            Some(false)
        );
    }

    #[test]
    fn test_assert_dtr_delegates_through_shared_session_to_transport() {
        let transport = FakeTransport::new();
        let radio = Ft991a::new(SerialCatSession::new(transport));

        radio.assert_dtr(true).unwrap();
        assert_eq!(
            radio.session.borrow().transport.last_set_dtr.get(),
            Some(true)
        );

        radio.assert_dtr(false).unwrap();
        assert_eq!(
            radio.session.borrow().transport.last_set_dtr.get(),
            Some(false)
        );
    }

    #[test]
    fn test_read_cts_delegates_through_shared_session_to_transport() {
        let transport = FakeTransport::new();
        let radio = Ft991a::new(SerialCatSession::new(transport));
        radio.session.borrow().transport.cts.set(true);
        assert!(radio.read_cts().unwrap());

        radio.session.borrow().transport.cts.set(false);
        assert!(!radio.read_cts().unwrap());
    }

    #[test]
    fn test_read_dsr_delegates_through_shared_session_to_transport() {
        let transport = FakeTransport::new();
        let radio = Ft991a::new(SerialCatSession::new(transport));
        radio.session.borrow().transport.dsr.set(true);
        assert!(radio.read_dsr().unwrap());

        radio.session.borrow().transport.dsr.set(false);
        assert!(!radio.read_dsr().unwrap());
    }

    #[test]
    fn test_read_dcd_delegates_through_shared_session_to_transport() {
        let transport = FakeTransport::new();
        let radio = Ft991a::new(SerialCatSession::new(transport));
        radio.session.borrow().transport.dcd.set(true);
        assert!(radio.read_dcd().unwrap());

        radio.session.borrow().transport.dcd.set(false);
        assert!(!radio.read_dcd().unwrap());
    }

    #[test]
    fn test_shared_session_modem_control_lines_all_delegate_independently() {
        // One combined test exercising all five methods against a single
        // session, proving `SharedSession<S>`'s blanket `ModemControlLines`
        // delegation (mirroring its `CatSession` delegation) round-trips
        // every method, not just the ones exercised individually above.
        let transport = FakeTransport::new();
        let radio = Ft991a::new(SerialCatSession::new(transport));

        radio.assert_rts(true).unwrap();
        radio.assert_dtr(false).unwrap();
        radio.session.borrow().transport.cts.set(true);
        radio.session.borrow().transport.dsr.set(false);
        radio.session.borrow().transport.dcd.set(true);

        assert_eq!(
            radio.session.borrow().transport.last_set_rts.get(),
            Some(true)
        );
        assert_eq!(
            radio.session.borrow().transport.last_set_dtr.get(),
            Some(false)
        );
        assert!(radio.read_cts().unwrap());
        assert!(!radio.read_dsr().unwrap());
        assert!(radio.read_dcd().unwrap());
    }
}
