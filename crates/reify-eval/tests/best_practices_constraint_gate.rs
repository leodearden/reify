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
