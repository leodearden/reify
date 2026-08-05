//! Real-OCCT end-to-end pin test for `intersects(Geometry, Geometry) -> Bool`
//! (task 3612, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-γ).
//!
//! The fixture `examples/kernel_queries/intersects_smoke.ri` contains:
//!
//! ```ri
//! structure def IntersectsSmoke {
//!     let a         = box(10mm, 10mm, 10mm)
//!     let b_overlap = translate(box(10mm, 10mm, 10mm), 5mm, 0mm, 0mm)
//!     let b_far     = translate(box(10mm, 10mm, 10mm), 100mm, 0mm, 0mm)
//!     let overlapping = intersects(a, b_overlap)
//!     let apart       = intersects(a, b_far)
//! }
//! ```
//!
//! Box geometry: `a` is 10 mm × 10 mm × 10 mm centred at origin ⟹ spans ±5 mm
//! (±0.005 m in SI).  `b_overlap` translated 5 mm in X spans 0..10 mm in X —
//! positive-volume overlap with `a` (0..5 mm in X), so BRep min distance = 0.0 →
//! `intersects` = `true`.  `b_far` translated 100 mm in X spans 95..105 mm in X
//! — ~90 mm face gap from `a`, so BRep min distance ≈ 0.09 m > 0.0 →
//! `intersects` = `false`.
//!
//! Dispatch route (task 3612 design decision): routes through
//! `GeometryQuery::Distance{from,to}` classifying `d <= 0.0 → Bool`, identical
//! to the shipped `shapes_intersect` adapter
//! (`reify-kernel-occt/src/lib.rs:770`) and the `interferes_with` helper
//! (`geometry_ops.rs:1601`).
//!
//! The compilation check runs unconditionally so a grammar or compile regression
//! fails on every runner. The kernel build + assertion is gated on
//! `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners without OCCT.
//!
//! Modelled on `crates/reify-eval/tests/kernel_queries_contains.rs` (Bool
//! assertion pattern) and `crates/reify-eval/tests/kernel_queries_distance_smoke.rs`
//! (unconditional compile check + OCCT-gated value assertions).
//!
//! Also pins `examples/best_practices/clearance_oracle.ri`'s eval-surface
//! answers (#5674) via `clearance_oracle_evals_expected_fouls_and_gap` and
//! `clearance_oracle_check_surface_reports_indeterminate_geometry_constraints`
//! below. That fixture is unrelated to KGQ-γ intersects dispatch — the tests
//! live here rather than in their own module file only because #5674's
//! locked scope excludes `harness_kernel_realization.rs` (which would need a
//! new `mod`/`#[path]` registration); splitting them out into
//! `harness_kernel_realization/clearance_oracle_evals.rs` has been filed as
//! a tracked follow-up task rather than left as unanchored prose.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::{ExportFormat, Satisfaction, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const INTERSECTS_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/intersects_smoke.ri"
);

const CLEARANCE_ORACLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/best_practices/clearance_oracle.ri"
);

/// Shared scaffolding for the real-OCCT pin tests in this module: reads
/// `path`, asserts it compiles cleanly — this half runs unconditionally, so
/// a missing fixture or a grammar/compile regression fails on every runner,
/// OCCT or not — then either builds it with a real OCCT kernel and returns
/// the `BuildResult`, or, when OCCT is not available, emits the standard
/// skip line and returns `None`.
///
/// Extracted so `intersects_smoke_evals_expected_booleans` and
/// `clearance_oracle_evals_expected_fouls_and_gap` cannot drift against each
/// other on the skip/build scaffolding — the piece most likely to diverge
/// silently under copy-paste.
fn compile_and_build_with_occt(path: &str, what: &str) -> Option<reify_eval::BuildResult> {
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{what} should exist at {path}: {e}"));

    // Validate fixture compilation unconditionally — a grammar/compile regression
    // should fail on every runner.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "{what} should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip the OCCT-dependent kernel build if OCCT is not built.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }

    // Build with real OCCT kernel (SingleKernelHolder + OcctKernelHandle::spawn).
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

/// Pins the user-observable signal for KGQ-γ: `intersects(Geometry, Geometry)`
/// on two 10 mm boxes must evaluate to `Value::Bool(true)` when the boxes have
/// positive-volume overlap, and `Value::Bool(false)` when they are well apart.
///
/// Dispatches via `GeometryQuery::Distance` classified `d <= 0.0`:
/// - `overlapping`: BRep distance = 0.0 (touching/overlapping) → `true`.
/// - `apart`: BRep distance ≈ 0.09 m (90 mm face gap) → `false`.
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn intersects_smoke_evals_expected_booleans() {
    let Some(result) = compile_and_build_with_occt(
        INTERSECTS_SMOKE_PATH,
        "examples/kernel_queries/intersects_smoke.ri",
    ) else {
        return;
    };

    // Helper: assert a Bool cell on IntersectsSmoke equals the expected value.
    let assert_bool = |cell_name: &str, expected: bool| {
        let cell = ValueCellId::new("IntersectsSmoke", cell_name);
        let actual = result.values.get(&cell);
        assert_eq!(
            actual,
            Some(&Value::Bool(expected)),
            "IntersectsSmoke.{cell_name} should be Value::Bool({expected}), got: {actual:?}"
        );
    };

    // b_overlap translated 5mm in X → overlaps a (both span ±5mm centred at origin)
    // by 5mm in X → BRep min distance = 0.0 → intersects = true.
    assert_bool("overlapping", true);

    // b_far translated 100mm in X → spans 95..105mm in X, ~90mm face gap from a
    // → BRep min distance ≈ 0.09m > 0.0 → intersects = false.
    assert_bool("apart", false);

    // Pin §4 invariant #1: an inline geometry arg (CompiledExprKind::FunctionCall,
    // not ValueRef) is rejected by resolve_geometry_handle_arg → dispatch arm
    // short-circuits (returns None) → cell stays at its compiled default
    // (None or Value::Undef — never a Bool).  The short-circuit happens before
    // any kernel call, but engine.build() still requires OCCT for the other
    // geometry cells in this fixture, so the assertion lives here.
    let undef_cell = ValueCellId::new("IntersectsSmoke", "undef_inline");
    let undef_actual = result.values.get(&undef_cell);
    assert!(
        matches!(undef_actual, None | Some(&Value::Undef)),
        "IntersectsSmoke.undef_inline must be None or Value::Undef (inline arg \
         falls through resolve_geometry_handle_arg per §4 invariant #1), \
         got: {undef_actual:?}"
    );
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
    // Deliberately NOT keyed by `ConstraintNodeId` index: the fixture's
    // other three constraints (`bore_r > shaft_r`, `min_gap > 0mm`,
    // `offset_x > bore_r`) are ALSO trivially Satisfied on build(), so an
    // index-based lookup would silently start asserting against one of
    // those the moment the fixture gains or reorders a constraint —
    // defeating the very regression this block exists to catch. Asserting
    // the full set's length AND uniform satisfaction instead catches the
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
/// Counts, not `ConstraintNodeId` indices, for the same reordering-fragility
/// reason as the sibling build-surface test.
#[test]
fn clearance_oracle_check_surface_reports_indeterminate_geometry_constraints() {
    // Read + compile unconditionally, matching the unconditional half of
    // every sibling oracle pin in this module.
    let source = std::fs::read_to_string(CLEARANCE_ORACLE_PATH)
        .expect("examples/best_practices/clearance_oracle.ri should exist (task 5389)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/best_practices/clearance_oracle.ri should compile with no \
         error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
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
    let indeterminate_count = result
        .constraint_results
        .iter()
        .filter(|entry| entry.satisfaction == Satisfaction::Indeterminate)
        .count();
    let satisfied_count = result
        .constraint_results
        .iter()
        .filter(|entry| entry.satisfaction == Satisfaction::Satisfied)
        .count();
    assert_eq!(
        indeterminate_count, 2,
        "expected exactly 2 Indeterminate constraints (`not fouls`, `gap > min_gap`) \
         on the check surface, got {indeterminate_count} — full results: {:#?}",
        result.constraint_results
    );
    assert_eq!(
        satisfied_count, 3,
        "expected exactly 3 Satisfied constraints (the ordinary value-surface \
         constraints `bore_r > shaft_r`, `min_gap > 0mm`, `offset_x > bore_r`) \
         on the check surface, got {satisfied_count} — full results: {:#?}",
        result.constraint_results
    );
}
