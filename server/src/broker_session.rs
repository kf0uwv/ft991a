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

//! [`BrokerCatSession`]: a [`cat_transport_core::CatSession`] implementation
//! that submits raw wire-format requests through a [`cat_server::BrokerHandle`]
//! instead of talking to a transport directly.
//!
//! Per `planning/architect/task_plan.md` §12.2's "reuse, not
//! reimplementation" design point: this is what lets [`crate::rigctl`] build
//! a real `radio::Ft991a<BrokerCatSession>` and call its already-correct,
//! already-tested typed methods (`get_vfo_a`/`set_mode`/`transmit`/...)
//! instead of hand-rolling FT-991A wire frames a second time. `radio`
//! itself never sees this type — it lives here, in `server`, per this
//! repo's Rule 2 (`radio` never imports a transport crate directly).
//!
//! Its `Error` type is deliberately `cat_transport_core::TransportError`
//! (not a new local error type) specifically because `radio::Ft991a<S>`'s
//! existing impl blocks are bounded on `S: CatSession<Error =
//! TransportError>` — matching that bound exactly is what makes the reuse
//! possible at all, without widening `radio`'s own generic bounds.

use async_trait::async_trait;
use cat_framework::ResponseDisposition;
use cat_server::BrokerHandle;
use cat_transport_core::{CatSession, TransportError};

/// One logical remote client's session against a [`BrokerHandle`] — the
/// physical radio session is shared (via the broker's single ordered
/// worker), but each [`BrokerCatSession`] is tagged with the
/// [`cat_server::ClientId`] its raw TCP/rigctl connection was registered
/// under, purely for the broker's own logging/observability.
pub struct BrokerCatSession {
    handle: BrokerHandle,
    client_id: cat_server::ClientId,
}

impl BrokerCatSession {
    pub fn new(handle: BrokerHandle, client_id: cat_server::ClientId) -> Self {
        Self { handle, client_id }
    }
}

#[async_trait(?Send)]
impl CatSession for BrokerCatSession {
    type Error = TransportError;

    async fn execute(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8>,
    ) -> Result<ResponseDisposition, Self::Error> {
        let wire = self
            .handle
            .submit(self.client_id, request.to_vec())
            .await
            .ok_or_else(|| TransportError::Other("broker worker has shut down".to_string()))?;

        // `cat_server::broker::outcome_to_wire`'s convention: empty payload
        // = no response; a `b"ERR <message>"` payload (no leading 2-letter
        // command code, no trailing `;`, so it can never collide with a
        // real CAT frame) = a dispatch-time failure at any layer (malformed
        // request, broker timeout, or the physical session/radio itself);
        // anything else = the radio's real response text.
        if wire.is_empty() {
            return Ok(ResponseDisposition::NoResponse);
        }
        if let Some(message) = wire.strip_prefix(b"ERR ") {
            return Err(TransportError::Other(
                String::from_utf8_lossy(message).into_owned(),
            ));
        }
        response.extend_from_slice(&wire);
        Ok(ResponseDisposition::ResponseWritten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_framework::{CommandDefinition, CommandForm, CommandOperation, CommandTable};
    use cat_transport_core::test_support::{Exchange, ScriptedCatSession};

    // In-crate fake `CommandId`/`CommandTable`, mirroring `cat-server`'s own
    // `test_fixtures.rs` — never a real radio crate's command table.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeCommand {
        Frequency,
    }

    static DEFINITIONS: &[CommandDefinition<FakeCommand>] = &[CommandDefinition {
        id: FakeCommand::Frequency,
        code: "FA",
        name: "Frequency",
        description: "Test frequency",
        query_forms: &[CommandForm::fixed(CommandOperation::Query, 0)],
        // 9-digit frequency param (`Frequency::to_protocol_string`'s real
        // width), not an arbitrary test width — a mismatch here is exactly
        // what `CommandTable::parse`'s structural width gate is supposed to
        // catch (see this crate's `execute_maps_err_wire_convention_to_transport_error`
        // test), so the fixture must match the real wire shape it's
        // exercising.
        set_forms: &[CommandForm::fixed(CommandOperation::Set, 9)],
        action_forms: &[],
        response_forms: &[],
        readable: true,
        writable: true,
    }];
    static TABLE: CommandTable<FakeCommand> = CommandTable::new(DEFINITIONS);

    fn build_broker(
        exchanges: Vec<Exchange>,
    ) -> (
        cat_server::BrokerWorker<FakeCommand, ScriptedCatSession>,
        BrokerHandle,
    ) {
        let session = ScriptedCatSession::with_script(exchanges);
        cat_server::build(session, &TABLE)
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn execute_returns_response_written_with_response_bytes() {
        let (worker, handle) = build_broker(vec![Exchange::new("FA;", "FA014250000;")]);
        monoio::spawn(worker.run());

        let mut session = BrokerCatSession::new(handle, cat_server::ClientId::from_raw(0));
        let mut response = Vec::new();
        let disposition = session.execute(b"FA;", &mut response).await.unwrap();

        assert_eq!(disposition, ResponseDisposition::ResponseWritten);
        assert_eq!(response, b"FA014250000;");
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn execute_maps_err_wire_convention_to_transport_error() {
        let (worker, handle) = build_broker(vec![]);
        monoio::spawn(worker.run());

        let mut session = BrokerCatSession::new(handle, cat_server::ClientId::from_raw(0));
        let mut response = Vec::new();
        let err = session
            .execute(b"ZZ;", &mut response)
            .await
            .expect_err("unknown command must fail");

        assert!(matches!(err, TransportError::Other(_)));
        assert!(response.is_empty());
    }

    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn execute_returns_no_response_for_empty_wire_payload() {
        let (worker, handle) = build_broker(vec![Exchange::new("FA014250000;", "")]);
        monoio::spawn(worker.run());

        let mut session = BrokerCatSession::new(handle, cat_server::ClientId::from_raw(0));
        let mut response = Vec::new();
        let disposition = session
            .execute(b"FA014250000;", &mut response)
            .await
            .unwrap();

        assert_eq!(disposition, ResponseDisposition::NoResponse);
        assert!(response.is_empty());
    }
}
