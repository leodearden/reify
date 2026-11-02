// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared flattening of FEA e2e diagnostics into one string, and the needle
//! assertion over it.
//!
//! Consumers include it from their test-crate root with
//! `#[path = "common/failed_diagnostics.rs"] mod failed_diagnostics;`, following the
//! `common/numeric_cell.rs` precedent.

use reify_core::Diagnostic;
use reify_eval::ComputeOutcome;

/// Every message in `diagnostics` joined with `" | "`.
pub fn joined_messages(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// [`joined_messages`] of a `Failed` outcome's diagnostics; panics on any other
/// outcome.
pub fn joined_failure_messages(outcome: ComputeOutcome) -> String {
    match outcome {
        ComputeOutcome::Failed { diagnostics, .. } => joined_messages(&diagnostics),
        other => panic!("expected ComputeOutcome::Failed, got {other:?}"),
    }
}

/// Assert `joined` contains every `must` needle and no `must_not` needle.
pub fn assert_mentions(joined: &str, must: &[&str], must_not: &[&str]) {
    for &needle in must {
        assert!(
            joined.contains(needle),
            "expected the diagnostic to mention {needle:?}, got: {joined}"
        );
    }
    for &needle in must_not {
        assert!(
            !joined.contains(needle),
            "expected the diagnostic NOT to mention {needle:?}, got: {joined}"
        );
    }
}
