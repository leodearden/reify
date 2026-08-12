//! Corpus gate: constraint SATISFACTION over `examples/best_practices/*.ri`
//! (task #6215).
//!
//! # Invariant asserted
//!
//! Every constraint in every `.ri` file directly under
//! `examples/best_practices/` (flat, non-recursive) reports ZERO
//! `Satisfaction::Violated`, unconditionally — and the set of constraints
//! that report `Satisfaction::Indeterminate` instead of `Satisfied` is PINNED
//! by `EXPECTED_INDETERMINATE` below, checked in both directions: a new,
//! non-listed Indeterminate fails the gate (lost coverage), and a listed
//! entry that is now Satisfied ALSO fails, as stale (the exemption must be
//! deleted rather than linger and mask recovered coverage). This is
//! deliberately NOT "every constraint is Satisfied" — the corpus is not
//! uniformly green today, and both documented exceptions are intentional
//! (see `EXPECTED_INDETERMINATE`'s doc comment).
//!
//! # Why the seeded fire tests exist
//!
//! `seeded_violated_constraint_is_reported` / `seeded_satisfied_constraint_is_reported`
//! below are the MANDATED anti-silent-accept self-tests: the live corpus
//! sweep (see `run_corpus_gate`) is expected GREEN on the measured baseline,
//! so on its own it can never prove the Violated arm actually fires — a
//! `constraint_statuses` that always reported `Satisfied` would make the
//! whole sweep vacuously green. Both sibling debug gates in this crate
//! (`no_stale_undef_invariant_gate.rs`, `snapshot_cache_divergence_gate.rs`)
//! carry the same class of seeded fire test for the same reason.
//!
//! # `EXPECTED_INDETERMINATE` is not a `SKIP_SET`
//!
//! `examples/best_practices/INDEX.md:13` and
//! `.claude/skills/reify-design/SKILL.md:187-190` both forbid adding a
//! `SKIP_SET` entry to this corpus — but that prohibition is scoped to the
//! COMPILE gate ("a file that cannot reach a clean compile does not belong
//! here"). `EXPECTED_INDETERMINATE` exempts no file from anything: every
//! listed constraint is still fully asserted here, just against
//! `Indeterminate` instead of `Satisfied`, and bidirectionally (see above).
//! Do not read this const as a norm violation of the SKIP_SET prohibition —
//! it is a different contract entirely.
//!
//! # Deliberately un-sharded
//!
//! Unlike this crate's two sibling corpus gates (`no_stale_undef_invariant_gate.rs`,
//! `snapshot_cache_divergence_gate.rs`), which shard ~251-file sweeps across
//! 24 `#[test]` fns to stay under the verify pipeline's heartbeat-idle
//! backstop, this gate runs as a single test. `examples/best_practices/` is
//! ~6 files, and the full in-process sweep measures ~0.3s — far short of the
//! backstop, so sharding would be dead weight (`examples_smoke.rs` is
//! likewise un-sharded on purpose, for the same reason).

use reify_core::ConstraintNodeId;
use reify_ir::Satisfaction;

// ── Seeded anti-silent-accept fire tests (step 1/2) ──────────────────────────

/// Minimal template with exactly one constraint, deliberately FALSE:
/// `w = 1mm`, `constraint w > 2mm` is Violated. Feeds
/// `seeded_violated_constraint_is_reported` below — the non-vacuity proof
/// that `constraint_statuses` (and, eventually, the corpus sweep's Violated
/// arm) can actually fire. Mirrors the MANDATED anti-silent-accept
/// seeded-violation self-test both sibling debug gates carry
/// (`no_stale_undef_invariant_gate.rs`, `snapshot_cache_divergence_gate.rs`).
const SEED_VIOLATED_SOURCE: &str = r#"
structure def SeededConstraintGateDemo {
    param w : Length = 1mm

    constraint w > 2mm
}
"#;

/// Same template as `SEED_VIOLATED_SOURCE`, with the one constraint made TRUE
/// instead: `w = 1mm`, `constraint w > 0mm` is Satisfied. Feeds
/// `seeded_satisfied_constraint_is_reported` below — the discrimination
/// proof: a helper that reported `Violated` for every constraint regardless
/// of truth would still pass the seeded-violation test above, so this
/// companion is required too.
const SEED_SATISFIED_SOURCE: &str = r#"
structure def SeededConstraintGateDemo {
    param w : Length = 1mm

    constraint w > 0mm
}
"#;

/// MANDATED anti-silent-accept fire test: a deliberately-false constraint
/// MUST be reported as `Satisfaction::Violated`. A `constraint_statuses` that
/// always returned `Satisfaction::Satisfied` — or an empty vec — would make
/// the eventual corpus sweep's zero-Violated assertion vacuously green.
///
/// RED until `constraint_statuses` exists.
#[test]
fn seeded_violated_constraint_is_reported() {
    let statuses = constraint_statuses(SEED_VIOLATED_SOURCE);
    assert!(
        !statuses.is_empty(),
        "expected at least one constraint result from the seeded source, got zero"
    );
    assert!(
        statuses.iter().any(|(_, s)| *s == Satisfaction::Violated),
        "expected the seeded false constraint (w=1mm, w > 2mm) to report \
         Satisfaction::Violated, got {statuses:?}"
    );
}

/// Discrimination companion to `seeded_violated_constraint_is_reported`
/// above: the SAME template with its one constraint made true must report
/// `Satisfaction::Satisfied`, never `Violated`. Without this test, a
/// `constraint_statuses` that reported `Violated` unconditionally would still
/// pass the test above.
///
/// RED until `constraint_statuses` exists.
#[test]
fn seeded_satisfied_constraint_is_reported() {
    let statuses = constraint_statuses(SEED_SATISFIED_SOURCE);
    assert!(
        !statuses.is_empty(),
        "expected at least one constraint result from the seeded source, got zero"
    );
    assert!(
        statuses.iter().any(|(_, s)| *s == Satisfaction::Satisfied),
        "expected the seeded true constraint (w=1mm, w > 0mm) to report \
         Satisfaction::Satisfied, got {statuses:?}"
    );
    assert!(
        !statuses.iter().any(|(_, s)| *s == Satisfaction::Violated),
        "expected zero Violated results for the seeded true constraint, got {statuses:?}"
    );
}

// ── constraint_statuses: the shared check surface (step 2) ──────────────────

/// Runs `source` through the exact pure value-eval check surface `reify
/// check` uses — `check_source_with_stdlib` is `parse_and_compile_with_stdlib`
/// followed by `make_simple_engine().check(&compiled)`, i.e.
/// `SimpleConstraintChecker` with NO geometry kernel — and extracts each
/// constraint's id and satisfaction, preserving `constraint_results`' order.
///
/// # Panics
/// Panics on a parse or compile error (via `check_source_with_stdlib`). Every
/// caller in this file that walks real corpus files prints the file path to
/// stderr first, so such a panic stays attributable to a file — see
/// `run_corpus_gate`. Re-asserting the zero-Error compile contract itself is
/// deliberately out of scope: `examples_smoke.rs` is the designated compile
/// gate for this corpus.
fn constraint_statuses(source: &str) -> Vec<(reify_core::ConstraintNodeId, Satisfaction)> {
    let result = reify_test_support::check_source_with_stdlib(source);
    result
        .constraint_results
        .into_iter()
        .map(|entry| (entry.id, entry.satisfaction))
        .collect()
}

// ── audit_file: pure per-file failure-taxonomy comparison (steps 3/4) ───────
//
// The tests below unit-test the FULL failure taxonomy of audit_file against
// synthetic inputs only (no filesystem, no real corpus file) — the live
// corpus is green today and can never exercise the Violated,
// UnexpectedIndeterminate, or StaleExpectedIndeterminate arms on its own, so
// these are the only proof each arm actually fires.

/// A single per-constraint audit failure, naming the offending file and
/// constraint. `id`'s `Display` impl (`reify_core::identity`) renders as
/// `{entity}#constraint[{index}]` — the exact spelling `reify check` prints —
/// so a failure is directly reproducible by copy-pasting into the CLI.
#[derive(Debug, Clone, PartialEq)]
enum GateFailure {
    /// A constraint reported `Satisfaction::Violated` — a real regression.
    /// Fires regardless of `expected_indeterminate`: a Violated is never
    /// excusable by the allowlist.
    Violated { file: String, id: ConstraintNodeId },
    /// A constraint reported `Satisfaction::Indeterminate` but its index is
    /// NOT in `expected_indeterminate` — lost coverage (a regression turned
    /// a checked constraint's inputs Undef).
    UnexpectedIndeterminate { file: String, id: ConstraintNodeId },
    /// A constraint's index IS in `expected_indeterminate`, but it now
    /// reports `Satisfaction::Satisfied` — the exemption is stale and must
    /// be deleted from `EXPECTED_INDETERMINATE` instead of lingering and
    /// masking the recovered coverage.
    StaleExpectedIndeterminate { file: String, id: ConstraintNodeId },
}

/// Pure, total, side-effect-free comparison: comparing one file's actual
/// per-constraint `Satisfaction` results against its pinned
/// `expected_indeterminate` index set. No I/O, no panics — this keeps the
/// full failure taxonomy unit-testable without touching the filesystem.
///
/// Precedence, evaluated per entry in `actual`:
///   1. `Violated` -> `GateFailure::Violated`, ALWAYS, regardless of
///      `expected_indeterminate`.
///   2. `Indeterminate` && index not in `expected_indeterminate` ->
///      `GateFailure::UnexpectedIndeterminate`.
///   3. `Satisfied` && index IS in `expected_indeterminate` ->
///      `GateFailure::StaleExpectedIndeterminate`.
///   4. otherwise (`Satisfied` and not expected, or `Indeterminate` and
///      expected) -> nothing.
fn audit_file(
    file: &str,
    expected_indeterminate: &[u32],
    actual: &[(ConstraintNodeId, Satisfaction)],
) -> Vec<GateFailure> {
    let mut failures = Vec::new();
    for (id, satisfaction) in actual {
        match satisfaction {
            Satisfaction::Violated => failures.push(GateFailure::Violated {
                file: file.to_string(),
                id: id.clone(),
            }),
            Satisfaction::Indeterminate if !expected_indeterminate.contains(&id.index) => {
                failures.push(GateFailure::UnexpectedIndeterminate {
                    file: file.to_string(),
                    id: id.clone(),
                });
            }
            Satisfaction::Satisfied if expected_indeterminate.contains(&id.index) => {
                failures.push(GateFailure::StaleExpectedIndeterminate {
                    file: file.to_string(),
                    id: id.clone(),
                });
            }
            _ => {}
        }
    }
    failures
}

/// All actual results Satisfied, empty expected-indeterminate set: nothing to
/// report.
#[test]
fn audit_clean_file_reports_nothing() {
    let actual = vec![
        (ConstraintNodeId::new("Clean", 0), Satisfaction::Satisfied),
        (ConstraintNodeId::new("Clean", 1), Satisfaction::Satisfied),
    ];
    let failures = audit_file("clean.ri", &[], &actual);
    assert!(
        failures.is_empty(),
        "expected zero failures for an all-Satisfied file with an empty \
         expected-indeterminate set, got {failures:?}"
    );
}

/// A Violated constraint must be reported even when its index is (wrongly)
/// listed in `expected_indeterminate` — a Violated is NEVER excusable by the
/// allowlist. This is the arm that catches the ticket's named regression
/// class (a drifted magnitude flipping a constraint to VIOLATED).
#[test]
fn audit_reports_violated_constraint() {
    let id = ConstraintNodeId::new("Bad", 0);
    let actual = vec![(id.clone(), Satisfaction::Violated)];

    let failures = audit_file("bad.ri", &[0], &actual);
    assert_eq!(
        failures,
        vec![GateFailure::Violated {
            file: "bad.ri".to_string(),
            id,
        }],
        "a Violated constraint must be reported even when its index is listed \
         in expected_indeterminate — got {failures:?}"
    );
}

/// An Indeterminate constraint whose index is NOT in the expected set must be
/// reported as `UnexpectedIndeterminate`. This is the anti-coverage-erosion
/// arm: without it, a regression that flips a constraint's inputs Undef
/// (Satisfied -> Indeterminate) would pass the gate silently.
#[test]
fn audit_reports_unexpected_indeterminate() {
    let id = ConstraintNodeId::new("Drifted", 0);
    let actual = vec![(id.clone(), Satisfaction::Indeterminate)];

    let failures = audit_file("drifted.ri", &[], &actual);
    assert_eq!(
        failures,
        vec![GateFailure::UnexpectedIndeterminate {
            file: "drifted.ri".to_string(),
            id,
        }],
        "an Indeterminate constraint whose index is not in expected_indeterminate \
         must be reported as UnexpectedIndeterminate — got {failures:?}"
    );
}

/// An index IS in the expected set, but its actual status is now Satisfied:
/// the exemption is stale and must be reported, so the dead entry gets
/// deleted from `EXPECTED_INDETERMINATE` instead of lingering and masking
/// the recovered coverage. The live corpus is green and can never exercise
/// this arm, so this synthetic test is the only proof it fires.
#[test]
fn audit_reports_stale_expected_indeterminate() {
    let id = ConstraintNodeId::new("Recovered", 0);
    let actual = vec![(id.clone(), Satisfaction::Satisfied)];

    let failures = audit_file("recovered.ri", &[0], &actual);
    assert_eq!(
        failures,
        vec![GateFailure::StaleExpectedIndeterminate {
            file: "recovered.ri".to_string(),
            id,
        }],
        "a Satisfied constraint whose index is listed in expected_indeterminate \
         must be reported as StaleExpectedIndeterminate — got {failures:?}"
    );
}
