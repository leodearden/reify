//! End-to-end tests for the Phase A swept-body classifier wiring.
//!
//! These tests drive the full Engine::build pipeline with synthetic
//! `CompiledModule`s and assert that `engine.swept_kind_table()` records the
//! correct `SweptKind` (or stays empty) on the realization's final handle.
//!
//! See `crates/reify-eval/src/sweep_classifier.rs` for the pure classifier
//! plus its unit tests; this file pins the engine wire-up that calls it.

use reify_compiler::{
    CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PrimitiveKind, SweepKind,
};
use reify_eval::SweptKind;
use reify_test_support::*;
use reify_core::Type;
use reify_ir::{ExportFormat, Value};

/// (a) Extrude-only realization populates the table with a single
/// `SweptKind::Extrude` keyed by the realization's final handle.
///
/// Builds a CompiledModule with two ops:
///   Op 0: Sphere (stand-in profile, produces a handle)
///   Op 1: Sweep(Extrude) referencing Step(0) as profile, with distance=10mm
///
/// After `engine.build(...)`, `engine.swept_kind_table()` must contain
/// exactly one entry, keyed by the kernel-result handle of op 1, holding
/// `SweptKind::Extrude { axis: [0,0,1], length: ~0.01 m }`.
#[test]
fn engine_swept_kind_table_records_extrude_realization() {
    let e = "TestSweptExtrude";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere — stand-in profile to produce a handle at step index 0.
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Extrude(Step(0), distance=10mm). The args' "profile" entry is a
    // placeholder expression — the eval layer resolves the profile handle
    // from `profiles: [GeomRef::Step(0)]`, not from this entry.
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(10.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_swept_extrude"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 geometry operations (Sphere + Extrude), got {}",
        ops.len()
    );

    let final_handle = ops.last().unwrap().result_handle;

    let table = engine.swept_kind_table();
    assert_eq!(
        table.len(),
        1,
        "expected exactly one swept-kind table entry after a single Extrude realization, got len() == {}",
        table.len()
    );
    assert_eq!(
        table.lookup(final_handle),
        Some(&SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        }),
        "the realization's final handle must map to SweptKind::Extrude with axis=+Z and length=10mm (0.01m SI)"
    );
}

/// (b) A realization that finishes with a Modify op (Fillet) is NOT a
/// recognised swept body — the table must be empty after build.
///
/// The plan's original wording suggested `ModifyKind::Translate`, but
/// `Translate` is a `TransformKind`, not a `ModifyKind`. Per the plan's
/// fallback ("if Translate isn't a CompiledGeometryOp variant, use whatever
/// post-sweep modify op is reachable from the test_support harness"),
/// this test uses `ModifyKind::Fillet`. The classifier's contract is
/// "look at the LAST op": Modify is the last op here, so the classifier
/// returns `None` and the table stays empty.
///
/// Builds a CompiledModule with three ops:
///   Op 0: Sphere (stand-in profile, produces a handle)
///   Op 1: Sweep(Extrude) referencing Step(0)
///   Op 2: Modify(Fillet) referencing Step(1) with radius=1mm
#[test]
fn engine_swept_kind_table_empty_for_realization_with_modify_after_extrude() {
    let e = "TestSweptModified";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(10.0)),
        ],
    };

    // Op 2: Fillet(Step(1), radius=1mm). The eval layer resolves the target
    // handle from GeomRef::Step(1), not from any "target" entry in args.
    let fillet_op = CompiledGeometryOp::Modify {
        kind: ModifyKind::Fillet,
        target: GeomRef::Step(1),
        args: vec![("radius".into(), mm_literal(1.0))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op, fillet_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_swept_modified"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let table = engine.swept_kind_table();
    assert!(
        table.is_empty(),
        "swept-kind table must be empty when the realization's last op is a Modify (Fillet), got len() == {}",
        table.len()
    );
}

/// (c) Per-build reset: a second `engine.build(...)` call on a different
/// module clears any entries left by the first build.
///
/// Build #1 uses the extrude-only realization shape from test (a) and is
/// expected to leave exactly one entry in `swept_kind_table`. Build #2 uses
/// the modify-after-extrude shape from test (b) on the same engine instance
/// — the per-build reset at every build entry point must clear the table,
/// and the new build's modify-tail realization is rejected by the
/// classifier, so `is_empty()` must hold after build #2.
///
/// Pins the contract that `Engine::build()` is responsible for both:
///   1. clearing `swept_kind_table` (so stale entries from a prior build do
///      not leak into the next one), and
///   2. populating it from scratch via `classify_swept_body` for the new
///      realizations.
///
/// If a future refactor accidentally drops the
/// `self.swept_kind_table = SweptKindTable::default();` reset in `build()`,
/// this test fails: build #1 leaves an entry that build #2 fails to clear,
/// so `is_empty()` returns false.
#[test]
fn engine_swept_kind_table_resets_between_builds() {
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // ── Module #1: extrude-only realization (populates the table) ────────
    let e1 = "TestSweptExtrude";
    let sphere_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_op_1 = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(10.0)),
        ],
    };
    let template_1 = TopologyTemplateBuilder::new(e1)
        .realization(e1, 0, vec![sphere_op_1, extrude_op_1])
        .build();
    let module_1 =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_swept_reset_build1"))
            .template(template_1)
            .build();

    // ── Module #2: modify-after-extrude realization (table must be empty) ─
    let e2 = "TestSweptResetModified";
    let sphere_op_2 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_op_2 = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(10.0)),
        ],
    };
    let fillet_op_2 = CompiledGeometryOp::Modify {
        kind: ModifyKind::Fillet,
        target: GeomRef::Step(1),
        args: vec![("radius".into(), mm_literal(1.0))],
    };
    let template_2 = TopologyTemplateBuilder::new(e2)
        .realization(e2, 0, vec![sphere_op_2, extrude_op_2, fillet_op_2])
        .build();
    let module_2 =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_swept_reset_build2"))
            .template(template_2)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // ── Build #1: classifier records exactly one entry ───────────────────
    let _result_1 = engine.build(&module_1, ExportFormat::Step);
    let len_after_build_1 = engine.swept_kind_table().len();
    assert_eq!(
        len_after_build_1, 1,
        "after build #1 (single Extrude realization), swept_kind_table must contain exactly one entry, got len() == {}",
        len_after_build_1
    );

    // ── Build #2: per-build reset clears the prior entry; new module's
    //    modify-tail realization is rejected by the classifier so the
    //    table stays empty.
    let _result_2 = engine.build(&module_2, ExportFormat::Step);
    let table_after_build_2 = engine.swept_kind_table();
    assert!(
        table_after_build_2.is_empty(),
        "after build #2 (modify-tail realization on the same Engine), swept_kind_table must be empty (per-build reset cleared the build #1 entry, classifier rejected the modify tail), got len() == {}",
        table_after_build_2.len()
    );
}

/// (d) Revolve realization populates the table with a single
/// `SweptKind::Revolve` keyed by the realization's final handle.
///
/// # What this test covers
///
/// `CompiledGeometryOp::Sweep { kind: SweepKind::Revolve, profiles, args }` is
/// compiled by `compile_geometry_op`'s Revolve arm into
/// `GeometryOp::Revolve { profile, axis_origin, axis_dir, angle_rad }`. That arm
/// reads **seven named f64 args**: `ox`, `oy`, `oz` (axis origin), `ax`, `ay`,
/// `az` (axis direction), and `angle` (angle in radians). All seven are
/// `Type::Real` — they are dimensionless ratios/radians, not length-typed.
///
/// # Non-degenerate parameters chosen
///
/// axis_dir = (0, 0, 1) — unit-length +Z vector; axis_norm = 1.0, well above the
/// `GEOMETRY_EPSILON ≈ 1e-12` degeneracy guard in `compile_geometry_op` and the
/// `REVOLVE_DEGENERATE_TOLERANCE` check in the classifier. angle = π/2 (90°);
/// |angle| = π/2, well above the `DEGENERATE_ANGLE_RAD = 1e-12` guard. Avoids
/// any edge-case paths (full 2π, negative angle) — those are covered by the unit
/// tests in `sweep_classifier.rs`.
///
/// # Regression guarded
///
/// A future change to `compile_geometry_op`'s Revolve arm that drops
/// `GeometryOp::Revolve` emission, swaps the axis/angle field order, or changes
/// the degeneracy threshold would cause `classify_swept_body` to miss the entry
/// or produce the wrong variant fields. The unit tests in `sweep_classifier.rs`
/// exercise the pure classifier with hand-built `&[GeometryOp]` slices, bypassing
/// `compile_geometry_op` and `Engine::execute_realization_ops` — this e2e test
/// pins the full wiring path.
#[test]
fn engine_swept_kind_table_records_revolve_realization() {
    let e = "TestSweptRevolve";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal = |v: f64| reify_ir::CompiledExpr::literal(Value::Real(v), Type::Real);

    // Op 0: Sphere — stand-in profile to produce a handle at step index 0.
    // The classifier only inspects the *last* op, so any handle-producing op works.
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Revolve(Step(0), axis=+Z, angle=π/2). The seven f64 args are
    // Type::Real (not length-typed) because they are dimensionless ratios /
    // radians — matches the precedent in stress_sweep_degenerate.rs:108.
    let revolve_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Revolve,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)),
            ("angle".into(), real_literal(std::f64::consts::FRAC_PI_2)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, revolve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_swept_revolve"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 geometry operations (Sphere + Revolve), got {}",
        ops.len()
    );

    let final_handle = ops.last().unwrap().result_handle;

    let table = engine.swept_kind_table();
    assert_eq!(
        table.len(),
        1,
        "expected exactly one swept-kind table entry after a single Revolve realization, got len() == {}",
        table.len()
    );
    assert_eq!(
        table.lookup(final_handle),
        Some(&SweptKind::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        }),
        "the realization's final handle must map to SweptKind::Revolve with axis=+Z and angle=π/2"
    );
}

/// (e) Sweep-along-LineSegment realization populates the table with a single
/// `SweptKind::SweepLinear` keyed by the realization's final handle.
///
/// # What this test covers
///
/// `CompiledGeometryOp::Sweep { kind: SweepKind::Sweep, profiles: [Step(0), Step(1)], args: vec![] }`
/// is compiled by `compile_geometry_op`'s Sweep arm into
/// `GeometryOp::Sweep { profile, path }`. The eval layer reads only
/// `profiles[0]` (profile handle) and `profiles[1]` (path handle); `args` is
/// intentionally empty (per task-383 S4b/S6).
///
/// # Why the path op MUST be a LineSegment
///
/// The classifier's Sweep arm (`sweep_classifier.rs:235-254`) resolves the
/// `path` handle by scanning the parallel `handles` slice and matches against
/// `GeometryOp::LineSegment { .. }` source ops. A Sphere-as-path would resolve
/// to `GeometryOp::Sphere` and the classifier would return `None` instead of
/// `SweptKind::SweepLinear`. This test pins the LineSegment-source resolution wiring:
/// the path op (Op 1) is `CompiledGeometryOp::Curve { kind: CurveKind::LineSegment }`
/// so it compiles to `GeometryOp::LineSegment`, satisfying the classifier's guard.
///
/// # Regression guarded
///
/// A future change to `compile_geometry_op`'s Sweep arm that drops or swaps the
/// `profile`/`path` handles, or reorders `profiles[0]`/`profiles[1]` resolution,
/// would cause the classifier to record the wrong handle ids (or fail to match
/// `GeometryOp::LineSegment`). The assertion pins both the variant tag AND the
/// kernel-allocated handle ids, catching any swap/drop in the eval layer.
/// The unit tests in `sweep_classifier.rs` bypass `compile_geometry_op` and
/// `Engine::execute_realization_ops`; this e2e test pins the full wiring path.
#[test]
fn engine_swept_kind_table_records_sweep_along_line_segment_realization() {
    let e = "TestSweptSweepLine";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere — stand-in profile to produce a handle at step index 0.
    // The classifier only inspects the *last* op, so any handle-producing op works here.
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: LineSegment (0,0,0) → (0,0,10mm) — +Z line segment, compiles to
    // GeometryOp::LineSegment so the classifier's Sweep arm recognises it as a
    // non-twisted path. A Sphere-as-path would fail the classifier's guard.
    let line_op = CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![
            ("x1".into(), mm_literal(0.0)),
            ("y1".into(), mm_literal(0.0)),
            ("z1".into(), mm_literal(0.0)),
            ("x2".into(), mm_literal(0.0)),
            ("y2".into(), mm_literal(0.0)),
            ("z2".into(), mm_literal(10.0)),
        ],
    };

    // Op 2: Sweep(profile=Step(0), path=Step(1)). args is intentionally empty —
    // the eval layer reads only profiles[0] and profiles[1].
    let sweep_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Sweep,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
        args: vec![],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, line_op, sweep_op])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_swept_sweep_line"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        3,
        "expected 3 geometry operations (Sphere + LineSegment + Sweep), got {}",
        ops.len()
    );

    let profile_handle = ops[0].result_handle;
    let path_handle = ops[1].result_handle;
    let final_handle = ops.last().unwrap().result_handle;

    let table = engine.swept_kind_table();
    assert_eq!(
        table.len(),
        1,
        "expected exactly one swept-kind table entry after a single Sweep-along-LineSegment realization, got len() == {}",
        table.len()
    );
    assert_eq!(
        table.lookup(final_handle),
        Some(&SweptKind::SweepLinear {
            profile: profile_handle,
            path: path_handle,
        }),
        "the realization's final handle must map to SweptKind::SweepLinear with profile=ops[0].result_handle and path=ops[1].result_handle"
    );
}
