//! Pin: the machine-readable count lines in
//! `docs/architecture-audit/g-tool-baseline-report.md` must stay within
//! a small tolerance of the live `scripts/audit-orphan-producers.sh` output,
//! so the doc cannot silently re-stale without a test failure.
//!
//! User-observable signal (on-demand):
//!   `cargo test -p reify-audit --test baseline_report_freshness -- --ignored`
//!
//! Anti-gaming rationale: the test is intentionally `#[ignore]`d because
//! (i) the underlying script is documented as corpus-level / reviewer-cadence
//! (scripts/audit-orphan-producers.sh header lines 12-14: "corpus-level only
//! … Reviewers run it at `/review` cadence or on demand"); (ii) natural orphan
//! churn (~+1.5/day historically) against the small ORPHAN_TOL=8 would
//! otherwise convert this into a time-bomb that blocks unrelated PRs within
//! ~a week; (iii) reviewers, `/audit` runs, and ad-hoc verification still
//! invoke it via `-- --ignored`, so the anti-re-staling intent is preserved.
//! If you hit it: regenerate the doc per the "How to regenerate" section in
//! the report (splice below the preamble, bump `**Captured:**`).
//!
//! Graceful skip: if `python3`, `git`, or the audit script are absent
//! from PATH/disk the test prints a note to stderr and returns without
//! failing.  The shared helper is `reify_test_support::run_orphan_audit`.

use std::path::Path;

use reify_test_support::run_orphan_audit;

/// How many orphan candidates the live count may drift from the doc's declared
/// value before the on-demand check trips.
///
/// Rationale: historical churn ≈ +9 orphans / 6 days ≈ 1.5/day.  A tolerance
/// of 8 catches real drift since the last regeneration when the test is invoked
/// on demand (`-- --ignored`), while giving a few days of slack for minor churn.
/// (Historical context: this value was originally chosen so that the stale doc
/// at step-1 — drift=9, doc=425 vs live=434 — would FAIL the TDD-red step,
/// while a freshly regenerated doc would PASS with ample margin.)
const ORPHAN_TOL: i64 = 8;

/// How many allow-listed entries the live count may drift from the doc's
/// declared value.  Changes to `// G-allow:` markers are deliberate and rare,
/// so a tight tolerance is appropriate.
const ALLOWED_TOL: i64 = 3;

#[ignore = "on-demand drift check; run via --ignored. Tolerance trips on natural orphan churn \
    (~+1.5/day) and would block unrelated PRs if always-on. Aligned with \
    scripts/audit-orphan-producers.sh review-cadence operating model."]
#[test]
fn baseline_report_counts_are_fresh() {
    // --- 1. Run live audit (graceful-skip if env not available) ---
    let Some(result) = run_orphan_audit("crates/reify-*/src") else {
        return;
    };

    let live_orphans = result["orphan_count"]
        .as_u64()
        .expect("orphan_count field present in JSON output") as i64;
    let live_allowed = result["allowed_count"]
        .as_u64()
        .expect("allowed_count field present in JSON output") as i64;

    // --- 2. Resolve the doc path ---
    // CARGO_MANIFEST_DIR = crates/reify-audit → two .parent() → repo root
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let doc_path = Path::new(manifest_dir)
        .parent()
        .expect("crates/reify-audit has a parent (crates/)")
        .parent()
        .expect("crates/ has a parent (repo root)")
        .join("docs/architecture-audit/g-tool-baseline-report.md");

    let doc = std::fs::read_to_string(&doc_path).unwrap_or_else(|e| {
        panic!(
            "failed to read baseline report at {:?}: {e}",
            doc_path
        )
    });

    // --- 3. Parse declared counts from the doc ---
    // Expect lines like:
    //   - **Orphan candidates:** 425  (zero non-test callers, no `// G-allow:`)
    //   - **Allow-listed:** 28  (zero callers; marked legitimate API surface)
    let doc_orphans = parse_count(&doc, "Orphan candidates")
        .unwrap_or_else(|| {
            panic!(
                "could not find '- **Orphan candidates:** <N>' in {:?}",
                doc_path
            )
        });
    let doc_allowed = parse_count(&doc, "Allow-listed")
        .unwrap_or_else(|| {
            panic!(
                "could not find '- **Allow-listed:** <N>' in {:?}",
                doc_path
            )
        });

    // --- 4. Assert bounded drift ---
    let orphan_drift = (doc_orphans - live_orphans).abs();
    assert!(
        orphan_drift <= ORPHAN_TOL,
        "Baseline report orphan count is stale: doc says {doc_orphans}, live audit says \
         {live_orphans} (drift = {orphan_drift}, tolerance = {ORPHAN_TOL}).\n\
         Regenerate the doc:\n\
         1. Run:  ./scripts/audit-orphan-producers.sh --format markdown\n\
         2. Preserve lines 1-74 of the doc (the hand-written preamble) verbatim,\n\
         3.   except bump `**Captured:**` to today's date.\n\
         4. Replace lines 75-EOF with the fresh script output.\n\
         See 'How to regenerate' in docs/architecture-audit/g-tool-baseline-report.md."
    );

    let allowed_drift = (doc_allowed - live_allowed).abs();
    assert!(
        allowed_drift <= ALLOWED_TOL,
        "Baseline report allow-listed count is stale: doc says {doc_allowed}, live audit says \
         {live_allowed} (drift = {allowed_drift}, tolerance = {ALLOWED_TOL}).\n\
         Regenerate the doc per the 'How to regenerate' section, or investigate \
         unexpected changes to `// G-allow:` markers."
    );
}

/// Parse a count from a doc line of the form `- **<label>:** <N>`.
/// Returns `None` if no such line exists; panics if the number cannot be parsed.
///
/// Only searches in the machine-generated section that begins at the
/// `# Orphan-producer audit` heading, so preamble prose (which may contain
/// bullet points with the same label keywords) can never shadow the real
/// count line.
fn parse_count(doc: &str, label: &str) -> Option<i64> {
    // Restrict to the generated body to guard against future preamble edits
    // that might accidentally match the needle (e.g. a bullet like
    // `- **Orphan candidates:** are ...` in an explanatory paragraph).
    let search_region = doc
        .find("# Orphan-producer audit")
        .map(|pos| &doc[pos..])
        .unwrap_or(doc);
    let needle = format!("- **{label}:**");
    let line = search_region.lines().find(|l| l.contains(&needle))?;
    // Everything after the colon+space is the number (possibly followed by
    // two spaces and a parenthetical comment like "  (zero non-test callers …)").
    let after_colon = line
        .split_once(&format!("**{label}:**"))
        .map(|(_, rest)| rest)
        .unwrap_or(line)
        .trim_start();
    // The number is the first whitespace-delimited token.
    let token = after_colon.split_whitespace().next().unwrap_or_default();
    Some(
        token
            .parse::<i64>()
            .unwrap_or_else(|e| panic!("could not parse count for '{label}' from token {token:?} on line {line:?}: {e}")),
    )
}
