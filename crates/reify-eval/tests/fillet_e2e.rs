//! End-to-end tests for the fillet() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::ModifyKind;
use reify_core::Type;
use reify_ir::GeometryOp;
use reify_test_support::*;

// ---------------------------------------------------------------------------
// step-1: Compiler rejects wrong arg counts, accepts correct count
// ---------------------------------------------------------------------------

/// fillet() with 1 arg (missing radius) should produce diagnostics.
/// Before step-2, fillet is not recognized at all and no realization is built.
#[test]
fn fillet_compiler_rejects_one_arg() {
    let source = r#"structure S {
    param target: Length = 5mm
    let result = fillet(target)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fl1"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    let has_fillet_op = realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Modify {
                    kind: reify_compiler::ModifyKind::Fillet,
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
        !has_fillet_op,
        "should not produce Modify(Fillet) op with wrong arg count (1 arg)"
    );
}

/// fillet() with 3 args should produce diagnostics (too many args).
#[test]
fn fillet_compiler_rejects_three_args() {
    let source = r#"structure S {
    param target: Length = 5mm
    param rad: Length = 2mm
    let result = fillet(target, rad, rad)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fl3"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    let has_fillet_op = realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Modify {
                    kind: reify_compiler::ModifyKind::Fillet,
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
        !has_fillet_op,
        "should not produce Modify(Fillet) op with wrong arg count (3 args)"
    );
}

/// fillet() with correct 2 args should produce a Modify(Fillet) realization.
/// This test fails before step-2 because fillet is not recognized as a geometry function.
#[test]
fn fillet_compiler_accepts_two_args() {
    let source = r#"structure S {
    param target: Length = 5mm
    param rad: Length = 2mm
    let result = fillet(target, rad)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fl2"));
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
        "expected 1 realization for fillet call, got {}",
        template.realizations.len()
    );
    assert!(
        !template.realizations[0].operations.is_empty(),
        "expected at least one operation in the realization"
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            reify_compiler::CompiledGeometryOp::Modify {
                kind: reify_compiler::ModifyKind::Fillet,
                ..
            }
        ),
        "expected Modify(Fillet), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "expected no diagnostics for fillet(target, rad), got: {:?}",
        compiled.diagnostics
    );
}

// ---------------------------------------------------------------------------
// step-3: Full eval pipeline test
// ---------------------------------------------------------------------------

/// Exercises the full compile -> eval path for Fillet.
///
/// Creates a module with 2 ops:
///   Op 0: Box (produces a solid body, serves as fillet target, produces a handle)
///   Op 1: Modify(Fillet) referencing Step(0) as target, with radius=3mm
///
/// Verifies that the Fillet operation receives the correct target handle
/// and a radius value of approximately 0.003 m (3 mm in SI).
///
/// This test should pass immediately since the eval layer already dispatches
/// ModifyKind::Fillet correctly (reify-eval/src/lib.rs).
#[test]
fn fillet_through_full_eval_pipeline() {
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 1 args: mirrors what the compiler emits (geometry.rs:900-907).
    // The eval layer resolves the target from GeomRef::Step(0), not from 'target' in args.
    let args = vec![
        ("target".to_string(), mm_literal(0.0)),
        ("radius".to_string(), mm_literal(3.0)),
    ];

    let (_result, ops) = run_modify_pipeline(ModifyKind::Fillet, args);
    assert_eq!(
        ops.len(),
        2,
        "expected 2 geometry operations, got {}",
        ops.len()
    );

    let target_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Fillet { target, radius } => {
            assert_eq!(
                *target, target_handle,
                "Fillet target should be handle from op 0 ({:?}), got {:?}",
                target_handle, target
            );
            let radius_si = radius.as_f64().expect("radius should be a numeric value");
            assert!(
                (radius_si - 0.003).abs() < 1e-9,
                "Fillet radius should be 0.003 m (3 mm SI), got {}",
                radius_si
            );
        }
        other => panic!("expected GeometryOp::Fillet at op index 1, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-4 (1819): Contract test — eval only needs 'radius' arg for Fillet
// ---------------------------------------------------------------------------

/// Documents the minimal args contract for Fillet: only 'radius' is needed in args.
///
/// The eval layer resolves the target handle from GeomRef::Step(0) on the Modify
/// variant, not from any 'target' entry in the args vec. This test constructs a
/// Fillet op with ONLY ('radius', ...) in args (no 'target' entry at all) and
/// verifies that eval still produces the correct Fillet geometry op.
#[test]
fn fillet_modify_only_needs_radius_arg() {
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Only 'radius' in args — no 'target' entry.
    // "target" handle is resolved from GeomRef::Step(0), not from args.
    let args = vec![("radius".to_string(), mm_literal(3.0))];

    let (_result, ops) = run_modify_pipeline(ModifyKind::Fillet, args);
    assert_eq!(
        ops.len(),
        2,
        "expected 2 geometry operations, got {}",
        ops.len()
    );

    let target_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Fillet { target, radius } => {
            assert_eq!(
                *target, target_handle,
                "Fillet target should be handle from op 0 ({:?}), got {:?}",
                target_handle, target
            );
            let radius_si = radius.as_f64().expect("radius should be a numeric value");
            assert!(
                (radius_si - 0.003).abs() < 1e-9,
                "Fillet radius should be 0.003 m (3 mm SI), got {}",
                radius_si
            );
        }
        other => panic!("expected GeometryOp::Fillet at op index 1, got {:?}", other),
    }
}
