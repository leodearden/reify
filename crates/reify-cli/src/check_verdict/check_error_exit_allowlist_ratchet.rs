//! Ratchet pin for `CHECK_ERROR_EXIT_ALLOWLIST` (PRD §7: "the ratchet test
//! asserts the exact table contents").
//!
//! ## Standing obligation (2026-08-26 ruling)
//!
//! The allowlist is a **bounded migration ratchet**, not a policy surface.  It
//! exists only because INV-SF-2's general `Severity::Error` exit gate landed
//! (#5403) before the individual legacy emissions it catches were corrected,
//! and it is burned to ZERO by #5404 — at which point `reify check` converges
//! on INV-SF-2's end state, where no per-code list mediates the exit at all.
//!
//! Consequently: **`DiagnosticCode`s minted after this gate landed — #6608's
//! in particular — MUST NEVER be added.**  A code that did not exist before
//! the gate has no legacy burn-down claim; if a new emission would trip the
//! gate, the emission's severity is what is wrong, not the gate.  This
//! whole-table equality assertion exists precisely so that an ADDITION is as
//! loud in review as a modification: there is no way to slip an entry in
//! without editing the literal below.

use super::{
    CheckErrorAllowlistDisposition, CheckErrorExitAllowance, CHECK_ERROR_EXIT_ALLOWLIST,
};
use reify_test_support::ptodo_cite_marker::has_liveness_marker;

/// Renders one table row to a comparable tuple.  The matcher goes through
/// its `Debug` rendering so the expected side below can be written as a
/// plain string literal rather than by re-constructing the same enum value
/// (which would make the assertion tautological).
fn key(
    e: &CheckErrorExitAllowance,
) -> (String, CheckErrorAllowlistDisposition, &'static str) {
    (format!("{:?}", e.matcher), e.disposition, e.cite)
}

#[test]
fn allowlist_table_is_exactly_the_seeded_burn_down_set() {
    assert_eq!(
        CHECK_ERROR_EXIT_ALLOWLIST.len(),
        4,
        "the seeded burn-down set has exactly four entries; growing it is a \
         regression against INV-SF-2's no-per-code-list end state, and \
         shrinking it means an entry was retired — update this pin \
         deliberately, in the same commit as the demotion/fix that retired it"
    );

    let expected: Vec<(String, CheckErrorAllowlistDisposition, &'static str)> = vec![
        (
            "Code(NoRegisteredComputeTrampoline)".to_string(),
            CheckErrorAllowlistDisposition::Demote,
            "#5404",
        ),
        (
            r#"MessageContains("is unresolved (Undef)")"#.to_string(),
            CheckErrorAllowlistDisposition::Demote,
            "#5404",
        ),
        (
            r#"MessageContains("all geometry operations failed")"#.to_string(),
            CheckErrorAllowlistDisposition::Demote,
            "#5404",
        ),
        (
            "Code(ConstraintViolated)".to_string(),
            CheckErrorAllowlistDisposition::FixPath,
            "#5404",
        ),
    ];

    assert_eq!(
        CHECK_ERROR_EXIT_ALLOWLIST
            .iter()
            .map(key)
            .collect::<Vec<_>>(),
        expected,
        "CHECK_ERROR_EXIT_ALLOWLIST drifted from its seeded contents. This \
         is a whole-table equality on purpose: an addition, a re-ordering, \
         a matcher widening and a cite change are all equally loud."
    );
}

/// Every cite must be in the PTODO canonical `#NNNN` form, so a
/// Greek-letter alias (`task ε`), a PRD-relative index (`task-5`) or a
/// prose form (`task 5404`) cannot be smuggled into the table and leave the
/// burn-down untrackable.  Matches `^#[1-9][0-9]*$`, hand-rolled because
/// `reify-cli` carries no regex dependency.
#[test]
fn every_allowlist_cite_is_canonical() {
    for e in CHECK_ERROR_EXIT_ALLOWLIST {
        let digits = e.cite.strip_prefix('#').unwrap_or_else(|| {
            panic!("allowlist cite {:?} must start with '#'", e.cite)
        });
        assert!(
            !digits.is_empty()
                && !digits.starts_with('0')
                && digits.bytes().all(|b| b.is_ascii_digit()),
            "allowlist cite {:?} is not PTODO-canonical (`^#[1-9][0-9]*$`)",
            e.cite
        );
    }
}

/// The liveness half of the cite pin; the form half is the test above.
/// Each distinct cite needs a sole-cite marker comment in check_verdict.rs, the
/// only route by which PTODO notices the owning task closing.
#[test]
fn every_allowlist_cite_has_a_ptodo_liveness_marker() {
    const SOURCE: &str = include_str!("../check_verdict.rs");
    let mut unmarked: Vec<&str> = CHECK_ERROR_EXIT_ALLOWLIST
        .iter()
        .map(|e| e.cite)
        .filter(|cite| !has_liveness_marker(SOURCE, cite))
        .collect();
    unmarked.sort_unstable();
    unmarked.dedup();
    assert!(
        unmarked.is_empty(),
        "allowlist cites {unmarked:?} have no PTODO liveness marker in check_verdict.rs. The \
         `cite` field is a string literal PTODO never reads, so without a sole-cite \
         marker comment in check_verdict.rs citing the same task, nothing turns red when that \
         task closes and the entry outlives its owner. Rule: \
         reify_test_support::ptodo_cite_marker."
    );
}
