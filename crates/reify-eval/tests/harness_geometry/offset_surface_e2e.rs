//! End-to-end tests for `offset_surface(surface, distance) -> Surface` (task
//! #4192, PRD task θ — `docs/prds/geometry-modify-sweep-completion.md`
//! Phase 4), spanning the full pipeline:
//!
//!   `parse → compile → Engine (real OCCT) → build`
//!
//! Two-tier convention (mirroring `crates/reify-eval/tests/offset_curve_e2e.rs`
//! and `crates/reify-eval/tests/nurbs_surface_e2e.rs`):
//!
//! * **COMPILE-LEVEL** (always runs, even without OCCT) — `offset_surface(...)`
//!   compiles with no error diagnostics and lowers to a
//!   `CompiledGeometryOp::Modify { kind: ModifyKind::OffsetSurface, .. }` op.
//!
//! * **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!   `offset_surface(rectangle(20mm, 10mm), 2mm)` is realized through a real
//!   `OcctKernelHandle`. A `bounding_box(s)` query proves `s` evaluated to a
//!   non-`Undef` Surface, and pins the geometric signal: offsetting a planar
//!   XY-plane rectangle (built at z=0 with +Z normal, task #4160) by +2mm
//!   along its normal lands the offset face's bbox at min.z ≈ max.z ≈ 0.002m
//!   (tol 0.5mm). The STEP export also yields non-empty bytes.
//!
//! RED until step-6 (task #4192) adds `ModifyKind::OffsetSurface`, the
//! `compile_modify_op` dispatch arm, and the dedicated
//! `try_infer_traits_for_function_call_in_env` arm.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

// ── Source fixture ────────────────────────────────────────────────────────────
//
// rectangle(20mm, 10mm) is a planar face in the XY plane at z=0 with +Z
// normal (task #4160). Offsetting it by +2mm along its normal lands the
// result at z ≈ +2mm.

const OFFSET_SURFACE_BBOX_SOURCE: &str = r#"
structure def OffsetSurfaceE2e {
    let s = offset_surface(rectangle(20mm, 10mm), 2mm)
    let bb = bounding_box(s)
}
"#;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// True iff `compiled` lowers at least one `offset_surface(...)` call to a
/// `CompiledGeometryOp::Modify { kind: ModifyKind::OffsetSurface, .. }` op.
fn lowers_to_offset_surface(compiled: &reify_compiler::CompiledModule) -> bool {
    compiled.templates.iter().any(|t| {
        t.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    reify_compiler::CompiledGeometryOp::Modify {
                        kind: reify_compiler::ModifyKind::OffsetSurface,
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
/// its numeric assertions (mirrors `nurbs_surface_e2e::compile_and_build_occt`).
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "offset_surface fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    assert!(
        lowers_to_offset_surface(&compiled),
        "offset_surface fixture should lower to a Modify(OffsetSurface) op"
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

/// Assert `value` is a NON-`Undef` `Value::BoundingBox` (proving the offset
/// realized to a real Surface) and return its `(min, max)` corners in SI
/// metres.
fn assert_bbox_min_max(value: Option<&Value>, what: &str) -> ([f64; 3], [f64; 3]) {
    match value {
        Some(Value::BoundingBox { min, max }) => (
            point3_si(min, &format!("{what} min")),
            point3_si(max, &format!("{what} max")),
        ),
        other => panic!(
            "{what}: expected a non-Undef Value::BoundingBox (offset_surface result should be \
             a realized Surface), got {other:?}"
        ),
    }
}

// ── COMPILE-LEVEL: fixture compiles clean + lowers to Modify(OffsetSurface) ──

/// Pins the compile + lowering path: `offset_surface(...)` must compile with
/// no error diagnostics and lower to a
/// `CompiledGeometryOp::Modify { kind: ModifyKind::OffsetSurface, .. }` op.
/// Runs unconditionally so a compile/lowering regression fails on every runner.
#[test]
fn offset_surface_compiles_clean_and_lowers_to_modify_offset_surface() {
    let compiled = parse_and_compile_with_stdlib(OFFSET_SURFACE_BBOX_SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "offset_surface should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    assert!(
        lowers_to_offset_surface(&compiled),
        "offset_surface should lower to a Modify(OffsetSurface) op"
    );
}

// ── OCCT-BACKED: rectangle offset by +2mm lands bbox z ≈ +2mm ────────────────

/// Full e2e signal: `offset_surface(rectangle(20mm,10mm), 2mm)` realizes to a
/// `BRepKind::Face` offset along +Z. A `bounding_box(s)` query is driven
/// through the full compile→eval→OCCT pipeline; the result's `min.z` and
/// `max.z` must both pin at +2mm (SI 0.002m, tol 0.5mm) — a planar face
/// rigidly translated along its normal. The STEP export must yield non-empty
/// bytes.
///
/// RED until step-6 (task #4192) replaces the missing compiler wiring with
/// the real `ModifyKind::OffsetSurface` lowering + eval constructor.
#[test]
fn offset_surface_lands_bbox_z_at_positive_2mm_e2e() {
    let Some(result) = compile_and_build_occt(OFFSET_SURFACE_BBOX_SOURCE) else {
        return;
    };

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "build errors: {errors:?}");

    let (min, max) = assert_bbox_min_max(
        result.values.get(&ValueCellId::new("OffsetSurfaceE2e", "bb")),
        "offset_surface(rectangle(20mm,10mm), 2mm)",
    );

    // min.z and max.z: a planar face offset +2mm along its normal is a rigid
    // translation, so both bbox z bounds land at +2mm.
    let expected_z = 0.002_f64; // 2mm in SI metres
    let tol = 0.0005_f64; // 0.5mm tolerance
    assert!(
        (min[2] - expected_z).abs() <= tol,
        "offset_surface: bbox.min.z should be ≈2mm (SI {expected_z}m), got {}m",
        min[2]
    );
    assert!(
        (max[2] - expected_z).abs() <= tol,
        "offset_surface: bbox.max.z should be ≈2mm (SI {expected_z}m), got {}m",
        max[2]
    );

    // STEP export: non-empty bytes prove the surface serialized successfully.
    assert!(
        result
            .geometry_output
            .as_ref()
            .map(|b| !b.is_empty())
            .unwrap_or(false),
        "offset_surface STEP export should yield non-empty bytes"
    );
}
