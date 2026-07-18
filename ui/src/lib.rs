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

//! FT-991A Terminal UI — Wave 1 placeholder.
//!
//! This crate is a genuinely minimal stub: [`run`] does nothing but
//! immediately return `Ok(())`. It exists only so `app`'s `src/main.rs`
//! (this repo's application wiring layer) has something concrete to hand
//! the constructed [`radio::Radio`] implementation to, unblocking the
//! workspace build.
//!
//! The real ratatui terminal interface (mirroring `ts570d/ui`'s layout,
//! widgets, and live-updating fields) is Wave 2's job, once the `radio`
//! crate's `Radio` trait surface has been reviewed and approved — see
//! `planning/architect/task_plan.md` §5. Per this repo's `CLAUDE.md`
//! dependency model, `ui` depends on `radio` only and never imports a
//! transport crate directly, even once the real TUI lands.

/// Errors that can occur while running the UI.
///
/// Currently unused by [`run`] (the stub never fails) — kept as a real
/// `thiserror` type now so Wave 2's terminal-setup/render error paths can
/// grow this enum without changing `run`'s signature.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience [`Result`] alias for UI operations.
pub type UiResult<T> = Result<T, UiError>;

/// Run the terminal UI against a live [`radio::Radio`] implementation.
///
/// Wave 1 placeholder: does nothing and returns immediately. Wave 2
/// replaces this body with the real ratatui event loop; the signature is
/// expected to stay stable (mirrors `ts570d::ui::run`'s shape).
pub async fn run<R: radio::Radio + 'static>(_radio: R) -> UiResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use radio::{Radio, RadioError, RadioResult};

    /// Minimal mock `Radio` implementation, per `CLAUDE.md`'s testing rule
    /// ("ui tests use an in-crate MockRadio impl of the Radio trait" — not
    /// the real `radio::Ft991a` client).
    struct MockRadio;

    #[async_trait(?Send)]
    impl Radio for MockRadio {
        async fn get_id(&mut self) -> RadioResult<String> {
            Ok("0670".to_string())
        }
    }

    #[monoio::test(driver = "legacy")]
    async fn test_run_returns_ok_immediately() {
        let radio = MockRadio;
        assert!(run(radio).await.is_ok());
    }

    #[test]
    fn test_ui_error_wraps_io_error() {
        let io_err = std::io::Error::other("boom");
        let ui_err: UiError = io_err.into();
        assert!(matches!(ui_err, UiError::Io(_)));
    }

    // Compile-time check only: RadioError must not appear directly in this
    // crate's public surface yet (Wave 1 stub does not call any Radio
    // methods) — referencing the type keeps the import from going stale
    // without adding a runtime assertion for something not yet exercised.
    #[allow(dead_code)]
    fn _radio_error_type_is_reachable(_e: RadioError) {}
}
