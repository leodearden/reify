//! End-to-end tests for `GeomRef::Sub` name-based resolution.
//!
//! Exercises the full path from builder-constructed `CompiledModule` through
//! `Engine::build()`, verifying that named realizations produce correct kernel
//! calls and that unknown names yield clean `Error`-severity diagnostics with
//! no "not yet implemented" fallback warnings.

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, ModifyKind, PrimitiveKind};
use reify_test_support::*;
use reify_types::{CompiledExpr, ExportFormat, GeometryOp, ModulePath, Severity, Type};

// ---------------------------------------------------------------------------
// Helper: build a 3-realization module for the Sub-resolution scenario
// ---------------------------------------------------------------------------
//
// The module contains one structure "S" with three realizations:
//   - "body"   (named, index=0): single Box primitive → kernel gives h1
//   - "hole"   (named, index=1): single Cylinder primitive → kernel gives h2
//   - unnamed  (index=2):        [Difference(Sub("body"), Sub("hole")),
//                                  Fillet(Sub("body"), radius=1mm)]
//
// After "body" and "hole" succeed, named_steps = {"body": h1, "hole": h2}.
// The unnamed realization's ops look up these names, so the kernel receives:
//   Difference { left: h1, right: h2 } → h3
//   Fillet { target: h1, radius: 1mm } → h4

fn make_sub_ref_module(result_ops: Vec<CompiledGeometryOp>) -> reify_compiler::CompiledModule {
    let e = "S";
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());

    let body_ops = vec![CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_lit(20.0)),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(20.0)),
        ],
    }];

    let hole_ops = vec![CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Cylinder,
        args: vec![
            ("radius".into(), mm_lit(5.0)),
            ("height".into(), mm_lit(25.0)),
        ],
    }];

    let template = TopologyTemplateBuilder::new(e)
        .realization_named(e, 0, "body", body_ops)
        .realization_named(e, 1, "hole", hole_ops)
        .realization(e, 2, result_ops)
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test_sub_ref"))
        .template(template)
        .build()
}

// ---------------------------------------------------------------------------
// Test 1 — happy path: Sub("body") and Sub("hole") resolve to correct handles
// ---------------------------------------------------------------------------

/// Verifies that:
/// (a) no `Warning`-severity diagnostic contains "not yet implemented",
/// (b) the MockGeometryKernel received a `Difference` call with
///     `left` = handle produced by body's Box op and `right` = handle
///     produced by hole's Cylinder op,
/// (c) `build_result.geometry_output.is_some()`.
#[test]
fn sketch_with_multiple_named_sub_refs_resolves_correctly() {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());

    let result_ops = vec![
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Difference,
            left: GeomRef::Sub("body".into()),
            right: GeomRef::Sub("hole".into()),
        },
        CompiledGeometryOp::Modify {
            kind: ModifyKind::Fillet,
            target: GeomRef::Sub("body".into()),
            args: vec![("radius".into(), mm_lit(1.0))],
        },
    ];

    let module = make_sub_ref_module(result_ops);

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // (a) No "not yet implemented" warning should appear.
    let not_yet_impl_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("not yet implemented"))
        .collect();
    assert!(
        not_yet_impl_warnings.is_empty(),
        "expected no 'not yet implemented' warnings, got: {:?}",
        not_yet_impl_warnings
    );

    // (b) The kernel must have recorded a Difference op with body/hole handles.
    //
    // Derive body_handle and hole_handle from the first two recorded ops (the
    // Box and Cylinder primitives) rather than hardcoding sequential IDs.
    // MockGeometryKernel's sequential-from-1 allocator policy is an
    // implementation detail; using result_handle directly means this test
    // won't break if that policy changes without a real regression.
    let recorded_ops = ops_ref.lock().unwrap().clone();
    assert!(
        recorded_ops.len() >= 2,
        "expected at least 2 recorded ops (body Box, hole Cylinder) before checking Difference; \
         got {}: {:?}",
        recorded_ops.len(),
        recorded_ops
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    let body_handle = recorded_ops[0].result_handle;
    let hole_handle = recorded_ops[1].result_handle;
    let diff_ops: Vec<_> = recorded_ops
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Difference { .. }))
        .collect();

    assert_eq!(
        diff_ops.len(),
        1,
        "expected exactly 1 Difference op, got {}: {:?}",
        diff_ops.len(),
        recorded_ops
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    match &diff_ops[0].op {
        GeometryOp::Difference { left, right } => {
            assert_eq!(
                *left, body_handle,
                "Difference.left should be body's handle ({:?}), got {:?}",
                body_handle, left
            );
            assert_eq!(
                *right, hole_handle,
                "Difference.right should be hole's handle ({:?}), got {:?}",
                hole_handle, right
            );
        }
        other => panic!("expected Difference op, got {:?}", other),
    }

    // (c) Build must succeed and produce geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Test 2 — unknown name: GeomRef::Sub("nonexistent") returns Error diagnostic
// ---------------------------------------------------------------------------

/// Verifies that referencing an unknown sub-name:
/// - produces exactly one `Error`-severity diagnostic containing the string
///   `"unresolvable GeomRef::Sub('nonexistent')"`, and
/// - produces zero `Warning`-severity diagnostics containing
///   `"not yet implemented"` (no regression to the old fallback).
#[test]
fn sketch_with_unknown_sub_ref_returns_error() {
    let result_ops = vec![CompiledGeometryOp::Boolean {
        op: BooleanOp::Difference,
        left: GeomRef::Sub("nonexistent".into()),
        right: GeomRef::Sub("hole".into()),
    }];

    let module = make_sub_ref_module(result_ops);

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Must have exactly one Error containing the unresolvable message.
    let unresolvable_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message
                    .contains("unresolvable GeomRef::Sub('nonexistent')")
        })
        .collect();

    assert_eq!(
        unresolvable_errors.len(),
        1,
        "expected exactly 1 Error containing \"unresolvable GeomRef::Sub('nonexistent')\", \
         got {}: {:?}",
        unresolvable_errors.len(),
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // Must have zero "not yet implemented" warnings.
    let not_yet_impl_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("not yet implemented"))
        .collect();
    assert!(
        not_yet_impl_warnings.is_empty(),
        "expected no 'not yet implemented' warnings, got: {:?}",
        not_yet_impl_warnings
    );
}
