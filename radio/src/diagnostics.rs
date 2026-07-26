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

//! Radio-owned, non-generic mirror of `radio-cat-rs`'s `cat-diagnostics`
//! report types (see `docs/adr/0004-shared-diagnostics-screen.md`).
//!
//! [`crate::Ft991aExtras::run_diagnostics_with`] is the only place this
//! crate calls `cat_diagnostics::run_diagnostics_with` — never `ui`. Two
//! independent reasons, only one of which is a style preference:
//!
//! 1. **A hard type-level constraint, not a judgment call.**
//!    `cat_diagnostics::run_diagnostics_with` takes `&mut
//!    cat_client::CatClient<C, S>` directly. `ui::run<R: Radio +
//!    Ft991aExtras + CwKeying + 'static>` only ever holds a generic `R`
//!    value — it has no way to obtain a `CatClient` from it at all, since
//!    `Ft991a<S>`'s own `client` field is `pub(crate)` (`ft991a.rs`) and
//!    `ui` never even names the concrete `Ft991a<S>` type in the first
//!    place (`R` is erased to the three trait bounds). So `ui` *cannot*
//!    call `cat_diagnostics::run_diagnostics_with` itself, regardless of
//!    any dependency-graph policy — this crate is the only one with a
//!    `CatClient` to hand it.
//! 2. **Once wrapped here, `ui` has no remaining reason to also depend on
//!    `cat-diagnostics` directly** — this module's [`DiagnosticSummary`]/
//!    [`DiagnosticOutcome`]/[`DiagnosticResult`] are concrete, non-generic
//!    types (no `cat_framework::CommandId` type parameter, unlike
//!    `cat_diagnostics::DiagnosticReport<C>`/`CommandOutcome<C>`), so `ui`
//!    can render a diagnostics screen using only what it already depends
//!    on (`radio`). Note this is **not** because `cat-diagnostics` would
//!    have violated this repo's "`ui` never imports a transport crate"
//!    rule if `ui` *had* depended on it directly — `cat-diagnostics` is
//!    radio-generic (depends only on `cat-framework`/`cat-client`/
//!    `cat-transport-core`, the same tier `cat-client` itself sits at),
//!    not a transport crate at all. Reason 1 above is what actually forces
//!    the wrapping; reason 2 is simply the natural consequence of doing so.

use std::time::Duration;

use crate::Ft991aCommandId;

/// Mirrors [`cat_diagnostics::CommandResult`], minus any dependency on
/// that crate's own types, so `ui` never needs to depend on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticResult {
    /// The command's documented read form answered within the configured
    /// timeout, with no protocol/transport error. `response` is the raw
    /// wire response text, verbatim.
    Success { response: String },
    /// The command was sent but the radio (or transport) returned an
    /// error. `message` is the underlying error's `Display` text.
    Failure { message: String },
    /// No response arrived within the configured per-command timeout.
    Timeout,
    /// This command has no read form at all (write-only, non-selector
    /// `Set`), so it was never sent. `reason` explains why.
    Skipped { reason: &'static str },
}

impl DiagnosticResult {
    /// `true` for [`Self::Success`] only — mirrors
    /// `cat_diagnostics::CommandResult::is_success` exactly.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

impl From<cat_diagnostics::CommandResult> for DiagnosticResult {
    fn from(r: cat_diagnostics::CommandResult) -> Self {
        match r {
            cat_diagnostics::CommandResult::Success { response } => Self::Success { response },
            cat_diagnostics::CommandResult::Failure { message } => Self::Failure { message },
            cat_diagnostics::CommandResult::Timeout => Self::Timeout,
            cat_diagnostics::CommandResult::Skipped { reason } => Self::Skipped { reason },
        }
    }
}

/// Mirrors [`cat_diagnostics::CommandOutcome<Ft991aCommandId>`], with the
/// typed `id: Ft991aCommandId` field dropped — `ui` has no use for it (the
/// human-readable `code`/`name` are sufficient for display), and dropping
/// it means this type carries no `cat_framework::CommandId`-bounded
/// generic parameter at all, keeping it renderable by `ui` without that
/// crate depending on `cat-framework`/`cat-diagnostics` itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticOutcome {
    /// The command's two-letter wire code (e.g. `"FA"`).
    pub code: &'static str,
    /// The command's human-readable name, per [`crate::FT991A_COMMAND_TABLE`].
    pub name: &'static str,
    /// The raw wire request text sent; empty if [`DiagnosticResult::Skipped`].
    pub request: String,
    pub result: DiagnosticResult,
    /// Round-trip latency; zero if [`DiagnosticResult::Skipped`].
    pub latency: Duration,
}

impl From<&cat_diagnostics::CommandOutcome<Ft991aCommandId>> for DiagnosticOutcome {
    fn from(o: &cat_diagnostics::CommandOutcome<Ft991aCommandId>) -> Self {
        Self {
            code: o.code,
            name: o.name,
            request: o.request.clone(),
            result: o.result.clone().into(),
            latency: o.latency,
        }
    }
}

/// Mirrors [`cat_diagnostics::DiagnosticReport<Ft991aCommandId>`] — one
/// [`DiagnosticOutcome`] per [`crate::FT991A_COMMAND_TABLE`] entry, in
/// table order. Every command appears: tested or explicitly
/// [`DiagnosticResult::Skipped`], never silently omitted (same guarantee
/// `cat_diagnostics::DiagnosticReport` itself documents).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagnosticSummary {
    pub outcomes: Vec<DiagnosticOutcome>,
}

impl DiagnosticSummary {
    pub fn total(&self) -> usize {
        self.outcomes.len()
    }
    pub fn passed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| o.result.is_success())
            .count()
    }
    /// [`DiagnosticResult::Failure`] + [`DiagnosticResult::Timeout`].
    pub fn failed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| {
                matches!(
                    o.result,
                    DiagnosticResult::Failure { .. } | DiagnosticResult::Timeout
                )
            })
            .count()
    }
    pub fn skipped(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o.result, DiagnosticResult::Skipped { .. }))
            .count()
    }
}

impl From<cat_diagnostics::DiagnosticReport<Ft991aCommandId>> for DiagnosticSummary {
    fn from(report: cat_diagnostics::DiagnosticReport<Ft991aCommandId>) -> Self {
        Self {
            outcomes: report
                .outcomes
                .iter()
                .map(DiagnosticOutcome::from)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(result: DiagnosticResult) -> DiagnosticOutcome {
        DiagnosticOutcome {
            code: "FA",
            name: "VFO A frequency",
            request: "FA;".to_string(),
            result,
            latency: Duration::from_millis(5),
        }
    }

    #[test]
    fn diagnostic_result_is_success_true_only_for_success() {
        assert!(DiagnosticResult::Success {
            response: "ok".to_string()
        }
        .is_success());
        assert!(!DiagnosticResult::Failure {
            message: "e".to_string()
        }
        .is_success());
        assert!(!DiagnosticResult::Timeout.is_success());
        assert!(!DiagnosticResult::Skipped { reason: "r" }.is_success());
    }

    #[test]
    fn summary_totals_count_each_bucket_independently() {
        let summary = DiagnosticSummary {
            outcomes: vec![
                outcome(DiagnosticResult::Success {
                    response: "ok".to_string(),
                }),
                outcome(DiagnosticResult::Success {
                    response: "ok2".to_string(),
                }),
                outcome(DiagnosticResult::Failure {
                    message: "bad".to_string(),
                }),
                outcome(DiagnosticResult::Timeout),
                outcome(DiagnosticResult::Skipped {
                    reason: "write-only",
                }),
            ],
        };
        assert_eq!(summary.total(), 5);
        assert_eq!(summary.passed(), 2);
        assert_eq!(summary.failed(), 2);
        assert_eq!(summary.skipped(), 1);
    }

    #[test]
    fn empty_summary_reports_all_zero() {
        let summary = DiagnosticSummary::default();
        assert_eq!(summary.total(), 0);
        assert_eq!(summary.passed(), 0);
        assert_eq!(summary.failed(), 0);
        assert_eq!(summary.skipped(), 0);
    }

    #[test]
    fn diagnostic_result_from_cat_diagnostics_converts_every_variant() {
        assert_eq!(
            DiagnosticResult::from(cat_diagnostics::CommandResult::Success {
                response: "x".to_string()
            }),
            DiagnosticResult::Success {
                response: "x".to_string()
            }
        );
        assert_eq!(
            DiagnosticResult::from(cat_diagnostics::CommandResult::Failure {
                message: "y".to_string()
            }),
            DiagnosticResult::Failure {
                message: "y".to_string()
            }
        );
        assert_eq!(
            DiagnosticResult::from(cat_diagnostics::CommandResult::Timeout),
            DiagnosticResult::Timeout
        );
        assert_eq!(
            DiagnosticResult::from(cat_diagnostics::CommandResult::Skipped { reason: "z" }),
            DiagnosticResult::Skipped { reason: "z" }
        );
    }
}
