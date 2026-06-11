//! End-to-end tests for the extrude() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_core::Type;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

// ---------------------------------------------------------------------------
// step-3: Compiler rejects wrong arg counts
// ---------------------------------------------------------------------------

/// extrude() with 1 arg (missing distance) should produce diagnostics.
/// Before step-4, extrude is not recognized at all and no realization is built.
#[test]
fn extrude_compiler_rejects_one_arg() {
    let source = r#"structure S {
    param profile: Length = 5mm
    let result = extrude(profile)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ext1"));
    let compiled = reify_compiler::compile(&parsed);
    // After step-4: should have diagnostics (wrong arg count)
    // Before step-4: no realization produced (extrude not recognized)
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    // Either no realization at all (extrude not recognized) OR diagnostic for wrong arg count
    // We assert there's no Sweep(Extrude) realization with correct structure
    let has_extrude_op = realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Sweep {
                    kind: reify_compiler::SweepKind::Extrude,
                    ..
                }
            )
        })
    });
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected error diagnostic for wrong arg count (1 arg)"
    );
    assert!(
        !has_extrude_op,
        "should not produce Sweep(Extrude) op with wrong arg count (1 arg)"
    );
}

/// extrude() with 3 args should produce diagnostics (too many args).
#[test]
fn extrude_compiler_rejects_three_args() {
    let source = r#"structure S {
    param profile: Length = 5mm
    param dist: Length = 10mm
    let result = extrude(profile, dist, dist)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ext3"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    let has_extrude_op = realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Sweep {
                    kind: reify_compiler::SweepKind::Extrude,
                    ..
                }
            )
        })
    });
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected error diagnostic for wrong arg count (3 args)"
    );
    assert!(
        !has_extrude_op,
        "should not produce Sweep(Extrude) op with wrong arg count (3 args)"
    );
}

/// extrude() with correct 2 args should produce a Sweep(Extrude) realization.
/// This test fails before step-4 because extrude is not recognized as a geometry function.
#[test]
fn extrude_compiler_accepts_two_args() {
    let source = r#"structure S {
    param profile: Length = 5mm
    param dist: Length = 10mm
    let result = extrude(profile, dist)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ext2"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for extrude call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            reify_compiler::CompiledGeometryOp::Sweep {
                kind: reify_compiler::SweepKind::Extrude,
                ..
            }
        ),
        "expected Sweep(Extrude), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "expected no diagnostics for extrude(profile, dist), got: {:?}",
        compiled.diagnostics
    );
}

/// Exercises the full compile -> eval path for Extrude.
///
/// Creates a module with 2 ops:
///   Op 0: Sphere (serves as a stand-in profile, produces a handle)
///   Op 1: Sweep(Extrude) referencing Step(0) as profile, with distance=10mm
///
/// Verifies that the Extrude operation receives the correct profile handle
/// and a distance value of approximately 0.01 m (10 mm in SI).
#[test]
fn extrude_through_full_eval_pipeline() {
    let e = "TestExtrude";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (produces handle at step index 0)
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Extrude referencing Step(0) as profile, distance = 10mm
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)), // placeholder expr
            ("distance".into(), mm_literal(10.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extrude"))
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
        "expected 2 geometry operations, got {}",
        ops.len()
    );

    let profile_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::Extrude { profile, distance } => {
            assert_eq!(
                *profile, profile_handle,
                "Extrude profile should be handle from op 0 ({:?}), got {:?}",
                profile_handle, profile
            );
            let dist_si = distance
                .as_f64()
                .expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.01).abs() < 1e-9,
                "Extrude distance should be 0.01 m (10 mm SI), got {}",
                dist_si
            );
        }
        other => panic!(
            "expected GeometryOp::Extrude at op index 1, got {:?}",
            other
        ),
    }
}
