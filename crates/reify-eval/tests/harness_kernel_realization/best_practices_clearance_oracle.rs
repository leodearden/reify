//! Pins `examples/best_practices/clearance_oracle.ri`'s eval-surface answers
//! (#5674) via `clearance_oracle_evals_expected_fouls_and_gap` and
//! `clearance_oracle_check_surface_reports_indeterminate_geometry_constraints`
//! below. That fixture is unrelated to KGQ-γ intersects dispatch
//! (`kernel_queries_intersects_smoke.rs`) — these tests used to co-tenant
//! that file only because #5674's locked scope excluded
//! `harness_kernel_realization.rs` (which needed a new `mod`/`#[path]`
//! registration to split them out); this module is that follow-up (#5982).
//!
//! Reuses `kernel_queries_intersects_smoke`'s `read_and_compile_fixture` /
//! `compile_and_build_with_occt` scaffolding (`pub(crate)` there for this
//! purpose) rather than duplicating the read/compile/skip-without-OCCT
//! pattern.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::{Satisfaction, Value};

use super::kernel_queries_intersects_smoke::{
    compile_and_build_with_occt, read_and_compile_fixture,
};

const CLEARANCE_ORACLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/best_practices/clearance_oracle.ri"
);

/// Runs the pure check surface (kernel-less `Engine::check`, mirroring
/// `clearance_oracle_check_surface_reports_indeterminate_geometry_constraints`)
/// for `clearance_oracle.ri` and returns the `ConstraintNodeId`s that come
/// back `Satisfaction::Indeterminate` — the geometry-gated constraints
/// (`not fouls`, `gap > min_gap`) that cannot resolve without a kernel.
///
/// Shared by `clearance_oracle_check_surface_reports_indeterminate_geometry_constraints`
/// (which pins this set's size) and `clearance_oracle_evals_expected_fouls_and_gap`
/// (which pins that *exactly these ids* — not merely "5 constraints, all
/// Satisfied" — flip to `Satisfied` once the geometry-consumer builtins
/// resolve on `build()`). Factoring this out means a fixture edit that
/// changes *which* constraint is geometry-gated cannot silently leave both
/// tests green while no longer pinning the fixture header's actual claim:
/// both tests always agree on what "the geometry-gated pair" means. Each
/// caller gets an independently compiled+checked fixture — no shared state
/// or ordering dependency between the two `#[test]` functions.
fn indeterminate_ids_on_check_surface() -> std::collections::HashSet<reify_core::ConstraintNodeId> {
    let compiled = read_and_compile_fixture(
        CLEARANCE_ORACLE_PATH,
        "examples/best_practices/clearance_oracle.ri",
    );
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);
    result
        .constraint_results
        .into_iter()
        .filter(|entry| entry.satisfaction == Satisfaction::Indeterminate)
        .map(|entry| entry.id)
        .collect()
}

/// Pins the eval-surface answers documented in `examples/best_practices/
/// clearance_oracle.ri` (the authority for parameters and expected values;
/// #5674, filed as an out-of-scope follow-up by #5389): the canonical FORM B
/// (no-ceremony) interference-oracle idiom was compile-gated only
/// (`examples_smoke.rs::all_examples_parse_and_compile_with_stdlib`), with no
/// test pinning the values `reify eval` / the GUI actually resolve.
///
/// Derivation: `housing = cylinder_centered(bore_r=10mm, length=40mm)`
/// centred at the origin has its lateral surface at x = 10mm; `bracket =
/// translate(box(20mm,20mm,20mm), 30mm,0mm,0mm)` spans x = 20..40mm — no
/// overlap, so `fouls = intersects(housing, bracket) -> false` and
/// `gap = distance(housing, bracket) -> 0.01 m` (20mm − 10mm).
///
/// Skips cleanly (via early return) when OCCT is not available, matching the
/// sibling oracle pins (`kernel_queries_distance_smoke.rs`,
/// `cli_vc_clearance.rs`, `kinematic_examples_e2e.rs`).
#[test]
fn clearance_oracle_evals_expected_fouls_and_gap() {
    let Some(result) = compile_and_build_with_occt(
        CLEARANCE_ORACLE_PATH,
        "examples/best_practices/clearance_oracle.ri",
    ) else {
        return;
    };

    let fouls_cell = ValueCellId::new("ClearanceOracle", "fouls");
    let fouls_actual = result.values.get(&fouls_cell);
    assert_eq!(
        fouls_actual,
        Some(&Value::Bool(false)),
        "ClearanceOracle.fouls should be Value::Bool(false) per the file's own \
         header comment, got: {fouls_actual:?}"
    );

    let gap_cell = ValueCellId::new("ClearanceOracle", "gap");
    let gap_actual = result.values.get(&gap_cell);

    // Allow a small floating-point epsilon on the si_value while requiring the
    // LENGTH dimension. Unlike kernel_queries_distance_smoke.rs's box-point
    // case (an exactly representable closest-feature pair), the closest
    // features here are a cylinder's lateral surface and a planar box face,
    // which OCCT resolves through its extrema machinery at a working
    // tolerance around `Precision::Confusion()` (1e-7 m) — so the bound below
    // is chosen relative to the kernel's own extrema tolerance, not copied
    // from the planar box-point case. The semantic margin (min_gap = 1mm vs
    // gap = 10mm) is enormous, so 1µm is ample without being version-fragile.
    match gap_actual {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == reify_core::DimensionVector::LENGTH => {
            let expected = 0.01_f64; // 10 mm in SI metres, per the header comment
            let epsilon = 1e-6;
            assert!(
                (si_value - expected).abs() < epsilon,
                "ClearanceOracle.gap si_value should be 0.01 (10 mm), \
                 got {si_value:.15} (delta {delta:.3e})",
                delta = (si_value - expected).abs()
            );
        }
        other => panic!(
            "ClearanceOracle.gap should be Value::Scalar{{LENGTH, ≈0.01}}, got: {:?}",
            other
        ),
    }

    // Pin the fixture's own documented `reify check` behaviour (its
    // "EVAL/BUILD ONLY" header section): `constraint not fouls` and
    // `constraint gap > min_gap` are `Indeterminate` under `check()` alone
    // but must resolve on the build() surface once the geometry-consumer
    // builtins they depend on (`intersects`/`distance`) are realized (the
    // task-4229 post-geometry re-check, `engine_build.rs` `BuildResult`
    // construction). Asserting this — not just the raw `fouls`/`gap` cell
    // values above — pins the "run it on a surface that can answer it"
    // promise the fixture's header makes: a regression that stalls these
    // constraints at `Indeterminate` on `build()` while the value cells
    // still populate from the same kernel queries would otherwise leave
    // this test green.
    //
    // Deliberately NOT keyed by a hardcoded `ConstraintNodeId` literal (e.g.
    // `ConstraintNodeId::new("ClearanceOracle", 0)`): the fixture's other
    // three constraints (`bore_r > shaft_r`, `min_gap > 0mm`, `offset_x >
    // bore_r`) are ALSO trivially Satisfied on build(), so a hardcoded
    // position would silently start asserting against one of those the
    // moment the fixture gains or reorders a constraint — defeating the
    // very regression this block exists to catch. Asserting the full set's
    // length AND uniform satisfaction instead catches the
    // Indeterminate-stall regression and a dropped constraint, and cannot
    // be defeated by fixture reordering.
    assert_eq!(
        result.constraint_results.len(),
        5,
        "ClearanceOracle should report exactly 5 constraint results (not fouls, \
         gap > min_gap, bore_r > shaft_r, min_gap > 0mm, offset_x > bore_r), \
         got: {:#?}",
        result.constraint_results
    );
    assert!(
        result
            .constraint_results
            .iter()
            .all(|entry| entry.satisfaction == Satisfaction::Satisfied),
        "every ClearanceOracle constraint should be Satisfaction::Satisfied on the \
         build() surface (this includes `not fouls` / `gap > min_gap`, which are \
         only Indeterminate on the pure check surface — see the companion test \
         clearance_oracle_check_surface_reports_indeterminate_geometry_constraints), \
         got: {:#?}",
        result.constraint_results
    );

    // The blanket "all 5 Satisfied" assertion above cannot distinguish a
    // correctly re-resolved `not fouls`/`gap > min_gap` pair from a fixture
    // edit that swaps in some other geometry-gated constraint while the
    // resolution machinery silently stops covering the *original* pair —
    // both would still show "5 results, all Satisfied". Bind the specific
    // identities instead: whichever `ConstraintNodeId`s the pure check
    // surface reports `Indeterminate` (discovered dynamically via
    // `indeterminate_ids_on_check_surface`, never a hardcoded literal id —
    // same non-fragility rationale as above) must show up `Satisfied` right
    // here on build(), pinning the "run it on a surface that can answer it"
    // promise to those specific constraints.
    let geometry_gated_ids = indeterminate_ids_on_check_surface();
    assert_eq!(
        geometry_gated_ids.len(),
        2,
        "expected exactly 2 geometry-gated constraint ids from the check \
         surface (`not fouls`, `gap > min_gap`), got: {geometry_gated_ids:#?}"
    );
    for id in &geometry_gated_ids {
        let build_entry = result
            .constraint_results
            .iter()
            .find(|entry| &entry.id == id)
            .unwrap_or_else(|| {
                panic!(
                    "constraint {id} was Indeterminate on the check surface but \
                     is missing entirely from build()'s constraint_results, \
                     got: {:#?}",
                    result.constraint_results
                )
            });
        assert_eq!(
            build_entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {id} was Indeterminate on the pure check surface \
             (geometry-gated) but did not resolve to Satisfied on build(), \
             got {:?}. Full results: {:#?}",
            build_entry.satisfaction,
            result.constraint_results
        );
    }
}

/// Companion to `clearance_oracle_evals_expected_fouls_and_gap`: pins the
/// OTHER half of the fixture header's "EVAL/BUILD ONLY" contract — the pure
/// check/eval surface (kernel-less `Engine`), where `intersects`/`distance`
/// are geometry-consumer builtins that CANNOT resolve. Runs unconditionally
/// on every runner (no OCCT needed), so it also raises the value of the
/// otherwise-unconditional half of the sibling test.
///
/// Expected on `Engine::check` with `kernel: None`:
/// - `fouls`/`gap` each emit a `DiagnosticCode::EvalUnresolved` error (one
///   for the `intersects` call, one for the `distance` call) — the fixture
///   header's quoted "`intersects` could not be resolved: ..." message.
/// - `ClearanceOracle#constraint[0]` (`not fouls`) and `#constraint[1]`
///   (`gap > min_gap`) come back `Satisfaction::Indeterminate`; the other
///   three ordinary value-surface constraints (`bore_r > shaft_r`,
///   `min_gap > 0mm`, `offset_x > bore_r`) come back `Satisfaction::Satisfied`.
///
/// The Indeterminate *count* here is pinned by counting, not by a hardcoded
/// `ConstraintNodeId` literal, for the same reordering-fragility reason as
/// the sibling build-surface test. The specific *identities* backing that
/// count come from `indeterminate_ids_on_check_surface` (the same helper
/// `clearance_oracle_evals_expected_fouls_and_gap` uses to cross-check the
/// build() surface), so the two tests can never silently disagree on which
/// ids "the geometry-gated pair" refers to — see that helper's doc comment.
#[test]
fn clearance_oracle_check_surface_reports_indeterminate_geometry_constraints() {
    // Read + compile unconditionally, matching the unconditional half of
    // every sibling oracle pin in the harness.
    let compiled = read_and_compile_fixture(
        CLEARANCE_ORACLE_PATH,
        "examples/best_practices/clearance_oracle.ri",
    );

    // Kernel-less engine: `intersects`/`distance` are geometry-consumer
    // builtins and cannot resolve here, per the fixture's own header.
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let unresolved_messages: Vec<&str> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::EvalUnresolved) && d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        unresolved_messages.len(),
        2,
        "expected exactly 2 EvalUnresolved errors (one for `fouls = intersects(...)`, \
         one for `gap = distance(...)`), got {} — full diagnostics: {:#?}",
        unresolved_messages.len(),
        result.diagnostics
    );
    assert!(
        unresolved_messages.iter().any(|m| m.contains("intersects")),
        "expected an EvalUnresolved message naming `intersects`, got: {unresolved_messages:#?}"
    );
    assert!(
        unresolved_messages.iter().any(|m| m.contains("distance")),
        "expected an EvalUnresolved message naming `distance`, got: {unresolved_messages:#?}"
    );

    assert_eq!(
        result.constraint_results.len(),
        5,
        "ClearanceOracle should report exactly 5 constraint results on the check \
         surface too, got: {:#?}",
        result.constraint_results
    );
    // Computed via the shared helper (see its doc comment) rather than an
    // inline filter, so this test and the build-surface test's cross-check
    // can never define "the geometry-gated pair" two different ways.
    let indeterminate_ids = indeterminate_ids_on_check_surface();
    assert_eq!(
        indeterminate_ids.len(),
        2,
        "expected exactly 2 Indeterminate constraints (`not fouls`, `gap > min_gap`) \
         on the check surface, got: {indeterminate_ids:#?} — full results: {:#?}",
        result.constraint_results
    );

    let satisfied_count = result
        .constraint_results
        .iter()
        .filter(|entry| entry.satisfaction == Satisfaction::Satisfied)
        .count();
    assert_eq!(
        satisfied_count, 3,
        "expected exactly 3 Satisfied constraints (the ordinary value-surface \
         constraints `bore_r > shaft_r`, `min_gap > 0mm`, `offset_x > bore_r`) \
         on the check surface, got {satisfied_count} — full results: {:#?}",
        result.constraint_results
    );
}
