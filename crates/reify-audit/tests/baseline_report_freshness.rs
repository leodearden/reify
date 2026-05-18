//! Pin: the machine-readable count lines in
//! `docs/architecture-audit/g-tool-baseline-report.md` must stay within
//! a small tolerance of the live `scripts/audit-orphan-producers.sh` output,
//! so the doc cannot silently re-stale without a test failure.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test baseline_report_freshness`
//!
//! Anti-gaming rationale: the tolerance constants are tuned so that the
//! current stale doc (Orphan candidates: 425, live ~434, drift = 9) FAILS
//! (red), while a freshly regenerated doc (drift ≈ 0) PASSES with comfortable
//! margin (green).  The tolerance is intentionally small — it tolerates at
//! most a few days of normal code churn (~+1.5 orphans/day historically)
//! before tripping, making this a periodic freshness tripwire rather than an
//! unconditional gating check.  If you hit it: regenerate the doc per the
//! "How to regenerate" section in the report (splice below the preamble,
//! bump `**Captured:**`).
//!
//! Graceful skip: if `python3`, `git`, or the audit script are absent
//! from PATH/disk the test prints a note to stderr and returns without
//! failing.  The shared helper is `reify_test_support::run_orphan_audit`.

use std::path::Path;

use reify_test_support::run_orphan_audit;

/// How many orphan candidates the live count may drift from the doc's declared
/// value before the test trips.
///
/// Rationale: historical churn ≈ +9 orphans / 6 days ≈ 1.5/day.  A tolerance
/// of 8 tolerates ~5 days of drift but fails at the current stale-doc drift
/// of 9 (425 doc vs ~434 live), satisfying the TDD-red requirement.  After
/// regeneration the drift will be ≈ 0, giving ample margin.
const ORPHAN_TOL: i64 = 8;

/// How many allow-listed entries the live count may drift from the doc's
/// declared value.  Changes to `// G-allow:` markers are deliberate and rare,
/// so a tight tolerance is appropriate.
const ALLOWED_TOL: i64 = 3;

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
fn parse_count(doc: &str, label: &str) -> Option<i64> {
    let needle = format!("- **{label}:**");
    let line = doc.lines().find(|l| l.contains(&needle))?;
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
