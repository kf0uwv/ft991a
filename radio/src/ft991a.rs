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

use std::cell::RefCell;
use std::rc::Rc;

use cat_client::CatClient;
use cat_transport_core::{CatSession, ResponseDisposition, TransportError};

use crate::ft991a_radio::{Ft991aCommandId, FT991A_COMMAND_TABLE};
use crate::{Frequency, Mode, RadioError, RadioResult, TxState};

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

/// Strip a wire response's leading command code and trailing `;`,
/// returning the parameter body, or [`RadioError::InvalidProtocolString`]
/// if the response isn't shaped like `"<code><body>;"`.
fn parse_frame<'a>(raw: &'a str, code: &str) -> RadioResult<&'a str> {
    raw.strip_prefix(code)
        .and_then(|rest| rest.strip_suffix(';'))
        .ok_or_else(|| RadioError::InvalidProtocolString(raw.to_string()))
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

    /// Flush the session's receive buffer, discarding unsolicited or stale
    /// data.
    pub fn flush_rx(&mut self) {
        let mut session = self.session.take();
        session.flush_rx();
        self.session.put_back(session);
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
    use std::collections::VecDeque;

    // -----------------------------------------------------------------------
    // In-memory fake transport (mirrors ts570d/radio/src/ts570d.rs's own
    // test module — wire-framing-level tests, exercising the real
    // `cat-transport-serial::SerialCatSession` read-until-`;` framing
    // against a local fake `Transport`, never a production transport).
    // -----------------------------------------------------------------------

    struct FakeTransport {
        writes: Vec<u8>,
        reads: VecDeque<u8>,
    }

    impl FakeTransport {
        fn new() -> Self {
            Self {
                writes: Vec::new(),
                reads: VecDeque::new(),
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
    // SharedSession / flush_rx
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_flush_rx_does_not_panic() {
        let transport = FakeTransport::new();
        let mut radio = Ft991a::new(SerialCatSession::new(transport));
        radio.flush_rx();
    }
}
