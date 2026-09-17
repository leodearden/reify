//! Wave-protocol contract pin for PDIAG (task #6768, against landed #5405).
//!
//! Pins the PDIAG contract the suite's Phase-3 wave protocol depends on: the
//! composed before/after transaction, the `Exceeded`-vs-`NewFile` directions
//! that make mandating a same-diff shrink safe, the zero-row deletion rule,
//! and the two model deltas accepted 2026-08-28. The protocol text and the
//! "PDIAG is the ONE ratchet — no suite-local second ratchet" rule are
//! normative in `docs/prds/v0_6/spec-conformance-suite.md` (D8, Phase-3);
//! this file pins them rather than restating them (SPOT). Remediation menu:
//! `docs/notes/diagnostic-severity-policy.md` §3.
//!
//! Drives `pdiag::check` only through its public
//! `pdiag::test_support::Fixture` seam. `crates/reify-audit/src/pdiag.rs`'s
//! own `mod tests` exhaustively covers `ratchet()`'s four verdicts and the
//! generator seam is pinned by `crates/reify-audit/tests/pdiag_baseline.rs`
//! — neither is restated here. New here: a single file's `coded_src` +
//! `codeless_src` MIX (that combination appears nowhere in `pdiag.rs`'s own
//! tests), and the zero-row-vs-dropped-row fork. A future change to the
//! ratchet's severity/exit-code table should be checked against both
//! `pdiag.rs`'s `mod tests` and this file.
//!
//! Signal: `cargo test -p reify-audit --test pdiag_wave_protocol`

use reify_audit::pdiag::test_support::{Fixture, codeless_src, coded_src};
use reify_audit::{EvidenceRef, Finding, Pattern, Severity};

/// The one fixed swept path every test in this file shares. `crates/<name>/src/`
/// is required by `pdiag::is_swept_path`; `reify-compiler` is an arbitrary
/// swept crate distinct from the detector's own — `crates/reify-audit/` is
/// scope-excluded (the detector cannot sweep itself; see `pdiag.rs`'s
/// `SCOPE_EXCLUDE_PREFIXES`).
const WAVE_SECTION: &str = "crates/reify-compiler/src/wave_section.rs";

/// Shared builder behind [`wave_tree`] and the raw-baseline escape a
/// deliberately malformed row (e.g. a literal `0`) needs — taking bytes
/// directly here is what keeps that escape from duplicating fixture setup.
fn wave_tree_raw(codeless: usize, coded: usize, raw_baseline: &str) -> Vec<Finding> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write(WAVE_SECTION, &format!("{}{}", coded_src(coded), codeless_src(codeless)));
    fx.baseline(raw_baseline);
    fx.run()
}

/// [`wave_tree_raw`] for the common well-formed-row case: `row` plants that
/// file's baseline allowance, `None` omits the row entirely. Every ordinary
/// test in this file shares this one builder, so a wave's before/after pair
/// reads as a two-line change of counts. `row` is always >= 1 — `0` is a
/// real `parse_baseline` rejection, so that case goes through
/// [`wave_tree_raw`] instead.
fn wave_tree(codeless: usize, coded: usize, row: Option<u32>) -> Vec<Finding> {
    let baseline = row.map_or_else(String::new, |n| {
        debug_assert!(n >= 1, "a real baseline row is never 0 — use wave_tree_raw for that");
        format!("{WAVE_SECTION} {n}\n")
    });
    wave_tree_raw(codeless, coded, &baseline)
}

/// The gate-facing subset of a wave's findings. High is the ONLY severity
/// that moves `reify-audit`'s exit code, so "does this wave red the merge
/// gate?" is exactly "is `highs()` non-empty?" — do not widen this to all
/// severities, or the two Medium advisories silently become failures.
fn highs(findings: &[Finding]) -> Vec<&Finding> {
    findings.iter().filter(|f| f.severity == Severity::High).collect()
}

#[test]
fn a_wave_that_codes_sites_and_shrinks_its_row_in_the_same_diff_stays_clean() {
    let n = 5usize;
    let k = 2usize;

    // BEFORE: the wave hasn't landed yet — N code-less sites, baseline at N.
    let before = wave_tree(n, 0, Some(n as u32));
    assert_eq!(before, Vec::new(), "BEFORE: N code-less sites at a baseline of N must be clean");

    // AFTER: the wave diff codes K of them and shrinks the row by K in the
    // SAME diff. Assert emptiness of the WHOLE finding list — the absence of
    // the Medium `pdiag-baseline-stale` advisory is half the claim, since
    // that is what distinguishes a same-diff shrink from a deferred one.
    let after = wave_tree(n - k, k, Some((n - k) as u32));
    assert_eq!(
        after,
        Vec::new(),
        "AFTER: coding K sites and shrinking the row in the same diff must stay clean — \
         no High, and no Medium pdiag-baseline-stale advisory either"
    );
}

#[test]
fn a_row_shrunk_further_than_its_sites_were_coded_is_a_high() {
    // The shrink is not a rubber stamp: the wave rewrites the row to N-K but
    // only codes K-1 sites, so live (N-K+1) exceeds the row it just wrote.
    // Without this direction, mandating a same-diff shrink would be
    // mandating a way to launder sites past the gate. This is the
    // `Exceeded` arm — a row that IS present but too small — distinct from
    // the `NewFile` arm (no row at all) the next test pins.
    let n = 5usize;
    let k = 2usize;
    let live = n - (k - 1);
    let baseline = n - k;
    let findings = wave_tree(live, k - 1, Some(baseline as u32));
    let high = highs(&findings);
    assert_eq!(high.len(), 1, "expected exactly one High, got {findings:?}");
    // Verdict identity, not just the path: an `Exceeded` naming this path
    // must not be confused with some other High also naming it. The summary
    // needle pins WHICH verdict fired, mirroring pdiag.rs's own
    // `live_count_over_the_baseline_row_is_one_high_finding`.
    assert_eq!(high[0].pattern, Pattern::PDiag);
    assert_eq!(high[0].task_id, WAVE_SECTION);
    assert_eq!(high[0].evidence, vec![EvidenceRef::File { path: WAVE_SECTION.to_string() }]);
    assert!(
        high[0].summary.contains(&format!("has {live} code-less"))
            && high[0].summary.contains(&format!("baseline allows {baseline}")),
        "expected an Exceeded verdict naming live={live}/baseline={baseline}, got {:?}",
        high[0].summary
    );
}

#[test]
fn a_wave_introducing_a_new_section_file_is_a_newfile_high() {
    // The other seeded-fire direction, genuinely distinct from the one
    // above (not just a re-parameterization of it): a wave that opens a
    // brand-new section file mid-migration — some sites already coded, the
    // rest not, and NO baseline row at all yet — hits `NewFile`, not
    // `Exceeded`. There is no row to compare against, versus a row that is
    // merely too small; this is the direction a wave that introduces a new
    // file for its section actually trips first.
    let n = 5usize;
    let k = 2usize;
    let live = n - k;
    let findings = wave_tree(live, k, None);
    let high = highs(&findings);
    assert_eq!(high.len(), 1, "expected exactly one High, got {findings:?}");
    assert_eq!(high[0].pattern, Pattern::PDiag);
    assert_eq!(high[0].task_id, WAVE_SECTION);
    assert_eq!(high[0].evidence, vec![EvidenceRef::File { path: WAVE_SECTION.to_string() }]);
    assert!(
        high[0].summary.contains("is new to the baseline")
            && high[0].summary.contains(&format!("has {live} code-less")),
        "expected a NewFile verdict naming live={live}, got {:?}",
        high[0].summary
    );
}

#[test]
fn a_fully_coded_file_must_drop_its_row_rather_than_write_zero() {
    // The trap most likely to bite ρ's first wave: when a wave codes EVERY
    // site in a file, the row must be DELETED, not set to zero.
    // `parse_baseline` rejects a count of 0, so a literal "<path> 0" row is a
    // single High malformed-baseline finding INSTEAD OF the ratchet's
    // verdicts — a red gate, not a clean one.
    let n = 3usize;
    let zero_row = wave_tree_raw(0, n, &format!("{WAVE_SECTION} 0\n"));
    // Assert the WHOLE list, not just its High subset: "INSTEAD OF the
    // ratchet's verdicts" means the malformed-baseline finding is the ONLY
    // finding produced. Filtering through `highs()` first would let a stray
    // Medium ride alongside it undetected, silently passing the one claim
    // this test exists to pin.
    assert_eq!(zero_row.len(), 1, "expected exactly one finding total, got {zero_row:?}");
    assert_eq!(zero_row[0].severity, Severity::High);
    assert!(
        zero_row[0].summary.contains("pdiag-baseline-unreadable"),
        "expected the malformed-baseline finding, not a ratchet verdict, got {:?}",
        zero_row[0].summary
    );

    // The correct fork: DELETE the row instead. Confirmed here rather than
    // assumed: the degenerate-census guard keys on the swept-FILE total, not
    // the code-less total, so a fully-coded-but-still-swept file must not
    // trip it.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write(WAVE_SECTION, &coded_src(n));
    let (swept, _) = fx.summary();
    assert_eq!(swept, 1, "the fully-coded file must still count as swept");

    let dropped_row = wave_tree(0, n, None);
    assert_eq!(dropped_row, Vec::new(), "dropping the row for a fully coded file must be clean");
}

// Both gaps pinned by the pair below were ACCEPTED with the 2026-08-28
// ruling: the per-file COUNT model trades away the line-level precision the
// retired fingerprint-row design would have kept, in exchange for the
// simplicity that made #5405 tractable at all. If a wave ever demonstrates
// that either gap matters in practice, the improvement is filed against
// #5405's own PRD (`docs/prds/v0_6/eradicate-silent-undef.md`) — NEVER as a
// second ratchet over this population (this file's module doc states why).
// That routing is the durable half of κ's item 2: it is what stops the next
// agent who trips delta (a) from rebuilding the design this task retired.

#[test]
fn accepted_delta_a_same_file_code_and_new_uncoded_site_cancel() {
    // ACCEPTED with the 2026-08-28 ruling: the per-file COUNT model — versus
    // the retired line-erased fingerprint-row design — cannot see a same-file
    // swap. Coding one previously code-less site and adding one brand-new
    // code-less site in the same file leaves the count unchanged, so the
    // gate stays clean.
    let n = 5usize;
    let findings = wave_tree(n, 1, Some(n as u32));
    assert_eq!(findings, Vec::new(), "a same-file code+add pair must cancel and stay clean");
}

#[test]
fn accepted_delta_b_coding_without_shrinking_the_row_is_exit_neutral() {
    // ACCEPTED: coding K sites without shrinking the row in the same diff is
    // exit-neutral. The point is the PAIR: the gate stays green (highs()
    // empty) while a Medium advisory IS emitted — the same-diff shrink is a
    // protocol discipline PDIAG *advises* but does not *enforce*, which is
    // exactly why item 1 must write it into the wave protocol as a rule
    // rather than leaving it to the ratchet.
    let n = 5usize;
    let k = 2usize;
    let findings = wave_tree(n - k, k, Some(n as u32));
    assert!(highs(&findings).is_empty(), "the gate must stay green, got {findings:?}");
    assert_eq!(findings.len(), 1, "expected exactly one Medium advisory, got {findings:?}");
    assert_eq!(findings[0].severity, Severity::Medium);
}
