//! End-to-end tests for `offset_curve(curve, distance[, reference|direction])`
//! (task 4193, PRD task ι — `docs/prds/geometry-modify-sweep-completion.md`
//! Phase 4), spanning the FULL pipeline for all three overloads:
//!
//!   1. `offset_curve(c, 2mm)`                  — planar 2-D offset
//!   2. `offset_curve(c, 2mm, faces(b)[0])`     — +reference Surface
//!   3. `offset_curve(c, 2mm, vec3(0,0,1))`     — +direction Vector3
//!
//! `parse → compile → Engine (real OCCT) → build`, mirroring the proven
//! `geometry_query_kernel_dispatch.rs` / `kernel_queries_curvature_smoke.rs`
//! two-tier convention:
//!
//! * **COMPILE-LEVEL** (always runs, even with no OCCT) — every overload source
//!   compiles with no error-severity diagnostics (so the mixed 3rd-argument
//!   shapes — a bound Surface handle vs a `vec3` — are NOT rejected by the
//!   type-checker) and lowers to a `Modify(OffsetCurve)` op. A grammar / lowering
//!   regression therefore fails on every runner.
//!
//! * **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!   each overload is realized through a real `OcctKernelHandle` and probed with
//!   a `bounding_box(o)` query that flows back through the build pipeline into
//!   `BuildResult::values`. A valid `Value::BoundingBox` proves `o` evaluated to
//!   a NON-`Undef` realized Curve (an `Undef` offset would yield an `Undef`
//!   bounding box and fail the decode). For overload 1 the box additionally pins
//!   the radius signal: the planar offset of a radius-10mm arc symmetric about
//!   angle 0 by +2mm is a concentric radius-12mm arc, so its rightmost point —
//!   `bounding_box(o).max.x` — sits at 12mm (ratio 1.2) within 2%.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

// ── Source fixtures ──────────────────────────────────────────────────────────
//
// The shared input curve is a radius-10mm circular arc in the XY plane swept
// SYMMETRICALLY about angle 0 (θ ∈ [-π/4, +π/4]) — identical to the proven
// kernel signal test (`kernel_execute_offset_curve_all_three_branches`). The
// symmetry guarantees the offset arc's angle-0 point (its bounding-box maximum
// in X) lies on the +X axis at the offset radius.
//
//   arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)

/// Overload 1 — planar 2-D offset, plus the `bounding_box` radius probe.
const PLANAR_SOURCE: &str = r#"
structure def OffsetPlanar {
    let c = arc(0mm, 0mm, 0mm, 10mm, -0.7853981633974483rad, 0.7853981633974483rad, 0mm, 0mm, 1mm)
    let o = offset_curve(c, 2mm)
    let bb = bounding_box(o)
}
"#;

/// Overload 3 — `+direction Vector3`. The 3rd arg is a `vec3`, disambiguated at
/// eval time as the offset direction.
const DIRECTIONAL_SOURCE: &str = r#"
structure def OffsetDirectional {
    let c = arc(0mm, 0mm, 0mm, 10mm, -0.7853981633974483rad, 0.7853981633974483rad, 0mm, 0mm, 1mm)
    let o = offset_curve(c, 2mm, vec3(0.0, 0.0, 1.0))
    let bb = bounding_box(o)
}
"#;

/// Overload 2 — `+reference Surface`. The 3rd arg is a LET-BOUND `faces(b)[0]`
/// sub-handle (a `Value::GeometryHandle`), disambiguated at eval time as the
/// reference surface via `resolve_parent_geometry_handle_arg` (exactly how
/// `split` consumes its solid arg).
const ON_SURFACE_SOURCE: &str = r#"
structure def OffsetOnSurface {
    let c = arc(0mm, 0mm, 0mm, 10mm, -0.7853981633974483rad, 0.7853981633974483rad, 0mm, 0mm, 1mm)
    let b = box(20mm, 20mm, 20mm)
    let f = faces(b)[0]
    let o = offset_curve(c, 2mm, f)
    let bb = bounding_box(o)
}
"#;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// True iff `compiled` lowers at least one `offset_curve(...)` call to a
/// `CompiledGeometryOp::Modify { kind: ModifyKind::OffsetCurve, .. }` op.
fn lowers_to_offset_curve(compiled: &reify_compiler::CompiledModule) -> bool {
    compiled.templates.iter().any(|t| {
        t.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    reify_compiler::CompiledGeometryOp::Modify {
                        kind: reify_compiler::ModifyKind::OffsetCurve,
                        ..
                    }
                )
            })
        })
    })
}

/// Compile `source` (asserting no error-severity diagnostics), then — if OCCT is
/// available — build it through a real-OCCT `Engine` and return the
/// `BuildResult`. Returns `None` when OCCT is unavailable so the caller skips its
/// numeric assertions (mirrors `geometry_query_kernel_dispatch::compile_and_build_occt`).
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "offset_curve fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    assert!(
        lowers_to_offset_curve(&compiled),
        "offset_curve fixture should lower to a Modify(OffsetCurve) op"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

/// Decode a `Point3<Length>` `Value` into its 3 SI (metre) components.
fn point3_si(value: &Value, what: &str) -> [f64; 3] {
    match value {
        Value::Point(components) if components.len() == 3 => {
            let mut out = [0.0_f64; 3];
            for (i, comp) in components.iter().enumerate() {
                out[i] = match comp {
                    Value::Scalar { si_value, .. } => *si_value,
                    other => panic!("{what}: component {i} should be Scalar<Length>, got {other:?}"),
                };
            }
            out
        }
        other => panic!("{what}: expected Point3 of 3 components, got {other:?}"),
    }
}

/// Assert `value` is a NON-`Undef` `Value::BoundingBox` (proving the offset
/// realized to a real Curve) and return its `max` corner in SI metres.
fn assert_bbox_max(value: Option<&Value>, what: &str) -> [f64; 3] {
    match value {
        Some(Value::BoundingBox { max, .. }) => point3_si(max, &format!("{what} max")),
        other => panic!(
            "{what}: expected a non-Undef Value::BoundingBox (offset_curve result should be a \
             realized Curve), got {other:?}"
        ),
    }
}

// ── COMPILE-LEVEL: all three overloads compile clean and lower to OffsetCurve ─

/// Pins the compile + type-check glue for every overload call shape: the mixed
/// 3rd argument (a bound Surface handle in overload 2, a `vec3` in overload 3)
/// must NOT be rejected, and each call must lower to `Modify(OffsetCurve)`. Runs
/// unconditionally so a compile/lowering regression fails on every runner.
#[test]
fn offset_curve_three_overloads_compile_clean_and_lower() {
    for (label, source) in [
        ("planar (overload 1)", PLANAR_SOURCE),
        ("directional (overload 3)", DIRECTIONAL_SOURCE),
        ("on-surface reference (overload 2)", ON_SURFACE_SOURCE),
    ] {
        let compiled = parse_and_compile_with_stdlib(source);
        assert!(
            errors_only(&compiled).is_empty(),
            "{label}: should compile with no error-severity diagnostics, got:\n{:#?}",
            errors_only(&compiled)
        );
        assert!(
            lowers_to_offset_curve(&compiled),
            "{label}: should lower to a Modify(OffsetCurve) op"
        );
    }
}

// ── OCCT-BACKED: overload 1 — planar offset grows the radius to 12mm ──────────

/// Overload 1 end-to-end signal: `offset_curve(arc r=10mm, 2mm)` realizes a
/// concentric radius-12mm arc. Driven through the full compile→eval→OCCT
/// pipeline, the result's `bounding_box(o).max.x` (the arc's angle-0 point on
/// the +X axis) measures the offset radius = 12mm (ratio 1.2) within 2%.
#[test]
fn offset_curve_planar_radius_grows_to_12mm_e2e() {
    let Some(result) = compile_and_build_occt(PLANAR_SOURCE) else {
        return;
    };

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "build errors: {errors:?}");

    let max = assert_bbox_max(
        result.values.get(&ValueCellId::new("OffsetPlanar", "bb")),
        "offset_curve(arc r=10mm, 2mm)",
    );

    // max.x is the offset arc's rightmost point (its angle-0 point on +X), which
    // equals the offset radius for an arc concentric about the origin.
    let radius = max[0];
    let expected = 0.012_f64; // 12mm in SI metres
    let rel_err = (radius - expected).abs() / expected;
    assert!(
        rel_err <= 0.02,
        "planar offset of a radius-10mm arc by +2mm should grow to radius 12mm (ratio 1.2) \
         within 2%; bounding_box max.x = {radius} m (rel_err = {rel_err})"
    );
}

// ── OCCT-BACKED: overloads 2 & 3 each eval to a non-Undef Curve ───────────────

/// Overload 3 (`+direction Vector3`) end-to-end: `offset_curve(c, 2mm,
/// vec3(0,0,1))` realizes a non-`Undef` Curve. A valid `bounding_box(o)` proves
/// the directional offset evaluated to real geometry.
#[test]
fn offset_curve_directional_overload_evals_non_undef_e2e() {
    let Some(result) = compile_and_build_occt(DIRECTIONAL_SOURCE) else {
        return;
    };

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "build errors: {errors:?}");

    assert_bbox_max(
        result.values.get(&ValueCellId::new("OffsetDirectional", "bb")),
        "offset_curve(c, 2mm, vec3(0,0,1))",
    );
}

/// Overload 2 (`+reference Surface`) end-to-end: `offset_curve(c, 2mm,
/// faces(b)[0])` realizes a non-`Undef` Curve. The let-bound `faces(b)[0]`
/// sub-handle is resolved as the reference surface at eval time, and a valid
/// `bounding_box(o)` proves the on-surface offset evaluated to real geometry.
#[test]
fn offset_curve_reference_surface_overload_evals_non_undef_e2e() {
    let Some(result) = compile_and_build_occt(ON_SURFACE_SOURCE) else {
        return;
    };

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "build errors: {errors:?}");

    assert_bbox_max(
        result.values.get(&ValueCellId::new("OffsetOnSurface", "bb")),
        "offset_curve(c, 2mm, faces(b)[0])",
    );
}
