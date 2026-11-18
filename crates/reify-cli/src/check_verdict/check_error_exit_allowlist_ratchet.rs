//! Ratchet pin for `CHECK_ERROR_EXIT_ALLOWLIST` (PRD §7: "the ratchet test
//! asserts the exact table contents").
//!
//! ## Standing obligation (2026-08-26 ruling): never add a row
//!
//! The allowlist was a **bounded migration ratchet**, not a policy surface.  It
//! existed only because INV-SF-2's general `Severity::Error` exit gate landed
//! (#5403) before the individual legacy emissions it caught were corrected.
//! The burn-down is complete: the table is pinned EMPTY, so `reify check` is
//! at INV-SF-2's end state, where no per-code list mediates the exit at all.
//!
//! Consequently: **never add a row.**  A `DiagnosticCode` minted after the
//! gate landed — #6608's in particular — has no legacy burn-down claim, and
//! neither does anything else now: if a new emission would trip the gate on a
//! healthy path, the emission's severity is what is wrong, not the gate.
//! `allowlist_table_is_empty` makes an addition as loud in review as a
//! modification; the two cite guards below stay as the check on any row that
//! is added regardless, and are vacuous at zero rows by design.

use super::CHECK_ERROR_EXIT_ALLOWLIST;
use reify_test_support::ptodo_cite_marker::has_liveness_marker;

#[test]
fn allowlist_table_is_empty() {
    assert!(
        CHECK_ERROR_EXIT_ALLOWLIST.is_empty(),
        "CHECK_ERROR_EXIT_ALLOWLIST must stay EMPTY: any entry is a regression \
         against INV-SF-2's end state, where no per-code list mediates `reify \
         check`'s exit. A new Error on a healthy path means the emission's \
         severity is wrong, not the gate"
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
