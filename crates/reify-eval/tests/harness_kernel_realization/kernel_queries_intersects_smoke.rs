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
//! Modelled on `crates/reify-eval/tests/harness_kernel_realization/kernel_queries_contains.rs` (Bool
//! assertion pattern) and `crates/reify-eval/tests/harness_kernel_realization/kernel_queries_distance_smoke.rs`
//! (unconditional compile check + OCCT-gated value assertions).
//!
//! The `read_and_compile_fixture`/`compile_and_build_with_occt` scaffolding
//! below is `pub(crate)` and shared with the sibling
//! `best_practices_clearance_oracle` module (task #5982 split out of #5674's
//! co-tenancy fallback), which pins `examples/best_practices/clearance_oracle.ri`'s
//! eval-surface answers using the same read/compile/skip-without-OCCT pattern.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const INTERSECTS_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/intersects_smoke.ri"
);

/// Reads `path` and parses+compiles it against the stdlib, asserting it
/// compiles cleanly. Runs unconditionally — no OCCT dependency — so a
/// missing fixture or a grammar/compile regression fails on every runner.
///
/// Extracted out of `compile_and_build_with_occt` so the read/compile/
/// assert-clean scaffolding has exactly one implementation shared by every
/// pin test that uses it — including the sibling `best_practices_clearance_oracle`
/// module's `clearance_oracle_check_surface_reports_indeterminate_geometry_constraints`
/// and `indeterminate_ids_on_check_surface`, which need a compiled fixture
/// but never touch OCCT — instead of the risk of two copies (one here, one
/// inlined in a kernel-less test) silently drifting under copy-paste.
/// `pub(crate)` so the sibling module can reuse it (task #5982 split).
pub(crate) fn read_and_compile_fixture(path: &str, what: &str) -> reify_compiler::CompiledModule {
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
    compiled
}

/// Shared scaffolding for the real-OCCT pin tests in this module: compiles
/// `path` via `read_and_compile_fixture`, then either builds it with a real
/// OCCT kernel and returns the `BuildResult`, or, when OCCT is not
/// available, emits the standard skip line (naming `what`) and returns
/// `None`.
///
/// Extracted so `intersects_smoke_evals_expected_booleans` and the sibling
/// `best_practices_clearance_oracle` module's
/// `clearance_oracle_evals_expected_fouls_and_gap` cannot drift against each
/// other on the skip/build scaffolding — the piece most likely to diverge
/// silently under copy-paste. `pub(crate)` so the sibling module can reuse
/// it (task #5982 split).
pub(crate) fn compile_and_build_with_occt(
    path: &str,
    what: &str,
) -> Option<reify_eval::BuildResult> {
    let compiled = read_and_compile_fixture(path, what);

    // Skip the OCCT-dependent kernel build if OCCT is not built.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions for {what}: OCCT not available");
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
