//! End-to-end tests for `restrict(field, region)` field evaluation
//! (task 4222 δ, PRD docs/prds/v0_6/std-fields-api.md §5.3 / B5).
//!
//! **step-5 RED** — test `impl ContainmentQuery for Engine`.
//!   Build an Engine with a real OCCT kernel, realize a box geometry, obtain
//!   its `Value::GeometryHandle`, then call the `ContainmentQuery` trait method
//!   directly on `engine`:
//!   - `engine.contains(&solid_handle, &inside_point)` → `Some(true)`
//!   - `engine.contains(&solid_handle, &outside_point)` → `Some(false)`
//!   - `engine.contains(&Value::Real(0.0), &inside_point)` → `None` (non-geometry)
//!   - `engine.contains(&solid_handle, &Value::Real(0.0))` → `None` (non-point)
//!     Skips the OCCT-dependent assertions cleanly when OCCT is not available.
//!
//! **step-7 RED→GREEN** — B5 integration test loading `examples/fields/restrict.ri`.
//!   Asserts `v_in == Value::Real(42.0)` (inside) and `v_out == Value::Undef`
//!   (outside) after full Engine build.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, ValueCellId};
use reify_expr::ContainmentQuery;
use reify_ir::{ExportFormat, GeometryHandleId, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const CONTAINS_BOX_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/contains_box.ri"
);

const RESTRICT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/restrict.ri"
);

// ── step-5 RED: impl ContainmentQuery for Engine ────────────────────────────

/// Test the four dispatch cases of `impl ContainmentQuery for Engine`:
///   (a) GeometryHandle region + inside point → Some(true)
///   (b) GeometryHandle region + outside point → Some(false)
///   (c) non-geometry region (Value::Real) → None
///   (d) non-point point (Value::Real) → None
///
/// Uses the `contains_box.ri` fixture (10 mm³ box centred at origin) to obtain
/// a live `Value::GeometryHandle` from the Engine's built value map, then calls
/// `engine.contains(...)` via the `ContainmentQuery` trait.
///
/// **RED today**: `impl ContainmentQuery for Engine` does not exist in
/// `reify-eval` → compile-fail.
/// **GREEN after step-6**: the trait impl is added.
///
/// Skips OCCT-dependent assertions when OCCT is not available.
#[test]
fn engine_containment_query_impl_dispatches_correctly() {
    // Fixture compilation is kernel-independent — validate unconditionally.
    let source = std::fs::read_to_string(CONTAINS_BOX_PATH)
        .expect("examples/kernel_queries/contains_box.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "contains_box.ri should compile with no errors, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    // Build with real OCCT kernel to hydrate the GeometryHandle.
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Obtain the `solid` GeometryHandle from the built value map.
    let solid_cell = ValueCellId::new("ContainsBox", "solid");
    let solid_handle = result
        .values
        .get(&solid_cell)
        .expect("ContainsBox.solid cell should exist")
        .clone();
    assert!(
        matches!(solid_handle, Value::GeometryHandle { .. }),
        "ContainsBox.solid should be Value::GeometryHandle, got: {:?}",
        solid_handle
    );

    // (a) Inside point (0, 0, 0 m) → Some(true).
    let inside_point = Value::Point(vec![
        Value::length(0.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    assert_eq!(
        engine.contains(&solid_handle, &inside_point),
        Some(true),
        "point at origin should be inside the 10mm box"
    );

    // (b) Outside point (0.020 m, 0, 0) → Some(false).
    let outside_point = Value::Point(vec![
        Value::length(0.020),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    assert_eq!(
        engine.contains(&solid_handle, &outside_point),
        Some(false),
        "point at 20mm should be outside the 10mm box"
    );

    // (c) Non-geometry region → None.
    assert_eq!(
        engine.contains(&Value::Real(0.0), &inside_point),
        None,
        "non-GeometryHandle region should yield None"
    );

    // (d) Non-point point value → None.
    assert_eq!(
        engine.contains(&solid_handle, &Value::Real(0.0)),
        None,
        "non-Point3<Length> point should yield None"
    );
}

// ── step-7 RED→GREEN: B5 integration gate ───────────────────────────────────

/// Full seam integration test for `restrict(field, region)` sampling (B5).
///
/// Loads `examples/fields/restrict.ri`, builds with a real OCCT kernel, and
/// asserts:
/// - `v_in`  == `Value::Real(42.0)` (inside the box → inner field value)
/// - `v_out` == `Value::Undef`      (outside the box → strict-Undef)
///
/// Exercises the complete path:
///   restrict constructor → GeometryHandle hydration → cell_eval_ctx
///   containment wiring → kernel Contains query → inside/outside dispatch.
///
/// **RED→GREEN** once steps 2/4/6 + pre-1 are all in place (this test goes
/// GREEN at step-7 once step-6 is done).
///
/// Skips OCCT-dependent assertions when OCCT is not available.
#[test]
fn restrict_field_b5_integration() {
    let source = std::fs::read_to_string(RESTRICT_PATH)
        .expect("examples/fields/restrict.ri should exist (task 4222 pre-1)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/fields/restrict.ri should compile with no errors, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    // Build with real OCCT kernel.
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Helper: look up a value cell.
    let cell = |name: &str| ValueCellId::new("RestrictField", name);

    // v_in: inside the box (0,0,0) → inner field value 42.0.
    let v_in = result.values.get(&cell("v_in")).cloned();
    assert_eq!(
        v_in,
        Some(Value::Real(42.0)),
        "v_in (inside the box) should be Value::Real(42.0), got: {:?}",
        v_in
    );

    // v_out: outside the box (20mm,0,0) → Value::Undef.
    let v_out = result.values.get(&cell("v_out")).cloned();
    assert_eq!(
        v_out,
        Some(Value::Undef),
        "v_out (outside the box) should be Value::Undef, got: {:?}",
        v_out
    );
}

// ── Kernel-independent unit tests ────────────────────────────────────────────

/// Kernel-independent unit tests for `impl ContainmentQuery for Engine`.
///
/// These cases short-circuit BEFORE reaching the geometry kernel, so they run
/// unconditionally in every CI environment (no OCCT required):
///
///   (a) No kernel registered → `None` for any valid-looking inputs
///       (`default_query_kernel()` returns `None`; the `?` short-circuits
///       before calling `kernel.query()`).
///   (b) Non-`GeometryHandle` region → `None` (early match-arm return,
///       regardless of whether a kernel is present).
///   (c) Non-`Point3<Length>` point → `None` (early match-arm return after
///       extracting the kernel_handle).
///
/// These invariants complement the `reify-expr` mock-resolver unit tests in
/// `field_op_dispatch_tests.rs`.  The OCCT-guarded tests above cover the full
/// live path; this test fills the gap that runs in OCCT-less CI lanes.
#[test]
fn engine_containment_query_no_kernel_short_circuits() {
    // Build an Engine with NO geometry planner / kernel so that
    // `default_query_kernel()` always returns `None`.
    let engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);

    // A fake GeometryHandle — the kernel_handle value is irrelevant because
    // the kernel is never reached (no kernel registered).
    let fake_handle = Value::GeometryHandle {
        realization_ref: RealizationNodeId::new("Fake", 0),
        upstream_values_hash: [0u8; 32],
        kernel_handle: GeometryHandleId(1),
    };
    let valid_point = Value::Point(vec![
        Value::length(0.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);

    // (a) No kernel → None for an otherwise-valid (GeometryHandle, Point3) pair.
    //     default_query_kernel() returns None; the `?` in `...?.query(...)` exits.
    assert_eq!(
        engine.contains(&fake_handle, &valid_point),
        None,
        "no-kernel engine: valid GeometryHandle + Point3 should yield None"
    );

    // (b) Non-geometry region → None (early match-arm return before kernel lookup).
    assert_eq!(
        engine.contains(&Value::Real(0.0), &valid_point),
        None,
        "non-GeometryHandle region should yield None (no kernel required)"
    );

    // (c) Non-Point3 point → None (early match-arm return after handle extraction).
    assert_eq!(
        engine.contains(&fake_handle, &Value::Real(0.0)),
        None,
        "non-Point3 point should yield None (no kernel required)"
    );
}
