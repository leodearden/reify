//! End-to-end tests for the chamfer() geometry operation.
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

/// chamfer() with 1 arg (missing distance) should produce diagnostics.
/// Before step-2, chamfer is not recognized at all and no realization is built.
#[test]
fn chamfer_compiler_rejects_one_arg() {
    let source = r#"structure S {
    param target: Length = 5mm
    let result = chamfer(target)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ch1"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    let has_chamfer_op = realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Modify {
                    kind: reify_compiler::ModifyKind::Chamfer,
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
        !has_chamfer_op,
        "should not produce Modify(Chamfer) op with wrong arg count (1 arg)"
    );
}

/// chamfer() with 3 args is the curated-edge form `chamfer(solid, edges,
/// distance)` (task β, step-10). It is no longer an arity error: the compiler
/// recognises it and lowers it to a `Modify(Chamfer)` op carrying the curated
/// `edges` slot ([target, edges, distance]), mirroring fillet's 2/3-arg
/// dispatch. (Before β the 3-arg form was rejected as "too many args".)
#[test]
fn chamfer_compiler_accepts_three_args_curated_edges() {
    let source = r#"structure S {
    param target: Length = 5mm
    param dist: Length = 2mm
    let result = chamfer(target, dist, dist)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ch3"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    // The 3-arg form is recognised and lowered to a Modify(Chamfer) op carrying
    // the curated-edge "edges" slot — NOT rejected with an arity diagnostic.
    let chamfer_args = realizations.iter().find_map(|r| {
        r.operations.iter().find_map(|op| match op {
            reify_compiler::CompiledGeometryOp::Modify {
                kind: reify_compiler::ModifyKind::Chamfer,
                args,
                ..
            } => Some(args.clone()),
            _ => None,
        })
    });
    let args =
        chamfer_args.expect("3-arg chamfer should produce a Modify(Chamfer) op (curated edges)");
    let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["target", "edges", "distance"],
        "3-arg chamfer must lower to the curated-edge arg shape [target, edges, distance]"
    );
}

/// chamfer() with correct 2 args should produce a Modify(Chamfer) realization.
/// This test fails before step-2 because chamfer is not recognized as a geometry function.
#[test]
fn chamfer_compiler_accepts_two_args() {
    let source = r#"structure S {
    param target: Length = 5mm
    param dist: Length = 2mm
    let result = chamfer(target, dist)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ch2"));
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
        "expected 1 realization for chamfer call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            reify_compiler::CompiledGeometryOp::Modify {
                kind: reify_compiler::ModifyKind::Chamfer,
                ..
            }
        ),
        "expected Modify(Chamfer), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "expected no diagnostics for chamfer(target, dist), got: {:?}",
        compiled.diagnostics
    );
}

// ---------------------------------------------------------------------------
// step-3: Full eval pipeline test
// ---------------------------------------------------------------------------

/// Exercises the full compile -> eval path for Chamfer.
///
/// Creates a module with 2 ops:
///   Op 0: Box (produces a solid body, serves as chamfer target, produces a handle)
///   Op 1: Modify(Chamfer) referencing Step(0) as target, with distance=3mm
///
/// Verifies that the Chamfer operation receives the correct target handle
/// and a distance value of approximately 0.003 m (3 mm in SI).
///
/// This test should pass immediately since the eval layer already dispatches
/// ModifyKind::Chamfer correctly (reify-eval/src/lib.rs).
#[test]
fn chamfer_through_full_eval_pipeline() {
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 1 args: mirrors what the compiler emits (geometry.rs:882-884).
    // The eval layer resolves the target from GeomRef::Step(0), not from 'target' in args.
    let args = vec![
        ("target".to_string(), mm_literal(20.0)),
        ("distance".to_string(), mm_literal(3.0)),
    ];

    let (_result, ops) = run_modify_pipeline(ModifyKind::Chamfer, args);
    assert_eq!(
        ops.len(),
        2,
        "expected 2 geometry operations, got {}",
        ops.len()
    );

    let target_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Chamfer {
            target, distance, ..
        } => {
            assert_eq!(
                *target, target_handle,
                "Chamfer target should be handle from op 0 ({:?}), got {:?}",
                target_handle, target
            );
            let dist_si = distance
                .as_f64()
                .expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.003).abs() < 1e-9,
                "Chamfer distance should be 0.003 m (3 mm SI), got {}",
                dist_si
            );
        }
        other => panic!(
            "expected GeometryOp::Chamfer at op index 1, got {:?}",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// step-4 (1819): Contract test — eval only needs 'distance' arg for Chamfer
// ---------------------------------------------------------------------------

/// Documents the minimal args contract for Chamfer: only 'distance' is needed in args.
///
/// The eval layer resolves the target handle from GeomRef::Step(0) on the Modify
/// variant, not from any 'target' entry in the args vec. This test constructs a
/// Chamfer op with ONLY ('distance', ...) in args (no 'target' entry at all) and
/// verifies that eval still produces the correct Chamfer geometry op.
#[test]
fn chamfer_modify_only_needs_distance_arg() {
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Only 'distance' in args — no 'target' entry.
    // "target" handle is resolved from GeomRef::Step(0), not from args.
    let args = vec![("distance".to_string(), mm_literal(3.0))];

    let (_result, ops) = run_modify_pipeline(ModifyKind::Chamfer, args);
    assert_eq!(
        ops.len(),
        2,
        "expected 2 geometry operations, got {}",
        ops.len()
    );

    let target_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Chamfer {
            target, distance, ..
        } => {
            assert_eq!(
                *target, target_handle,
                "Chamfer target should be handle from op 0 ({:?}), got {:?}",
                target_handle, target
            );
            let dist_si = distance
                .as_f64()
                .expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.003).abs() < 1e-9,
                "Chamfer distance should be 0.003 m (3 mm SI), got {}",
                dist_si
            );
        }
        other => panic!(
            "expected GeometryOp::Chamfer at op index 1, got {:?}",
            other
        ),
    }
}
