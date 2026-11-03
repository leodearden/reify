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

#[test]
fn joined_messages_joins_each_message_with_a_pipe_separator() {
    assert_eq!(
        joined_messages(&[Diagnostic::error("a"), Diagnostic::error("b")]),
        "a | b"
    );
}

#[test]
fn joined_failure_messages_flattens_a_failed_outcome() {
    let failed = ComputeOutcome::Failed {
        diagnostics: vec![Diagnostic::error("a"), Diagnostic::error("b")],
        structured_detail: Vec::new(),
    };
    assert_eq!(joined_failure_messages(failed), "a | b");
}

#[test]
#[should_panic(expected = "expected ComputeOutcome::Failed, got Cancelled")]
fn joined_failure_messages_panics_on_a_non_failed_outcome() {
    joined_failure_messages(ComputeOutcome::Cancelled);
}

#[test]
fn assert_mentions_accepts_present_must_and_absent_must_not_needles() {
    assert_mentions("a | b", &["a", "b"], &["c"]);
}

#[test]
#[should_panic(expected = "NOT to mention \"b\"")]
fn assert_mentions_rejects_a_present_must_not_needle() {
    assert_mentions("a | b", &["a"], &["b"]);
}

#[test]
#[should_panic(expected = "diagnostic to mention \"zzz\"")]
fn assert_mentions_rejects_a_missing_must_needle() {
    assert_mentions("a | b", &["zzz"], &[]);
}
