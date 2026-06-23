//! End-to-end tests for `nurbs_surface(control_points, weights, u_knots, v_knots, u_degree,
//! v_degree) -> Surface` (task #4191, PRD task η —
//! `docs/prds/geometry-modify-sweep-completion.md`), spanning the full pipeline:
//!
//!   `parse → compile → Engine (real OCCT) → build`
//!
//! Two-tier convention (mirroring `crates/reify-eval/tests/offset_curve_e2e.rs`):
//!
//! * **COMPILE-LEVEL** (always runs, even without OCCT) — a `nurbs_surface(...)` call
//!   compiles with no error diagnostics and lowers to a
//!   `CompiledGeometryOp::Surface { kind: SurfaceKind::Nurbs, .. }` operation;
//!   using the patch as an `extrude(p, 5mm)` profile emits
//!   `DiagnosticCode::GeometryProfileRequired`, because a free-form NURBS surface
//!   is non-planar and non-closed (violates the Surface∧Closed∧Planar precondition).
//!
//! * **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!   a bilinear degree-1×1 patch with corners (0,0,0), (0,10,0), (10,0,0),
//!   (10,10,5) mm is realized through a real `OcctKernelHandle`. A
//!   `bounding_box(p)` query proves `p` evaluated to a non-`Undef` surface and
//!   pins the geometry: `bbox.max.z ≈ 5mm` (SI: 0.005m, tol 0.5mm) captures
//!   the lifted corner, and `bbox.max.x`, `bbox.max.y` each span ≈10mm. The STEP
//!   export also yields non-empty bytes.
//!
//! The runtime tier FAILS (RED) until step-10 (task #4191) replaces the stub
//! eval lowering with the real nested-grid decode.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{compile_source_with_stdlib, errors_only, parse_and_compile_with_stdlib};

// ── Source fixtures ──────────────────────────────────────────────────────────
//
// Bilinear patch (degree-1×1, clamped knots [0,0,1,1]×[0,0,1,1]).
// Control-point grid (u-rows × v-columns):
//
//   row 0:  (0, 0, 0)mm   (0, 10, 0)mm
//   row 1: (10, 0, 0)mm  (10, 10, 5)mm
//
// degree-1 clamped basis = bilinear interpolation — corners interpolate control
// points exactly, so bbox.max.z == 5mm exactly (lifted corner).
// Used-as-extrude-profile → GeometryProfileRequired because closed=false.

/// Primary fixture: surface + bounding-box probe.
const NURBS_SURFACE_BBOX_SOURCE: &str = r#"
structure def NurbsSurfaceE2e {
    let p = nurbs_surface(
        [[point3(0mm,0mm,0mm),point3(0mm,10mm,0mm)],[point3(10mm,0mm,0mm),point3(10mm,10mm,5mm)]],
        [[1.0,1.0],[1.0,1.0]],
        [0,0,1,1],
        [0,0,1,1],
        1,
        1
    )
    let bb = bounding_box(p)
}
"#;

/// Profile-rejection fixture: `extrude(nurbs_surface(...), 5mm)` must emit
/// `GeometryProfileRequired` — a NURBS surface is not Closed∧Planar.
///
/// NOTE: the nurbs_surface call must be INLINE (not let-bound) to trigger the
/// statically-known-mismatch path in check_profile_arg. Per PRD decision 5,
/// let-bound operands compile to ValueRefs and are skipped (permissive
/// back-compat). Only FunctionCall CompiledExprs are inspected.
const EXTRUDE_PROFILE_SOURCE: &str = r#"
structure def NurbsExtrudeRejected {
    let e = extrude(nurbs_surface(
        [[point3(0mm,0mm,0mm),point3(0mm,10mm,0mm)],[point3(10mm,0mm,0mm),point3(10mm,10mm,5mm)]],
        [[1.0,1.0],[1.0,1.0]],
        [0,0,1,1],
        [0,0,1,1],
        1,
        1
    ), 5mm)
}
"#;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// True iff `compiled` lowers at least one `nurbs_surface(...)` call to a
/// `CompiledGeometryOp::Surface { kind: SurfaceKind::Nurbs, .. }` operation.
fn lowers_to_nurbs_surface(compiled: &reify_compiler::CompiledModule) -> bool {
    compiled.templates.iter().any(|t| {
        t.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    reify_compiler::CompiledGeometryOp::Surface {
                        kind: reify_compiler::SurfaceKind::Nurbs,
                        ..
                    }
                )
            })
        })
    })
}

/// Compile `source` (asserting no error-severity diagnostics), then — if OCCT
/// is available — build it through a real-OCCT `Engine` and return the
/// `BuildResult`. Returns `None` when OCCT is unavailable so the caller skips
/// its numeric assertions (mirrors `offset_curve_e2e::compile_and_build_occt`).
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "nurbs_surface fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    assert!(
        lowers_to_nurbs_surface(&compiled),
        "nurbs_surface fixture should lower to a Surface(Nurbs) op"
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
                    other => {
                        panic!("{what}: component {i} should be Scalar<Length>, got {other:?}")
                    }
                };
            }
            out
        }
        other => panic!("{what}: expected Point3 of 3 components, got {other:?}"),
    }
}

/// Assert `value` is a NON-`Undef` `Value::BoundingBox` (proving the surface
/// realized to a real geometry object) and return its `max` corner in SI
/// metres.
fn assert_bbox_max(value: Option<&Value>, what: &str) -> [f64; 3] {
    match value {
        Some(Value::BoundingBox { max, .. }) => point3_si(max, &format!("{what} max")),
        other => panic!(
            "{what}: expected a non-Undef Value::BoundingBox (nurbs_surface result should be \
             a realized Surface), got {other:?}"
        ),
    }
}

// ── COMPILE-LEVEL: fixture compiles clean + lowers to Surface(Nurbs) ─────────

/// Pins the compile + lowering path: `nurbs_surface(...)` must compile with no
/// error diagnostics and lower to a
/// `CompiledGeometryOp::Surface { kind: SurfaceKind::Nurbs, .. }` operation.
/// Runs unconditionally so a compile/lowering regression fails on every runner.
#[test]
fn nurbs_surface_compiles_clean_and_lowers_to_surface_nurbs() {
    let compiled = parse_and_compile_with_stdlib(NURBS_SURFACE_BBOX_SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "nurbs_surface should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    assert!(
        lowers_to_nurbs_surface(&compiled),
        "nurbs_surface should lower to a Surface(Nurbs) op"
    );
}

// ── COMPILE-LEVEL: extrude(nurbs_surface(...)) → GeometryProfileRequired ─────

/// Pins the profile-precondition gate: using a `nurbs_surface` patch as the
/// profile operand of `extrude(...)` must emit
/// `DiagnosticCode::GeometryProfileRequired`, because a free-form NURBS
/// surface is non-closed (violates the Surface∧Closed∧Planar precondition).
/// Runs unconditionally — a regression in the precondition check fails on
/// every runner.
#[test]
fn nurbs_surface_as_extrude_profile_is_rejected() {
    let compiled = compile_source_with_stdlib(EXTRUDE_PROFILE_SOURCE);
    let n_profile_required = errors_only(&compiled)
        .into_iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryProfileRequired))
        .count();
    assert!(
        n_profile_required >= 1,
        "extrude(nurbs_surface(...), 5mm) should emit at least one \
         GeometryProfileRequired diagnostic (NURBS surface is non-closed), \
         got {n_profile_required}"
    );
}

// ── OCCT-BACKED: bilinear patch corner pins bbox.max.z ≈ 5mm ─────────────────

/// Full e2e signal: the bilinear degree-1 patch with lifted corner (10,10,5)mm
/// realizes to a `BRepKind::Face`. A `bounding_box(p)` query is driven through
/// the full compile→eval→OCCT pipeline; the result's `max.z` must pin at 5mm
/// (SI 0.005m, tol 0.5mm). `max.x` and `max.y` must each reach ≈10mm (SI
/// 0.010m, tol 0.5mm). The STEP export must yield non-empty bytes.
///
/// FAILS (RED) until step-10 (task #4191) replaces the stub eval lowering
/// with the real nested-grid decode.
#[test]
fn nurbs_surface_bilinear_patch_bbox_e2e() {
    let Some(result) = compile_and_build_occt(NURBS_SURFACE_BBOX_SOURCE) else {
        return;
    };

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "build errors: {errors:?}");

    let max = assert_bbox_max(
        result.values.get(&ValueCellId::new("NurbsSurfaceE2e", "bb")),
        "nurbs_surface bilinear patch",
    );

    // max.z: the lifted corner (10,10,5)mm must be on the surface.
    // degree-1 clamped B-spline = bilinear interp; corners interpolate exactly.
    let expected_z = 0.005_f64; // 5mm in SI metres
    let tol = 0.0005_f64; // 0.5mm tolerance
    assert!(
        (max[2] - expected_z).abs() <= tol,
        "nurbs_surface bilinear patch: bbox.max.z should be ≈5mm (SI {expected_z}m), got {}m",
        max[2]
    );

    // max.x, max.y: grid corners span exactly 10mm in x and y.
    let expected_xy = 0.010_f64; // 10mm in SI metres
    assert!(
        (max[0] - expected_xy).abs() <= tol,
        "nurbs_surface bilinear patch: bbox.max.x should be ≈10mm (SI {expected_xy}m), got {}m",
        max[0]
    );
    assert!(
        (max[1] - expected_xy).abs() <= tol,
        "nurbs_surface bilinear patch: bbox.max.y should be ≈10mm (SI {expected_xy}m), got {}m",
        max[1]
    );

    // STEP export: non-empty bytes prove the surface serialized successfully.
    assert!(
        result
            .geometry_output
            .as_ref()
            .map(|b| !b.is_empty())
            .unwrap_or(false),
        "nurbs_surface STEP export should yield non-empty bytes"
    );
}
