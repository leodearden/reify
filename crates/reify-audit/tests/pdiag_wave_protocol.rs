//! Wave-protocol contract pin for PDIAG (task #6768, against landed #5405).
//!
//! `docs/prds/v0_6/spec-conformance-suite.md`'s Phase-3 wave protocol
//! mandates that each rejection-surface wave (leaf ρ) codes its section's
//! `Diagnostic::error`/`Diagnostic::warning` construction sites and shrinks
//! that file's row in `crates/reify-audit/pdiag-baseline.txt` IN THE SAME
//! DIFF. This file pins the PDIAG-side contract that mandate leans on: the
//! composed before/after transaction, the two seeded-fire directions that
//! make mandating a same-diff shrink safe, the zero-row deletion rule, and
//! the two model deltas accepted with the 2026-08-28 ruling.
//!
//! PDIAG (`reify_audit::pdiag`, #5405) is the ONE ratchet over the
//! `Diagnostic::error`/`Diagnostic::warning`-without-code population. A
//! suite-local second ratchet over the same population is FORBIDDEN — that
//! is precisely the rescope task #6768 exists to install. Every assertion
//! below drives `pdiag::check` through its public
//! `pdiag::test_support::Fixture` seam; nothing here re-derives a census, a
//! count, or a baseline parse.
//!
//! This file deliberately does NOT restate PDIAG's own static-census
//! coverage — `ratchet()`'s four verdicts are exhaustively unit-tested in
//! `crates/reify-audit/src/pdiag.rs`, and the generator seam is pinned by
//! `crates/reify-audit/tests/pdiag_baseline.rs`. What is pinned here is the
//! composed wave TRANSACTION those unit tests never compose: a before/after
//! pair with coding-as-the-mechanism.
//!
//! Nor does this file restate the remediation menu (attach a
//! `DiagnosticCode` / take the reviewed `pdiag:allow` opt-out / shrink the
//! row same-commit / regenerate on a move-or-rename) — that lives in
//! `docs/notes/diagnostic-severity-policy.md` §3, which `pdiag.rs`'s own
//! High findings already cite. A third copy here would be exactly the
//! two-copy drift the suite PRD's G7 walk warns against.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test pdiag_wave_protocol`

use reify_audit::pdiag::test_support::{Fixture, codeless_src, coded_src};
use reify_audit::{Finding, Severity};

/// The one fixed swept path every test in this file shares. `crates/<name>/src/`
/// is required by `pdiag::is_swept_path`; `reify-compiler` is an arbitrary
/// swept crate distinct from the detector's own — `crates/reify-audit/` is
/// scope-excluded (the detector cannot sweep itself; see `pdiag.rs`'s
/// `SCOPE_EXCLUDE_PREFIXES`).
const WAVE_SECTION: &str = "crates/reify-compiler/src/wave_section.rs";

/// Build a tempdir tree with exactly ONE swept file at [`WAVE_SECTION`],
/// composed of `coded` coded sites followed by `codeless` code-less ones,
/// plant `raw_baseline` verbatim as the manifest content, and run the
/// detector end to end.
///
/// The shared builder behind [`wave_tree`] AND the raw-row escape a
/// deliberately malformed row (e.g. a literal `0`, which `parse_baseline`
/// rejects) needs: taking the bytes directly here is what keeps that escape
/// from duplicating the fixture setup.
fn wave_tree_raw(codeless: usize, coded: usize, raw_baseline: &str) -> Vec<Finding> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write(WAVE_SECTION, &format!("{}{}", coded_src(coded), codeless_src(codeless)));
    fx.baseline(raw_baseline);
    fx.run()
}

/// [`wave_tree_raw`], for the common case of a well-formed baseline row:
/// `row` plants that file's baseline allowance (`None` omits the row — an
/// empty baseline, the "no row" idiom `pdiag.rs`'s own tests already use).
///
/// The ONE tree-builder every ordinary test in this file shares, so a wave's
/// before/after pair reads as a two-line change of counts rather than
/// duplicated fixture setup. `row` is always a real ratchet count (>= 1):
/// `parse_baseline` itself rejects `0`, so a test that needs exactly that
/// malformed row goes through [`wave_tree_raw`] instead of here.
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
    // mandating a way to launder sites past the gate.
    let n = 5usize;
    let k = 2usize;
    let findings = wave_tree(n - (k - 1), k - 1, Some((n - k) as u32));
    let high = highs(&findings);
    assert_eq!(high.len(), 1, "expected exactly one High, got {findings:?}");
    assert!(
        high[0].summary.contains(WAVE_SECTION),
        "{WAVE_SECTION} missing from {:?}",
        high[0].summary
    );
}

#[test]
fn a_new_uncoded_site_added_during_a_wave_reds_against_the_shrunk_row() {
    // The second half of κ's user-observable signal: start from step-1's
    // clean AFTER state (coded K / code-less N-K, row N-K), then add ONE
    // further code-less site during the same wave.
    let n = 5usize;
    let k = 2usize;
    let findings = wave_tree(n - k + 1, k, Some((n - k) as u32));
    let high = highs(&findings);
    assert_eq!(high.len(), 1, "expected exactly one High, got {findings:?}");
    assert!(
        high[0].summary.contains(WAVE_SECTION),
        "{WAVE_SECTION} missing from {:?}",
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
    let high = highs(&zero_row);
    assert_eq!(high.len(), 1, "expected exactly one High, got {zero_row:?}");
    assert!(
        high[0].summary.contains("pdiag-baseline-unreadable"),
        "expected the malformed-baseline finding, not a ratchet verdict, got {:?}",
        high[0].summary
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
