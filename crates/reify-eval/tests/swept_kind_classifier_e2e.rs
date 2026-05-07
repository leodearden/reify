//! End-to-end tests for the Phase A swept-body classifier wiring.
//!
//! These tests drive the full Engine::build pipeline with synthetic
//! `CompiledModule`s and assert that `engine.swept_kind_table()` records the
//! correct `SweptKind` (or stays empty) on the realization's final handle.
//!
//! See `crates/reify-eval/src/sweep_classifier.rs` for the pure classifier
//! plus its unit tests; this file pins the engine wire-up that calls it.

use reify_compiler::{CompiledGeometryOp, GeomRef, ModifyKind, PrimitiveKind, SweepKind};
use reify_eval::SweptKind;
use reify_test_support::*;
use reify_types::{ExportFormat, Type, Value};

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
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_swept_extrude"))
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
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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

    let module =
        CompiledModuleBuilder::new(reify_types::ModulePath::single("test_swept_modified"))
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
