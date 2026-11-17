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
