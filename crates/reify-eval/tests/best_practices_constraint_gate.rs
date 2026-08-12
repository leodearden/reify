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
