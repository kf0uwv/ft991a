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

//! Diagnostic mode data model — mirrors `ts570d/ui/src/diag.rs`'s role
//! (plain, `ui`-owned data types for the `[D]` diagnostics screen), not its
//! exact shape. See `docs/adr/0006-hand-coded-full-parity-diagnostics.md`.
//!
//! The actual run engine (`RadioSnapshot`/`snapshot_state`/`restore_state`/
//! `run_diagnostics_task`) lives in `terminal.rs`, same as ts570d — it needs
//! `Terminal`/live-progress redraw access this module deliberately does not
//! depend on.
//!
//! Replaces `radio::{DiagnosticOutcome, DiagnosticResult, DiagnosticSummary}`
//! (removed along with `radio/src/diagnostics.rs` and
//! `Ft991aExtras::run_diagnostics_with` — this repo no longer calls
//! `cat_diagnostics::run_diagnostics_with` at all, matching `ts570d`, which
//! never did). Two differences from the old shape, both deliberate:
//! - No `Timeout` variant — every step here calls a real typed
//!   `Radio`/`Ft991aExtras` method and gets back a plain `RadioResult<T>`;
//!   there is no separate "no response arrived" case distinct from any
//!   other protocol error, so timeouts (if any) surface as [`DiagResult::Failure`].
//! - No `request` (raw wire text) field — steps call typed methods, not raw
//!   CAT strings, so there is no single request string to show per step.

use std::time::Duration;

/// Result of one diagnostic step (one method call, or one set+verify pair).
#[derive(Debug, Clone, PartialEq)]
pub enum DiagResult {
    /// The step's method call(s) succeeded and any verification matched.
    Success { detail: String },
    /// The step's method call failed, or a verification mismatched.
    Failure { message: String },
    /// This step was not run at all — either the underlying command has no
    /// safe, well-understood test-and-restore strategy (documented reason),
    /// or (CW keying only) the operator declined to supply a callsign.
    Skipped { reason: String },
}

impl DiagResult {
    /// `true` for [`Self::Success`] only.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

/// One diagnostic step's outcome — one row in the live-progress/completed
/// report views.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagOutcome {
    /// The underlying CAT command's two-letter wire code (e.g. `"FA"`),
    /// for traceability against `radio::FT991A_COMMAND_TABLE` — purely
    /// informational now (this engine calls typed methods, not raw CAT
    /// commands).
    pub code: &'static str,
    /// Human-readable step description, e.g. `"set_vfo_a"` or
    /// `"Clarifier Down (verify via get_information)"`.
    pub name: &'static str,
    pub result: DiagResult,
    /// Wall-clock time the step's method call(s) took.
    pub duration: Duration,
}

/// The full report: one [`DiagOutcome`] per diagnostic step, in execution
/// order. Unlike the old `cat_diagnostics`-backed report, this is **not**
/// one row per [`radio::FT991A_COMMAND_TABLE`] entry — some commands need
/// several steps (e.g. `FA`: `set_vfo_a` + `get_vfo_a`), and a few steps
/// exercise two closely-related commands together (e.g. `Ft991aExtras`
/// getters with no top-level command-table row of their own, like
/// `get_repeater_shift`). See the ADR for the full per-command breakdown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiagSummary {
    pub outcomes: Vec<DiagOutcome>,
}

impl DiagSummary {
    pub fn total(&self) -> usize {
        self.outcomes.len()
    }
}

/// Count [`DiagResult::Success`] outcomes in `outcomes`. A free function
/// (not just a [`DiagSummary`] method) so `layout.rs` can share this exact
/// counting logic against a growing, not-yet-complete outcomes slice during
/// a live run, not only against a finished [`DiagSummary`].
pub fn count_passed(outcomes: &[DiagOutcome]) -> usize {
    outcomes.iter().filter(|o| o.result.is_success()).count()
}

/// Count [`DiagResult::Failure`] outcomes in `outcomes`.
pub fn count_failed(outcomes: &[DiagOutcome]) -> usize {
    outcomes
        .iter()
        .filter(|o| matches!(o.result, DiagResult::Failure { .. }))
        .count()
}

/// Count [`DiagResult::Skipped`] outcomes in `outcomes`.
pub fn count_skipped(outcomes: &[DiagOutcome]) -> usize {
    outcomes
        .iter()
        .filter(|o| matches!(o.result, DiagResult::Skipped { .. }))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(result: DiagResult) -> DiagOutcome {
        DiagOutcome {
            code: "FA",
            name: "get_vfo_a",
            result,
            duration: Duration::from_millis(1),
        }
    }

    #[test]
    fn is_success_true_only_for_success() {
        assert!(DiagResult::Success {
            detail: "ok".into()
        }
        .is_success());
        assert!(!DiagResult::Failure {
            message: "e".into()
        }
        .is_success());
        assert!(!DiagResult::Skipped { reason: "r".into() }.is_success());
    }

    #[test]
    fn summary_totals_count_each_bucket_independently() {
        let summary = DiagSummary {
            outcomes: vec![
                outcome(DiagResult::Success {
                    detail: "ok".into(),
                }),
                outcome(DiagResult::Success {
                    detail: "ok".into(),
                }),
                outcome(DiagResult::Failure {
                    message: "bad".into(),
                }),
                outcome(DiagResult::Skipped {
                    reason: "no callsign".into(),
                }),
            ],
        };
        assert_eq!(summary.total(), 4);
        assert_eq!(count_passed(&summary.outcomes), 2);
        assert_eq!(count_failed(&summary.outcomes), 1);
        assert_eq!(count_skipped(&summary.outcomes), 1);
    }

    #[test]
    fn empty_summary_reports_all_zero() {
        let summary = DiagSummary::default();
        assert_eq!(summary.total(), 0);
        assert_eq!(count_passed(&summary.outcomes), 0);
        assert_eq!(count_failed(&summary.outcomes), 0);
        assert_eq!(count_skipped(&summary.outcomes), 0);
    }
}
