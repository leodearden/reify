// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared flattening of a `ComputeOutcome::Failed`'s diagnostics for FEA e2e tests.
//!
//! Consumers include it from their test-crate root with
//! `#[path = "common/failed_diagnostics.rs"] mod failed_diagnostics;`, following the
//! `common/numeric_cell.rs` precedent.

use reify_eval::ComputeOutcome;

/// Every diagnostic message of a `Failed` outcome joined with `" | "`; panics on
/// any other outcome.
pub fn joined_failure_messages(outcome: ComputeOutcome) -> String {
    match outcome {
        ComputeOutcome::Failed { diagnostics, .. } => diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        other => panic!("expected ComputeOutcome::Failed, got {other:?}"),
    }
}
